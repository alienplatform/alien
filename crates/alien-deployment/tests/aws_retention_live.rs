//! Deleting a real AWS deployment keeps its data and removes its access; an explicit erase
//! then removes the data.
//!
//! Creates a small direct-setup deployment (Live storage, Frozen KV, Frozen artifact registry
//! and a service account that can use all three), writes an object, an item and an image,
//! deletes it with the default retain choice, and checks as the account owner that:
//!
//! - the object, item and image are still readable;
//! - no S3 lifecycle rule and no enabled DynamoDB TTL remain to expire them later;
//! - every IAM role the deployment created is gone, so no deployment identity can reach them.
//!
//! It then erases the deployment and checks the bucket and table. The repository the
//! application pushed to remains and is cleaned up explicitly by the test. A deletion waits for
//! DynamoDB's TTL change window, so a run takes a little over an hour.
//!
//! ```text
//! export ALIEN_RETENTION_LIVE_TEST=1 \
//!        AWS_TARGET_ACCOUNT_ID=... AWS_TARGET_REGION=... \
//!        AWS_TARGET_ACCESS_KEY_ID=... AWS_TARGET_SECRET_ACCESS_KEY=...
//! cargo test -p alien-deployment --test aws_retention_live -- --ignored --nocapture
//! ```
//!
//! Needs `aws` and `docker` on the path to push the test image. Every name it creates starts
//! with the printed resource prefix; delete those by hand if a run is interrupted.

use std::{collections::HashMap, process::Command, sync::Arc, time::Duration};

use alien_aws_clients::{
    dynamodb::PutItemRequest,
    dynamodb::{AttributeValue, DescribeTableRequest, DescribeTimeToLiveRequest, GetItemRequest},
    ecr::{
        BatchGetImageRequest, DeleteRepositoryRequest, DescribeRepositoriesRequest, ImageIdentifier,
    },
    s3::{
        GetObjectRequest, LifecycleConfiguration, LifecycleExpiration, LifecycleRule,
        LifecycleRuleFilter, LifecycleRuleStatus, PutObjectRequest,
    },
};
use alien_core::{
    ArtifactRegistry, AwsClientConfig, AwsCredentials, ClientConfig, DeletionPolicy,
    DeploymentConfig, DeploymentState, DeploymentStatus, EnvironmentVariablesSnapshot,
    ExternalBindings, InitialSetupAuthority, Kv, PermissionProfile, Platform, ResourceLifecycle,
    ResourceStatus, RuntimeMetadata, Stack, StackSettings, StackState, Storage,
};
use alien_deployment::{
    loop_contract::{LoopOperation, LoopOutcome},
    runner::{DelayStrategy, RunnerPolicy},
    transport::{DeploymentLoopTransport, StepReconcileResult},
};
use alien_error::AlienError;
use alien_infra::{DefaultPlatformServiceProvider, PlatformServiceProvider, StackExecutor};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn aws_config() -> AwsClientConfig {
    assert_eq!(
        std::env::var("ALIEN_RETENTION_LIVE_TEST").as_deref(),
        Ok("1"),
        "set ALIEN_RETENTION_LIVE_TEST=1 to run against real AWS; it creates billed resources"
    );
    AwsClientConfig {
        account_id: env("AWS_TARGET_ACCOUNT_ID"),
        region: env("AWS_TARGET_REGION"),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: env("AWS_TARGET_ACCESS_KEY_ID"),
            secret_access_key: env("AWS_TARGET_SECRET_ACCESS_KEY"),
            session_token: None,
        },
        service_overrides: None,
    }
}

/// Setup-authority callers report each checkpoint to a manager; this run keeps it local.
struct LocalTransport;

#[async_trait::async_trait]
impl DeploymentLoopTransport for LocalTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        _state: &DeploymentState,
        _config: &DeploymentConfig,
        _update_heartbeat: bool,
        _suggested_delay_ms: Option<u64>,
        _heartbeats: Vec<alien_core::ResourceHeartbeat>,
        _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
    ) -> Result<StepReconcileResult, AlienError> {
        Ok(StepReconcileResult {
            state: None,
            config: None,
        })
    }
}

