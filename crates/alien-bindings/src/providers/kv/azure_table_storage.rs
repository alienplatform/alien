use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use alien_core::{AzureClientConfig, AzureCredentials};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use azure_core::{
    cloud::{CloudConfiguration, CustomConfiguration},
    credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions},
    http::ClientOptions,
    time::{Duration as AzureDuration, OffsetDateTime},
};
use azure_identity::{
    ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
    ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
    WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use reqwest::{Client, Method, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};

use super::{validate_key, validate_value};

const STORAGE_SCOPE: &str = "https://storage.azure.com/.default";
const TABLE_API_VERSION: &str = "2020-12-06";

/// Convert a KV operation to a Table Storage entity.
fn create_table_entity(
    partition_key: String,
    row_key: String,
    value: &[u8],
    expires_at: Option<DateTime<Utc>>,
) -> TableEntity {
    let mut properties = HashMap::new();
    properties.insert("Value".to_string(), Value::String(BASE64.encode(value)));
    properties.insert(
        "CreatedAt".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );

    if let Some(expiry) = expires_at {
        properties.insert("ExpiresAt".to_string(), Value::String(expiry.to_rfc3339()));
    }

    TableEntity {
        partition_key,
        row_key,
        timestamp: None,
        properties,
    }
}

/// Extract KV value from Table Storage entity.
fn extract_value_from_entity(entity: &TableEntity) -> Result<Vec<u8>> {
    let value_str = entity
        .properties
        .get("Value")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidInput {
                operation_context: "Azure Table Storage KV extract value".to_string(),
                details: "Entity missing Value property or not a string".to_string(),
                field_name: Some("Value".to_string()),
            })
        })?;

    BASE64
        .decode(value_str)
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            operation_context: "Azure Table Storage KV extract value".to_string(),
            details: "Failed to decode base64 value".to_string(),
            field_name: Some("Value".to_string()),
        })
}

/// Check if entity has expired based on TTL.
fn is_entity_expired(entity: &TableEntity) -> bool {
    entity
        .properties
        .get("ExpiresAt")
        .and_then(|value| value.as_str())
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|expires_at| Utc::now() > expires_at.with_timezone(&Utc))
}

/// Cursor state for pagination across partitions.
#[derive(Serialize, Deserialize)]
struct CursorState {
    current_partition: u32,
    next_partition_key: Option<String>,
    next_row_key: Option<String>,
}

/// Azure Table Storage implementation of the KV trait.
pub struct AzureTableStorageKv {
    resource_group_name: String,
    account_name: String,
    table_name: String,
    endpoint: String,
    client: Client,
    credential: Arc<dyn TokenCredential>,
    num_partitions: u32,
}

impl Debug for AzureTableStorageKv {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureTableStorageKv")
            .field("resource_group_name", &self.resource_group_name)
            .field("account_name", &self.account_name)
            .field("table_name", &self.table_name)
            .field("endpoint", &self.endpoint)
            .field("num_partitions", &self.num_partitions)
            .finish()
    }
}

impl AzureTableStorageKv {
    pub fn new(
        azure_config: &AzureClientConfig,
        resource_group_name: String,
        account_name: String,
        table_name: String,
    ) -> Result<Self> {
        let endpoint = azure_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("table"))
            .cloned()
            .unwrap_or_else(|| format!("https://{}.table.core.windows.net", account_name));

