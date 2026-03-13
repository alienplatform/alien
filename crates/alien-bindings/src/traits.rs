use crate::error::Result;
use crate::presigned::PresignedRequest;
use alien_core::{BuildConfig, BuildExecution};
use async_trait::async_trait;
use object_store::path::Path;
use object_store::ObjectStore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

#[cfg(feature = "openapi")]
use utoipa::ToSchema;

/// Marker trait for all binding types.
pub trait Binding: Send + Sync + std::fmt::Debug {}

/// A storage binding that provides object store capabilities.
#[async_trait]
pub trait Storage: Binding + ObjectStore {
    /// Gets the base directory path configured for this storage binding.
    fn get_base_dir(&self) -> Path;
    /// Gets the underlying URL configured for this storage binding.
    fn get_url(&self) -> Url;

    /// Creates a presigned request for uploading data to the specified path.
    /// The request can be serialized, stored, and executed later.
    async fn presigned_put(&self, path: &Path, expires_in: Duration) -> Result<PresignedRequest>;

    /// Creates a presigned request for downloading data from the specified path.
    /// The request can be serialized, stored, and executed later.
    async fn presigned_get(&self, path: &Path, expires_in: Duration) -> Result<PresignedRequest>;

    /// Creates a presigned request for deleting the object at the specified path.
    /// The request can be serialized, stored, and executed later.
    async fn presigned_delete(&self, path: &Path, expires_in: Duration)
        -> Result<PresignedRequest>;
}

/// A build binding that provides build execution capabilities.
#[async_trait]
pub trait Build: Binding {
    /// Starts a new build with the given configuration.
    /// Returns the build execution information.
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution>;

    /// Gets the status of a specific build execution.
    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution>;

    /// Stops or cancels a running build.
    async fn stop_build(&self, build_id: &str) -> Result<()>;
}

/// AWS IAM Role service account information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AwsServiceAccountInfo {
    /// The IAM role name
    pub role_name: String,
    /// The IAM role ARN (for AssumeRole)
    pub role_arn: String,
}

/// GCP Service Account information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct GcpServiceAccountInfo {
    /// The service account email (for impersonation)
    pub email: String,
    /// The service account unique ID
    pub unique_id: String,
}

/// Azure User-Assigned Managed Identity information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AzureServiceAccountInfo {
    /// The managed identity client ID (for authentication)
    pub client_id: String,
    /// The managed identity resource ID (ARM ID)
    pub resource_id: String,
    /// The managed identity principal ID
    pub principal_id: String,
}

/// Platform-specific service account information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum ServiceAccountInfo {
    /// AWS IAM Role
    Aws(AwsServiceAccountInfo),
    /// GCP Service Account
    Gcp(GcpServiceAccountInfo),
    /// Azure User-Assigned Managed Identity
    Azure(AzureServiceAccountInfo),
}

/// Configuration for impersonation
#[derive(Debug, Clone)]
pub struct ImpersonationRequest {
    /// Optional session name (AWS only)
    pub session_name: Option<String>,
    /// Optional session duration in seconds  
    pub duration_seconds: Option<i32>,
    /// Optional scopes (GCP only)
    pub scopes: Option<Vec<String>>,
}

impl Default for ImpersonationRequest {
    fn default() -> Self {
        Self {
            session_name: None,
            duration_seconds: Some(3600), // 1 hour default
            scopes: None,
        }
    }
}

/// A service account binding that provides identity and impersonation capabilities.
#[async_trait]
pub trait ServiceAccount: Binding {
    /// Gets information about the service account
    async fn get_info(&self) -> Result<ServiceAccountInfo>;

    /// Impersonates the service account and returns credentials as a ClientConfig.
    ///
    /// This performs the cloud-specific impersonation:
    /// - AWS: STS AssumeRole to get temporary credentials
    /// - GCP: IAM Credentials API generateAccessToken
    /// - Azure: Uses the attached managed identity (no API call needed)
    async fn impersonate(&self, request: ImpersonationRequest) -> Result<alien_core::ClientConfig>;

