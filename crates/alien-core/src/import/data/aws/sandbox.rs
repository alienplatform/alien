use serde::{Deserialize, Serialize};

/// AWS Sandbox ImportData.
///
/// Carries the Frozen parent from the setup emitter to the runtime controller. The image
/// **version** is not decoration: `RunMicrovm` has no `tags`, so image plus version is the only
/// session identity there is, and a controller holding a stale version would enumerate the wrong
/// set and orphan every session started on the previous one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsSandboxImportData {
    /// MicroVM image identifier.
    pub image_identifier: String,
    /// MicroVM image ARN.
    pub image_arn: String,
    /// Image version the sessions are scoped to. Re-imported on every image roll.
    pub image_version: String,
    /// Egress network connectors. Deleting one while MicroVMs still reference it breaks their
    /// networking, so teardown needs them named rather than rediscovered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress_connector_arns: Vec<String>,
    /// Ports a preview capability may be minted for; empty means preview is not offered.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::import::data::deserialize_u16_vec_from_numbers_or_strings"
    )]
    pub preview_ports: Vec<u16>,
    /// Whether the declaration asked for open egress.
    ///
    /// The controller builds the binding from this, and an empty connector list cannot be read
    /// without it: a stripped `deny` import would otherwise look exactly like an open sandbox.
    #[serde(
        default,
        skip_serializing_if = "std::ops::Not::not",
        deserialize_with = "crate::import::data::deserialize_bool_from_bool_or_string"
    )]
    pub allow_egress: bool,
}
