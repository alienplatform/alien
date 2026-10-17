use alien_client_core::ErrorData as ClientError;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::gcs::{GcsApi, GcsNotification};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ErrorData, Result};

const OWNER_ATTRIBUTE: &str = "alien-create-token";

/// Saved before insertion because notification IDs are assigned by the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GcpNotificationIntent {
    /// Original bucket; cleanup never scans other buckets.
    bucket: String,
    /// Exact notification configuration with a unique ownership marker.
    notification: GcsNotification,
}

impl GcpNotificationIntent {
    pub fn new(bucket: String, mut notification: GcsNotification) -> Self {
        notification.custom_attributes.insert(OWNER_ATTRIBUTE.into(), Uuid::new_v4().to_string());
        Self { bucket, notification }
    }

    pub fn validate_request(&self, bucket: &str, desired: &GcsNotification, resource_id: &str) -> Result<()> {
        let mut expected = self.notification.clone();
        expected.custom_attributes.remove(OWNER_ATTRIBUTE);
        if self.bucket != bucket || !super::gcp::gcs_notification_matches_existing(&expected, desired) {
            return Err(invalid(resource_id, "Notification configuration changed during creation; retire its recorded intent before replacing it"));
        }
        Ok(())
    }

    pub fn validate_scope(&self, project: &str, topic: &str, resource_id: &str) -> Result<()> {
        let expected = format!("//pubsub.googleapis.com/projects/{project}/topics/{topic}");
        if self.bucket.is_empty() || self.notification.topic.as_deref() != Some(expected.as_str()) {
            return Err(invalid(resource_id, "Notification intent belongs to a different topic or project"));
        }
        Ok(())
    }

    fn owns(&self, notification: &GcsNotification) -> bool {
        self.notification.custom_attributes.get(OWNER_ATTRIBUTE).is_some()
            && self.notification.custom_attributes.get(OWNER_ATTRIBUTE) == notification.custom_attributes.get(OWNER_ATTRIBUTE)
    }

    fn confirm(&self, notification: &GcsNotification, resource_id: &str) -> Result<String> {
        if !self.owns(notification) || !super::gcp::gcs_notification_matches_existing(notification, &self.notification) {
            return Err(invalid(resource_id, "Refusing a foreign or changed storage notification"));
        }
        notification.id.as_ref().filter(|id| !id.is_empty()).cloned()
            .ok_or_else(|| invalid(resource_id, "Owned storage notification has no provider ID"))
    }

    pub async fn create_or_recover(&self, client: &dyn GcsApi, resource_id: &str) -> Result<String> {
        let existing = client.list_notifications(self.bucket.clone()).await
            .context(context(resource_id, "inspect pending storage notification"))?.items;
        let owned: Vec<_> = existing.iter().filter(|item| self.owns(item)).collect();
        if owned.len() > 1 {
            return Err(invalid(resource_id, "Multiple notifications have the saved owner marker; retire the recorded intent before recreating"));
        }
        if let Some(notification) = owned.first() {
            return self.confirm(notification, resource_id);
        }
        // Another configuration targeting our topic is not evidence of ownership.
        if existing.iter().any(|item| item.topic == self.notification.topic) {
            return Err(invalid(resource_id, "A foreign notification already targets the requested topic"));
        }
        let created = client.insert_notification(self.bucket.clone(), self.notification.clone()).await
            .context(context(resource_id, "create storage notification"))?;
        self.confirm(&created, resource_id)
    }

    pub async fn delete(&self, client: &dyn GcsApi, resource_id: &str) -> Result<()> {
        let existing = match client.list_notifications(self.bucket.clone()).await {
            Ok(response) => response.items,
            Err(error) if matches!(error.error, Some(ClientError::RemoteResourceNotFound { .. })) => return Ok(()),
            Err(error) => return Err(error.context(context(resource_id, "inspect storage notification before deletion"))),
        };
        // Verify all owned configurations before any deletion. Foreign IDs are untouched.
        let ids = existing.iter().filter(|item| self.owns(item))
            .map(|item| self.confirm(item, resource_id)).collect::<Result<Vec<_>>>()?;
        for id in ids {
            match client.delete_notification(self.bucket.clone(), id).await {
                Ok(()) => {},
                Err(error) if matches!(error.error, Some(ClientError::RemoteResourceNotFound { .. })) => {},
                Err(error) => return Err(error.context(context(resource_id, "delete owned storage notification"))),
            }
        }
        Ok(())
    }

}

