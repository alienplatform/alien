use serde::{Deserialize, Serialize};

/// AWS Sandbox ImportData.
///
/// Carries the sandbox's parent from the setup emitter to the runtime controller. The image
/// **version** is not decoration: `RunMicrovm` has no `tags`, so image plus version is the only
/// session identity there is, and a controller holding a stale version would enumerate the wrong
/// set and orphan every session started on the previous one.
///
/// Two shapes arrive here, and which fields are present says which. A Frozen sandbox is built by
/// stack creation and names its image; a Live one is built by the controller after the deployment
/// registers, so it names the build role and bundle instead, leaving the image fields empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AwsSandboxImportData {
    /// MicroVM image identifier. Absent until a runtime-provisioned image has been built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_identifier: Option<String>,
    /// MicroVM image ARN. Absent until a runtime-provisioned image has been built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_arn: Option<String>,
    /// Image version the sessions are scoped to. Re-imported on every image roll, and absent
    /// until a runtime-provisioned image has been built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_version: Option<String>,
    /// Role the controller passes to `CreateMicrovmImage`. Setup owns it because
    /// `sandbox/provision` grants the controller `iam:PassRole` and no `iam:CreateRole`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_role_arn: Option<String>,
    /// Bundle the controller builds the image from. Only a runtime-provisioned sandbox carries it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_uri: Option<String>,
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
