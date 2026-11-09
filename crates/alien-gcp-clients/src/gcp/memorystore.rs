//! Memorystore for Valkey client (REST/JSON, API v1) and the Network Connectivity service
//! connection policy it needs for Private Service Connect. Only the operations the Redis
//! controller needs are implemented.

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::longrunning::Operation;
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

#[derive(Debug)]
pub struct MemorystoreServiceConfig;

impl GcpServiceConfig for MemorystoreServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://memorystore.googleapis.com/v1"
    }
    fn default_audience(&self) -> &'static str {
        "https://memorystore.googleapis.com/"
    }
    fn service_name(&self) -> &'static str {
        "Memorystore"
    }
    fn service_key(&self) -> &'static str {
        "memorystore"
    }
}

#[derive(Debug)]
pub struct NetworkConnectivityServiceConfig;

impl GcpServiceConfig for NetworkConnectivityServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://networkconnectivity.googleapis.com/v1"
    }
    fn default_audience(&self) -> &'static str {
        "https://networkconnectivity.googleapis.com/"
    }
    fn service_name(&self) -> &'static str {
        "Network Connectivity"
    }
    fn service_key(&self) -> &'static str {
        "networkconnectivity"
    }
}

// ─────────────────────────── data shapes (camelCase JSON) ───────────────────────────

