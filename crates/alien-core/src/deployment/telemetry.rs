//! Explicit deployment-owned OTLP destinations. Credentials are environment references.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct TelemetryDestinations {
    pub metrics: Option<OtlpSignalDestination>,
    pub traces: Option<OtlpSignalDestination>,
    pub logs: Option<OtlpSignalDestination>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OtlpSignalDestination {
    pub endpoint: OtlpDestinationEndpoint,
    pub authentication: OtlpDestinationAuthentication,
    /// HTTP must be explicitly approved, for example for a local development collector.
    #[serde(default)]
    pub allow_insecure_http: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OtlpDestinationEndpoint {
    Url {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    Resource {
        resource_id: String,
        public_endpoint_name: String,
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OtlpDestinationAuthentication {
    #[serde(rename_all = "camelCase")]
    Bearer { token_environment_variable: String },
    #[serde(rename_all = "camelCase")]
    JwtHs256 {
        key_environment_variable: String,
        issuer: String,
        audience: String,
        /// Application-owned claims. Standard issuer/audience/time claims cannot be overridden.
        claims: BTreeMap<String, serde_json::Value>,
    },
}