fn string(value: &str) -> AttributeValue {
    serde_json::from_value(serde_json::json!({ "S": value })).unwrap()
}

/// IAM role ARNs recorded anywhere in a resource's checkpoint.
fn recorded_roles(value: &serde_json::Value, roles: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text)
            if text.starts_with("arn:aws:iam::") && text.contains(":role/") =>
        {
            roles.push(text.clone())
        }
        serde_json::Value::Array(items) => {
            items.iter().for_each(|item| recorded_roles(item, roles))
        }
        serde_json::Value::Object(fields) => {
            fields.values().for_each(|item| recorded_roles(item, roles))
        }
        _ => {}
    }
}

fn internal(state: &StackState, id: &str, field: &str) -> String {
    state.resources[id]
        .internal_state
        .as_ref()
        .and_then(|value| value.get(field))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{id} has no recorded {field}"))
        .to_string()
}

fn run(command: &mut Command) {
    let status = command.status().expect("start command");
    assert!(
        status.success(),
        "{} exited with {status}",
        command.get_program().to_string_lossy()
    );
}

/// An `aws` CLI invocation (or a shell pipeline that runs one) against the test target,
/// whatever credentials or profile the calling shell has.
fn target_command(mut command: Command, target: &AwsClientConfig) -> Command {
    let AwsCredentials::AccessKeys {
        access_key_id,
        secret_access_key,
        session_token,
    } = &target.credentials
    else {
        panic!("the live retention fixture requires explicit target access keys");
    };
    command
        .env("AWS_ACCESS_KEY_ID", access_key_id)
        .env("AWS_SECRET_ACCESS_KEY", secret_access_key)
        .env("AWS_REGION", &target.region)
        .env("AWS_DEFAULT_REGION", &target.region)
        .env_remove("AWS_SESSION_TOKEN")
        .env_remove("AWS_PROFILE");
    if let Some(token) = session_token {
        command.env("AWS_SESSION_TOKEN", token);
    }
    command
}

#[test]
fn child_cli_uses_only_the_selected_test_target_credentials() {
    let target = AwsClientConfig {
        account_id: "123456789012".into(),
        region: "us-east-2".into(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "fixture-access".into(),
            secret_access_key: "fixture-secret".into(),
            session_token: None,
        },
        service_overrides: None,
    };
    let mut ambient = Command::new("sh");
    ambient
        .env("AWS_ACCESS_KEY_ID", "wrong-account")
        .env("AWS_SECRET_ACCESS_KEY", "wrong-secret")
        .env("AWS_SESSION_TOKEN", "wrong-session")
        .env("AWS_PROFILE", "wrong-profile")
        .env("AWS_REGION", "wrong-region");
    let status = target_command(ambient, &target)
        .args([
            "-c",
            r#"
        test "$AWS_ACCESS_KEY_ID" = fixture-access &&
        test "$AWS_SECRET_ACCESS_KEY" = fixture-secret &&
        test "$AWS_REGION" = us-east-2 &&
        test "$AWS_DEFAULT_REGION" = us-east-2 &&
        test -z "${AWS_SESSION_TOKEN+x}" && test -z "${AWS_PROFILE+x}"
    "#,
        ])
        .status()
        .expect("start child CLI credential probe");
    assert!(
        status.success(),
        "child CLI must use the selected target, not ambient credentials"
    );
}

/// Steps the deployment until it reaches one of `stops`, sleeping as each step asks.
async fn step_until(
    mut state: DeploymentState,
    config: &DeploymentConfig,
    client: &ClientConfig,
    services: &Arc<dyn PlatformServiceProvider>,
    stops: &[DeploymentStatus],
) -> DeploymentState {
    for _ in 0..400 {
        if stops.contains(&state.status) {
            return state;
        }
        assert!(
            !state.status.is_failed(),
            "deletion failed at {:?}: {:?}",
            state.status,
            state.error
        );
        // Every step resumes from a reloaded checkpoint, as a restarted runner would.
        state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        let result = alien_deployment::step(
            state,
            config.clone(),
            client.clone(),
            Some(services.clone()),
        )
        .await
        .expect("deletion step");
        state = result.state;
        if let Some(delay) = result.suggested_delay_ms {
            tokio::time::sleep(Duration::from_millis(delay.min(30_000))).await;
        }
    }
    panic!(
        "deletion did not reach {stops:?}; stopped at {:?}",
        state.status
    );
}

