//! Opt-in AWS check of queue permission sets through the real queue binding.
//!
//! Run with `AWS_PROFILE`, `AWS_REGION`, and `ALIEN_TEST_AWS_EXPECTED_ACCOUNT_ID` set.
//! The test loads `.env.test` from the workspace root, or `ALIEN_TEST_ENV_FILE`.
//! `cargo test -p alien-bindings --test queue_iam_aws --no-default-features --features aws -- --ignored --nocapture`

#![cfg(feature = "aws")]

use alien_aws_clients::{
    iam::{CreateRoleRequest, IamApi, IamClient},
    sqs::{CreateQueueRequest, SqsApi, SqsClient},
    sts::{AssumeRoleRequest, StsApi, StsClient},
    AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider, AwsCredentials,
};
use alien_bindings::{
    traits::{BindingsProviderApi, MessagePayload},
    BindingsProvider,
};
use alien_core::bindings::{self, QueueBinding};
use alien_permissions::{
    generators::AwsCloudFormationPermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use serde_json::json;
use std::{collections::HashMap, env, time::Duration};
use test_context::AsyncTestContext;

type TestResult<T> = Result<T, String>;

fn expect_access_denied<T>(
    result: alien_bindings::error::Result<T>,
    operation: &str,
) -> TestResult<()> {
    match result {
        Ok(_) => Err(format!("{operation} unexpectedly succeeded")),
        Err(error) => {
            let detail = format!("{error:?}");
            if detail.contains("RemoteAccessDenied") || detail.contains("AccessDenied") {
                Ok(())
            } else {
                Err(format!(
                    "{operation} failed for a reason other than IAM denial: {detail}"
                ))
            }
        }
    }
}

struct AwsQueueContext {
    account_id: String,
    region: String,
    prefix: String,
    queue_url: Option<String>,
    roles: Vec<String>,
    iam: IamClient,
    sqs: SqsClient,
    sts: StsClient,
}

impl AwsQueueContext {
    async fn create() -> TestResult<Self> {
        let env_file = env::var("ALIEN_TEST_ENV_FILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| workspace_root::get_workspace_root().join(".env.test"));
        dotenvy::from_path(&env_file)
            .map_err(|error| format!("load {}: {error}", env_file.display()))?;

        let profile = env::var("AWS_PROFILE").map_err(|_| "set AWS_PROFILE".to_string())?;
        let region = env::var("AWS_REGION").map_err(|_| "set AWS_REGION".to_string())?;
        let expected_account = env::var("ALIEN_TEST_AWS_EXPECTED_ACCOUNT_ID")
            .map_err(|_| "set ALIEN_TEST_AWS_EXPECTED_ACCOUNT_ID".to_string())?;
        let config = AwsClientConfig {
            account_id: expected_account.clone(),
            region: region.clone(),
            credentials: AwsCredentials::Profile { name: profile },
            service_overrides: None,
        }
        .get_web_identity_credentials()
        .await
        .map_err(|error| format!("resolve AWS profile: {error}"))?;
        let sts = StsClient::new(reqwest::Client::new(), config.clone());
        let actual_account = sts
            .get_caller_identity()
            .await
            .map_err(|error| format!("identify AWS account: {error}"))?
            .get_caller_identity_result
            .account
            .ok_or("STS returned no account ID")?;
        if actual_account != expected_account {
            return Err(format!(
                "AWS account mismatch: expected {expected_account}, got {actual_account}"
            ));
        }

        let credentials = AwsCredentialProvider::from_config(config)
            .await
            .map_err(|error| format!("resolve AWS profile: {error}"))?;
        let prefix = format!(
            "alien-test-queue-iam-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let mut context = Self {
            account_id: actual_account,
            region,
            prefix,
            queue_url: None,
            roles: Vec::new(),
            iam: IamClient::new(reqwest::Client::new(), credentials.clone()),
            sqs: SqsClient::new(reqwest::Client::new(), credentials),
            sts,
        };
        println!("AWS queue IAM test prefix: {}", context.prefix);
        if let Err(error) = context.provision().await {
            let cleanup = context.cleanup().await;
            return Err(format!("provision: {error}; cleanup: {cleanup:?}"));
        }
        Ok(context)
    }

    async fn provision(&mut self) -> TestResult<()> {
        let queue_name = format!("{}-queue", self.prefix);
        let queue = self
            .sqs
            .create_queue(
                CreateQueueRequest::builder()
                    .queue_name(queue_name.clone())
                    .attributes(HashMap::from([(
                        "VisibilityTimeout".to_string(),
                        "3".to_string(),
                    )]))
                    .build(),
            )
            .await
            .map_err(|error| format!("create SQS queue: {error}"))?;
        self.queue_url = Some(queue.create_queue_result.queue_url);

        let queue_arn = format!(
            "arn:aws:sqs:{}:{}:{queue_name}",
            self.region, self.account_id
        );
        for (suffix, permission_set) in [
            ("publisher", "queue/publish"),
            ("consumer", "queue/data-read"),
        ] {
            let role_name = format!("{}-{suffix}", self.prefix);
            let trust = json!({
                "Version": "2012-10-17",
                "Statement": [{
                    "Effect": "Allow",
                    "Principal": {"AWS": format!("arn:aws:iam::{}:root", self.account_id)},
                    "Action": "sts:AssumeRole"
                }]
            });
            self.iam
                .create_role(
                    CreateRoleRequest::builder()
                        .role_name(role_name.clone())
                        .assume_role_policy_document(trust.to_string())
                        .description(
                            "Disposable Alien queue permission integration test".to_string(),
                        )
                        .build(),
                )
                .await
                .map_err(|error| format!("create {suffix} role: {error}"))?;
            self.roles.push(role_name.clone());

            let context = PermissionContext::new()
                .with_aws_account_id(&self.account_id)
                .with_aws_region(&self.region)
                .with_resource_name(&queue_name);
            let policy = AwsCloudFormationPermissionsGenerator::new()
                .generate_policy(
                    get_permission_set(permission_set)
                        .ok_or_else(|| format!("missing {permission_set}"))?,
                    BindingTarget::Resource,
                    &context,
                )
                .map_err(|error| format!("generate {permission_set}: {error}"))?;
            if policy.statement.len() != 1 {
                return Err(format!("expected one {permission_set} statement"));
            }
            let statement = &policy.statement[0];
            let expected_resource = json!({
                "Fn::Sub": format!(
                    "arn:${{AWS::Partition}}:sqs:${{AWS::Region}}:${{AWS::AccountId}}:{queue_name}"
                )
            });
            if statement.resource != [expected_resource] || statement.condition.is_some() {
                return Err(format!("unexpected {permission_set} resource or condition"));
            }
            let concrete_policy = json!({
                "Version": policy.version,
                "Statement": [{
                    "Sid": statement.sid,
                    "Effect": statement.effect,
                    "Action": statement.action,
                    "Resource": queue_arn
                }]
            });
            self.iam
                .put_role_policy(&role_name, "QueueAccess", &concrete_policy.to_string())
                .await
                .map_err(|error| format!("attach {suffix} policy: {error}"))?;
        }
        Ok(())
    }

    async fn binding_for(
        &self,
        suffix: &str,
    ) -> TestResult<std::sync::Arc<dyn alien_bindings::traits::Queue>> {
        let role_arn = format!(
            "arn:aws:iam::{}:role/{}-{suffix}",
            self.account_id, self.prefix
        );
        let mut assumed = None;
        for _ in 0..12 {
            match self
                .sts
                .assume_role(
                    AssumeRoleRequest::builder()
                        .role_arn(role_arn.clone())
                        .role_session_name("alien-queue-iam-test".to_string())
                        .build(),
                )
                .await
            {
                Ok(response) => {
                    assumed = Some(response.assume_role_result.credentials);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_secs(5)).await,
            }
        }
        let credentials = assumed.ok_or_else(|| format!("could not assume {role_arn}"))?;
        let mut env_map = HashMap::from([
            ("ALIEN_DEPLOYMENT_TYPE".to_string(), "aws".to_string()),
            ("AWS_REGION".to_string(), self.region.clone()),
            ("AWS_ACCOUNT_ID".to_string(), self.account_id.clone()),
            ("AWS_ACCESS_KEY_ID".to_string(), credentials.access_key_id),
            (
                "AWS_SECRET_ACCESS_KEY".to_string(),
                credentials.secret_access_key,
            ),
            ("AWS_SESSION_TOKEN".to_string(), credentials.session_token),
        ]);
        let binding =
            QueueBinding::sqs(self.queue_url.as_ref().ok_or("queue URL missing")?.clone());
        env_map.insert(
            bindings::binding_env_var_name("jobs"),
            serde_json::to_string(&binding).map_err(|error| error.to_string())?,
        );
        let provider = BindingsProvider::from_env(env_map)
            .await
            .map_err(|error| format!("load {suffix} credentials: {error}"))?;
        provider
            .load_queue("jobs")
            .await
            .map_err(|error| format!("load {suffix} queue binding: {error}"))
    }

    async fn exercise(&self) -> TestResult<()> {
        let publisher = self.binding_for("publisher").await?;
        let consumer = self.binding_for("consumer").await?;
        let marker = format!("{}-message", self.prefix);

        // IAM changes can take a few seconds to reach SQS. Retry only the first
        // permitted operation; all denied operations below must fail immediately.
        let mut sent = false;
        for attempt in 0..12 {
            match publisher
                .send("jobs", MessagePayload::Text(marker.clone()))
                .await
            {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(error) if attempt < 11 => {
                    let detail = format!("{error:?}");
                    if !detail.contains("RemoteAccessDenied") && !detail.contains("AccessDenied") {
                        return Err(format!("publisher send: {detail}"));
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Err(error) => return Err(format!("publisher send: {error}")),
            }
        }
        if !sent {
            return Err("publisher could not send after IAM propagation".to_string());
        }
        expect_access_denied(publisher.receive("jobs", 1).await, "publisher receive")?;
        expect_access_denied(
            consumer
                .send("jobs", MessagePayload::Text("forbidden".to_string()))
                .await,
            "consumer send",
        )?;

        let mut messages = None;
        for attempt in 0..12 {
            match consumer.receive("jobs", 1).await {
                Ok(received) => {
                    messages = Some(received);
                    break;
                }
                Err(error) if attempt < 11 => {
                    let detail = format!("{error:?}");
                    if !detail.contains("RemoteAccessDenied") && !detail.contains("AccessDenied") {
                        return Err(format!("consumer receive: {detail}"));
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Err(error) => return Err(format!("consumer receive: {error}")),
            }
        }
        let messages = messages.ok_or("consumer received no result")?;
        if messages.len() != 1
            || !matches!(&messages[0].payload, MessagePayload::Text(text) if text == &marker)
        {
            return Err(format!("consumer did not receive marker {marker}"));
        }
        expect_access_denied(
            publisher.ack("jobs", &messages[0].receipt_handle).await,
            "publisher ack",
        )?;
        consumer
            .ack("jobs", &messages[0].receipt_handle)
            .await
            .map_err(|error| format!("consumer ack: {error}"))?;
        tokio::time::sleep(Duration::from_secs(4)).await;
        if !consumer
            .receive("jobs", 1)
            .await
            .map_err(|error| format!("check redelivery: {error}"))?
            .is_empty()
        {
            return Err("acknowledged message was redelivered".to_string());
        }
        Ok(())
    }

    async fn cleanup(&self) -> Vec<String> {
        let mut failures = Vec::new();
        for role in &self.roles {
            if let Err(error) = self.iam.delete_role_policy(role, "QueueAccess").await {
                failures.push(format!("delete policy on {role}: {error}"));
            }
            if let Err(error) = self.iam.delete_role(role).await {
                failures.push(format!("delete role {role}: {error}"));
            }
        }
        if let Some(queue_url) = &self.queue_url {
            if let Err(error) = self.sqs.delete_queue(queue_url).await {
                failures.push(format!("delete queue {queue_url}: {error}"));
            }
        }
        failures
    }
}

impl AsyncTestContext for AwsQueueContext {
    async fn setup() -> Self {
        Self::create()
            .await
            .expect("create disposable AWS queue and roles")
    }

    async fn teardown(self) {
        let failures = self.cleanup().await;
        assert!(failures.is_empty(), "AWS cleanup failed: {failures:?}");
    }
}

#[tokio::test]
#[ignore = "creates a disposable queue and IAM roles in a real AWS account"]
async fn publisher_and_consumer_can_only_perform_their_queue_operations() {
    let context = AwsQueueContext::setup().await;
    let result = context.exercise().await;
    context.teardown().await;
    result.expect("real AWS queue permissions and bindings");
}