    /// Helper for downcasting trait object
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Response from repository operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct RepositoryResponse {
    /// Repository name.
    pub name: String,
    /// Repository URI for pushing/pulling images. None if repository is not ready yet.
    pub uri: Option<String>,
    /// Optional creation timestamp in ISO8601 format.
    pub created_at: Option<String>,
}

/// Permissions level for artifact registry access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum ArtifactRegistryPermissions {
    /// Pull-only access (download artifacts).
    Pull,
    /// Push and pull access (upload and download artifacts).
    PushPull,
}

/// Credentials for accessing a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct ArtifactRegistryCredentials {
    /// Username for authentication.
    pub username: String,
    /// Password or token for authentication.
    pub password: String,
    /// Optional expiration time in ISO8601 format.
    pub expires_at: Option<String>,
}

/// Types of compute services that can access artifact registries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum ComputeServiceType {
    /// Serverless functions
    Function,
    // In the future, we could add Container, VirtualMachine, Kubernetes, etc.
}

/// Cross-account access configuration for AWS artifact registries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct AwsCrossAccountAccess {
    /// AWS account IDs that should have cross-account access.
    pub account_ids: Vec<String>,
    /// Types of compute services that should have access.
    pub allowed_service_types: Vec<ComputeServiceType>,
    /// Specific IAM role ARNs to grant access to.
    /// These are typically deployment/management roles or service-specific roles.
    pub role_arns: Vec<String>,
}

/// Cross-account access configuration for GCP artifact registries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct GcpCrossAccountAccess {
    /// GCP project numbers that should have access.
    pub project_numbers: Vec<String>,
    /// Types of compute services that should have access.
    pub allowed_service_types: Vec<ComputeServiceType>,
    /// Additional service account emails to grant access to.
    /// These are typically deployment/management service accounts.
    pub service_account_emails: Vec<String>,
}

/// Platform-specific cross-account access configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum CrossAccountAccess {
    /// AWS-specific cross-account access configuration.
    Aws(AwsCrossAccountAccess),
    /// GCP-specific cross-account access configuration.
    Gcp(GcpCrossAccountAccess),
}

/// Current cross-account access permissions for a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct CrossAccountPermissions {
    /// Platform-specific access configuration currently applied.
    pub access: CrossAccountAccess,
    /// Timestamp when permissions were last updated.
    pub last_updated: Option<String>,
}

/// A trait for artifact registry bindings that provide container image repository management.
#[async_trait]
pub trait ArtifactRegistry: Binding {
    /// Creates a repository within the artifact registry.
    /// Returns the repository details. URI will be None if repository is still being created.
    async fn create_repository(&self, repo_name: &str) -> Result<RepositoryResponse>;

    /// Gets repository details including name, URI, and creation time.
    async fn get_repository(&self, repo_id: &str) -> Result<RepositoryResponse>;

    /// Adds cross-account access permissions for a repository.
    /// This adds the specified permissions to any existing cross-account permissions.
    ///
    /// For AWS: Grants access to specified account IDs with configurable principals and compute service types.
    /// For GCP: Grants access to serverless robots and service accounts based on compute service types.
    /// For Azure: Not supported - returns OperationNotSupported error.
    async fn add_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()>;

    /// Removes cross-account access permissions for a repository.
    /// This removes the specified permissions from existing cross-account permissions.
    ///
    /// For AWS: Removes access for specified account IDs and compute service types.
    /// For GCP: Removes access for specified project numbers and service accounts.
    /// For Azure: Not supported - returns OperationNotSupported error.
    async fn remove_cross_account_access(
        &self,
        repo_id: &str,
        access: CrossAccountAccess,
    ) -> Result<()>;

    /// Gets the current cross-account access permissions for a repository.
    /// For Azure: Not supported - returns OperationNotSupported error.
    async fn get_cross_account_access(&self, repo_id: &str) -> Result<CrossAccountPermissions>;

    /// Generates credentials for accessing a repository with specified permissions.
    /// On AWS: assumes the relevant role and calls get_authorization_token.
    /// On GCP: impersonates the relevant service account and gets an oauth token.
    /// On Azure: uses the built-in token mechanism.
    async fn generate_credentials(
        &self,
        repo_id: &str,
        permissions: ArtifactRegistryPermissions,
        ttl_seconds: Option<u32>,
    ) -> Result<ArtifactRegistryCredentials>;

