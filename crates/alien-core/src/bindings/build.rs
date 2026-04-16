//! Service-type based build binding definitions

use super::BindingValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// AWS CodeBuild binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodebuildBuildBinding {
    /// The CodeBuild project name
    pub project_name: BindingValue<String>,
    /// Environment variables to pass to the build process
    pub build_env_vars: BindingValue<HashMap<String, String>>,
    /// Optional monitoring configuration for sending build logs
    pub monitoring: BindingValue<Option<crate::MonitoringConfig>>,
}

/// Azure Container Apps Jobs binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcaBuildBinding {
    /// The managed environment ID for Container Apps
    pub managed_environment_id: BindingValue<String>,
    /// Resource group name where the Container Apps environment is located
    pub resource_group_name: BindingValue<String>,
    /// Environment variables to pass to the build process
    pub build_env_vars: BindingValue<HashMap<String, String>>,
    /// Optional managed identity ID for authentication
    pub managed_identity_id: BindingValue<Option<String>>,
    /// Resource prefix for generating unique job names
    pub resource_prefix: BindingValue<String>,
    /// Optional monitoring configuration for sending build logs
    pub monitoring: BindingValue<Option<crate::MonitoringConfig>>,
}

/// Google Cloud Build binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudbuildBuildBinding {
    /// Environment variables to pass to the build process
    pub build_env_vars: BindingValue<HashMap<String, String>>,
    /// Service account email for Cloud Build
    pub service_account: BindingValue<String>,
    /// Optional monitoring configuration for sending build logs
    pub monitoring: BindingValue<Option<crate::MonitoringConfig>>,
}

/// Local build execution binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBuildBinding {
    /// The base data directory for build artifacts
    pub data_dir: BindingValue<String>,
    /// Environment variables to pass to the build process
    pub build_env_vars: BindingValue<HashMap<String, String>>,
}

/// Kubernetes build execution binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesBuildBinding {
    /// The Kubernetes namespace where build jobs will be created
    pub namespace: BindingValue<String>,
    /// The name of the ServiceAccount that has permissions to create jobs
    pub service_account_name: BindingValue<String>,
    /// Environment variables to pass to the build process
    pub build_env_vars: BindingValue<HashMap<String, String>>,
}

/// Service-type based build binding that supports multiple build providers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum BuildBinding {
    /// AWS CodeBuild
    Codebuild(CodebuildBuildBinding),
    /// Azure Container Apps Jobs
    Aca(AcaBuildBinding),
    /// Google Cloud Build
    Cloudbuild(CloudbuildBuildBinding),
    /// Local build execution
    Local(LocalBuildBinding),
    /// Kubernetes build execution
    Kubernetes(KubernetesBuildBinding),
}

impl BuildBinding {
    /// Creates a CodeBuild binding
    pub fn codebuild(
        project_name: impl Into<BindingValue<String>>,
        build_env_vars: impl Into<BindingValue<HashMap<String, String>>>,
        monitoring: impl Into<BindingValue<Option<crate::MonitoringConfig>>>,
    ) -> Self {
        Self::Codebuild(CodebuildBuildBinding {
            project_name: project_name.into(),
            build_env_vars: build_env_vars.into(),
            monitoring: monitoring.into(),
        })
    }

    /// Creates an ACA (Azure Container Apps) build binding
    pub fn aca(
        managed_environment_id: impl Into<BindingValue<String>>,
        resource_group_name: impl Into<BindingValue<String>>,
        build_env_vars: impl Into<BindingValue<HashMap<String, String>>>,
        managed_identity_id: impl Into<BindingValue<Option<String>>>,
        resource_prefix: impl Into<BindingValue<String>>,
        monitoring: impl Into<BindingValue<Option<crate::MonitoringConfig>>>,
    ) -> Self {
        Self::Aca(AcaBuildBinding {
            managed_environment_id: managed_environment_id.into(),
            resource_group_name: resource_group_name.into(),
            build_env_vars: build_env_vars.into(),
            managed_identity_id: managed_identity_id.into(),
            resource_prefix: resource_prefix.into(),
            monitoring: monitoring.into(),
        })
    }

    /// Creates a Cloud Build binding
    pub fn cloudbuild(
        build_env_vars: impl Into<BindingValue<HashMap<String, String>>>,
        service_account: impl Into<BindingValue<String>>,
        monitoring: impl Into<BindingValue<Option<crate::MonitoringConfig>>>,
    ) -> Self {
        Self::Cloudbuild(CloudbuildBuildBinding {
            build_env_vars: build_env_vars.into(),
            service_account: service_account.into(),
            monitoring: monitoring.into(),
        })
    }

    /// Creates a local build binding
    pub fn local(
        data_dir: impl Into<BindingValue<String>>,
        build_env_vars: impl Into<BindingValue<HashMap<String, String>>>,
    ) -> Self {
        Self::Local(LocalBuildBinding {
            data_dir: data_dir.into(),
            build_env_vars: build_env_vars.into(),
        })
    }

    /// Creates a Kubernetes build binding
    pub fn kubernetes(
        namespace: impl Into<BindingValue<String>>,
        service_account_name: impl Into<BindingValue<String>>,
        build_env_vars: impl Into<BindingValue<HashMap<String, String>>>,
    ) -> Self {
        Self::Kubernetes(KubernetesBuildBinding {
            namespace: namespace.into(),
            service_account_name: service_account_name.into(),
            build_env_vars: build_env_vars.into(),
        })
    }
}
