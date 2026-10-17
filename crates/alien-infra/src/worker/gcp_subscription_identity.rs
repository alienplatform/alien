use alien_client_core::ErrorData as ClientError;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::pubsub::{PubSubApi, Subscription};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ErrorData, Result};

const OWNER_LABEL: &str = "alien-create-token";

/// Persist before subscription creation; confirmed and uncertain creates share cleanup ownership.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GcpSubscriptionIntent {
    /// Exact native name in the original project.
    full_name: String,
    /// Original push delivery configuration and unique ownership marker.
    subscription: Subscription,
}

impl GcpSubscriptionIntent {
    pub fn new(project: &str, name: &str, mut subscription: Subscription) -> Self {
        subscription.name = Some(name.to_string());
        subscription
            .labels
            .get_or_insert_with(Default::default)
            .insert(OWNER_LABEL.to_string(), Uuid::new_v4().to_string());
        Self {
            full_name: format!("projects/{project}/subscriptions/{name}"),
            subscription,
        }
    }

    pub fn validate_scope(&self, project: &str, name: &str, resource_id: &str) -> Result<()> {
        if self.full_name != format!("projects/{project}/subscriptions/{name}") {
            return Err(invalid(
                resource_id,
                "Pending subscription belongs to a different project or name",
            ));
        }
        Ok(())
    }

    fn configuration_matches(&self, actual: &Subscription) -> bool {
        let expected = &self.subscription;
        let push_matches = match (&expected.push_config, &actual.push_config) {
            (Some(expected), Some(actual)) => {
                expected.push_endpoint == actual.push_endpoint
                    && actual.no_wrapper.is_none()
                    && match (&expected.oidc_token, &actual.oidc_token) {
                        (Some(expected), Some(actual)) => {
                            expected.service_account_email == actual.service_account_email
                                && expected.audience == actual.audience
                        }
                        _ => false,
                    }
            }
            _ => false,
        };
        expected.topic == actual.topic
            && expected.ack_deadline_seconds == actual.ack_deadline_seconds
            && expected.filter.as_deref().unwrap_or("") == actual.filter.as_deref().unwrap_or("")
            && expected.retain_acked_messages.unwrap_or(false)
                == actual.retain_acked_messages.unwrap_or(false)
            && !actual.detached.unwrap_or(false)
            && actual.bigquery_config.is_none()
            && actual.cloud_storage_config.is_none()
            && push_matches
    }

    pub fn validate_request(&self, desired: &Subscription, resource_id: &str) -> Result<()> {
        if !self.configuration_matches(desired) {
            return Err(invalid(resource_id, "Subscription configuration changed during creation; retire the recorded intent before replacing it"));
        }
        Ok(())
    }

    fn confirm(&self, actual: &Subscription, resource_id: &str) -> Result<()> {
        let expected_token = self
            .subscription
            .labels
            .as_ref()
            .and_then(|labels| labels.get(OWNER_LABEL));
        let actual_token = actual
            .labels
            .as_ref()
            .and_then(|labels| labels.get(OWNER_LABEL));
        if expected_token.is_none()
            || expected_token != actual_token
            || actual.name.as_deref() != Some(self.full_name.as_str())
            || !self.configuration_matches(actual)
        {
            return Err(invalid(
                resource_id,
                "Refusing a foreign or changed push subscription",
            ));
        }
        Ok(())
    }

    pub async fn create_or_recover(
        &self,
        client: &dyn PubSubApi,
        name: &str,
        resource_id: &str,
    ) -> Result<()> {
        match client.get_subscription(name.to_string()).await {
            Ok(subscription) => return self.confirm(&subscription, resource_id),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => {
                return Err(error.context(context(resource_id, "inspect pending push subscription")))
            }
        }
        let created = match client
            .create_subscription(name.to_string(), self.subscription.clone())
            .await
        {
            Ok(subscription) => subscription,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceConflict { .. })
                ) =>
            {
                client
                    .get_subscription(name.to_string())
                    .await
                    .context(context(
                        resource_id,
                        "inspect conflicting push subscription",
                    ))?
            }
            Err(error) => {
                return Err(error.context(context(resource_id, "create push subscription")))
            }
        };
        self.confirm(&created, resource_id)
    }

    pub async fn delete(
        &self,
        client: &dyn PubSubApi,
        name: &str,
        resource_id: &str,
    ) -> Result<()> {
        match client.get_subscription(name.to_string()).await {
            Ok(subscription) => self.confirm(&subscription, resource_id)?,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(())
            }
            Err(error) => {
                return Err(error.context(context(
                    resource_id,
                    "inspect push subscription before deletion",
                )))
            }
        }
        match client.delete_subscription(name.to_string()).await {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.context(context(resource_id, "delete push subscription"))),
        }
    }
}