    /// Deletes a repository and all contained images.
    async fn delete_repository(&self, repo_id: &str) -> Result<()>;
}

/// A trait for vault bindings that provide secure secret management.
#[async_trait]
pub trait Vault: Binding {
    /// Gets a secret value by name.
    async fn get_secret(&self, secret_name: &str) -> Result<String>;

    /// Sets a secret value, creating it if it doesn't exist or updating it if it does.
    async fn set_secret(&self, secret_name: &str, value: &str) -> Result<()>;

    /// Deletes a secret by name.
    async fn delete_secret(&self, secret_name: &str) -> Result<()>;
}

/// Represents options for put operations in KV stores.
#[derive(Debug, Clone, Default)]
pub struct PutOptions {
    /// Optional TTL for automatic expiration (soft hint - items MAY be deleted after expiry)
    pub ttl: Option<Duration>,
    /// Only put if the key does not exist
    pub if_not_exists: bool,
}

/// Represents the result of a scan operation.
#[derive(Debug)]
pub struct ScanResult {
    /// Key-value pairs found (may be ≤ limit, no guarantee to fill)
    pub items: Vec<(String, Vec<u8>)>,
    /// Opaque cursor for pagination. None if no more results.
    /// **Warning**: Cursor may become invalid if data changes. No TTL guarantees.
    pub next_cursor: Option<String>,
}

/// A trait for key-value store bindings that provide minimal, platform-agnostic KV operations.
/// This API is designed to work consistently across DynamoDB, Firestore, Redis, and Azure Table Storage.
#[async_trait]
pub trait Kv: Binding {
    /// Get a value by key. Returns None if key doesn't exist or has expired.
    ///
    /// **TTL Behavior**: TTL is a soft hint for automatic cleanup. If `now >= expires_at`,
    /// implementations SHOULD behave as if the key is absent, even if the item still exists
    /// physically in the backend. Physical deletion is eventual and not guaranteed.
    ///
    /// **Validation**: Keys are validated against MAX_KEY_BYTES and portable charset.
    /// Invalid keys return `KvError::InvalidKey` immediately.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Put a value with optional options. When options.if_not_exists is true, returns true if created,
    /// false if already exists. When options.if_not_exists is false or options is None, always returns true.
    ///
    /// **Size Limits**:
    /// - Keys: ≤ MAX_KEY_BYTES (512 bytes) with portable ASCII charset
    /// - Values: ≤ MAX_VALUE_BYTES (24,576 bytes = 24 KiB)
    ///
    /// **Validation**: Size and charset constraints are enforced before backend calls.
    /// Invalid inputs return `KvError::InvalidKey` or `KvError::InvalidValue` immediately.
    ///
    /// **TTL Behavior**: TTL is a soft hint for automatic cleanup. If TTL is specified,
    /// item expires at `put_time + ttl`. Expired items SHOULD appear absent on subsequent
    /// reads, but physical deletion is eventual and not guaranteed.
    ///
    /// **Conditional Logic**: The if_not_exists operation maps to backend primitives:
    /// - Redis: SETNX
    /// - DynamoDB: PutItem with condition_expression="attribute_not_exists(pk)"
    /// - Firestore: create() with Precondition::DoesNotExist
    /// - Azure Table Storage: InsertEntity (409 on conflict)
    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool>;

    /// Delete a key. No error if key doesn't exist.
    ///
    /// **Validation**: Keys are validated against MAX_KEY_BYTES and portable charset.
    /// Invalid keys return `KvError::InvalidKey` immediately.
    async fn delete(&self, key: &str) -> Result<()>;

    /// Check if a key exists without retrieving the value.
    ///
    /// **TTL Behavior**: TTL is a soft hint for automatic cleanup. If `now >= expires_at`,
    /// SHOULD return false even if physically present. Physical deletion is eventual and not guaranteed.
    ///
    /// **Validation**: Keys are validated against MAX_KEY_BYTES and portable charset.
    /// Invalid keys return `KvError::InvalidKey` immediately.
    async fn exists(&self, key: &str) -> Result<bool>;

