use crate::{
    error::{ErrorData, Result},
    resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef},
    ResourceType,
};
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::{any::Any, fmt::Debug};

/// Days after which S3 aborts a multipart upload that was started and never completed. Every
/// S3 bucket Alien creates, whether setup (CloudFormation, Terraform) or the storage controller
/// creates it, carries this rule next to the app's own lifecycle rules. It gives each bucket the
/// lifecycle configuration that compliance checks require and never expires a stored object.
pub const AWS_ABORT_INCOMPLETE_UPLOAD_DAYS: i32 = 7;

/// Most lifecycle rules S3 accepts on one bucket. The incomplete-upload rule is added only while
/// a bucket has room for it: a bucket at the limit already has a lifecycle configuration.
pub const AWS_LIFECYCLE_RULE_LIMIT: usize = 1000;

/// Id of the incomplete-upload rule. App rules are numbered (`Rule1`, `rule-1`), so it can't
/// collide with one.
pub const AWS_ABORT_INCOMPLETE_UPLOAD_RULE_ID: &str = "abort-incomplete-uploads";

/// Request headers an S3 CORS rule lets a browser send with a signed upload (`PUT` to a
/// presigned URL): the content headers a signer may bind, and `x-amz-*`, which covers the
/// checksums AWS SDKs add and `x-amz-meta-*`. S3 allows one `*` per header.
pub const AWS_SIGNED_UPLOAD_HEADERS: &[&str] = &[
    "Content-Type",
    "Content-MD5",
    "Content-Disposition",
    "Content-Encoding",
    "Cache-Control",
    "x-amz-*",
];

/// Headers a GCS CORS rule lists for a signed upload. GCS uses one list (`responseHeader`)
/// for both the request headers a browser may send and the response headers it may read,
/// and matches each name exactly, so `x-goog-meta-*` headers can't be listed.
pub const GCP_SIGNED_UPLOAD_HEADERS: &[&str] = &[
    "Content-Type",
    "Content-MD5",
    "Content-Disposition",
    "Content-Encoding",
    "Cache-Control",
    "x-goog-content-length-range",
    "ETag",
];

/// Request headers an Azure Blob CORS rule lets a browser send with a signed upload (`PUT`
/// to a SAS URL). `x-ms-*` covers `x-ms-blob-type`, which Put Blob requires, the
/// `x-ms-blob-content-*` and `x-ms-meta-*` headers, and the `x-ms-version` and
/// `x-ms-client-request-id` headers the Azure SDK sends.
pub const AZURE_SIGNED_UPLOAD_HEADERS: &[&str] = &["Content-Type", "Content-MD5", "x-ms-*"];

/// Response headers a browser may read after a signed upload.
pub const SIGNED_UPLOAD_EXPOSED_HEADERS: &[&str] = &["ETag"];

/// Defines a rule for managing the lifecycle of objects within a storage bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleRule {
    /// Number of days after which objects matching the rule expire.
    pub days: u32,
    /// Optional prefix to filter objects the rule applies to. If None, applies to all objects.
    #[serde(default)]
    pub prefix: Option<String>,
}

/// Object events that can be delivered to a queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum StorageNotificationEvent {
    /// An object was created or overwritten, including a completed multipart upload.
    Created,
}

fn default_notification_events() -> Vec<StorageNotificationEvent> {
    vec![StorageNotificationEvent::Created]
}

/// Delivers matching object events to a queue in the same stack.
/// Rules on one Storage must not overlap: no object key may match two of them.
/// Delivery is at least once, so consumers must tolerate duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StorageNotification {
    /// Target Queue. It must have the same lifecycle as the Storage.
    pub queue: ResourceRef,
    /// Events to deliver. Defaults to object creation; v1 supports only `created`.
    #[serde(default = "default_notification_events")]
    #[builder(default = default_notification_events())]
    pub events: Vec<StorageNotificationEvent>,
    /// Literal object key prefix, non-empty and at most 512 UTF-8 bytes when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Literal object key suffix, non-empty and at most 128 UTF-8 bytes when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

impl StorageNotification {
    /// Match literal prefixes and suffixes; wildcard characters have no special meaning.
    pub fn matches_key(&self, key: &str) -> bool {
        self.prefix
            .as_ref()
            .is_none_or(|prefix| key.starts_with(prefix))
            && self
                .suffix
                .as_ref()
                .is_none_or(|suffix| key.ends_with(suffix))
    }
}