fn invalid(resource_id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(resource_id.to_string()),
        message: message.to_string(),
    })
}

fn context(resource_id: &str, operation: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(resource_id.to_string()),
        message: format!("Failed to {operation}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::pubsub::{MockPubSubApi, OidcToken, PushConfig};
    use std::sync::{Arc, Mutex};

    fn subscription() -> Subscription {
        Subscription {
            topic: Some("projects/test/topics/queue".into()),
            ack_deadline_seconds: Some(300),
            push_config: Some(PushConfig {
                push_endpoint: Some("https://owned.run.app/".into()),
                attributes: None,
                oidc_token: Some(OidcToken {
                    service_account_email: "worker@test.iam.gserviceaccount.com".into(),
                    audience: Some("https://owned.run.app".into()),
                }),
                pubsub_wrapper: None,
                no_wrapper: None,
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn lost_subscription_create_reply_is_recovered_and_cleaned_after_reload() {
        let intent = GcpSubscriptionIntent::new("test", "owned", subscription());
        let saved = serde_json::to_value(&intent).unwrap();
        let native = Arc::new(Mutex::new(None::<Subscription>));
        let reads = native.clone();
        let writes = native.clone();
        let deletes = native.clone();
        let mut client = MockPubSubApi::new();
        client.expect_get_subscription().returning(move |name| {
            reads.lock().unwrap().clone().ok_or_else(|| {
                AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "subscription".into(),
                    resource_name: name,
                })
            })
        });
        client
            .expect_create_subscription()
            .times(1)
            .returning(move |name, mut subscription| {
                assert_eq!(name, "owned");
                subscription.name = Some("projects/test/subscriptions/owned".into());
                *writes.lock().unwrap() = Some(subscription);
                Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "reply lost after accepted create".into(),
                }))
            });
        client
            .expect_delete_subscription()
            .times(1)
            .returning(move |name| {
                assert_eq!(name, "owned");
                assert!(deletes.lock().unwrap().take().is_some());
                Ok(())
            });
        assert!(intent
            .create_or_recover(&client, "owned", "worker")
            .await
            .is_err());
        let restored: GcpSubscriptionIntent = serde_json::from_value(saved).unwrap();
        restored
            .create_or_recover(&client, "owned", "worker")
            .await
            .unwrap();
        restored.delete(&client, "owned", "worker").await.unwrap();
        assert!(native.lock().unwrap().is_none());
    }

    #[rstest::rstest]
    #[case("owner")]
    #[case("topic")]
    #[case("audience")]
    #[case("endpoint")]
    #[case("detached")]
    #[tokio::test]
    async fn foreign_or_changed_subscription_is_not_adopted_or_deleted(#[case] change: &str) {
        let intent = GcpSubscriptionIntent::new("test", "owned", subscription());
        let mut actual = intent.subscription.clone();
        actual.name = Some(intent.full_name.clone());
        match change {
            "owner" => actual.labels = None,
            "topic" => actual.topic = Some("projects/foreign/topics/queue".into()),
            "audience" => {
                actual
                    .push_config
                    .as_mut()
                    .unwrap()
                    .oidc_token
                    .as_mut()
                    .unwrap()
                    .audience = Some("https://foreign.run.app".into())
            }
            "endpoint" => {
                actual.push_config.as_mut().unwrap().push_endpoint =
                    Some("https://foreign.run.app/".into())
            }
            "detached" => actual.detached = Some(true),
            _ => unreachable!(),
        }
        let mut client = MockPubSubApi::new();
        client
            .expect_get_subscription()
            .times(2)
            .returning(move |_| Ok(actual.clone()));
        client.expect_create_subscription().never();
        client.expect_delete_subscription().never();
        assert_eq!(
            intent
                .create_or_recover(&client, "owned", "worker")
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        assert_eq!(
            intent
                .delete(&client, "owned", "worker")
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
    }

    #[test]
    fn changed_pending_delivery_request_is_rejected() {
        let intent = GcpSubscriptionIntent::new("test", "owned", subscription());
        intent.validate_request(&subscription(), "worker").unwrap();
        let mut changed = subscription();
        changed.topic = Some("projects/test/topics/replacement".into());
        assert_eq!(
            intent
                .validate_request(&changed, "worker")
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        assert!(intent.validate_scope("foreign", "owned", "worker").is_err());
    }
}