    /// Scan keys with a prefix, with pagination support.
    ///
    /// **Scan Contract**:
    /// - Returns an **arbitrary, unordered subset** in backend-natural order
    /// - **No ordering guarantees** across backends (Redis SCAN, Azure fan-out, etc.)
    /// - **May return ≤ limit items** (not guaranteed to fill even if more data exists)
    /// - **Clients MUST de-duplicate** keys across pages (backends may return duplicates)
    /// - **No completeness guarantee** under concurrent writes (may miss or duplicate)
    ///
    /// **Cursor Behavior**:
    /// - Opaque string, implementation-specific format
    /// - **May become invalid** anytime after backend state changes
    /// - **No TTL guarantees** - can expire without notice
    /// - Passing invalid cursor should return error, not partial results
    ///
    /// **TTL Behavior**: TTL is a soft hint for automatic cleanup. Expired items SHOULD
    /// be filtered out from results, but physical deletion is eventual and not guaranteed.
    ///
    /// **Validation**: Prefix follows same key validation rules.
    /// Invalid prefix returns `KvError::InvalidKey` immediately.
    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult>;
}

/// JSON/Text message payload for Queue
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub enum MessagePayload {
    /// JSON-serializable value
    Json(serde_json::Value),
    /// UTF-8 text payload
    Text(String),
}

/// A queue message with payload and receipt handle for acknowledgment
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct QueueMessage {
    /// JSON-first message payload
    pub payload: MessagePayload,
    /// Opaque receipt handle for acknowledgment (backend-specific, short-lived)
    pub receipt_handle: String,
}

/// Maximum message size in bytes (64 KiB = 65,536 bytes)
///
/// This limit ensures compatibility across all queue backends:
/// - **AWS SQS**: 256KB message limit (much higher, not constraining)
/// - **Azure Service Bus**: 1MB message limit (much higher, not constraining)  
/// - **GCP Pub/Sub**: 10MB message limit (much higher, not constraining)
///
/// The 64KB limit provides:
/// - Reasonable message sizes for most use cases
/// - Fast network transfer and low latency
/// - Consistent behavior across all cloud providers
/// - Efficient memory usage during batch processing
pub const MAX_MESSAGE_BYTES: usize = 65_536; // 64 KiB

/// Maximum number of messages per receive call
///
/// This limit balances throughput with processing simplicity:
/// - **AWS SQS**: Supports up to 10 messages per ReceiveMessage call
/// - **Azure Service Bus**: Can receive multiple messages via prefetch/batching
/// - **GCP Pub/Sub**: Supports configurable max_messages per Pull request
///
/// The 10-message limit ensures:
/// - Portable batch sizes across all backends
/// - Manageable memory usage
/// - Reasonable processing latency per batch
pub const MAX_BATCH_SIZE: usize = 10;

/// Fixed lease duration in seconds
///
/// Messages are leased for exactly 30 seconds after delivery:
/// - Long enough for most processing tasks
/// - Short enough to enable fast retry on failures
/// - Eliminates complexity of dynamic lease management
/// - Consistent across all platforms
pub const LEASE_SECONDS: u64 = 30;

/// A trait for queue bindings providing minimal, portable queue operations.
#[async_trait]
pub trait Queue: Binding {
    /// Send a message to the specified queue
    async fn send(&self, queue: &str, message: MessagePayload) -> Result<()>;