fn invalid(resource_id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::CloudPlatformError { message: message.into(), resource_id: Some(resource_id.into()) })
}
fn context(resource_id: &str, operation: &str) -> ErrorData {
    ErrorData::CloudPlatformError { message: format!("Failed to {operation}"), resource_id: Some(resource_id.into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::gcs::{ListNotificationsResponse, MockGcsApi};
    use std::sync::{Arc, Mutex};

    fn intent() -> GcpNotificationIntent {
        GcpNotificationIntent::new("bucket".into(), GcsNotification {
            topic: Some("//pubsub.googleapis.com/projects/test/topics/owned".into()),
            event_types: vec!["OBJECT_FINALIZE".into()],
            payload_format: Some("JSON_API_V1".into()), ..Default::default()
        })
    }

    #[tokio::test]
    async fn accepted_create_lost_reply_recovers_after_reload_without_duplicate() {
        let intent = intent();
        let saved = serde_json::to_value(&intent).unwrap();
        let native = Arc::new(Mutex::new(Vec::<GcsNotification>::new()));
        let reads = native.clone(); let writes = native.clone(); let deletes = native.clone();
        let mut client = MockGcsApi::new();
        client.expect_list_notifications().returning(move |bucket| {
            assert_eq!(bucket, "bucket");
            Ok(ListNotificationsResponse { items: reads.lock().unwrap().clone(), ..Default::default() })
        });
        client.expect_insert_notification().times(1).returning(move |bucket, mut notification| {
            assert_eq!(bucket, "bucket"); notification.id = Some("123".into());
            writes.lock().unwrap().push(notification);
            Err(AlienError::new(ClientError::RemoteServiceUnavailable { message: "accepted create, lost reply".into() }))
        });
        client.expect_delete_notification().times(1).returning(move |bucket, id| {
            assert_eq!(bucket, "bucket"); assert_eq!(id, "123"); deletes.lock().unwrap().clear(); Ok(())
        });
        assert!(intent.create_or_recover(&client, "worker").await.is_err());
        let restored: GcpNotificationIntent = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.create_or_recover(&client, "worker").await.unwrap(), "123");
        restored.delete(&client, "worker").await.unwrap();
        assert!(native.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn foreign_matching_configuration_is_neither_adopted_nor_deleted() {
        let intent = intent(); let mut foreign = intent.notification.clone();
        foreign.custom_attributes.clear(); foreign.id = Some("foreign".into());
        let mut client = MockGcsApi::new();
        client.expect_list_notifications().times(2).returning(move |_| Ok(ListNotificationsResponse { items: vec![foreign.clone()], ..Default::default() }));
        client.expect_insert_notification().never(); client.expect_delete_notification().never();
        assert!(intent.create_or_recover(&client, "worker").await.is_err());
        intent.delete(&client, "worker").await.unwrap();
    }

    #[tokio::test]
    async fn duplicate_owned_notifications_are_refused_then_all_cleaned() {
        let intent = intent();
        let items: Vec<_> = ["1", "2"].into_iter().map(|id| {
            let mut notification = intent.notification.clone(); notification.id = Some(id.into()); notification
        }).collect();
        let mut client = MockGcsApi::new();
        client.expect_list_notifications().times(2).returning(move |_| Ok(ListNotificationsResponse { items: items.clone(), ..Default::default() }));
        client.expect_insert_notification().never();
        let deleted = Arc::new(Mutex::new(Vec::new())); let writes = deleted.clone();
        client.expect_delete_notification().times(2).returning(move |bucket, id| {
            assert_eq!(bucket, "bucket"); writes.lock().unwrap().push(id); Ok(())
        });
        assert!(intent.create_or_recover(&client, "worker").await.is_err());
        intent.delete(&client, "worker").await.unwrap();
        assert_eq!(*deleted.lock().unwrap(), vec!["1", "2"]);
    }

    #[tokio::test]
    async fn missing_id_or_changed_owned_configuration_refuses_mutation() {
        for changed in [false, true] {
            let intent = intent(); let mut actual = intent.notification.clone();
            if changed { actual.id = Some("123".into()); actual.payload_format = Some("NONE".into()); }
            let mut client = MockGcsApi::new();
            client.expect_list_notifications().times(2).returning(move |_| Ok(ListNotificationsResponse { items: vec![actual.clone()], ..Default::default() }));
            client.expect_insert_notification().never(); client.expect_delete_notification().never();
            assert!(intent.create_or_recover(&client, "worker").await.is_err());
            assert!(intent.delete(&client, "worker").await.is_err());
        }
    }

    #[test]
    fn changed_request_or_scope_is_refused() {
        let intent = intent(); let mut desired = intent.notification.clone(); desired.custom_attributes.remove(OWNER_ATTRIBUTE);
        intent.validate_request("bucket", &desired, "worker").unwrap();
        desired.event_types.push("OBJECT_DELETE".into());
        assert!(intent.validate_request("bucket", &desired, "worker").is_err());
        assert!(intent.validate_scope("other", "owned", "worker").is_err());
        assert!(intent.validate_request("other", &intent.notification, "worker").is_err());
    }
}
