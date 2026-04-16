use alien_core::ComputeType;
use alien_sdk::AlienContext;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use utoipa::ToSchema;

/// Shared application state for the test server.
#[derive(Clone)]
pub struct AppState {
    /// The Alien context that provides both bindings and wait_until functionality
    pub ctx: Arc<AlienContext>,
}

// Health check models

/// Health check response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    /// Status of the service
    pub status: String,
    /// Timestamp of the health check
    pub timestamp: DateTime<Utc>,
}

// Environment variable models

/// Environment variable response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnvVarResponse {
    /// Whether the operation was successful
    pub success: bool,
    /// Name of the environment variable
    pub name: String,
    /// Value of the environment variable (if found)
    pub value: Option<String>,
    /// Error message (if not found)
    pub error: Option<String>,
}

// Inspect request models

/// Response from inspect endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectResponse {
    /// Whether the operation was successful
    pub success: bool,
    /// The request body that was received
    pub request_body: serde_json::Value,
}

/// Response from queue message retrieval handler
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueMessageRetrievalResponse {
    /// Whether the operation was successful
    pub success: bool,
    /// KV binding name used for retrieval
    pub kv_binding_name: String,
    /// Number of messages retrieved
    pub retrieved_count: usize,
    /// The messages that were retrieved from KV storage
    pub messages: Vec<serde_json::Value>,
}

// Storage test models

/// Response from storage test endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageTestResponse {
    /// Name of the binding being tested
    pub binding_name: String,
    /// Whether the test was successful
    pub success: bool,
}

// Vault test models

/// Response from vault test endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VaultTestResponse {
    /// Name of the binding being tested
    pub binding_name: String,
    /// Whether the test was successful
    pub success: bool,
}

// KV test models

/// Response from KV test endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KvTestResponse {
    /// Name of the binding being tested
    pub binding_name: String,
    /// Whether the test was successful
    pub success: bool,
}

// Build test models

/// Request for build test
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildTestRequest {
    /// Optional custom build configuration
    pub config: Option<BuildTestConfig>,
}

/// Build test configuration
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildTestConfig {
    /// Script to run in the build
    pub script: Option<String>,
    /// Environment variables for the build
    pub environment: Option<HashMap<String, String>>,
    /// Docker image to use for the build
    pub image: Option<String>,
    /// Timeout in seconds
    pub timeout_seconds: Option<u32>,
    /// Compute type for the build
    pub compute_type: Option<ComputeType>,
}

/// Response from build test endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildTestResponse {
    /// Name of the binding being tested
    pub binding_name: String,
    /// Build execution ID
    pub execution_id: String,
    /// Final status of the build
    pub final_status: String,
    /// Whether the test was successful
    pub success: bool,
}

// Artifact registry test models

/// Request for artifact registry test
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRegistryTestRequest {
    /// Optional custom repository name prefix
    pub repo_name_prefix: Option<String>,
    /// Whether to skip Docker operations
    pub skip_docker_operations: Option<bool>,
}

/// Response from artifact registry test endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRegistryTestResponse {
    /// Name of the binding being tested
    pub binding_name: String,
    /// Repository name that was created
    pub repo_name: String,
    /// Whether the test was successful
    pub success: bool,
}

// Service account test models

/// Response from service account test endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountTestResponse {
    /// Name of the binding being tested
    pub binding_name: String,
    /// Whether the test was successful
    pub success: bool,
    /// Service account identity info
    pub info: serde_json::Value,
}

// Queue test models

/// Response from queue test endpoint
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueTestResponse {
    /// Name of the binding being tested
    pub binding_name: String,
    /// Whether the test was successful
    pub success: bool,
}

// SSE models

/// SSE message data
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SSEMessage {
    /// Message content
    pub data: String,
    /// Message ID
    pub id: Option<String>,
    /// Event type
    pub event: Option<String>,
}
