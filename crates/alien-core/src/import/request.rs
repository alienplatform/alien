use crate::{ManagementConfig, Platform, ResourceType, StackSettings, StackState};
use serde::{Deserialize, Serialize};

/// Distribution source that produced an import request. Observability label
/// only — the manager does not branch on this value, and any new deployment
/// pathway can omit it without affecting import behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ImportSourceKind {
    /// CloudFormation Custom Resource or Stack Outputs.
    #[serde(alias = "cloudformation")]
    CloudFormation,
    /// Terraform provider resource.
    Terraform,
    /// Helm chart local-import bootstrap path.
    Helm,
}

/// Request body for manager-side stack import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StackImportRequest {
    /// Deployment-group token authorizing the import.
    pub deployment_group_token: String,
    /// User-chosen deployment name. Must be unique within the deployment
    /// group; the manager returns 409 on collision rather than silently
    /// resolving to an existing deployment. Each distribution adapter picks
    /// the natural source: CloudFormation defaults to the CFN stack name,
    /// Helm to `{namespace}/{release}`, Terraform requires an explicit
    /// `name` attribute on the `alien_deployment` resource.
    pub deployment_name: String,
    /// Stable physical-name prefix used by the distribution artifact for
    /// generated resources. This is the Alien stack prefix, not merely a UI
    /// name: runtime controllers use it when addressing imported resources.
    pub stack_prefix: String,
    /// Optional source label for observability. Does not affect import
    /// behavior — the manager dispatches the same `ImporterRegistry`
    /// regardless of which distribution artifact emitted the payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<ImportSourceKind>,
    /// Optional release id that produced the distribution artifact. When
    /// omitted, the manager imports against the latest release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    /// Platform being imported.
    pub platform: Platform,
    /// Region or location reported by the distribution artifact.
    pub region: String,
    /// Resolved stack settings supplied by the distribution artifact.
    pub stack_settings: StackSettings,
    /// Platform-derived management configuration.
    pub management_config: ManagementConfig,
    /// Imported resources with typed per-resource payloads.
    pub resources: Vec<ImportedResource>,
}

/// One resolved resource import payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StackImportResponse {
    /// Deployment created.
    pub deployment_id: String,
    /// Stack settings persisted for the deployment.
    pub stack_settings: StackSettings,
    /// Fully populated imported stack state.
    pub stack_state: StackState,
}