/// Runs setup to completion, printing each resource status change.
async fn create(executor: &StackExecutor, mut state: StackState) -> StackState {
    let mut seen = HashMap::new();
    for _ in 0..300 {
        let result = executor.step(state).await.expect("setup step");
        state = result.next_state;
        for (id, resource) in &state.resources {
            if seen.insert(id.clone(), resource.status) != Some(resource.status) {
                println!("setup: {id} {:?}", resource.status);
            }
            assert!(
                !matches!(
                    resource.status,
                    ResourceStatus::ProvisionFailed
                        | ResourceStatus::UpdateFailed
                        | ResourceStatus::RefreshFailed
                ),
                "setup of {id} failed: {:?}",
                resource.error
            );
        }
        if state
            .resources
            .values()
            .all(|resource| resource.status == ResourceStatus::Running)
        {
            return state;
        }
        let delay = result.suggested_delay_ms.unwrap_or(500).min(20_000);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    panic!("setup did not finish");
}

async fn finish_setup_teardown(
    state: &mut DeploymentState,
    config: &mut DeploymentConfig,
    client: &ClientConfig,
    services: &Arc<dyn PlatformServiceProvider>,
) {
    // A refused step, such as a TTL change DynamoDB is not yet accepting, leaves
    // TeardownFailed; the next run resumes from it.
    for attempt in 0..90 {
        let result = alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
            state,
            config,
            client,
            "dep_retention_live",
            &RunnerPolicy {
                max_steps: 400,
                operation: LoopOperation::Delete,
                delay_strategy: DelayStrategy::Inline,
            },
            &LocalTransport,
            Some(services.clone()),
        )
        .await;
        match result {
            Ok(Some(result)) if result.loop_result.outcome == LoopOutcome::Success => return,
            Ok(None) => return,
            other => {
                println!(
                    "setup teardown attempt {attempt} stopped at {:?}: {:?}",
                    state.status,
                    other
                        .as_ref()
                        .err()
                        .map(ToString::to_string)
                        .or_else(|| state.error.as_ref().map(ToString::to_string))
                );
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
    }
    panic!(
        "setup teardown did not finish; stopped at {:?}",
        state.status
    );
}

#[tokio::test]
#[ignore = "creates and deletes billed resources in a real AWS account; see the module docs"]
async fn delete_keeps_data_and_removes_access_then_erase_removes_data() {
    let aws = aws_config();
    let client = ClientConfig::Aws(Box::new(aws.clone()));
    let services: Arc<dyn PlatformServiceProvider> =
        Arc::new(DefaultPlatformServiceProvider::default());
    let prefix = format!("rl{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    println!("resource prefix: {prefix}");
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let mut stack = Stack::new("retention-live".to_string())
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Live,
        )
        .add(
            Kv::new("ledger".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ArtifactRegistry::new("images".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.permissions.profiles.insert(
        "app".to_string(),
        PermissionProfile::new()
            // Live storage takes a stack-scoped grant; exact-resource grants are setup-owned.
            .resource("*", ["storage/data-read", "storage/data-write"])
            .resource("ledger", ["kv/data-read", "kv/data-write"])
            .resource("images", ["artifact-registry/pull"]),
    );
    let initial = StackState::with_resource_prefix(Platform::Aws, prefix.clone());
    let prepared = alien_preflights::runner::PreflightRunner::new()
        .run_deployment_time_preflights(
            stack,
            &initial,
            &config,
            &client,
            None,
            None,
            Some(InitialSetupAuthority::DirectSetup),
        )
        .await
        .expect("prepare stack")
        .0;
    let executor = StackExecutor::builder(&prepared, client.clone())
        .deployment_config(&config)
        .service_provider(services.clone())
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .build()
        .expect("setup executor");
    let installed = create(&executor, initial).await;

    let bucket = internal(&installed, "data", "bucketName");
    let table = internal(&installed, "ledger", "tableName");
    let repository = format!("{}-app", internal(&installed, "images", "repositoryPrefix"));
    let mut roles = Vec::new();
    for (id, resource) in &installed.resources {
        if !["data", "ledger"].contains(&id.as_str()) {
            if let Some(value) = &resource.internal_state {
                recorded_roles(value, &mut roles);
            }
        }
    }
    roles.sort();
    roles.dedup();
    assert!(!roles.is_empty(), "the deployment created IAM roles");
    println!("created: bucket {bucket}, table {table}, repository {repository}");
    println!("created roles: {roles:#?}");

    // The data a customer would keep.
    let s3 = services.get_aws_s3_client(&aws).await.unwrap();
    s3.put_object(
        &PutObjectRequest::builder()
            .bucket(bucket.clone())
            .key("retained.txt".to_string())
            .body(b"kept".to_vec())
            .build(),
    )
    .await
    .expect("write object");
    s3.put_bucket_lifecycle_configuration(
        &bucket,
        &LifecycleConfiguration {
            rules: vec![LifecycleRule {
                id: Some("expire".to_string()),
                status: LifecycleRuleStatus::Enabled,
                filter: LifecycleRuleFilter { prefix: None },
                expiration: Some(LifecycleExpiration { days: Some(1) }),
            }],
        },
    )
    .await
    .expect("add an expiry rule");
    let dynamodb = services.get_aws_dynamodb_client(&aws).await.unwrap();
    let key = HashMap::from([
        ("pk".to_string(), string("customer")),
        ("sk".to_string(), string("record")),
    ]);
    let mut item = key.clone();
    item.insert("value".to_string(), string("kept"));
    dynamodb
        .put_item(
            PutItemRequest::builder()
                .table_name(table.clone())
                .item(item)
                .build(),
        )
        .await
        .expect("write item");
    let registry = format!("{}.dkr.ecr.{}.amazonaws.com", aws.account_id, aws.region);
    let image = format!("{registry}/{repository}:kept");
    run(target_command(Command::new("aws"), &aws).args([
        "ecr",
        "create-repository",
        "--repository-name",
        &repository,
        "--tags",
        &serde_json::to_string(
            &alien_core::standard_resource_tags(&prefix, "images")
                .into_iter()
                .map(|(key, value)| serde_json::json!({"Key":key,"Value":value}))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        "--region",
        &aws.region,
        "--output",
        "text",
        "--query",
        "repository.repositoryArn",
    ]));
    let context = tempfile::tempdir().unwrap();
    std::fs::write(context.path().join("kept.txt"), b"kept").unwrap();
    std::fs::write(
        context.path().join("Dockerfile"),
        "FROM scratch\nCOPY kept.txt /kept.txt\n",
    )
    .unwrap();
    run(target_command(Command::new("sh"), &aws).arg("-c").arg(format!(
        "aws ecr get-login-password --region {} | docker login --username AWS --password-stdin {registry} >/dev/null",
        aws.region
    )));
    run(Command::new("docker")
        .args(["build", "-q", "-t", &image])
        .arg(context.path()));
    run(Command::new("docker").args(["push", "-q", &image]));

    // Delete with the default choice.
    let state = DeploymentState::builder()
        .status(DeploymentStatus::DeletePending)
        .platform(Platform::Aws)
        .stack_state(installed.clone())
        .runtime_metadata(RuntimeMetadata {
            initial_setup_authority: InitialSetupAuthority::DirectSetup,
            prepared_stack: Some(prepared),
            ..Default::default()
        })
        .retry_requested(false)
        .protocol_version(alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION)
        .build();
    let mut state = step_until(
        state,
        &config,
        &client,
        &services,
        &[DeploymentStatus::RetentionRequired],
    )
    .await;
    finish_setup_teardown(&mut state, &mut config, &client, &services).await;
    assert_eq!(state.status, DeploymentStatus::Retained);

    // The account owner can still read everything.
    let object = s3
        .get_object(&GetObjectRequest {
            bucket: bucket.clone(),
            key: "retained.txt".to_string(),
            version_id: None,
            range: None,
        })
        .await
        .expect("retained object is readable");
    assert_eq!(object.body, b"kept");
    let item = dynamodb
        .get_item(
            GetItemRequest::builder()
                .table_name(table.clone())
                .key(key.clone())
                .build(),
        )
        .await
        .expect("retained item is readable")
        .item
        .expect("retained item exists");
    assert_eq!(item["value"].s.as_deref(), Some("kept"));
    let ecr = services.get_aws_ecr_client(&aws).await.unwrap();
    let images = ecr
        .batch_get_image(BatchGetImageRequest {
            repository_name: repository.clone(),
            image_ids: vec![ImageIdentifier {
                image_tag: Some("kept".to_string()),
                image_digest: None,
            }],
            registry_id: None,
            accepted_media_types: None,
        })
        .await
        .expect("read retained image");
    assert_eq!(images.images.len(), 1, "retained image exists");

    // Nothing is left to expire it.
    let lifecycle = s3.get_bucket_lifecycle_configuration(&bucket).await;
    assert!(
        lifecycle
            .as_ref()
            .map_or(true, |configuration| configuration.rules.is_empty()),
        "no lifecycle rule remains: {lifecycle:?}"
    );
    let ttl = dynamodb
        .describe_time_to_live(DescribeTimeToLiveRequest {
            table_name: table.clone(),
        })
        .await
        .unwrap()
        .time_to_live_description
        .and_then(|description| description.time_to_live_status);
    assert!(
        matches!(ttl.as_deref(), Some("DISABLED" | "DISABLING")),
        "TTL is off: {ttl:?}"
    );
    println!("retained: object, item, image readable; lifecycle {lifecycle:?}; TTL {ttl:?}");

    // No deployment identity remains to reach it.
    let iam = services.get_aws_iam_client(&aws).await.unwrap();
    for role in &roles {
        let name = role.rsplit('/').next().unwrap();
        let error = iam
            .get_role(name)
            .await
            .expect_err("deployment role must be deleted");
        assert!(
            matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ),
            "{name}: {error}"
        );
    }
    println!("revoked: {} roles return NoSuchEntity", roles.len());
    let retained = state.stack_state.as_ref().unwrap();
    for id in ["data", "ledger"] {
        assert_ne!(retained.resources[id].status, ResourceStatus::Deleted);
    }

    // An explicit erase removes the data.
    state.runtime_metadata.as_mut().unwrap().deletion_policy = DeletionPolicy::Erase;
    state.status = DeploymentStatus::DeletePending;
    let mut state = step_until(
        state,
        &config,
        &client,
        &services,
        &[
            DeploymentStatus::Deleted,
            DeploymentStatus::TeardownRequired,
        ],
    )
    .await;
    if state.status == DeploymentStatus::TeardownRequired {
        finish_setup_teardown(&mut state, &mut config, &client, &services).await;
    }
    assert_eq!(state.status, DeploymentStatus::Deleted);
    let bucket_gone = s3.head_bucket(&bucket).await;
    assert!(bucket_gone.is_err(), "bucket {bucket} is erased");
    let table_state = dynamodb
        .describe_table(DescribeTableRequest {
            table_name: table.clone(),
        })
        .await;
    assert!(
        table_state
            .as_ref()
            .map_or(true, |described| described.table.table_status.as_deref()
                == Some("DELETING")),
        "table {table} is erased: {table_state:?}"
    );
    println!("erased: bucket and table gone");

    // Runtime ECR erase is deferred. Verify preservation, then clean up this test's data.
    ecr.describe_repositories(DescribeRepositoriesRequest {
        registry_id: None,
        repository_names: Some(vec![repository.clone()]),
        next_token: None,
        max_results: None,
    })
    .await
    .expect("runtime repository remains until the ECR erase follow-up");
    ecr.delete_repository(DeleteRepositoryRequest {
        repository_name: repository,
        registry_id: None,
        force: Some(true),
    })
    .await
    .expect("clean up the test repository");
}
