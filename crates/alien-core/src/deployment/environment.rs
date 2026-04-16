//! Environment info per cloud platform, environment variables, and snapshots.

use crate::Platform;
use serde::{Deserialize, Serialize};

/// AWS-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsEnvironmentInfo {
    /// AWS account ID
    pub account_id: String,
    /// AWS region
    pub region: String,
}

/// GCP-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpEnvironmentInfo {
    /// GCP project number (e.g., "123456789012")
    pub project_number: String,
    /// GCP project ID (e.g., "my-project")
    pub project_id: String,
    /// GCP region
    pub region: String,
}

/// Azure-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureEnvironmentInfo {
    /// Azure tenant ID
    pub tenant_id: String,
    /// Azure subscription ID
    pub subscription_id: String,
    /// Azure location/region
    pub location: String,
}

/// Local platform environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalEnvironmentInfo {
    /// Hostname of the machine running the deployment
    pub hostname: String,
    /// Operating system (e.g., "linux", "macos", "windows")
    pub os: String,
    /// Architecture (e.g., "x86_64", "aarch64")
    pub arch: String,
}

/// Test platform environment information (mock)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TestEnvironmentInfo {
    /// Test identifier for this environment
    pub test_id: String,
}

/// Platform-specific environment information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "platform")]
pub enum EnvironmentInfo {
    /// AWS environment information
    Aws(AwsEnvironmentInfo),
    /// GCP environment information
    Gcp(GcpEnvironmentInfo),
    /// Azure environment information
    Azure(AzureEnvironmentInfo),
    /// Local platform environment information
    Local(LocalEnvironmentInfo),
    /// Test platform environment information (mock)
    Test(TestEnvironmentInfo),
}

impl EnvironmentInfo {
    /// Get the platform for this environment info
    pub fn platform(&self) -> Platform {
        match self {
            EnvironmentInfo::Aws(_) => Platform::Aws,
            EnvironmentInfo::Gcp(_) => Platform::Gcp,
            EnvironmentInfo::Azure(_) => Platform::Azure,
            EnvironmentInfo::Local(_) => Platform::Local,
            EnvironmentInfo::Test(_) => Platform::Test,
        }
    }
}

/// Type of environment variable
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentVariableType {
    /// Plain variable (injected directly into function config)
    Plain,
    /// Secret variable (stored in vault, loaded at runtime)
    Secret,
}

/// Environment variable for deployment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    /// Variable name
    pub name: String,
    /// Variable value (decrypted - deployment has access to decryption keys)
    pub value: String,
    /// Variable type (plain or secret)
    #[serde(rename = "type")]
    pub var_type: EnvironmentVariableType,
    /// Target resource patterns (null = all resources, Some = wildcard patterns)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_resources: Option<Vec<String>>,
}

/// Snapshot of environment variables at a point in time
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariablesSnapshot {
    /// Environment variables in the snapshot
    pub variables: Vec<EnvironmentVariable>,
    /// Deterministic hash of all variables (for change detection)
    pub hash: String,
    /// ISO 8601 timestamp when snapshot was created
    pub created_at: String,
}
