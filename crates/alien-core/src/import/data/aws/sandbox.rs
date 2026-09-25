use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::{parse_bundle_uri, BundleUri, ErrorData, Result, Sandbox, SandboxCode, SandboxEgress};

/// AWS Sandbox ImportData.
///
/// Carries the sandbox's parent from the setup emitter to the runtime controller. The image
/// **version** is not decoration: `RunMicrovm` has no `tags`, so image plus version is the only
/// sandbox identity there is, and a controller holding a stale version would enumerate the wrong
/// set and orphan every sandbox started on the previous one.
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
    /// Image version the sandboxes are scoped to. Re-imported on every image roll, and absent
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

impl AwsSandboxImportData {
    /// What a runtime-built sandbox registers, with every template expression resolved. The
    /// setup emitters write the same fields as expressions, and parity tests hold them to this.
    ///
    /// `deny` names the connector setup recorded; `allow` names none, whatever is recorded, so a
    /// connector left over from an earlier `deny` never reaches a session.
    pub fn runtime_built(
        sandbox: &Sandbox,
        build_role_arn: String,
        region: &str,
        recorded_connector_arn: Option<&str>,
    ) -> Result<Self> {
        let refuse = |reason: String| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: format!("register sandbox '{}'", sandbox.id),
                reason,
            })
        };
        let SandboxCode::Image { image } = &sandbox.code else {
            return Err(refuse(
                "an AWS sandbox is built from a prebuilt s3:// bundle, not from source".to_string(),
            ));
        };
        let bundle_uri = match parse_bundle_uri(image).map_err(refuse)? {
            BundleUri::Literal(uri) => uri.to_string(),
            BundleUri::Regional { before, after } => format!("{before}{region}{after}"),
        };
        let egress_connector_arns = match &sandbox.egress {
            SandboxEgress::Allow => Vec::new(),
            SandboxEgress::Deny => vec![recorded_connector_arn
                .ok_or_else(|| {
                    refuse("egress: deny has no egress connector recorded yet".to_string())
                })?
                .to_string()],
            SandboxEgress::AllowDomains { .. } => {
                return Err(refuse(
                    "AWS has no connector configuration for egress: allowDomains".to_string(),
                ))
            }
        };

        Ok(Self {
            image_identifier: None,
            image_arn: None,
            image_version: None,
            build_role_arn: Some(build_role_arn),
            bundle_uri: Some(bundle_uri),
            egress_connector_arns,
            preview_ports: sandbox.preview_ports.clone(),
            allow_egress: matches!(sandbox.egress, SandboxEgress::Allow),
        })
    }
}
