use std::collections::BTreeMap;

use alien_permission_types::AwsPermissionEffect;
use serde::{Deserialize, Serialize};

pub const GCP_PROJECT_SCOPE: &str = "projects/${projectName}";
pub const GCP_BUCKET_SCOPE: &str = "projects/${projectName}/buckets/${resourceName}";
pub type AwsCondition = BTreeMap<String, BTreeMap<String, String>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub plugin: String,
    pub operation: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsDeclaration {
    pub effect: AwsPermissionEffect,
    pub actions: Vec<String>,
    pub resources: Vec<String>,
    pub condition: Option<AwsCondition>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsStatement {
    pub effect: AwsPermissionEffect,
    pub actions: Vec<String>,
    pub resources: Vec<String>,
    pub condition: Option<AwsCondition>,
    pub reasons: Vec<String>,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpDeclaration {
    pub permissions: Vec<String>,
    pub scope: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpGrant {
    pub permission: String,
    pub scope: String,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KubernetesRule {
    pub api_group: String,
    pub resource: String,
    pub verbs: Vec<String>,
    #[serde(default)]
    pub resource_names: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KubernetesPermissions {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub rules: Vec<KubernetesRule>,
}

fn schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesGrant {
    pub api_group: String,
    pub resource: String,
    pub verbs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_names: Vec<String>,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KubernetesMode {
    #[default]
    Diagnostics,
    Remediation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudDeclarations {
    #[serde(default)]
    pub aws: Vec<AwsDeclaration>,
    #[serde(default)]
    pub gcp: Vec<GcpDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogOperation {
    pub name: String,
    #[serde(default)]
    pub permissions: CloudDeclarations,
    pub kubernetes_permissions: Option<KubernetesPermissions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPlugin {
    pub name: String,
    pub enabled: bool,
    pub operations: Vec<CatalogOperation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsResourceCeilings {
    pub s3_bucket_arns: Vec<String>,
    pub sqs_queue_arns: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GcpResourceCeilings {
    pub gcs_bucket_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpBucketGrants {
    pub names: Vec<String>,
    pub grants: Vec<GcpGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpWorkloadIdentityGrants {
    pub project: Vec<GcpGrant>,
    pub buckets: GcpBucketGrants,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attribution {
    pub plugin: String,
    pub operation: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginOrigin {
    Builtin,
    #[default]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredOperation {
    pub name: String,
    pub tier: Option<String>,
    #[serde(default, alias = "requiredPermissions")]
    pub permissions: Vec<alien_permission_types::PermissionSetReference>,
    pub kubernetes_permissions: Option<KubernetesPermissions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredPlugin {
    pub name: String,
    pub tier: String,
    pub operations: Vec<DeclaredOperation>,
}
