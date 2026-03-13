//! Client configuration structures for different cloud platforms
//!
//! This module contains the configuration structs for all supported cloud platforms.
//! These structs define the authentication and platform-specific settings needed
//! to connect to cloud services, but do not contain implementation logic (which
//! remains in the respective client crates).

use crate::Platform;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Service endpoint overrides for testing AWS services
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsServiceOverrides {
    /// Override endpoints for specific AWS services
    /// Key is the service name (e.g., "lambda", "s3"), value is the base URL
    pub endpoints: HashMap<String, String>,
}

/// Configuration for AWS role impersonation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsImpersonationConfig {
    /// The ARN of the role to assume
    pub role_arn: String,
    /// Optional session name for the assumed role session
    pub session_name: Option<String>,
    /// Optional duration for the assumed role credentials (in seconds)
    pub duration_seconds: Option<i32>,
    /// Optional external ID for the assume role operation
    pub external_id: Option<String>,
}

/// Configuration for AWS Web Identity Token authentication
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWebIdentityConfig {
    /// The ARN of the role to assume
    pub role_arn: String,
    /// Optional session name for the assumed role session
    pub session_name: Option<String>,
    /// The path to the web identity token file
    pub web_identity_token_file: String,
    /// Optional duration for the assumed role credentials (in seconds)
    pub duration_seconds: Option<i32>,
}

/// Supported AWS authentication methods
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AwsCredentials {
    /// Direct access keys
    AccessKeys {
        /// AWS Access Key ID
        access_key_id: String,
        /// AWS Secret Access Key
        secret_access_key: String,
        /// Optional AWS Session Token
        session_token: Option<String>,
    },
    /// Web Identity Token for OIDC authentication
    WebIdentity {
        /// Web identity configuration
        config: AwsWebIdentityConfig,
    },
}

/// AWS client configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsClientConfig {
    /// The AWS Account ID.
    pub account_id: String,
    /// The AWS region.
    pub region: String,
    /// AWS authentication credentials.
    pub credentials: AwsCredentials,
    /// Service endpoint overrides for testing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_overrides: Option<AwsServiceOverrides>,
}

/// Service endpoint overrides for testing GCP services
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpServiceOverrides {
    /// Override endpoints for specific GCP services
    /// Key is the service name (e.g., "cloudrun", "storage"), value is the base URL
    pub endpoints: HashMap<String, String>,
}

/// Authentication options for talking to GCP APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum GcpCredentials {
    /// Use an already-minted OAuth2 access token.
    AccessToken { token: String },

    /// Use a full Service Account JSON key (as string). A short-lived JWT will
    /// be created and exchanged for a bearer token automatically.
    ServiceAccountKey { json: String },

    /// Use GCP metadata server for authentication (for instances running on GCP)
    ServiceMetadata,

    /// Use projected service account token (for Kubernetes workload identity)
    ProjectedServiceAccount {
        /// Path to the projected service account token
        token_file: String,
        /// Service account email
        service_account_email: String,
    },

    /// Use gcloud Application Default Credentials (authorized_user).
    /// Exchanges refresh_token for an access_token via Google's OAuth2 endpoint.
    AuthorizedUser {
        /// OAuth2 client ID
        client_id: String,
        /// OAuth2 client secret
        client_secret: String,
        /// OAuth2 refresh token
        refresh_token: String,
    },
}

/// Configuration for GCP service account impersonation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpImpersonationConfig {
    /// The email of the service account to impersonate
    pub service_account_email: String,
    /// The OAuth 2.0 scopes that define the access token's permissions
    pub scopes: Vec<String>,
    /// Optional sequence of service accounts in a delegation chain
    pub delegates: Option<Vec<String>>,
    /// Optional desired lifetime duration of the access token (max 3600s)
    pub lifetime: Option<String>,
}

impl Default for GcpImpersonationConfig {
    fn default() -> Self {
        Self {
            service_account_email: String::new(),
            scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
            delegates: None,
            lifetime: Some("3600s".to_string()),
        }
    }
}

/// GCP client configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpClientConfig {
    /// The GCP Project ID.
    pub project_id: String,
    /// The GCP region for resources.
    pub region: String,
    /// GCP authentication credentials.
    pub credentials: GcpCredentials,
    /// Service endpoint overrides for testing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_overrides: Option<GcpServiceOverrides>,
}

/// Service endpoint overrides for testing Azure services
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureServiceOverrides {
    /// Override endpoints for specific Azure services
    /// Key is the service name (e.g., "management", "storage", "containerApps"), value is the base URL
    pub endpoints: HashMap<String, String>,
}

/// Represents Azure authentication credentials
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AzureCredentials {
    /// Service principal with client secret
    ServicePrincipal {
        /// The client ID (application ID)
        client_id: String,
        /// The client secret
        client_secret: String,
    },
    /// Direct access token
    AccessToken {
        /// The bearer token to use for authentication
        token: String,
    },
    /// Azure AD Workload Identity (federated identity)
    WorkloadIdentity {
        /// The client ID of the managed identity or application
        client_id: String,
        /// The tenant ID for authentication
        tenant_id: String,
        /// Path to the federated token file
        federated_token_file: String,
        /// The authority host URL
        authority_host: String,
    },
}