        Ok(Self {
            resource_group_name,
            account_name,
            table_name,
            endpoint,
            client: crate::http_client::create_http_client(),
            credential: azure_credential_from_config(azure_config)?,
            num_partitions: 16,
        })
    }

    /// Creates a hash bucket for load distribution.
    fn hash_bucket(&self, key: &str) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish() as u32 % self.num_partitions
    }

    /// Splits key into partition key and row key.
    fn split_key(&self, key: &str) -> (String, String) {
        let partition_key = format!("p{}", self.hash_bucket(key));
        (partition_key, key.to_string())
    }

    /// Combines partition key and row key back to original key.
    fn combine_key(&self, _partition_key: &str, row_key: &str) -> String {
        row_key.to_string()
    }

    /// Encodes cursor state as base64 JSON for safe HTTP transmission.
    fn encode_cursor(&self, state: &CursorState) -> String {
        let json = serde_json::to_string(state).unwrap();
        BASE64.encode(json.as_bytes())
    }

    /// Decodes cursor state from base64 JSON.
    fn decode_cursor(&self, cursor: &str) -> Result<CursorState> {
        let decoded =
            BASE64
                .decode(cursor)
                .into_alien_error()
                .context(ErrorData::InvalidInput {
                    operation_context: "Azure Table Storage KV cursor decoding".to_string(),
                    details: "Invalid cursor encoding".to_string(),
                    field_name: Some("cursor".to_string()),
                })?;
        let json =
            String::from_utf8(decoded)
                .into_alien_error()
                .context(ErrorData::InvalidInput {
                    operation_context: "Azure Table Storage KV cursor decoding".to_string(),
                    details: "Invalid cursor UTF-8".to_string(),
                    field_name: Some("cursor".to_string()),
                })?;
        serde_json::from_str(&json)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                operation_context: "Azure Table Storage KV cursor decoding".to_string(),
                details: "Invalid cursor JSON".to_string(),
                field_name: Some("cursor".to_string()),
            })
    }

    fn build_url(&self, path: &str, query_params: &[(&str, String)]) -> Result<Url> {
        let mut url = Url::parse(&format!(
            "{}/{}",
            self.endpoint.trim_end_matches('/'),
            path.trim_start_matches('/')
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "kv.azureTable".to_string(),
            reason: format!("Invalid Azure Table Storage URL for path '{path}'"),
        })?;

        if !query_params.is_empty() {
            let mut query = url.query_pairs_mut();
            for (key, value) in query_params {
                query.append_pair(key, value);
            }
        }

        Ok(url)
    }

    fn entity_path(&self, partition_key: &str, row_key: &str) -> String {
        format!(
            "{}(PartitionKey='{}',RowKey='{}')",
            self.table_name,
            urlencoding::encode(partition_key),
            urlencoding::encode(row_key)
        )
    }

    async fn bearer_token(&self) -> Result<AccessToken> {
        self.credential
            .get_token(&[STORAGE_SCOPE], None)
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "kv.azureTable".to_string(),
                reason: "Failed to get Azure Table Storage bearer token".to_string(),
            })
    }

    async fn send_request(
        &self,
        method: Method,
        url: Url,
        body: Option<String>,
        if_match: Option<&str>,
    ) -> Result<Response> {
        let token = self.bearer_token().await?;
        let mut request = self
            .client
            .request(method, url.clone())
            .bearer_auth(token.token.secret())
            .header("x-ms-version", TABLE_API_VERSION)
            .header("DataServiceVersion", "3.0;NetFx")
            .header("MaxDataServiceVersion", "3.0;NetFx")
            .header("Accept", "application/json;odata=minimalmetadata");

        if let Some(if_match) = if_match {
            request = request.header("If-Match", if_match);
        }

        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        } else {
            request = request.header("Content-Length", "0");
        }

        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::KvOperationFailed {
                operation: "azure_table_request".to_string(),
                key: url.to_string(),
                reason: "Failed to execute Azure Table Storage request".to_string(),
            })
    }

    async fn get_entity(&self, partition_key: &str, row_key: &str) -> Result<Option<TableEntity>> {
        let url = self.build_url(&self.entity_path(partition_key, row_key), &[])?;
        let response = self
            .send_request(Method::GET, url.clone(), None, None)
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        ensure_success(response, "get", row_key, url)
            .await?
            .json::<TableEntity>()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "azure".to_string(),
                binding_name: "tableStorage".to_string(),
                field: "entity".to_string(),
                response_json: String::new(),
            })
            .map(Some)
    }

    async fn query_entities(
        &self,
        filter: String,
        top: u32,
        continuation: Option<&CursorState>,
    ) -> Result<EntityQueryResponse> {
        let mut query_params = vec![("$filter", filter), ("$top", top.to_string())];
        if let Some(state) = continuation {
            if let Some(next_partition_key) = &state.next_partition_key {
                query_params.push(("NextPartitionKey", next_partition_key.clone()));
            }
            if let Some(next_row_key) = &state.next_row_key {
                query_params.push(("NextRowKey", next_row_key.clone()));
            }
        }

        let url = self.build_url(&self.table_name, &query_params)?;
        let response = self
            .send_request(Method::GET, url.clone(), None, None)
            .await?;
        let next_partition_key = response
            .headers()
            .get("x-ms-continuation-NextPartitionKey")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let next_row_key = response
            .headers()
            .get("x-ms-continuation-NextRowKey")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        let mut query_response = ensure_success(response, "query", &self.table_name, url)
            .await?
            .json::<EntityQueryResponse>()
            .await
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "azure".to_string(),
                binding_name: "tableStorage".to_string(),
                field: "queryResponse".to_string(),
                response_json: String::new(),
            })?;
        query_response.next_partition_key = next_partition_key;
        query_response.next_row_key = next_row_key;

        Ok(query_response)
    }
}

