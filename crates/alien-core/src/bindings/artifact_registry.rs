//! Service-type based artifact registry binding definitions

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// AWS ECR (Elastic Container Registry) binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EcrArtifactRegistryBinding {
    /// Repository prefix for this registry (used to construct ECR repository names)
    pub repository_prefix: BindingValue<String>,
    /// ARN of the IAM role for pull permissions (optional — omit for single-account)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_role_arn: Option<BindingValue<String>>,
    /// ARN of the IAM role for push+pull permissions (optional — omit for single-account)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_role_arn: Option<BindingValue<String>>,
}

/// Azure Container Registry binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AcrArtifactRegistryBinding {
    /// Registry name (e.g., "myregistry") - endpoint is derived from this
    pub registry_name: BindingValue<String>,
    /// Resource group name where the registry is located
    pub resource_group_name: BindingValue<String>,
    /// Repository prefix for this registry (used for proxy routing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_prefix: Option<BindingValue<String>>,
}

/// Google Artifact Registry binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GarArtifactRegistryBinding {
    /// Repository name in the Artifact Registry (e.g., "alien-test").
    /// Used as the default when cross-account access methods are called
    /// without a specific repo name.
    pub repository_name: BindingValue<String>,
    /// Optional service account email for pull permissions (omit for single-project)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_service_account_email: Option<BindingValue<String>>,
    /// Optional service account email for push+pull permissions (omit for single-project)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_service_account_email: Option<BindingValue<String>>,
}

/// Local container registry binding configuration.
///
/// The local registry runs on localhost only and does not require authentication.
/// Security boundary is the OS process isolation on the customer's machine.
/// External image access is secured by the manager's registry proxy (deployment tokens).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalArtifactRegistryBinding {
    /// The registry URL endpoint (e.g., "localhost:5000")
    pub registry_url: BindingValue<String>,
    /// Optional base directory for registry data
    pub data_dir: BindingValue<Option<String>>,
}

/// Service-type based artifact registry binding that supports multiple registry providers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum ArtifactRegistryBinding {
    /// AWS ECR (Elastic Container Registry)
    Ecr(EcrArtifactRegistryBinding),
    /// Azure Container Registry
    Acr(AcrArtifactRegistryBinding),
    /// Google Artifact Registry
    Gar(GarArtifactRegistryBinding),
    /// Local container registry
    Local(LocalArtifactRegistryBinding),
}

impl ArtifactRegistryBinding {
    /// Creates an ECR artifact registry binding
    pub fn ecr(
        repository_prefix: impl Into<BindingValue<String>>,
        pull_role_arn: Option<impl Into<BindingValue<String>>>,
        push_role_arn: Option<impl Into<BindingValue<String>>>,
    ) -> Self {
        Self::Ecr(EcrArtifactRegistryBinding {
            repository_prefix: repository_prefix.into(),
            pull_role_arn: pull_role_arn.map(|v| v.into()),
            push_role_arn: push_role_arn.map(|v| v.into()),
        })
    }

    /// Creates an ACR artifact registry binding
    pub fn acr(
        registry_name: impl Into<BindingValue<String>>,
        resource_group_name: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Acr(AcrArtifactRegistryBinding {
            registry_name: registry_name.into(),
            resource_group_name: resource_group_name.into(),
            repository_prefix: None,
        })
    }

    /// Creates a GAR artifact registry binding
    pub fn gar(
        repository_name: impl Into<BindingValue<String>>,
        pull_service_account_email: Option<impl Into<BindingValue<String>>>,
        push_service_account_email: Option<impl Into<BindingValue<String>>>,
    ) -> Self {
        Self::Gar(GarArtifactRegistryBinding {
            repository_name: repository_name.into(),
            pull_service_account_email: pull_service_account_email.map(|v| v.into()),
            push_service_account_email: push_service_account_email.map(|v| v.into()),
        })
    }

    /// Creates a local artifact registry binding
    pub fn local(
        registry_url: impl Into<BindingValue<String>>,
        data_dir: impl Into<BindingValue<Option<String>>>,
    ) -> Self {
        Self::Local(LocalArtifactRegistryBinding {
            registry_url: registry_url.into(),
            data_dir: data_dir.into(),
        })
    }
}
