//! Azure Database for PostgreSQL — Flexible Server client (the Azure Postgres backend).
//! ARM REST/JSON, mirroring the other Azure clients. Public network access is always
//! disabled; reachability is via a Private Endpoint the controller wires through the
//! network client.

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

// ─────────────────────────── models (camelCase JSON) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServer {
    pub location: String,
    pub sku: FlexibleServerSku,
    pub properties: FlexibleServerProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerSku {
    pub name: String,
    /// Burstable / GeneralPurpose / MemoryOptimized.
    pub tier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerProperties {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub administrator_login: Option<String>,
    /// Request-only; never returned by the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub administrator_login_password: Option<String>,
    pub storage: FlexibleServerStorage,
    pub backup: FlexibleServerBackup,
    pub network: FlexibleServerNetwork,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_availability: Option<FlexibleServerHighAvailability>,
    /// Lifecycle state (response only): Ready / Provisioning / ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerStorage {
    /// `rename_all = camelCase` would emit `storageSizeGb`, but ARM's canonical key is
    /// `storageSizeGB` (capital GB). serde deserialize is case-sensitive, so the GET
    /// read-back of this required field would fail without the explicit rename.
    #[serde(rename = "storageSizeGB")]
    pub storage_size_gb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerBackup {
    /// Optional on reads: a server mid-deletion reports an empty `backup` object. Omitted from
    /// requests when `None` rather than fabricating a retention value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_retention_days: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerNetwork {
    /// Always "Disabled" — Postgres is private-only; reachability is via a Private Endpoint.
    pub public_network_access: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerHighAvailability {
    /// "ZoneRedundant" for highAvailability(), else "Disabled".
    pub mode: String,
}

/// A server configuration value (used to set `azure.extensions`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfiguration {
    pub properties: ServerConfigurationProperties,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfigurationProperties {
    pub value: String,
    pub source: String,
}

/// A database on a Flexible Server. The PUT body carries no properties: the database name is
/// in the URL, and Flexible Server defaults charset/collation, so an empty body suffices.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlexibleServerDatabase {}

pub type FlexibleServerOperationResult = OperationResult<FlexibleServer>;

// ─────────────────────────── trait + client ───────────────────────────

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait FlexibleServerApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_server(
        &self,
        resource_group: &str,
        server_name: &str,
        server: &FlexibleServer,
    ) -> Result<FlexibleServerOperationResult>;

    async fn get_server(&self, resource_group: &str, server_name: &str) -> Result<FlexibleServer>;

    async fn delete_server(
        &self,
        resource_group: &str,
        server_name: &str,
    ) -> Result<OperationResult<()>>;

    /// Sets a server configuration parameter (e.g. `azure.extensions` for pgvector).
    async fn set_configuration(
        &self,
        resource_group: &str,
        server_name: &str,
        configuration_name: &str,
        value: &str,
    ) -> Result<OperationResult<()>>;

    /// Creates a database on the server. Flexible Server's only default database is `postgres`,
    /// so the application database must be created explicitly. The PUT is idempotent: re-issuing
    /// it for an existing database succeeds rather than conflicting.
    async fn create_database(
        &self,
        resource_group: &str,
        server_name: &str,
        database_name: &str,
    ) -> Result<OperationResult<()>>;
}

#[derive(Debug)]
pub struct AzureFlexibleServerClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureFlexibleServerClient {
    const API_VERSION: &'static str = "2024-08-01";

    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        let endpoint = token_cache.management_endpoint().to_string();
        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }

    fn server_path(&self, resource_group: &str, server_name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.DBforPostgreSQL/flexibleServers/{}",
            self.token_cache.config().subscription_id,
            resource_group,
            server_name
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl FlexibleServerApi for AzureFlexibleServerClient {
    async fn create_or_update_server(
        &self,
        resource_group: &str,
        server_name: &str,
        server: &FlexibleServer,
    ) -> Result<FlexibleServerOperationResult> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.server_path(resource_group, server_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let body = serde_json::to_string(server).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Flexible Server: {server_name}"),
            },
        )?;
        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        // The request body carries `administratorLoginPassword`; strip the captured request body from
        // any error so the password can never reach logs or an external response.
        alien_client_core::redact_request_body(
            self.base
                .execute_request_with_long_running_support(
                    signed,
                    "CreateOrUpdateFlexibleServer",
                    server_name,
                )
                .await,
        )
    }

    async fn get_server(&self, resource_group: &str, server_name: &str) -> Result<FlexibleServer> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.server_path(resource_group, server_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let req = AzureRequestBuilder::new(Method::GET, url.clone())
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetFlexibleServer", server_name)
            .await?;
        // The response already succeeded (execute_request returns Ok only on 2xx), so a body-read /
        // parse failure is a serialization problem, not an HTTP failure — don't fabricate a 502.
        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure GetFlexibleServer: failed to read body for {server_name}"),
            })?;
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure GetFlexibleServer: JSON parse error for {server_name}"),
            })
    }

    async fn delete_server(
        &self,
        resource_group: &str,
        server_name: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.server_path(resource_group, server_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let req = AzureRequestBuilder::new(Method::DELETE, url)
            .content_length("")
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(signed, "DeleteFlexibleServer", server_name)
            .await
    }

    async fn set_configuration(
        &self,
        resource_group: &str,
        server_name: &str,
        configuration_name: &str,
        value: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!(
                "{}/configurations/{}",
                self.server_path(resource_group, server_name),
                configuration_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let config = ServerConfiguration {
            properties: ServerConfigurationProperties {
                value: value.to_string(),
                source: "user-override".to_string(),
            },
        };
        let body = serde_json::to_string(&config).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize configuration {configuration_name}"),
            },
        )?;
        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "SetServerConfiguration",
                configuration_name,
            )
            .await
    }

    async fn create_database(
        &self,
        resource_group: &str,
        server_name: &str,
        database_name: &str,
    ) -> Result<OperationResult<()>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!(
                "{}/databases/{}",
                self.server_path(resource_group, server_name),
                database_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let body = serde_json::to_string(&FlexibleServerDatabase {})
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize database body for {database_name}"),
            })?;
        let req = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(req, &token).await?;
        self.base
            .execute_request_with_long_running_support(
                signed,
                "CreateFlexibleServerDatabase",
                database_name,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_server() -> FlexibleServer {
        FlexibleServer {
            location: "eastus".into(),
            sku: FlexibleServerSku {
                name: "Standard_B1ms".into(),
                tier: "Burstable".into(),
            },
            properties: FlexibleServerProperties {
                version: "17".into(),
                administrator_login: Some("alien".into()),
                administrator_login_password: Some("secret".into()),
                storage: FlexibleServerStorage {
                    storage_size_gb: 32,
                },
                backup: FlexibleServerBackup {
                    backup_retention_days: Some(7),
                },
                network: FlexibleServerNetwork {
                    public_network_access: "Disabled".into(),
                },
                high_availability: Some(FlexibleServerHighAvailability {
                    mode: "ZoneRedundant".into(),
                }),
                state: None,
            },
        }
    }

    #[test]
    fn server_serializes_private_with_extensions_intent() {
        let json = serde_json::to_value(private_server()).unwrap();
        assert_eq!(json["properties"]["version"], "17");
        // private-only: public network access disabled.
        assert_eq!(
            json["properties"]["network"]["publicNetworkAccess"],
            "Disabled"
        );
        assert_eq!(
            json["properties"]["highAvailability"]["mode"],
            "ZoneRedundant"
        );
        assert_eq!(json["properties"]["administratorLoginPassword"], "secret");
        // ARM's canonical storage key is `storageSizeGB` (capital GB); camelCase would
        // wrongly emit `storageSizeGb`. Pin both so a dropped rename regresses here.
        assert_eq!(json["properties"]["storage"]["storageSizeGB"], 32);
        assert!(json["properties"]["storage"].get("storageSizeGb").is_none());
        // state is response-only; absent on the way out.
        assert!(json["properties"].get("state").is_none());
    }

    #[test]
    fn configuration_uses_user_override_source() {
        let json = serde_json::to_value(ServerConfiguration {
            properties: ServerConfigurationProperties {
                value: "VECTOR,PG_TRGM,UUID-OSSP,PGCRYPTO".into(),
                source: "user-override".into(),
            },
        })
        .unwrap();
        assert_eq!(json["properties"]["source"], "user-override");
        assert!(json["properties"]["value"]
            .as_str()
            .unwrap()
            .contains("VECTOR"));
    }

    #[test]
    fn database_put_body_is_empty_object() {
        // The database name rides the URL; the PUT body carries no properties, so an empty
        // JSON object is what goes on the wire.
        let json = serde_json::to_value(FlexibleServerDatabase {}).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn server_deserializes_get_response() {
        // Storage key mirrors a real ARM response: `storageSizeGB` (capital GB).
        let body = r#"{"location":"eastus","sku":{"name":"Standard_B1ms","tier":"Burstable"},
            "properties":{"version":"17","storage":{"storageSizeGB":32},
            "backup":{"backupRetentionDays":7},"network":{"publicNetworkAccess":"Disabled"},
            "state":"Ready"}}"#;
        let server: FlexibleServer = serde_json::from_str(body).unwrap();
        assert_eq!(server.properties.state.as_deref(), Some("Ready"));
        assert_eq!(server.properties.network.public_network_access, "Disabled");
        assert_eq!(server.properties.storage.storage_size_gb, 32);
    }

    #[test]
    fn server_deserializes_dropping_response_with_empty_backup() {
        // Azure deletes servers asynchronously and reports `backup: {}` mid-deletion (observed
        // live); a caller polling `get_server` after `delete_server` sees this partial document
        // until the API returns 404, so it must still parse.
        let body = r#"{"location":"eastus","sku":{"name":"Standard_B1ms","tier":"Burstable"},
            "properties":{"version":"17","storage":{"storageSizeGB":32},
            "backup":{},"network":{"publicNetworkAccess":"Disabled"},
            "state":"Dropping"}}"#;
        let server: FlexibleServer = serde_json::from_str(body).unwrap();
        assert_eq!(server.properties.state.as_deref(), Some("Dropping"));
        assert_eq!(server.properties.backup.backup_retention_days, None);
    }
}
