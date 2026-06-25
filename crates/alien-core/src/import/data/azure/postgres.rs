use serde::{Deserialize, Serialize};

/// Azure Postgres (Flexible Server + Private Endpoint) registration data: the handles by
/// which a setup-created Flexible Server (with its Private Endpoint, private DNS zone, and
/// the Key Vault secret holding the master password) is registered as a Frozen Postgres
/// resource. Setup owns the server, Alien refreshes and heartbeats it. The raw password is
/// never carried, only the Key Vault secret URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureFlexibleServerPostgresImportData {
    /// Flexible Server name on Azure.
    pub server_name: String,
    /// Full ARM resource id of the Flexible Server (the deletable handle).
    pub server_resource_id: String,
    /// Resource group the server lives in.
    pub resource_group: String,
    /// Default database name.
    pub database: String,
    /// Private DNS FQDN host workloads resolve to reach the server (the binding host).
    pub host: String,
    /// Key Vault base URL holding the password (e.g. `https://<vault>.vault.azure.net`).
    pub key_vault_url: String,
    /// Key Vault secret name of the master password (never the password).
    pub password_secret_name: String,
    /// Key Vault secret URI of the master password; the binding carries this and the
    /// workload resolves the value at load time.
    pub password_secret_uri: String,
    /// Private Endpoint name fronting the server in the dedicated PE subnet.
    pub private_endpoint_name: String,
    /// Azure region the server lives in.
    pub region: String,
    /// Engine version the server reports.
    pub version: String,
}
