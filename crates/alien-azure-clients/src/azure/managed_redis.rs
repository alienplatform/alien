//! Azure Managed Redis client (`Microsoft.Cache/redisEnterprise`): the cluster that owns the
//! capacity, and its single `default` database that owns the endpoint, eviction policy,
//! persistence and access keys. ARM REST/JSON.
//!
//! Public network access is always sent as `Disabled`; reachability is via a private endpoint the
//! controller wires through the private networking client.

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::{LongRunningOperation, OperationResult};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";

/// The only database name Azure Managed Redis supports.
pub const DEFAULT_DATABASE: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisCluster {
    pub location: String,
    pub sku: ManagedRedisSku,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<std::collections::HashMap<String, String>>,
    pub properties: ManagedRedisClusterProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisSku {
    /// e.g. `Balanced_B1`.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisClusterProperties {
    /// Always `Disabled` on create. Read back to detect out-of-band exposure.
    pub public_network_access: String,
    /// `Enabled` / `Disabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_availability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_tls_version: Option<String>,
    /// Response only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    /// Response only: `Running`, `Creating`, `Scaling`, `Deleting`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_state: Option<String>,
    /// Response only: `Succeeded`, `Failed`, `Canceled`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
    /// Response only: the engine version the provider runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_version: Option<String>,
}

/// Partial cluster update (PATCH).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisClusterUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<ManagedRedisSku>,
    pub properties: ManagedRedisClusterUpdateProperties,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisClusterUpdateProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_network_access: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisDatabase {
    pub properties: ManagedRedisDatabaseProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisDatabaseProperties {
    /// `Encrypted` (TLS only) or `Plaintext`.
    pub client_protocol: String,
    pub port: u16,
    /// `NoCluster` gives a plain-client single endpoint.
    pub clustering_policy: String,
    /// e.g. `VolatileLRU`, `AllKeysLRU`, `NoEviction`.
    pub eviction_policy: String,
    /// `Enabled` / `Disabled`.
    pub access_keys_authentication: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence: Option<ManagedRedisPersistence>,
    /// Response only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioning_state: Option<String>,
    /// Response only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_state: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisPersistence {
    #[serde(default)]
    pub rdb_enabled: bool,
    /// e.g. `1h`. Omitted when RDB is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rdb_frequency: Option<String>,
}

/// Access keys. No derived `Debug`: the keys must never reach logs.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRedisAccessKeys {
    pub primary_key: Option<String>,
    pub secondary_key: Option<String>,
}

impl std::fmt::Debug for ManagedRedisAccessKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedRedisAccessKeys")
            .field("primary_key", &"<redacted>")
            .field("secondary_key", &"<redacted>")
            .finish()
    }
}

/// Which key `regenerateKey` rotates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedRedisKeyType {
    Primary,
    Secondary,
}