/// Configuration for Azure managed identity impersonation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureImpersonationConfig {
    /// The client ID of the managed identity or service principal to impersonate
    pub client_id: String,
    /// The scope for the access token (e.g., "https://management.azure.com/.default")
    pub scope: String,
    /// Optional tenant ID for cross-tenant impersonation
    pub tenant_id: Option<String>,
}

impl Default for AzureImpersonationConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            scope: "https://management.azure.com/.default".to_string(),
            tenant_id: None,
        }
    }
}

/// Azure client configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AzureClientConfig {
    /// The Azure Subscription ID where resources will be deployed.
    pub subscription_id: String,
    /// The customer's Azure Tenant ID.
    pub tenant_id: String,
    /// Azure region for resources.
    pub region: Option<String>,
    /// Azure authentication credentials.
    pub credentials: AzureCredentials,
    /// Service endpoint overrides for testing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_overrides: Option<AzureServiceOverrides>,
}

/// Configuration mode for Kubernetes access
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum KubernetesClientConfig {
    /// Use in-cluster configuration (service account tokens, etc.)
    InCluster {
        /// The namespace to operate in
        #[serde(skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        /// Additional headers to include in requests
        #[serde(skip_serializing_if = "Option::is_none")]
        additional_headers: Option<HashMap<String, String>>,
    },
    /// Use kubeconfig file for configuration
    Kubeconfig {
        /// Path to kubeconfig file (optional, defaults to standard locations)
        #[serde(skip_serializing_if = "Option::is_none")]
        kubeconfig_path: Option<String>,
        /// Context name to use (optional, defaults to current-context)
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<String>,
        /// Cluster name to use (optional, defaults to context's cluster)
        #[serde(skip_serializing_if = "Option::is_none")]
        cluster: Option<String>,
        /// User name to use (optional, defaults to context's user)
        #[serde(skip_serializing_if = "Option::is_none")]
        user: Option<String>,
        /// The namespace to operate in
        #[serde(skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        /// Additional headers to include in requests
        #[serde(skip_serializing_if = "Option::is_none")]
        additional_headers: Option<HashMap<String, String>>,
    },
    /// Manual configuration with explicit values
    Manual {
        /// The Kubernetes cluster server URL
        server_url: String,
        /// The cluster certificate authority data (base64 encoded)
        certificate_authority_data: Option<String>,
        /// Skip TLS verification (insecure)
        insecure_skip_tls_verify: Option<bool>,
        /// Client certificate data (base64 encoded) for mutual TLS
        client_certificate_data: Option<String>,
        /// Client key data (base64 encoded) for mutual TLS
        client_key_data: Option<String>,
        /// Bearer token for authentication
        token: Option<String>,
        /// Username for basic authentication
        username: Option<String>,
        /// Password for basic authentication
        password: Option<String>,
        /// The namespace to operate in
        namespace: Option<String>,
        /// Additional headers to include in requests
        additional_headers: HashMap<String, String>,
    },
}

/// Cloud-agnostic impersonation configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "platform")]
pub enum ImpersonationConfig {
    Aws(AwsImpersonationConfig),
    Gcp(GcpImpersonationConfig),
    Azure(AzureImpersonationConfig),
    // Kubernetes doesn't support impersonation, so we don't include it here
}

/// Configuration for different cloud platform clients
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", tag = "platform")]
pub enum ClientConfig {
    Aws(Box<AwsClientConfig>),
    Gcp(Box<GcpClientConfig>),
    Azure(Box<AzureClientConfig>),
    Kubernetes(Box<KubernetesClientConfig>),
    Local {
        /// State directory for local resources and deployment state
        state_directory: String,
        /// Optional artifact registry configuration for pulling container images.
        /// When present, the local platform will fetch credentials from the agent manager
        /// before pulling images, enabling centralized registry access control.
        #[serde(skip_serializing_if = "Option::is_none")]
        artifact_registry_config: Option<crate::ArtifactRegistryConfig>,
    },
    /// Test platform - uses mock controllers without real cloud APIs
    #[serde(skip)]
    Test,
}

impl ClientConfig {
    /// Returns the platform enum for this configuration.
    pub fn platform(&self) -> Platform {
        match self {
            ClientConfig::Aws(_) => Platform::Aws,
            ClientConfig::Gcp(_) => Platform::Gcp,
            ClientConfig::Azure(_) => Platform::Azure,
            ClientConfig::Kubernetes(_) => Platform::Kubernetes,
            ClientConfig::Local { .. } => Platform::Local,
            ClientConfig::Test => Platform::Test,
        }
    }

    /// Returns the AWS configuration if this is an AWS client config.
    pub fn aws_config(&self) -> Option<&AwsClientConfig> {
        match self {
            ClientConfig::Aws(config) => Some(config),
            _ => None,
        }
    }

    /// Returns the GCP configuration if this is a GCP client config.
    pub fn gcp_config(&self) -> Option<&GcpClientConfig> {
        match self {
            ClientConfig::Gcp(config) => Some(config),
            _ => None,
        }
    }

    /// Returns the Azure configuration if this is an Azure client config.
    pub fn azure_config(&self) -> Option<&AzureClientConfig> {
        match self {
            ClientConfig::Azure(config) => Some(config),
            _ => None,
        }
    }

    /// Returns the Kubernetes configuration if this is a Kubernetes client config.
    pub fn kubernetes_config(&self) -> Option<&KubernetesClientConfig> {
        match self {
            ClientConfig::Kubernetes(config) => Some(config),
            _ => None,
        }
    }
}