impl Binding for AzureTableStorageKv {}

#[async_trait]
impl Kv for AzureTableStorageKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        validate_key(key)?;

        let (partition_key, row_key) = self.split_key(key);
        let Some(entity) = self.get_entity(&partition_key, &row_key).await? else {
            return Ok(None);
        };

        if is_entity_expired(&entity) {
            return Ok(None);
        }

        extract_value_from_entity(&entity).map(Some)
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        validate_key(key)?;
        validate_value(&value)?;

        let options = options.unwrap_or_default();
        let (partition_key, row_key) = self.split_key(key);
        let expires_at = options.ttl.map(|duration| Utc::now() + duration);
        let entity =
            create_table_entity(partition_key.clone(), row_key.clone(), &value, expires_at);
        let body = serde_json::to_string(&entity).into_alien_error().context(
            ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.to_string(),
                reason: "Failed to serialize Azure Table Storage entity".to_string(),
            },
        )?;

        if options.if_not_exists {
            let url = self.build_url(&self.table_name, &[])?;
            let response = self
                .send_request(Method::POST, url.clone(), Some(body), None)
                .await?;

            if response.status() == StatusCode::CONFLICT {
                return Ok(false);
            }

            ensure_success(response, "insert", key, url).await?;
            Ok(true)
        } else {
            let url = self.build_url(&self.entity_path(&partition_key, &row_key), &[])?;
            let response = self
                .send_request(Method::PUT, url.clone(), Some(body), None)
                .await?;

            ensure_success(response, "upsert", key, url).await?;
            Ok(true)
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;

        let (partition_key, row_key) = self.split_key(key);
        let url = self.build_url(&self.entity_path(&partition_key, &row_key), &[])?;
        let response = self
            .send_request(Method::DELETE, url.clone(), None, Some("*"))
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }

        ensure_success(response, "delete", key, url).await?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        validate_key(key)?;

        let (partition_key, row_key) = self.split_key(key);
        let Some(entity) = self.get_entity(&partition_key, &row_key).await? else {
            return Ok(false);
        };

        Ok(!is_entity_expired(&entity))
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        validate_key(prefix)?;

        let cursor_state = cursor.as_ref().map(|c| self.decode_cursor(c)).transpose()?;
        let mut all_items = Vec::new();
        let mut total_fetched = 0;
        let limit = limit.unwrap_or(1000);
        let start_partition = cursor_state.as_ref().map_or(0, |cs| cs.current_partition);

        for partition_id in start_partition..self.num_partitions {
            let partition_key = format!("p{}", partition_id);
            let prefix_end = format!("{}~", prefix);
            let filter = format!(
                "(PartitionKey eq '{}') and (RowKey ge '{}') and (RowKey lt '{}')",
                partition_key, prefix, prefix_end
            );
            let continuation = cursor_state
                .as_ref()
                .filter(|state| state.current_partition == partition_id);
            let response = self
                .query_entities(filter, (limit - total_fetched) as u32, continuation)
                .await?;

            for entity in response.entities {
                if total_fetched >= limit {
                    break;
                }

                if is_entity_expired(&entity) {
                    continue;
                }

                let key = self.combine_key(&entity.partition_key, &entity.row_key);
                let value = extract_value_from_entity(&entity)?;

                all_items.push((key, value));
                total_fetched += 1;
            }

            if total_fetched >= limit
                || response.next_partition_key.is_some()
                || response.next_row_key.is_some()
            {
                let next_cursor = self.encode_cursor(&CursorState {
                    current_partition: partition_id,
                    next_partition_key: response.next_partition_key,
                    next_row_key: response.next_row_key,
                });
                return Ok(ScanResult {
                    items: all_items,
                    next_cursor: Some(next_cursor),
                });
            }
        }

        Ok(ScanResult {
            items: all_items,
            next_cursor: None,
        })
    }
}