/// Represents an object storage bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Storage {
    /// Name of the the storage bucket.
    /// For names with dots, each dot-separated label must be ≤ 63 characters.
    #[builder(start_fn)]
    pub id: String,

    /// Allows public read access to objects without authentication.
    /// Default: `false`
    #[serde(default)]
    #[builder(default)]
    pub public_read: bool,

    /// Enables object versioning.
    /// Default: `false`
    #[serde(default)]
    #[builder(default)]
    pub versioning: bool,

    /// List of rules for automatic object management (e.g., expiration).
    /// Default: `[]` (empty list)
    #[serde(default)]
    #[builder(default)]
    pub lifecycle_rules: Vec<LifecycleRule>,

    /// Browser origins allowed to read objects through signed URLs.
    ///
    /// When non-empty, providers configure CORS for `GET` and `HEAD` requests.
    /// An origin of `*` is appropriate for private buckets whose signed URLs
    /// are bearer credentials and do not use browser cookies.
    /// Default: `[]` (CORS disabled).
    #[serde(default)]
    #[builder(default)]
    pub cors_allowed_origins: Vec<String>,

    /// Browser origins allowed to upload objects through signed URLs.
    ///
    /// When non-empty, providers also configure CORS for single-request `PUT` uploads from
    /// these origins (a presigned or SAS URL, or one presigned multipart part) and expose
    /// `ETag`. Allowed request headers: `Content-Type` and `Content-MD5` everywhere;
    /// `Content-Disposition`, `Content-Encoding`, `Cache-Control` and `x-amz-*` on S3;
    /// the same content headers and `x-goog-content-length-range` on Cloud Storage, which
    /// can't allow `x-goog-meta-*`; `x-ms-*` on Azure. Uploads that start with `POST`, such
    /// as multipart create and complete or resumable uploads, are not covered.
    ///
    /// Each origin is `*` or `scheme://host[:port]`; S3 also accepts one `*` wildcard, such
    /// as `https://*.example.com`.
    /// Omitted when empty to preserve existing serialized configurations and setup digests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub cors_upload_origins: Vec<String>,

    /// Object notification rules delivered into queues (at most 10).
    /// Omitted when empty to preserve existing serialized configurations and setup digests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    #[cfg_attr(feature = "openapi", schema(max_items = 10))]
    pub notifications: Vec<StorageNotification>,

    /// Customer-managed Key used by the provider's native storage encryption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<ResourceRef>,
}

impl Storage {
    /// The resource type identifier for Storage
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("storage");

    /// Returns the storage's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Whether the storage asks for any browser CORS rule, for reads or for signed uploads.
    pub fn has_cors(&self) -> bool {
        !self.cors_allowed_origins.is_empty() || !self.cors_upload_origins.is_empty()
    }
}

/// Outputs generated by a successfully provisioned Storage bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StorageOutputs {
    /// The globally unique name of the bucket.
    pub bucket_name: String,
    // Add other outputs like region or URL if needed later
}

impl ResourceOutputsDefinition for StorageOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Storage::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<StorageOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

