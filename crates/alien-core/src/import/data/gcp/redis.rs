use serde::{Deserialize, Serialize};

/// GCP Memorystore for Valkey registration data: the handles by which a setup-created
/// instance is registered as a Frozen Redis resource. Setup owns the instance and the secret
/// container; Alien copies the server-generated auth token into that secret, then refreshes
/// and heartbeats the instance. The token itself is never carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpMemorystoreImportData {
    /// Instance id.
    pub instance_name: String,
    /// Region the instance runs in.
    pub region: String,
    /// Secret Manager secret that receives the auth token (never the token itself).
    pub password_secret_name: String,
    /// Private Service Connect endpoint IP (the binding host).
    pub endpoint_ip: String,
    /// Listening port.
    pub port: u16,
    /// Node type, such as `shared-core-nano`.
    pub node_type: String,
    /// Engine version the instance reports.
    pub engine_version: String,
    /// Whether the instance has a replica.
    #[serde(default)]
    pub high_availability: bool,
    /// Whether RDB persistence is on.
    #[serde(default)]
    pub persistence: bool,
}
