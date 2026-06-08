use crate::{ManagementConfig, Platform, ResourceType, StackSettings, StackState};
use serde::{Deserialize, Serialize};

/// Oldest setup import payload format this binary can read.
pub const MIN_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION: u32 = 1;

/// Setup import payload format this binary writes.
pub const CURRENT_SETUP_IMPORT_FORMAT_VERSION: u32 = 1;

/// Package source that produced an import request. Observability label
/// only — the manager does not branch on this value, and any new deployment
/// pathway can omit it without affecting import behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum ImportSourceKind {
    /// CloudFormation Custom Resource or Stack Outputs.
    CloudFormation,
    /// Terraform provider resource.
    Terraform,
    /// Helm chart local-import bootstrap path.
    Helm,
}

/// Request body for manager-side stack import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackImportRequest {
    /// Wire-format version for the setup import payload.
    pub setup_import_format_version: u32,
    /// Deployment-group token authorizing the import.
    pub deployment_group_token: String,
    /// User-chosen deployment name. Must be unique within the deployment
    /// group; the manager returns 409 on collision rather than silently
    /// resolving to an existing deployment. Each setup adapter picks
    /// the natural source: CloudFormation defaults to the CFN stack name,
    /// Helm to `{namespace}/{release}`, Terraform requires an explicit
    /// `name` attribute on the `alien_deployment` resource.
    pub deployment_name: String,
    /// Stable physical-name prefix used by the setup package for generated
    /// resources. Runtime controllers use it when addressing imported
    /// resources.
    pub resource_prefix: String,
    /// Optional source label for observability. Does not affect import
    /// behavior — the manager dispatches the same `ImporterRegistry`
    /// regardless of which setup package emitted the payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<ImportSourceKind>,
    /// Setup source metadata needed by the control plane to guide privileged
    /// teardown. The manager treats this as opaque JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_metadata: Option<serde_json::Value>,
    /// Optional release id that produced the setup package. When
    /// omitted, the manager imports against the latest release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    /// Platform being imported.
    pub platform: Platform,
    /// Optional base cloud platform for Kubernetes setup targets such as
    /// EKS/GKE/AKS. The runtime platform remains Kubernetes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_platform: Option<Platform>,
    /// Region or location reported by the setup artifact.
    pub region: String,
    /// Setup target this package was generated for.
    pub setup_target: String,
    /// Setup compatibility fingerprint embedded in the package.
    pub setup_fingerprint: String,
    /// Setup fingerprint algorithm version embedded in the package.
    pub setup_fingerprint_version: u32,
    /// Resolved stack settings supplied by the setup artifact.
    pub stack_settings: StackSettings,
    /// Platform-derived management configuration, when this setup creates a
    /// cross-account/cross-tenant management identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_config: Option<ManagementConfig>,
    /// Imported resources with typed per-resource payloads.
    pub resources: Vec<ImportedResource>,
}

/// One resolved resource import payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ImportedResource {
    /// Resource id from the active stack.
    pub id: String,
    /// Resource type from the active stack.
    #[serde(rename = "type")]
    pub resource_type: ResourceType,
    /// Resolved typed payload for this resource.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub import_data: serde_json::Value,
}

/// Response body returned after a stack import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct StackImportResponse {
    /// Deployment created.
    pub deployment_id: String,
    /// Deployment bearer token for the imported deployment, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_token: Option<String>,
    /// Stack settings persisted for the deployment.
    pub stack_settings: StackSettings,
    /// Fully populated imported stack state.
    pub stack_state: StackState,
}

#[cfg(test)]
mod tests {
    use super::ImportSourceKind;

    #[test]
    fn import_source_kind_serializes_cloudformation_without_separator() {
        let value = serde_json::to_value(ImportSourceKind::CloudFormation).unwrap();
        assert_eq!(value, "cloudformation");

        let source: ImportSourceKind = serde_json::from_value(value).unwrap();
        assert_eq!(source, ImportSourceKind::CloudFormation);
    }
}