/// A Memorystore for Valkey instance. Fields not listed are ignored on read.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    /// Full resource name (response only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// CREATING / ACTIVE / UPDATING / DELETING (response only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// `CLUSTER_DISABLED`: one shard, one endpoint, `SELECT` works.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// For example `VALKEY_9_0`. Unset defaults to 7.2, so always set it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shard_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_count: Option<u32>,
    /// `TOKEN_AUTH` for password-style clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_mode: Option<String>,
    /// `SERVER_AUTHENTICATION` (TLS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transit_encryption_mode: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub engine_configs: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistence_config: Option<PersistenceConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_distribution_config: Option<ZoneDistributionConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<Endpoint>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletion_protection_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_config: Option<NodeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeConfig {
    pub size_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceConfig {
    /// `DISABLED` or `RDB`.
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rdb_config: Option<RdbConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RdbConfig {
    /// For example `SIX_HOURS`.
    pub rdb_snapshot_period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZoneDistributionConfig {
    /// `MULTI_ZONE` or `SINGLE_ZONE`.
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    #[serde(default)]
    pub connections: Vec<EndpointConnection>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EndpointConnection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psc_auto_connection: Option<PscAutoConnection>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PscAutoConnection {
    /// The consumer network, `projects/{p}/global/networks/{n}`.
    pub network: String,
    pub project_id: String,
    /// PSC address in the consumer subnet (response only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// `CONNECTION_TYPE_PRIMARY` or `CONNECTION_TYPE_READER` (response only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_type: Option<String>,
}

impl Instance {
    /// The primary PSC address and port, once the instance reports them.
    pub fn primary_address(&self) -> Option<(&str, u16)> {
        self.endpoints
            .iter()
            .flat_map(|e| &e.connections)
            .filter_map(|c| c.psc_auto_connection.as_ref())
            .find(|c| c.connection_type.as_deref() == Some("CONNECTION_TYPE_PRIMARY"))
            .and_then(|c| Some((c.ip_address.as_deref()?, c.port.unwrap_or(6379))))
    }
}

/// The instance CA. A CA holds the intermediate and the root certificate together; the
/// binding must carry every one of them or TLS verification fails.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CertificateAuthority {
    #[serde(default)]
    pub managed_server_ca: ManagedServerCa,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedServerCa {
    #[serde(default)]
    pub ca_certs: Vec<CaCerts>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CaCerts {
    #[serde(default)]
    pub certificates: Vec<String>,
}

impl CertificateAuthority {
    /// Every PEM certificate of every CA, in order.
    pub fn all_certificates(&self) -> Vec<String> {
        self.managed_server_ca
            .ca_certs
            .iter()
            .flat_map(|c| c.certificates.iter().cloned())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenAuthUser {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// ACTIVE once usable (response only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// One auth token of a user. The value is a secret: keep it out of logs and state.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthToken {
    pub name: String,
    pub token: String,
    #[serde(default)]
    pub state: Option<String>,
}

impl Debug for AuthToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthToken")
            .field("name", &self.name)
            .field("token", &"<redacted>")
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListAuthTokensResponse {
    #[serde(default)]
    auth_tokens: Vec<AuthToken>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListInstancesResponse {
    #[serde(default)]
    instances: Vec<Instance>,
    #[serde(default)]
    next_page_token: Option<String>,
}

/// Network Connectivity service connection policy. One may exist per
/// (network, region, service class); an existing one is reused, never replaced.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConnectionPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `projects/{p}/global/networks/{n}`.
    pub network: String,
    /// `gcp-memorystore`.
    pub service_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub psc_config: Option<PolicyPscConfig>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PolicyPscConfig {
    /// Subnetworks PSC addresses are allocated from: `projects/{p}/regions/{r}/subnetworks/{s}`.
    #[serde(default)]
    pub subnetworks: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListPoliciesResponse {
    #[serde(default)]
    service_connection_policies: Vec<ServiceConnectionPolicy>,
}

// ─────────────────────────── API traits ───────────────────────────

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait MemorystoreApi: Send + Sync + Debug {
    async fn create_instance(
        &self,
        location: &str,
        instance_id: &str,
        instance: Instance,
    ) -> Result<Operation>;
    /// `update_mask` is a comma-separated list such as `nodeType,engineConfigs`.
    async fn patch_instance(
        &self,
        location: &str,
        instance_id: &str,
        update_mask: &str,
        patch: Instance,
    ) -> Result<Operation>;
    async fn get_instance(&self, location: &str, instance_id: &str) -> Result<Instance>;
    async fn list_instances(&self, location: &str) -> Result<Vec<Instance>>;
    async fn delete_instance(&self, location: &str, instance_id: &str) -> Result<Operation>;
    async fn get_certificate_authority(
        &self,
        location: &str,
        instance_id: &str,
    ) -> Result<CertificateAuthority>;
    async fn create_token_auth_user(
        &self,
        location: &str,
        instance_id: &str,
        user: &str,
    ) -> Result<Operation>;
    async fn get_token_auth_user(
        &self,
        location: &str,
        instance_id: &str,
        user: &str,
    ) -> Result<TokenAuthUser>;
    /// Reads the user's tokens. Needs a role that can see secrets: only the controller calls it.
    async fn list_auth_tokens(
        &self,
        location: &str,
        instance_id: &str,
        user: &str,
    ) -> Result<Vec<AuthToken>>;
    async fn get_operation(&self, operation_name: &str) -> Result<Operation>;
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ServiceConnectionPolicyApi: Send + Sync + Debug {
    async fn list_policies(&self, location: &str) -> Result<Vec<ServiceConnectionPolicy>>;
    async fn create_policy(
        &self,
        location: &str,
        policy_id: &str,
        policy: ServiceConnectionPolicy,
    ) -> Result<Operation>;
    async fn delete_policy(&self, location: &str, policy_id: &str) -> Result<Operation>;
    async fn get_operation(&self, operation_name: &str) -> Result<Operation>;
}

// ─────────────────────────── clients ───────────────────────────

#[derive(Debug)]
pub struct MemorystoreClient {
    base: GcpClientBase,
    project_id: String,
}

impl MemorystoreClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(MemorystoreServiceConfig)),
            project_id,
        }
    }

    fn instance_path(&self, location: &str, instance_id: &str) -> String {
        format!(
            "projects/{}/locations/{}/instances/{}",
            self.project_id, location, instance_id
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl MemorystoreApi for MemorystoreClient {
    async fn create_instance(
        &self,
        location: &str,
        instance_id: &str,
        instance: Instance,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/instances",
            self.project_id, location
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(vec![("instanceId", instance_id.to_string())]),
                Some(instance),
                instance_id,
            )
            .await
    }

    async fn patch_instance(
        &self,
        location: &str,
        instance_id: &str,
        update_mask: &str,
        patch: Instance,
    ) -> Result<Operation> {
        // A lost response must be re-read, not replayed against a changed instance.
        self.base
            .execute_request_once(
                Method::PATCH,
                &self.instance_path(location, instance_id),
                Some(vec![("updateMask", update_mask.to_string())]),
                Some(patch),
                instance_id,
            )
            .await
    }

    async fn get_instance(&self, location: &str, instance_id: &str) -> Result<Instance> {
        self.base
            .execute_request(
                Method::GET,
                &self.instance_path(location, instance_id),
                None,
                Option::<()>::None,
                instance_id,
            )
            .await
    }

    async fn list_instances(&self, location: &str) -> Result<Vec<Instance>> {
        let path = format!(
            "projects/{}/locations/{}/instances",
            self.project_id, location
        );
        let mut all = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let query = page_token.clone().map(|token| vec![("pageToken", token)]);
            let page: ListInstancesResponse = self
                .base
                .execute_request(Method::GET, &path, query, Option::<()>::None, location)
                .await?;
            all.extend(page.instances);
            match page.next_page_token.filter(|t| !t.is_empty()) {
                Some(token) => page_token = Some(token),
                None => return Ok(all),
            }
        }
    }

    async fn delete_instance(&self, location: &str, instance_id: &str) -> Result<Operation> {
        self.base
            .execute_request(
                Method::DELETE,
                &self.instance_path(location, instance_id),
                None,
                Option::<()>::None,
                instance_id,
            )
            .await
    }

    async fn get_certificate_authority(
        &self,
        location: &str,
        instance_id: &str,
    ) -> Result<CertificateAuthority> {
        let path = format!(
            "{}/certificateAuthority",
            self.instance_path(location, instance_id)
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, instance_id)
            .await
    }

    async fn create_token_auth_user(
        &self,
        location: &str,
        instance_id: &str,
        user: &str,
    ) -> Result<Operation> {
        let path = format!(
            "{}/tokenAuthUsers",
            self.instance_path(location, instance_id)
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(vec![("tokenAuthUserId", user.to_string())]),
                Some(TokenAuthUser {
                    name: None,
                    state: None,
                }),
                user,
            )
            .await
    }

    async fn get_token_auth_user(
        &self,
        location: &str,
        instance_id: &str,
        user: &str,
    ) -> Result<TokenAuthUser> {
        let path = format!(
            "{}/tokenAuthUsers/{}",
            self.instance_path(location, instance_id),
            user
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, user)
            .await
    }

    async fn list_auth_tokens(
        &self,
        location: &str,
        instance_id: &str,
        user: &str,
    ) -> Result<Vec<AuthToken>> {
        let path = format!(
            "{}/tokenAuthUsers/{}/authTokens",
            self.instance_path(location, instance_id),
            user
        );
        // The response body is the secret: a 2xx response that fails to parse must not leave its
        // text in the error.
        let response: ListAuthTokensResponse = alien_client_core::redact_response_body(
            self.base
                .execute_request(Method::GET, &path, None, Option::<()>::None, user)
                .await,
        )?;
        Ok(response.auth_tokens)
    }

    async fn get_operation(&self, operation_name: &str) -> Result<Operation> {
        self.base
            .execute_request(
                Method::GET,
                operation_name,
                None,
                Option::<()>::None,
                operation_name,
            )
            .await
    }
}

#[derive(Debug)]
pub struct ServiceConnectionPolicyClient {
    base: GcpClientBase,
    project_id: String,
}

impl ServiceConnectionPolicyClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(NetworkConnectivityServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ServiceConnectionPolicyApi for ServiceConnectionPolicyClient {
    async fn list_policies(&self, location: &str) -> Result<Vec<ServiceConnectionPolicy>> {
        let path = format!(
            "projects/{}/locations/{}/serviceConnectionPolicies",
            self.project_id, location
        );
        let response: ListPoliciesResponse = self
            .base
            .execute_request(Method::GET, &path, None, Option::<()>::None, location)
            .await?;
        Ok(response.service_connection_policies)
    }

    async fn create_policy(
        &self,
        location: &str,
        policy_id: &str,
        policy: ServiceConnectionPolicy,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/serviceConnectionPolicies",
            self.project_id, location
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(vec![("serviceConnectionPolicyId", policy_id.to_string())]),
                Some(policy),
                policy_id,
            )
            .await
    }

    async fn delete_policy(&self, location: &str, policy_id: &str) -> Result<Operation> {
        let path = format!(
            "projects/{}/locations/{}/serviceConnectionPolicies/{}",
            self.project_id, location, policy_id
        );
        self.base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, policy_id)
            .await
    }

    async fn get_operation(&self, operation_name: &str) -> Result<Operation> {
        self.base
            .execute_request(
                Method::GET,
                operation_name,
                None,
                Option::<()>::None,
                operation_name,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from a real instance read in the test project (Valkey, cluster disabled).
    const INSTANCE: &str = r#"{
      "name": "projects/p/locations/us-east4/instances/redis-m0",
      "state": "ACTIVE", "mode": "CLUSTER_DISABLED", "engineVersion": "VALKEY_9_0",
      "nodeType": "STANDARD_SMALL", "shardCount": 1, "replicaCount": 0,
      "authorizationMode": "TOKEN_AUTH", "transitEncryptionMode": "SERVER_AUTHENTICATION",
      "engineConfigs": {"maxmemory-policy": "allkeys-lru"},
      "persistenceConfig": {"mode": "RDB", "rdbConfig": {"rdbSnapshotPeriod": "SIX_HOURS", "rdbSnapshotStartTime": "2026-11-08T12:16:20Z"}},
      "nodeConfig": {"sizeGb": 6.5},
      "endpoints": [{"connections": [
        {"pscAutoConnection": {"connectionType": "CONNECTION_TYPE_READER", "ipAddress": "10.77.0.2", "port": 6379, "network": "n", "projectId": "p"}},
        {"pscAutoConnection": {"connectionType": "CONNECTION_TYPE_PRIMARY", "ipAddress": "10.77.0.3", "port": 6379, "network": "n", "projectId": "p"}}
      ]}]
    }"#;

    #[test]
    fn instance_parses_and_picks_the_primary_address_not_the_reader() {
        let instance: Instance = serde_json::from_str(INSTANCE).unwrap();
        assert_eq!(instance.primary_address(), Some(("10.77.0.3", 6379)));
        assert_eq!(instance.engine_configs["maxmemory-policy"], "allkeys-lru");
        assert_eq!(instance.persistence_config.unwrap().mode, "RDB");
    }

    #[test]
    fn instance_without_endpoints_has_no_address_yet() {
        let instance: Instance = serde_json::from_str(r#"{"state":"CREATING"}"#).unwrap();
        assert_eq!(instance.primary_address(), None);
    }

    #[test]
    fn a_create_body_omits_response_only_fields() {
        let body = serde_json::to_value(Instance {
            mode: Some("CLUSTER_DISABLED".into()),
            engine_version: Some("VALKEY_9_0".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(body["mode"], "CLUSTER_DISABLED");
        assert!(body.get("state").is_none() && body.get("endpoints").is_none());
    }

    #[test]
    fn certificate_authority_keeps_every_certificate_of_the_chain() {
        let ca: CertificateAuthority = serde_json::from_str(
            r#"{"managedServerCa":{"caCerts":[{"certificates":["-----A-----","-----B-----"]}]}}"#,
        )
        .unwrap();
        assert_eq!(ca.all_certificates(), vec!["-----A-----", "-----B-----"]);
    }

    #[test]
    fn auth_token_debug_hides_the_token() {
        let tokens: ListAuthTokensResponse = serde_json::from_str(
            r#"{"authTokens":[{"name":"n/version-1","token":"s3cret","state":"ACTIVE"}]}"#,
        )
        .unwrap();
        assert_eq!(tokens.auth_tokens[0].token, "s3cret");
        assert!(!format!("{:?}", tokens.auth_tokens[0]).contains("s3cret"));
    }

    /// A 2xx token list that doesn't parse must not leave the plaintext tokens in the error
    /// chain, which callers log and persist.
    #[tokio::test]
    async fn an_unparseable_token_list_does_not_leak_the_token() {
        use alien_core::{GcpClientConfig, GcpCredentials, GcpServiceOverrides};
        use httpmock::prelude::*;

        let server = MockServer::start_async().await;
        // `authTokens` is an object instead of a list, so parsing fails after the 200.
        server
            .mock_async(|when, then| {
                when.method(GET).path(
                    "/projects/p/locations/us-east4/instances/i/tokenAuthUsers/default/authTokens",
                );
                then.status(200)
                    .body(r#"{"authTokens":{"token":"PLAINTEXT-TOKEN-VALUE"}}"#);
            })
            .await;
        let client = MemorystoreClient::new(
            Client::new(),
            GcpClientConfig {
                project_id: "p".to_string(),
                region: "us-east4".to_string(),
                credentials: GcpCredentials::AccessToken {
                    token: "test-token".to_string(),
                },
                service_overrides: Some(GcpServiceOverrides {
                    endpoints: std::collections::HashMap::from([(
                        "memorystore".to_string(),
                        server.base_url(),
                    )]),
                }),
                project_number: None,
            },
        );
        let error = client
            .list_auth_tokens("us-east4", "i", "default")
            .await
            .expect_err("the response does not parse");
        let rendered = format!("{error:?} {}", serde_json::to_string(&error).unwrap());
        assert!(
            !rendered.contains("PLAINTEXT-TOKEN-VALUE"),
            "token leaked into the error: {rendered}"
        );
    }
}
