//! Observe the actual cloud before and after notification deployment teardown.

use alien_aws_clients::{
    aws::sqs::{GetQueueAttributesRequest, GetQueueUrlRequest},
    AwsCredentialProvider, S3Api, S3Client, SqsApi, SqsClient,
};
use alien_azure_clients::{AzureResourcesClient, AzureTokenCache, ResourcesApi};
use alien_core::{
    storage_notifications::AWS_QUEUE_INGRESS_SID, AzureResourceGroupOutputs, ClientConfig,
    QueueOutputs, StackState, StorageOutputs,
};
use alien_gcp_clients::{
    gcs::{GcsApi, GcsClient},
    pubsub::{PubSubApi, PubSubClient},
};
use alien_test::{TestConfig, TestContext};
use anyhow::{bail, ensure, Context};
use reqwest::Client;

struct Observation {
    config: ClientConfig,
    state: StackState,
}

impl Observation {
    async fn capture(ctx: &TestContext) -> anyhow::Result<Self> {
        let deployment = ctx
            .manager
            .client()
            .get_deployment()
            .id(&ctx.deployment.id)
            .send()
            .await?;
        let state: StackState = serde_json::from_value(serde_json::to_value(
            deployment
                .stack_state
                .as_ref()
                .context("Running deployment must have state")?,
        )?)?;
        let observation = Self {
            config: alien_test::setup::build_direct_target_config(
                &TestConfig::from_env(),
                ctx.platform,
            )?,
            state,
        };
        // A successful read before deletion prevents a wrong account/name or missing
        // observer permission from being mistaken for successful cleanup afterward.
        observation.check(true).await?;
        Ok(observation)
    }

