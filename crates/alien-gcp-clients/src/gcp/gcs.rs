use crate::gcp::gcp_request_utils::{auth_send_json, auth_send_no_response, GcpAuthConfig};
use crate::gcp::iam::IamPolicy;
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::ErrorData;
use alien_client_core::RequestBuilderExt;
use alien_client_core::Result;
use alien_error::AlienError;
use bon::Builder;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap; // For Object metadata
use url::Url;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

// Base URLs for Google Cloud Storage JSON API
const GCS_API_BASE: &str = "https://storage.googleapis.com/storage/v1";
const GCS_UPLOAD_BASE: &str = "https://storage.googleapis.com/upload/storage/v1";

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait GcsApi: Send + Sync + Debug {
    /// Create a new bucket inside the configured project.
    async fn create_bucket(&self, bucket_name: String, bucket: Bucket) -> Result<Bucket>;

    /// Retrieve bucket metadata.
    async fn get_bucket(&self, bucket_name: String) -> Result<Bucket>;

    /// Patch/update bucket fields.
    async fn update_bucket(&self, bucket_name: String, bucket_patch: Bucket) -> Result<Bucket>;

    /// Delete a bucket (must be empty).
    async fn delete_bucket(&self, bucket_name: String) -> Result<()>;

    /// Get the IAM policy attached to `bucket_name`.
    async fn get_bucket_iam_policy(&self, bucket_name: String) -> Result<IamPolicy>;

    /// Replace the IAM policy of `bucket_name`.
    async fn set_bucket_iam_policy(
        &self,
        bucket_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;

    /// List objects inside `bucket_name` with optional filtering/pagination.
    #[allow(clippy::too_many_arguments)]
    async fn list_objects(
        &self,
        bucket_name: String,
        prefix: Option<String>,
        delimiter: Option<String>,
        page_token: Option<String>,
        max_results: Option<u32>,
        versions: Option<bool>,
    ) -> Result<ListObjectsResponse>;

    /// Permanently delete an object (optionally a specific generation).
    async fn delete_object(
        &self,
        bucket_name: String,
        object_name: String,
        generation: Option<i64>,
    ) -> Result<()>;

    /// Simple media upload (small object, single request).
    async fn insert_object(
        &self,
        bucket_name: String,
        object_resource: Object,
        object_data: Vec<u8>,
    ) -> Result<Object>;

    /// Empty a bucket by deleting all objects (including all versions if versioned).
    /// Treats RemoteResourceNotFound as success to make deletion idempotent.
    async fn empty_bucket(&self, bucket_name: String) -> Result<()>;

    /// Create a Pub/Sub notification configuration on a bucket.
    /// When events matching the configuration occur, GCS publishes to the specified Pub/Sub topic.
    async fn insert_notification(
        &self,
        bucket_name: String,
        notification: GcsNotification,
    ) -> Result<GcsNotification>;

    /// Delete a notification configuration from a bucket.
    async fn delete_notification(
        &self,
        bucket_name: String,
        notification_id: String,
    ) -> Result<()>;
}

/// Google Cloud Storage client relying on the new cloud-agnostic helpers.
#[derive(Debug)]
pub struct GcsClient {
    http: Client,
    cfg: GcpClientConfig,
}

impl GcsClient {
    /// Construct a new client using the provided hyper/reqwest `Client` and
    /// platform configuration.
    pub fn new(http: Client, cfg: GcpClientConfig) -> Self {
        Self { http, cfg }
    }

    /// Helper that fetches a fresh bearer token and wraps it as `GcpAuthConfig`.
    async fn auth(&self) -> Result<GcpAuthConfig> {
        let token = self
            .cfg
            .get_bearer_token("https://storage.googleapis.com/")
            .await?;
        Ok(GcpAuthConfig {
            bearer_token: token,
        })
    }

    /// Get the API base URL, checking for overrides first
    fn get_api_base_url(&self) -> &str {
        if let Some(override_url) = self.cfg.get_service_endpoint_option("storage") {
            override_url
        } else {
            GCS_API_BASE
        }
    }

    /// Get the upload base URL, checking for overrides first
    fn get_upload_base_url(&self) -> &str {
        if let Some(override_url) = self.cfg.get_service_endpoint_option("storage") {
            // For overrides, assume the same base URL handles both API and upload
            override_url
        } else {
            GCS_UPLOAD_BASE
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl GcsApi for GcsClient {
    // ------------------------------------------------------------
    // Bucket operations
    // ------------------------------------------------------------

    /// Create a new bucket inside the configured project.
    async fn create_bucket(&self, bucket_name: String, mut bucket: Bucket) -> Result<Bucket> {
        bucket.name = Some(bucket_name.to_string());

        let url = format!(
            "{}/b?project={}",
            self.get_api_base_url(),
            self.cfg.project_id
        );

        let builder = self.http.post(url).json(&bucket);
        auth_send_json(
            builder,
            &self.auth().await?,
            "CreateBucket",
            &bucket_name,
            "GCS",
        )
        .await
    }

    /// Retrieve bucket metadata.
    async fn get_bucket(&self, bucket_name: String) -> Result<Bucket> {
        let url = format!("{}/b/{}", self.get_api_base_url(), bucket_name);
        let builder = self.http.get(url);
        auth_send_json(
            builder,
            &self.auth().await?,
            "GetBucket",
            &bucket_name,
            "GCS",
        )
        .await
    }

    /// Patch/update bucket fields.
    async fn update_bucket(&self, bucket_name: String, bucket_patch: Bucket) -> Result<Bucket> {
        let url = format!("{}/b/{}", self.get_api_base_url(), bucket_name);
        let builder = self.http.patch(url).json(&bucket_patch);
        auth_send_json(
            builder,
            &self.auth().await?,
            "UpdateBucket",
            &bucket_name,
            "GCS",
        )
        .await
    }

    /// Delete a bucket (must be empty).
    async fn delete_bucket(&self, bucket_name: String) -> Result<()> {
        let url = format!("{}/b/{}", self.get_api_base_url(), bucket_name);
        let builder = self.http.delete(url);
        auth_send_no_response(
            builder,
            &self.auth().await?,
            "DeleteBucket",
            &bucket_name,
            "GCS",
        )
        .await
    }

    /// Get the IAM policy attached to `bucket_name`.
    async fn get_bucket_iam_policy(&self, bucket_name: String) -> Result<IamPolicy> {
        let url = format!("{}/b/{}/iam", self.get_api_base_url(), bucket_name);
        let builder = self.http.get(url);
        auth_send_json(
            builder,
            &self.auth().await?,
            "GetBucketIamPolicy",
            &bucket_name,
            "GCS",
        )
        .await
    }

    /// Replace the IAM policy of `bucket_name`.
    async fn set_bucket_iam_policy(
        &self,
        bucket_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let url = format!("{}/b/{}/iam", self.get_api_base_url(), bucket_name);
        let builder = self.http.put(url).json(&iam_policy);
        auth_send_json(
            builder,
            &self.auth().await?,
            "SetBucketIamPolicy",
            &bucket_name,
            "GCS",
        )
        .await
    }

    // ------------------------------------------------------------
    // Object operations
    // ------------------------------------------------------------

    /// List objects inside `bucket_name` with optional filtering/pagination.
    #[allow(clippy::too_many_arguments)]
    async fn list_objects(
        &self,
        bucket_name: String,
        prefix: Option<String>,
        delimiter: Option<String>,
        page_token: Option<String>,
        max_results: Option<u32>,
        versions: Option<bool>,
    ) -> Result<ListObjectsResponse> {
        let mut url =
            Url::parse(&format!("{}/b/{}/o", self.get_api_base_url(), bucket_name)).unwrap();
        {
            let mut qp = url.query_pairs_mut();
            if let Some(v) = prefix {
                qp.append_pair("prefix", v.as_str());
            }
            if let Some(v) = delimiter {
                qp.append_pair("delimiter", v.as_str());
            }
            if let Some(v) = page_token {
                qp.append_pair("pageToken", v.as_str());
            }
            if let Some(v) = max_results {
                qp.append_pair("maxResults", &v.to_string());
            }
            if let Some(v) = versions {
                qp.append_pair("versions", &v.to_string());
            }
        }

        let builder = self.http.get(url);
        auth_send_json(
            builder,
            &self.auth().await?,
            "ListObjects",
            &bucket_name,
            "GCS",
        )
        .await
    }

    /// Permanently delete an object (optionally a specific generation).
    async fn delete_object(
        &self,
        bucket_name: String,
        object_name: String,
        generation: Option<i64>,
    ) -> Result<()> {
        let encoded_object_name = urlencoding::encode(object_name.as_str());
        let mut url = Url::parse(&format!(
            "{}/b/{}/o/{}",
            self.get_api_base_url(),
            bucket_name,
            encoded_object_name
        ))
        .unwrap();

        if let Some(g) = generation {
            url.query_pairs_mut()
                .append_pair("generation", &g.to_string());
        }

        let builder = self.http.delete(url);
        auth_send_no_response(
            builder,
            &self.auth().await?,
            "DeleteObject",
            &object_name,
            "GCS",
        )
        .await
    }

    /// Simple media upload (small object, single request).
    async fn insert_object(
        &self,
        bucket_name: String,
        object_resource: Object,
        object_data: Vec<u8>,
    ) -> Result<Object> {
        let object_name = object_resource.name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::InvalidInput {
                message: "Object name is required for insert_object operation".to_string(),
                field_name: Some("name".to_string()),
            })
        })?;

        let upload_url = format!(
            "{}/b/{}/o?uploadType=media&name={}",
            self.get_upload_base_url(),
            bucket_name,
            urlencoding::encode(object_name)
        );

        let content_type = object_resource
            .content_type
            .as_deref()
            .unwrap_or("application/octet-stream");

        let builder = self
            .http
            .post(upload_url)
            .header("content-type", content_type)
            .body(object_data)
            // The metadata is passed as JSON query params – GCS ignores body JSON in simple upload.
            ;

        // Successful upload returns the Object metadata as JSON.
        auth_send_json(
            builder,
            &self.auth().await?,
            "InsertObject",
            &object_name,
            "GCS",
        )
        .await
    }

    /// Empty a bucket by deleting all objects (including all versions if versioned).
    /// Treats RemoteResourceNotFound as success to make deletion idempotent.
    async fn empty_bucket(&self, bucket_name: String) -> Result<()> {
        let mut page_token: Option<String> = None;

        loop {
            // ------------------------------------------------------------------
            // 1. List up to 1000 objects (including all generations) in the bucket
            // ------------------------------------------------------------------
            let list_response = match self
                .list_objects(
                    bucket_name.clone(),
                    None, // prefix
                    None, // delimiter
                    page_token.clone(),
                    Some(1000), // max_results
                    Some(true), // versions – include *all* generations
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    // If the bucket does not exist anymore – treat as success (idempotent)
                    if matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
                        return Ok(());
                    }
                    return Err(e);
                }
            };

            // ------------------------------------------------------------------
            // 2. Delete every object returned in this page
            // ------------------------------------------------------------------
            for object in &list_response.items {
                if let Some(object_name) = &object.name {
                    let generation = object
                        .generation
                        .as_ref()
                        .and_then(|g| g.parse::<i64>().ok());

                    if let Err(e) = self
                        .delete_object(bucket_name.clone(), object_name.clone(), generation)
                        .await
                    {
                        // Ignore "not found" errors to keep operation idempotent
                        if !matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
                            return Err(e);
                        }
                    }
                }
            }

            // ------------------------------------------------------------------
            // 3. Decide whether to continue or finish based on pagination token
            // ------------------------------------------------------------------
            if let Some(token) = list_response.next_page_token {
                page_token = Some(token);
            } else {
                break; // Completed – no more pages
            }
        }

        Ok(())
    }

    async fn insert_notification(
        &self,
        bucket_name: String,
        notification: GcsNotification,
    ) -> Result<GcsNotification> {
        let url = format!(
            "{}/b/{}/notificationConfigs",
            self.get_api_base_url(),
            bucket_name
        );
        let builder = self.http.post(url).json(&notification);
        auth_send_json(
            builder,
            &self.auth().await?,
            "InsertNotification",
            &bucket_name,
            "GCS",
        )
        .await
    }

    async fn delete_notification(
        &self,
        bucket_name: String,
        notification_id: String,
    ) -> Result<()> {
        let url = format!(
            "{}/b/{}/notificationConfigs/{}",
            self.get_api_base_url(),
            bucket_name,
            notification_id
        );
        let builder = self.http.delete(url);
        auth_send_no_response(
            builder,
            &self.auth().await?,
            "DeleteNotification",
            &bucket_name,
            "GCS",
        )
        .await
    }
}

