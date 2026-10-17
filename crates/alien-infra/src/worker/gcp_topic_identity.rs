use alien_client_core::ErrorData as ClientError;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::iam::Binding;
use alien_gcp_clients::pubsub::{PubSubApi, Topic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{ErrorData, Result};

const OWNER_LABEL: &str = "alien-create-token";

/// Persisted before topic creation, retained until ownership-checked cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GcpTopicIntent {
    /// Original project-qualified topic name.
    full_name: String,
    /// Unique marker distinguishing a lost reply from a foreign same-name topic.
    token: Uuid,
}

impl GcpTopicIntent {
    pub fn new(project: &str, name: &str) -> Self {
        Self {
            full_name: format!("projects/{project}/topics/{name}"),
            token: Uuid::new_v4(),
        }
    }

    pub fn validate_scope(&self, project: &str, name: &str, resource_id: &str) -> Result<()> {
        if self.full_name != format!("projects/{project}/topics/{name}") {
            return Err(invalid(
                resource_id,
                "Pending topic belongs to a different project or name",
            ));
        }
        Ok(())
    }

    fn confirm(&self, topic: &Topic, resource_id: &str) -> Result<()> {
        if topic.name.as_deref() != Some(self.full_name.as_str())
            || topic
                .labels
                .as_ref()
                .and_then(|labels| labels.get(OWNER_LABEL))
                != Some(&self.token.to_string())
        {
            return Err(invalid(
                resource_id,
                "Refusing a foreign or replaced storage notification topic",
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
        match client.get_topic(name.to_string()).await {
            Ok(topic) => return self.confirm(&topic, resource_id),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => return Err(error.context(context(resource_id, "inspect pending topic"))),
        }
        let topic = Topic {
            labels: Some(HashMap::from([(
                OWNER_LABEL.to_string(),
                self.token.to_string(),
            )])),
            ..Default::default()
        };
        let created = match client.create_topic(name.to_string(), topic).await {
            Ok(topic) => topic,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceConflict { .. })
                ) =>
            {
                client
                    .get_topic(name.to_string())
                    .await
                    .context(context(resource_id, "inspect conflicting topic"))?
            }
            Err(error) => {
                return Err(error.context(context(resource_id, "create storage notification topic")))
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
        match client.get_topic(name.to_string()).await {
            Ok(topic) => self.confirm(&topic, resource_id)?,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(())
            }
            Err(error) => {
                return Err(error.context(context(resource_id, "inspect topic before deletion")))
            }
        }
        match client.delete_topic(name.to_string()).await {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => {
                Err(error.context(context(resource_id, "delete storage notification topic")))
            }
        }
    }

    pub async fn grant_publisher(
        &self,
        client: &dyn PubSubApi,
        name: &str,
        member: &str,
        resource_id: &str,
    ) -> Result<()> {
        let topic = client.get_topic(name.to_string()).await.context(context(
            resource_id,
            "inspect topic before permission update",
        ))?;
        self.confirm(&topic, resource_id)?;
        grant_publisher(client, name, member, resource_id).await
    }
}

/// Add the required publisher while preserving unrelated bindings and the read etag.
/// The caller must already hold a confirmed legacy receipt or a verified create intent.
pub(super) async fn grant_publisher(
    client: &dyn PubSubApi,
    name: &str,
    member: &str,
    resource_id: &str,
) -> Result<()> {
    let mut policy = client
        .get_topic_iam_policy(name.to_string())
        .await
        .context(context(resource_id, "read notification topic permissions"))?;
    if policy.bindings.iter().any(|binding| {
        binding.role == "roles/pubsub.publisher"
            && binding.condition.is_none()
            && binding.members.iter().any(|value| value == member)
    }) {
        return Ok(());
    }
    if policy.etag.as_deref().is_none_or(str::is_empty) {
        return Err(invalid(
            resource_id,
            "Topic policy has no concurrency token; refusing to replace it",
        ));
    }
    if policy
        .bindings
        .iter()
        .any(|binding| binding.role.contains("_withcond_"))
    {
        return Err(invalid(
            resource_id,
            "Topic policy omitted conditions; read policy version 3 before updating",
        ));
    }
    if let Some(binding) = policy
        .bindings
        .iter_mut()
        .find(|binding| binding.role == "roles/pubsub.publisher" && binding.condition.is_none())
    {
        binding.members.push(member.to_string());
    } else {
        policy.bindings.push(Binding {
            role: "roles/pubsub.publisher".to_string(),
            members: vec![member.to_string()],
            condition: None,
        });
    }
    policy.version = Some(3);
    client
        .set_topic_iam_policy(name.to_string(), policy)
        .await
        .context(context(resource_id, "grant storage notification publisher"))?;
    Ok(())
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
    use alien_gcp_clients::iam::{Expr, IamPolicy};
    use alien_gcp_clients::pubsub::MockPubSubApi;
    use std::sync::{Arc, Mutex};

    fn missing(name: String) -> AlienError<ClientError> {
        AlienError::new(ClientError::RemoteResourceNotFound {
            resource_type: "topic".into(),
            resource_name: name,
        })
    }

    #[tokio::test]
    async fn accepted_topic_with_lost_reply_is_recovered_and_retired_after_reload() {
        let intent = GcpTopicIntent::new("test", "owned");
        let saved = serde_json::to_value(&intent).unwrap();
        let native = Arc::new(Mutex::new(None::<Topic>));
        let reads = native.clone();
        let writes = native.clone();
        let deletes = native.clone();
        let mut client = MockPubSubApi::new();
        client
            .expect_get_topic()
            .returning(move |name| reads.lock().unwrap().clone().ok_or_else(|| missing(name)));
        client
            .expect_create_topic()
            .times(1)
            .returning(move |name, mut topic| {
                assert_eq!(name, "owned");
                topic.name = Some("projects/test/topics/owned".into());
                *writes.lock().unwrap() = Some(topic);
                Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "reply lost after create".into(),
                }))
            });
        client
            .expect_delete_topic()
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
        let restored: GcpTopicIntent = serde_json::from_value(saved).unwrap();
        restored
            .create_or_recover(&client, "owned", "worker")
            .await
            .unwrap();
        restored.delete(&client, "owned", "worker").await.unwrap();
        assert!(native.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn foreign_topic_cannot_be_adopted_granted_or_deleted() {
        let intent = GcpTopicIntent::new("test", "owned");
        let mut client = MockPubSubApi::new();
        client.expect_get_topic().times(3).returning(|_| {
            Ok(Topic {
                name: Some("projects/test/topics/owned".into()),
                ..Default::default()
            })
        });
        client.expect_create_topic().never();
        client.expect_get_topic_iam_policy().never();
        client.expect_set_topic_iam_policy().never();
        client.expect_delete_topic().never();
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
                .grant_publisher(
                    &client,
                    "owned",
                    "serviceAccount:storage@test.iam.gserviceaccount.com",
                    "worker"
                )
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
        assert!(intent.validate_scope("foreign", "owned", "worker").is_err());
    }

    fn policy() -> IamPolicy {
        IamPolicy::builder()
            .version(3)
            .etag("saved-etag".to_string())
            .bindings(vec![
                Binding {
                    role: "roles/pubsub.viewer".into(),
                    members: vec!["group:operators@example.test".into()],
                    condition: Some(Expr {
                        expression: "request.time < timestamp('2030-01-01T00:00:00Z')".into(),
                        title: Some("expires".into()),
                        description: None,
                        location: None,
                    }),
                },
                Binding {
                    role: "roles/pubsub.publisher".into(),
                    members: vec!["serviceAccount:existing@test.iam.gserviceaccount.com".into()],
                    condition: None,
                },
            ])
            .build()
    }

    #[tokio::test]
    async fn publisher_merge_preserves_conditions_members_and_etag() {
        let original = policy();
        let expected = original.clone();
        let mut client = MockPubSubApi::new();
        client
            .expect_get_topic_iam_policy()
            .times(1)
            .return_once(|_| Ok(policy()));
        client
            .expect_set_topic_iam_policy()
            .times(1)
            .return_once(move |name, policy| {
                assert_eq!(name, "owned");
                assert_eq!(policy.version, Some(3));
                assert_eq!(policy.etag, expected.etag);
                assert_eq!(
                    serde_json::to_value(&policy.bindings[0]).unwrap(),
                    serde_json::to_value(&expected.bindings[0]).unwrap()
                );
                assert_eq!(policy.bindings.len(), 2);
                assert_eq!(
                    policy.bindings[1].members,
                    [
                        "serviceAccount:existing@test.iam.gserviceaccount.com",
                        "serviceAccount:storage@test.iam.gserviceaccount.com"
                    ]
                );
                Ok(policy)
            });
        grant_publisher(
            &client,
            "owned",
            "serviceAccount:storage@test.iam.gserviceaccount.com",
            "worker",
        )
        .await
        .unwrap();
    }

    #[rstest::rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn incomplete_policy_is_not_overwritten(#[case] missing_etag: bool) {
        let mut policy = policy();
        if missing_etag {
            policy.etag = None;
        } else {
            policy.bindings[0].role = "roles/pubsub.viewer_withcond_hash".into();
            policy.bindings[0].condition = None;
        }
        let mut client = MockPubSubApi::new();
        client
            .expect_get_topic_iam_policy()
            .times(1)
            .return_once(move |_| Ok(policy));
        client.expect_set_topic_iam_policy().never();
        assert_eq!(
            grant_publisher(
                &client,
                "owned",
                "serviceAccount:storage@test.iam.gserviceaccount.com",
                "worker"
            )
            .await
            .unwrap_err()
            .code,
            "RESOURCE_CONFIG_INVALID"
        );
    }
}
