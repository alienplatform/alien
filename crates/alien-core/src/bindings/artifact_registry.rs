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
    /// ARN of the IAM role for pull permissions (optional)
    pub pull_role_arn: BindingValue<Option<String>>,
    /// ARN of the IAM role for push+pull permissions (optional)
    pub push_role_arn: BindingValue<Option<String>>,
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
}

/// Google Artifact Registry binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GarArtifactRegistryBinding {
    /// Optional service account email for pull permissions
    pub pull_service_account_email: BindingValue<Option<String>>,
    /// Optional service account email for push+pull permissions
    pub push_service_account_email: BindingValue<Option<String>>,
}

/// Local container registry binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalArtifactRegistryBinding {
    /// The registry URL endpoint (e.g., "http://localhost:5000")
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
        pull_role_arn: impl Into<BindingValue<Option<String>>>,
        push_role_arn: impl Into<BindingValue<Option<String>>>,
    ) -> Self {
        Self::Ecr(EcrArtifactRegistryBinding {
            repository_prefix: repository_prefix.into(),
            pull_role_arn: pull_role_arn.into(),
            push_role_arn: push_role_arn.into(),
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
        })
    }

    /// Creates a GAR artifact registry binding
    pub fn gar(
        pull_service_account_email: impl Into<BindingValue<Option<String>>>,
        push_service_account_email: impl Into<BindingValue<Option<String>>>,
    ) -> Self {
        Self::Gar(GarArtifactRegistryBinding {
            pull_service_account_email: pull_service_account_email.into(),
            push_service_account_email: push_service_account_email.into(),
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
