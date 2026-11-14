use serde::{Deserialize, Serialize};

/// The verifier setup created, on any cloud: its name, `<prefix>-verifier` (the Lambda function,
/// the Cloud Run service or the Container App).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SigningVerifierImportData {
    /// The verifier's name in the cloud.
    pub name: String,
}
