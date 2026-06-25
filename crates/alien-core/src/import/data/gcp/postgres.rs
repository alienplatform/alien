use serde::{Deserialize, Serialize};

/// GCP Postgres (Cloud SQL + Private Service Connect) registration data: the handles by
/// which a setup-created Cloud SQL instance and its PSC consumer endpoint are registered
/// as a Frozen Postgres resource. Setup owns the instance, Alien refreshes and heartbeats it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcpPostgresImportData {
    /// Cloud SQL instance name.
    pub instance_name: String,
    /// Default database name.
    pub database: String,
    /// Secret Manager secret name of the master password (never the password).
    pub password_secret_name: String,
    /// PSC service-attachment URI exposed by the instance.
    pub psc_service_attachment: String,
    /// Regional internal address backing the PSC consumer endpoint.
    pub psc_address_name: String,
    /// Regional forwarding rule for the PSC consumer endpoint.
    pub psc_forwarding_rule_name: String,
    /// PSC consumer endpoint IP (the binding host).
    pub endpoint_ip: String,
    /// GCP region.
    pub region: String,
    /// Engine version the instance reports.
    pub database_version: String,
}