impl ManagedRedisKeyType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
        }
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ManagedRedisApi: Send + Sync + std::fmt::Debug {
    async fn create_or_update_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
        cluster: &ManagedRedisCluster,
    ) -> Result<OperationResult<ManagedRedisCluster>>;

    /// One PATCH attempt; callers persist the returned operation before polling.
    async fn update_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
        update: &ManagedRedisClusterUpdate,
    ) -> Result<Option<LongRunningOperation>>;

    async fn get_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<ManagedRedisCluster>;

    async fn delete_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<OperationResult<()>>;

    /// PUT of the whole database. PATCH is not supported by the API.
    async fn create_or_update_database(
        &self,
        resource_group: &str,
        cluster_name: &str,
        database: &ManagedRedisDatabase,
    ) -> Result<OperationResult<ManagedRedisDatabase>>;

    async fn get_database(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<ManagedRedisDatabase>;

    async fn delete_database(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<OperationResult<()>>;

    /// Reads the database access keys. The management identity of a deployment never calls
    /// this; it exists for the workload-side binding and for tests.
    async fn list_keys(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<ManagedRedisAccessKeys>;

    /// Rotates one access key. Used to recover when a lost create response left the key unknown.
    async fn regenerate_key(
        &self,
        resource_group: &str,
        cluster_name: &str,
        key_type: ManagedRedisKeyType,
    ) -> Result<OperationResult<()>>;
}

#[derive(Debug)]
pub struct AzureManagedRedisClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureManagedRedisClient {
    const API_VERSION: &'static str = "2025-07-01";

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

    fn cluster_path(&self, resource_group: &str, cluster_name: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Cache/redisEnterprise/{}",
            self.token_cache.config().subscription_id,
            resource_group,
            cluster_name
        )
    }

    fn database_path(&self, resource_group: &str, cluster_name: &str) -> String {
        format!(
            "{}/databases/{DEFAULT_DATABASE}",
            self.cluster_path(resource_group, cluster_name)
        )
    }

    async fn signed(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
    ) -> Result<reqwest::Request> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(MANAGEMENT_SCOPE)
            .await?;
        let url = self
            .base
            .build_url(path, Some(vec![("api-version", Self::API_VERSION.into())]));
        let mut builder = AzureRequestBuilder::new(method, url);
        builder = match body {
            Some(body) => builder.content_type_json().content_length(&body).body(body),
            None => builder.content_length(""),
        };
        self.base.sign_request(builder.build()?, &token).await
    }

    fn serialize<T: Serialize>(value: &T, what: &str) -> Result<String> {
        serde_json::to_string(value)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Failed to serialize {what}"),
            })
    }

    async fn read_json<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        what: &str,
    ) -> Result<T> {
        let body = response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure {what}: failed to read response body"),
            })?;
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: format!("Azure {what}: JSON parse error"),
            })
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ManagedRedisApi for AzureManagedRedisClient {
    async fn create_or_update_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
        cluster: &ManagedRedisCluster,
    ) -> Result<OperationResult<ManagedRedisCluster>> {
        let body = Self::serialize(cluster, "Managed Redis cluster")?;
        let req = self
            .signed(
                Method::PUT,
                &self.cluster_path(resource_group, cluster_name),
                Some(body),
            )
            .await?;
        self.base
            .execute_request_with_long_running_support(
                req,
                "CreateOrUpdateManagedRedisCluster",
                cluster_name,
            )
            .await
    }

    async fn update_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
        update: &ManagedRedisClusterUpdate,
    ) -> Result<Option<LongRunningOperation>> {
        let body = Self::serialize(update, "Managed Redis cluster update")?;
        let req = self
            .signed(
                Method::PATCH,
                &self.cluster_path(resource_group, cluster_name),
                Some(body),
            )
            .await?;
        let response = self
            .base
            .execute_request_once(req, "UpdateManagedRedisCluster", cluster_name)
            .await?;
        LongRunningOperation::from_response_headers(&response)
    }

    async fn get_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<ManagedRedisCluster> {
        let req = self
            .signed(
                Method::GET,
                &self.cluster_path(resource_group, cluster_name),
                None,
            )
            .await?;
        let response = self
            .base
            .execute_request(req, "GetManagedRedisCluster", cluster_name)
            .await?;
        Self::read_json(response, "GetManagedRedisCluster").await
    }

    async fn delete_cluster(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<OperationResult<()>> {
        let req = self
            .signed(
                Method::DELETE,
                &self.cluster_path(resource_group, cluster_name),
                None,
            )
            .await?;
        self.base
            .execute_request_with_long_running_support(
                req,
                "DeleteManagedRedisCluster",
                cluster_name,
            )
            .await
    }

    async fn create_or_update_database(
        &self,
        resource_group: &str,
        cluster_name: &str,
        database: &ManagedRedisDatabase,
    ) -> Result<OperationResult<ManagedRedisDatabase>> {
        let body = Self::serialize(database, "Managed Redis database")?;
        let req = self
            .signed(
                Method::PUT,
                &self.database_path(resource_group, cluster_name),
                Some(body),
            )
            .await?;
        self.base
            .execute_request_with_long_running_support(
                req,
                "CreateOrUpdateManagedRedisDatabase",
                cluster_name,
            )
            .await
    }

    async fn get_database(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<ManagedRedisDatabase> {
        let req = self
            .signed(
                Method::GET,
                &self.database_path(resource_group, cluster_name),
                None,
            )
            .await?;
        let response = self
            .base
            .execute_request(req, "GetManagedRedisDatabase", cluster_name)
            .await?;
        Self::read_json(response, "GetManagedRedisDatabase").await
    }

    async fn delete_database(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<OperationResult<()>> {
        let req = self
            .signed(
                Method::DELETE,
                &self.database_path(resource_group, cluster_name),
                None,
            )
            .await?;
        self.base
            .execute_request_with_long_running_support(
                req,
                "DeleteManagedRedisDatabase",
                cluster_name,
            )
            .await
    }

    async fn list_keys(
        &self,
        resource_group: &str,
        cluster_name: &str,
    ) -> Result<ManagedRedisAccessKeys> {
        let path = format!("{}/listKeys", self.database_path(resource_group, cluster_name));
        let req = self.signed(Method::POST, &path, None).await?;
        let response = self
            .base
            .execute_request(req, "ListManagedRedisKeys", cluster_name)
            .await?;
        // The body holds the access keys, and serde's parse errors can quote the offending value:
        // report only that the parse failed.
        let body = response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::SerializationError {
                message: "Azure ListManagedRedisKeys: failed to read response body".to_string(),
            })?;
        serde_json::from_str(&body).map_err(|_| {
            alien_error::AlienError::new(ErrorData::SerializationError {
                message: "Azure ListManagedRedisKeys: the response did not parse (body withheld \
                          because it holds access keys)"
                    .to_string(),
            })
        })
    }

    async fn regenerate_key(
        &self,
        resource_group: &str,
        cluster_name: &str,
        key_type: ManagedRedisKeyType,
    ) -> Result<OperationResult<()>> {
        let path = format!(
            "{}/regenerateKey",
            self.database_path(resource_group, cluster_name)
        );
        let body = serde_json::json!({ "keyType": key_type.as_str() }).to_string();
        let req = self.signed(Method::POST, &path, Some(body)).await?;
        self.base
            .execute_request_with_long_running_support(
                req,
                "RegenerateManagedRedisKey",
                cluster_name,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_serializes_private_only_and_omits_response_fields() {
        let cluster = ManagedRedisCluster {
            location: "eastus".into(),
            sku: ManagedRedisSku {
                name: "Balanced_B1".into(),
            },
            tags: None,
            properties: ManagedRedisClusterProperties {
                public_network_access: "Disabled".into(),
                high_availability: Some("Disabled".into()),
                minimum_tls_version: Some("1.2".into()),
                host_name: None,
                resource_state: None,
                provisioning_state: None,
                redis_version: None,
            },
        };
        let json = serde_json::to_value(cluster).unwrap();
        assert_eq!(json["properties"]["publicNetworkAccess"], "Disabled");
        assert_eq!(json["properties"]["highAvailability"], "Disabled");
        assert!(json["properties"].get("hostName").is_none());
        assert!(json["properties"].get("resourceState").is_none());
    }

    #[test]
    fn database_deserializes_a_real_read_back() {
        // Field names and values following the documented API schema.
        let body = r#"{"properties":{"clientProtocol":"Encrypted","port":10000,
            "provisioningState":"Succeeded","resourceState":"Running",
            "clusteringPolicy":"NoCluster","evictionPolicy":"VolatileLRU",
            "accessKeysAuthentication":"Enabled",
            "persistence":{"rdbEnabled":false}}}"#;
        let db: ManagedRedisDatabase = serde_json::from_str(body).unwrap();
        assert_eq!(db.properties.port, 10000);
        assert_eq!(db.properties.eviction_policy, "VolatileLRU");
        assert!(!db.properties.persistence.unwrap().rdb_enabled);
    }

    #[test]
    fn rdb_frequency_is_omitted_when_persistence_is_off() {
        let json = serde_json::to_value(ManagedRedisPersistence {
            rdb_enabled: false,
            rdb_frequency: None,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({"rdbEnabled": false}));
    }

    #[test]
    fn access_keys_never_print() {
        let keys: ManagedRedisAccessKeys =
            serde_json::from_str(r#"{"primaryKey":"hunter2hunter2","secondaryKey":"swordfish"}"#)
                .unwrap();
        let printed = format!("{keys:?}");
        assert!(!printed.contains("hunter2") && !printed.contains("swordfish"));
    }
}