async fn ensure_success(
    response: Response,
    operation: &str,
    key: &str,
    url: Url,
) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(AlienError::new(ErrorData::KvOperationFailed {
        operation: operation.to_string(),
        key: key.to_string(),
        reason: format!("Azure Table Storage request to {url} failed with status {status}: {body}"),
    }))
}

#[derive(Debug)]
struct StaticAzureAccessTokenCredential {
    token: String,
}

#[async_trait]
impl TokenCredential for StaticAzureAccessTokenCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::Result<AccessToken> {
        if scopes.is_empty() {
            return Err(azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "no scopes specified",
            ));
        }

        Ok(AccessToken::new(
            self.token.clone(),
            OffsetDateTime::now_utc() + AzureDuration::days(365),
        ))
    }
}

fn azure_credential_from_config(config: &AzureClientConfig) -> Result<Arc<dyn TokenCredential>> {
    match &config.credentials {
        AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
            token: token.clone(),
        })),
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => ClientSecretCredential::new(
            &config.tenant_id,
            client_id.clone(),
            Secret::new(client_secret.clone()),
            Some(ClientSecretCredentialOptions {
                client_options: azure_client_options(None),
            }),
        )
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "kv.azureTable".to_string(),
            reason: "Failed to build official Azure service principal credentials".to_string(),
        }),
        AzureCredentials::WorkloadIdentity {
            client_id,
            tenant_id,
            federated_token_file,
            authority_host,
        } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
            credential_options: ClientAssertionCredentialOptions {
                client_options: azure_client_options(Some(authority_host)),
            },
            client_id: Some(client_id.clone()),
            tenant_id: Some(tenant_id.clone()),
            token_file_path: Some(PathBuf::from(federated_token_file)),
        }))
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "kv.azureTable".to_string(),
            reason: "Failed to build official Azure workload identity credentials".to_string(),
        }),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "kv.azureTable".to_string(),
                    reason: format!(
                        "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                        identity_endpoint
                    ),
                }));
            }

            ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                client_options: azure_client_options(None),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "kv.azureTable".to_string(),
                reason: "Failed to build official Azure VM managed identity credentials"
                    .to_string(),
            })
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => Err(AlienError::new(ErrorData::BindingSetupFailed {
            binding_type: "kv.azureTable".to_string(),
            reason: format!(
                "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
                identity_endpoint, client_id
            ),
        })),
    }
}

fn azure_client_options(authority_host: Option<&str>) -> ClientOptions {
    let cloud = authority_host.map(|authority_host| {
        let mut custom = CustomConfiguration::default();
        custom.authority_host = authority_host.to_string();
        Arc::new(CloudConfiguration::Custom(custom))
    });

    ClientOptions {
        cloud,
        ..Default::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TableEntity {
    #[serde(rename = "PartitionKey")]
    partition_key: String,
    #[serde(rename = "RowKey")]
    row_key: String,
    #[serde(rename = "Timestamp", skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
    #[serde(flatten)]
    properties: HashMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct EntityQueryResponse {
    #[serde(rename = "value")]
    entities: Vec<TableEntity>,
    #[serde(skip)]
    next_partition_key: Option<String>,
    #[serde(skip)]
    next_row_key: Option<String>,
}
