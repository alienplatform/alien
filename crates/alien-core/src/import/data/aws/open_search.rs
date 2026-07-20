use serde::{Deserialize, Serialize};

/// AWS OpenSearch (`experimental/aws-opensearch`) ImportData.
///
/// Mirrors the `emit_import_ref` payload of the AWS OpenSearch Serverless
/// emitter. Next-generation collections expose no Dashboards endpoint, so
/// only the data-plane endpoint and identifiers are carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsOpenSearchImportData {
    /// Physical collection name (`{id}-{stack-suffix}`).
    pub collection_name: String,
    /// Server-assigned collection id.
    pub collection_id: String,
    /// ARN of the collection.
    pub collection_arn: String,
    /// Collection endpoint (`https://<collectionId>.aoss.<region>.on.aws`).
    /// Requests must be SigV4-signed with service name `aoss`.
    pub endpoint: String,
}