// Implementation of ResourceDefinition trait for Storage
impl ResourceDefinition for Storage {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn replace_after_failed_create_is_safe(&self) -> bool {
        // Deleting it destroys the objects in the bucket.
        false
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        let mut dependencies: Vec<_> = self.encryption_key.iter().cloned().collect();
        for notification in &self.notifications {
            if !dependencies.contains(&notification.queue) {
                dependencies.push(notification.queue.clone());
            }
        }
        dependencies
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        // Downcast to Storage type to use the existing validate_update method
        let new_storage = new_config
            .as_any()
            .downcast_ref::<Storage>()
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: self.id.clone(),
                    expected: Self::RESOURCE_TYPE,
                    actual: new_config.get_resource_type(),
                })
            })?;

        if self.id != new_storage.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }
        if self.encryption_key != new_storage.encryption_key {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'encryptionKey' field is immutable once the resource exists"
                    .to_string(),
            }));
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Storage>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_key_is_an_implicit_dependency() {
        let key = ResourceRef::new(crate::Key::RESOURCE_TYPE, "customer-key");
        let storage = Storage::new("customer-data".to_string())
            .encryption_key(key.clone())
            .build();

        assert_eq!(storage.get_dependencies(), vec![key]);
    }

    #[test]
    fn notifications_preserve_existing_serialization_and_deduplicate_dependencies() {
        let original: Storage = serde_json::from_value(serde_json::json!({
            "id": "objects", "publicRead": false, "versioning": false,
            "lifecycleRules": [], "corsAllowedOrigins": []
        }))
        .unwrap();
        assert!(original.notifications.is_empty());
        assert_eq!(
            serde_json::to_value(&original).unwrap(),
            serde_json::json!({
                "id": "objects", "publicRead": false, "versioning": false,
                "lifecycleRules": [], "corsAllowedOrigins": []
            })
        );
        let key = ResourceRef::new(crate::Key::RESOURCE_TYPE, "key");
        let queue = ResourceRef::new(crate::Queue::RESOURCE_TYPE, "work");
        let rule = StorageNotification::builder().queue(queue.clone()).build();
        let storage = Storage::new("objects".to_string())
            .encryption_key(key.clone())
            .notifications(vec![rule.clone(), rule])
            .build();
        assert_eq!(storage.get_dependencies(), vec![key, queue]);
        let encoded = serde_json::to_value(&storage).unwrap();
        assert_eq!(
            encoded["notifications"][0]["events"],
            serde_json::json!(["created"])
        );
        assert_eq!(serde_json::from_value::<Storage>(encoded).unwrap(), storage);
    }

    #[test]
    fn upload_origins_round_trip_and_stay_out_of_existing_serialization() {
        let declared = serde_json::json!({
            "id": "uploads", "publicRead": false, "versioning": false,
            "lifecycleRules": [], "corsAllowedOrigins": [],
            "corsUploadOrigins": ["https://app.example.com"]
        });
        let storage: Storage = serde_json::from_value(declared.clone())
            .expect("a storage declaration may allow signed browser uploads");
        assert_eq!(storage.cors_upload_origins, ["https://app.example.com"]);
        assert!(storage.has_cors());
        assert_eq!(serde_json::to_value(&storage).unwrap(), declared);

        let read_only = Storage::new("objects".to_string())
            .cors_allowed_origins(vec!["https://app.example.com".into()])
            .build();
        assert!(read_only.has_cors());
        assert!(serde_json::to_value(&read_only)
            .unwrap()
            .get("corsUploadOrigins")
            .is_none());
        assert!(!Storage::new("plain".to_string()).build().has_cors());
    }

    #[test]
    fn notification_filters_are_literal_case_sensitive_and_combined() {
        let rule = StorageNotification::builder()
            .queue(ResourceRef::new(crate::Queue::RESOURCE_TYPE, "work"))
            .prefix("in/*/é/".to_string())
            .suffix(".json".to_string())
            .build();
        assert!(rule.matches_key("in/*/é/object.json"));
        for key in [
            "in/a/é/object.json",
            "in/*/e/object.json",
            "in/*/é/object.JSON",
            "other/object.json",
            "in/*/é/object.json.tmp",
        ] {
            assert!(!rule.matches_key(key), "{key}");
        }
        let mut unfiltered = rule.clone();
        unfiltered.prefix = None;
        unfiltered.suffix = None;
        assert!(unfiltered.matches_key("anything"));
        unfiltered.suffix = Some(".json".to_string());
        assert!(unfiltered.matches_key("any/path.json"));
        assert!(!unfiltered.matches_key("any/path.txt"));
        unfiltered.prefix = Some("same".to_string());
        unfiltered.suffix = Some("same".to_string());
        assert!(unfiltered.matches_key("same"));
    }

    #[test]
    fn notification_events_default_to_created_and_reject_unknown_events() {
        let rule: StorageNotification = serde_json::from_value(serde_json::json!({
            "queue": { "type": "queue", "id": "work" }
        }))
        .unwrap();
        assert_eq!(rule.events, vec![StorageNotificationEvent::Created]);
        assert!(
            serde_json::from_value::<StorageNotification>(serde_json::json!({
                "queue": { "type": "queue", "id": "work" }, "events": ["deleted"]
            }))
            .is_err()
        );
    }
}
