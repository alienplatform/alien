use serde::{Deserialize, Serialize};

/// GCP Cloud KMS key created by the customer-installed setup stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpKeyImportData {
    /// Full CryptoKey resource name, without a CryptoKeyVersion.
    pub crypto_key_name: String,
    /// Full primary CryptoKeyVersion resource name used for new wrapping operations.
    pub primary_version: String,
}