// --- Data Structures ---

/// Represents a Bucket resource in Google Cloud Storage.
/// Fields are based on the JSON API documentation for Buckets.
/// https://cloud.google.com/storage/docs/json_api/v1/buckets#resource
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // Typically "storage#bucket"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_created: Option<String>, // RFC 3339 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>, // RFC 3339 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metageneration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_event_based_hold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_policy: Option<RetentionPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iam_configuration: Option<IamConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versioning: Option<Versioning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<Website>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<Lifecycle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,
    // ... other fields can be added as needed based on the API docs ...
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_period: Option<String>, // u64 as string in API
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct IamConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniform_bucket_level_access: Option<UniformBucketLevelAccess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_access_prevention: Option<String>, // "enforced" or "inherited"
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UniformBucketLevelAccess {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked_time: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Versioning {
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Website {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_page_suffix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_found_page: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Logging {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_bucket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_object_prefix: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Lifecycle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<Vec<LifecycleRule>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<LifecycleAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<LifecycleCondition>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleAction {
    #[serde(rename = "type")]
    pub action_type: String, // e.g., "Delete", "SetStorageClass"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_class: Option<String>, // if action_type is SetStorageClass
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleCondition {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<i32>, // Age in days
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_before: Option<String>, // Date string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_live: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_newer_versions: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matches_storage_class: Option<Vec<String>>,
    // ... other conditions
}

/// Represents an Object resource in Google Cloud Storage.
/// https://cloud.google.com/storage/docs/json_api/v1/objects#resource
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // Typically "storage#object"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metageneration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_created: Option<String>, // RFC 3339 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>, // RFC 3339 format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_deleted: Option<String>, // RFC 3339 format, for soft-deleted objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>, // Represented as string in JSON, can be parsed to u64
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_encoding: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_disposition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crc32c: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
    // customer_encryption, kms_key_name, etc. can be added if needed
}

/// Response for listing objects.
/// https://cloud.google.com/storage/docs/json_api/v1/objects/list#response
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ListObjectsResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // Typically "storage#objects"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prefixes: Vec<String>,
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Object>,
}

/// GCS Pub/Sub notification configuration.
/// See: https://cloud.google.com/storage/docs/json_api/v1/notifications
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GcsNotification {
    /// Server-assigned notification ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The Pub/Sub topic to publish to (format: `projects/{project}/topics/{topic}`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Event types to trigger on (e.g., "OBJECT_FINALIZE", "OBJECT_DELETE")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_types: Vec<String>,
    /// Payload format: "JSON_API_V1" or "NONE"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_format: Option<String>,
    /// Optional object name prefix filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_name_prefix: Option<String>,
    /// Custom attributes to attach to each notification message
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_attributes: HashMap<String, String>,
}