    /// Receive up to `max_messages` (1..=10) from the specified queue
    async fn receive(&self, queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>>;

    /// Acknowledge a message using its receipt handle (idempotent)
    async fn ack(&self, queue: &str, receipt_handle: &str) -> Result<()>;
}

/// Request for invoking a function directly
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FunctionInvokeRequest {
    /// Function identifier (name, ARN, URL, etc.)
    pub target_function: String,
    /// HTTP method
    pub method: String,
    /// Request path
    pub path: String,
    /// HTTP headers
    pub headers: BTreeMap<String, String>,
    /// Request body bytes
    pub body: Vec<u8>,
    /// Optional timeout for the invocation
    pub timeout: Option<Duration>,
}

/// Response from function invocation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
pub struct FunctionInvokeResponse {
    /// HTTP status code
    pub status: u16,
    /// HTTP response headers
    pub headers: BTreeMap<String, String>,
    /// Response body bytes
    pub body: Vec<u8>,
}

/// A trait for function bindings that enable direct function-to-function calls
#[async_trait]
pub trait Function: Binding {
    /// Invoke a function with HTTP request data.
    ///
    /// This enables direct, low-latency function-to-function communication within
    /// the same cloud environment, bypassing ARC for internal calls.
    ///
    /// Platform implementations:
    /// - AWS: Uses InvokeFunction API directly
    /// - GCP: Calls private service URL directly  
    /// - Azure: Calls private container app URL directly
    /// - Kubernetes: HTTP call to internal service
    async fn invoke(&self, request: FunctionInvokeRequest) -> Result<FunctionInvokeResponse>;

    /// Get the public URL of the function, if available.
    ///
    /// Returns the function's public URL if it exists and is accessible.
    /// This is useful for exposing public endpoints or getting URLs for
    /// external integration.
    ///
    /// Platform implementations:
    /// - AWS: Uses GetFunctionUrlConfig API or returns URL from binding
    /// - GCP: Returns Cloud Run service URL or calls get_service API
    /// - Azure: Returns Container App URL or calls get_container_app API
    async fn get_function_url(&self) -> Result<Option<String>>;

    /// Get a reference to this object as `Any` for dynamic casting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// A trait for container bindings that enable container-to-container communication
#[async_trait]
pub trait Container: Binding {
    /// Get the internal URL for container-to-container communication.
    ///
    /// This returns the internal service discovery URL that other containers
    /// in the same network can use to communicate with this container.
    ///
    /// Platform implementations:
    /// - Horizon (AWS/GCP/Azure): Returns internal DNS URL (e.g., "http://api.svc:8080")
    /// - Local (Docker): Returns Docker network DNS URL (e.g., "http://api.svc:3000")
    fn get_internal_url(&self) -> &str;

    /// Get the public URL of the container, if available.
    ///
    /// Returns the container's public URL if it exists and is accessible
    /// from outside the cluster/network.
    ///
    /// Platform implementations:
    /// - Horizon: Returns load balancer URL if exposed publicly
    /// - Local: Returns localhost URL with mapped port (e.g., "http://localhost:62844")
    fn get_public_url(&self) -> Option<&str>;

    /// Get the container name/ID.
    fn get_container_name(&self) -> &str;

    /// Get a reference to this object as `Any` for dynamic casting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// A provider must implement methods to load the various types of bindings
/// based on environment variables or other configuration sources.
#[async_trait]
pub trait BindingsProviderApi: Send + Sync + std::fmt::Debug {
    /// Given a binding identifier, builds a Storage implementation.
    async fn load_storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>>;

    /// Given a binding identifier, builds a Build implementation.
    async fn load_build(&self, binding_name: &str) -> Result<Arc<dyn Build>>;

    /// Given a binding identifier, builds an ArtifactRegistry implementation.
    async fn load_artifact_registry(&self, binding_name: &str)
        -> Result<Arc<dyn ArtifactRegistry>>;

    /// Given a binding identifier, builds a Vault implementation.
    async fn load_vault(&self, binding_name: &str) -> Result<Arc<dyn Vault>>;

    /// Given a binding identifier, builds a KV implementation.
    async fn load_kv(&self, binding_name: &str) -> Result<Arc<dyn Kv>>;

    /// Given a binding identifier, builds a Queue implementation.
    async fn load_queue(&self, binding_name: &str) -> Result<Arc<dyn Queue>>;

    /// Given a binding identifier, builds a Function implementation.
    async fn load_function(&self, binding_name: &str) -> Result<Arc<dyn Function>>;

    /// Given a binding identifier, builds a Container implementation.
    async fn load_container(&self, binding_name: &str) -> Result<Arc<dyn Container>>;

    /// Given a binding identifier, builds a ServiceAccount implementation.
    async fn load_service_account(&self, binding_name: &str) -> Result<Arc<dyn ServiceAccount>>;
}