    fn bucket(&self, id: &str) -> anyhow::Result<&str> {
        Ok(&self
            .state
            .resources
            .get(id)
            .context("Storage state missing")?
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<StorageOutputs>())
            .context("Storage outputs missing")?
            .bucket_name)
    }

    fn queue(&self) -> anyhow::Result<&QueueOutputs> {
        self.state
            .resources
            .get("jobs")
            .context("Queue state missing")?
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<QueueOutputs>())
            .context("Queue outputs missing")
    }

    async fn check(&self, present: bool) -> anyhow::Result<()> {
        let http = Client::new();
        let prefix = &self.state.resource_prefix;
        match &self.config {
            ClientConfig::Aws(config) => {
                let credentials = AwsCredentialProvider::from_config(*config.clone()).await?;
                let s3 = S3Client::new(http.clone(), credentials.clone());
                let sqs = SqsClient::new(http, credentials);
                let queue_arn = format!(
                    "arn:aws:sqs:{}:{}:{}",
                    config.region,
                    config.account_id,
                    self.queue()?.queue_name
                );
                // Native S3 delivery: each rule is a queue configuration on its bucket.
                for (id, rules) in [("objects", 2), ("second", 1), ("results", 0)] {
                    let bucket = self.bucket(id)?;
                    let notifications = s3.get_bucket_notification_configuration(bucket).await;
                    if present {
                        let notifications = notifications
                            .with_context(|| format!("Read notifications of {bucket}"))?;
                        ensure!(
                            notifications.queue_configurations.len() == rules
                                && notifications
                                    .queue_configurations
                                    .iter()
                                    .all(|rule| rule.queue_arn == queue_arn
                                        && rule.events == ["s3:ObjectCreated:*"]),
                            "Unexpected notification rules on {bucket}: {notifications:?}"
                        );
                    } else {
                        check_presence(notifications, present, bucket)?;
                    }
                }
                let queue_url = sqs
                    .get_queue_url(GetQueueUrlRequest {
                        queue_name: self.queue()?.queue_name.clone(),
                        queue_owner_aws_account_id: Some(config.account_id.clone()),
                    })
                    .await;
                if !present {
                    check_presence(queue_url, present, "queue and its policy")?;
                    return Ok(());
                }
                let queue_url = queue_url
                    .context("Read queue URL")?
                    .get_queue_url_result
                    .queue_url;
                let policy = sqs
                    .get_queue_attributes(
                        &queue_url,
                        GetQueueAttributesRequest::builder()
                            .attribute_names(vec!["Policy".into()])
                            .build(),
                    )
                    .await?
                    .get_queue_attributes_result
                    .attributes
                    .into_iter()
                    .find(|attribute| attribute.name == "Policy")
                    .context("Queue has no policy")?
                    .value;
                let policy: serde_json::Value = serde_json::from_str(&policy)?;
                let ingress = policy["Statement"]
                    .as_array()
                    .context("Queue policy has no statements")?
                    .iter()
                    .find(|statement| statement["Sid"] == AWS_QUEUE_INGRESS_SID)
                    .context("Queue policy has no storage ingress statement")?;
                let mut sources: Vec<String> =
                    match &ingress["Condition"]["ArnLike"]["aws:SourceArn"] {
                        serde_json::Value::String(source) => vec![source.clone()],
                        serde_json::Value::Array(sources) => sources
                            .iter()
                            .filter_map(|source| source.as_str().map(str::to_string))
                            .collect(),
                        other => bail!("Unexpected ingress sources: {other}"),
                    };
                sources.sort();
                let mut expected = vec![
                    format!("arn:aws:s3:::{}", self.bucket("objects")?),
                    format!("arn:aws:s3:::{}", self.bucket("second")?),
                ];
                expected.sort();
                ensure!(
                    ingress["Principal"]["Service"] == "s3.amazonaws.com" && sources == expected,
                    "Queue ingress must admit exactly the notifying buckets: {ingress}"
                );
            }
            ClientConfig::Gcp(config) => {
                let gcs = GcsClient::new(http.clone(), *config.clone());
                let pubsub = PubSubClient::new(http, *config.clone());
                for id in ["objects", "second", "results"] {
                    let bucket = self.bucket(id)?;
                    check_presence(gcs.get_bucket(bucket.into()).await, present, bucket)?;
                    if present && id != "results" {
                        let notifications = gcs.list_notifications(bucket.into()).await?;
                        ensure!(
                            notifications.items.len() == if id == "objects" { 2 } else { 1 },
                            "Unexpected notification count on {bucket}"
                        );
                    }
                }
                check_presence(
                    pubsub.get_topic(self.queue()?.queue_name.clone()).await,
                    present,
                    "queue topic and its publisher policy",
                )?;
                // Include both the queue pull subscription and the Worker's push route.
                // Paginate the independent list so a leaked route cannot hide on another page.
                let mut page = None;
                let mut subscriptions = Vec::new();
                loop {
                    let response = pubsub.list_subscriptions(Some(1000), page).await?;
                    subscriptions.extend(response.subscriptions.into_iter().filter_map(
                        |subscription| {
                            subscription.name.filter(|name| {
                                name.rsplit('/')
                                    .next()
                                    .is_some_and(|name| name.starts_with(&format!("{prefix}-")))
                            })
                        },
                    ));
                    page = response.next_page_token.filter(|value| !value.is_empty());
                    if page.is_none() {
                        break;
                    }
                }
                ensure!(
                    present != subscriptions.is_empty(),
                    "Unexpected subscription inventory (present={present}): {subscriptions:?}"
                );
            }
            ClientConfig::Azure(config) => {
                let resources =
                    AzureResourcesClient::new(http, AzureTokenCache::new(*config.clone()));
                let group = self
                    .state
                    .resources
                    .values()
                    .find_map(|resource| {
                        resource
                            .outputs
                            .as_ref()?
                            .downcast_ref::<AzureResourceGroupOutputs>()
                    })
                    .context("Azure resource group outputs missing")?;
                // All notification source accounts and queue namespaces belong to this
                // deployment's group. Its deletion removes their policies/subscriptions.
                check_presence(
                    resources.get_resource_group(&group.name).await,
                    present,
                    &group.name,
                )?;
            }
            _ => bail!("Notification cloud cleanup requires AWS, GCP, or Azure"),
        }
        Ok(())
    }
}

fn check_presence<T>(
    result: alien_aws_clients::Result<T>,
    present: bool,
    resource: &str,
) -> anyhow::Result<()> {
    match (present, result) {
        (true, Ok(_)) => Ok(()),
        (false, Err(error)) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => Ok(()),
        (false, Ok(_)) => bail!("Cloud resource survived teardown: {resource}"),
        (_, Err(error)) => Err(anyhow::anyhow!("Cannot verify {resource}: {error}")),
    }
}

/// Always run teardown, including when the pre-deletion observation fails.
pub async fn cleanup(ctx: TestContext) -> anyhow::Result<()> {
    let observation = Observation::capture(&ctx).await;
    let cleanup = ctx.cleanup().await;
    match (observation, cleanup) {
        (Ok(observation), Ok(())) => observation.check(false).await,
        (Err(observation), Ok(())) => Err(observation),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(observation), Err(cleanup)) => {
            bail!("Cloud observation failed: {observation:#}; teardown also failed: {cleanup:#}")
        }
    }
}
