use serde::{Deserialize, Serialize};

/// AWS Worker ImportData.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWorkerImportData {
    /// Lambda function name.
    pub function_name: String,
    /// Lambda function ARN.
    pub function_arn: String,
    /// Public HTTPS URL, when public ingress is enabled.
    pub url: Option<String>,
    /// API Gateway HTTP API ID, when public ingress is enabled.
    pub api_id: Option<String>,
    /// API Gateway integration ID, when public ingress is enabled.
    pub integration_id: Option<String>,
    /// API Gateway route ID, when public ingress is enabled.
    pub route_id: Option<String>,
    /// API Gateway stage name, when public ingress is enabled.
    pub stage_name: Option<String>,
    /// Queue event-source mapping UUIDs.
    pub event_source_mappings: Vec<String>,
    /// EventBridge rule names for schedule triggers.
    pub eventbridge_rule_names: Vec<String>,
    /// Lambda permission statement IDs granted for S3 storage triggers.
    pub s3_permission_statement_ids: Vec<String>,
    /// Lambda permission statement IDs granted for EventBridge schedule triggers.
    pub eventbridge_permission_statement_ids: Vec<String>,
}
