use crate::azure::common::{
    create_azure_http_error_with_context, AzureClientBase, AzureRequestBuilder,
};
use crate::azure::models::table::*;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::Utc;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

// -----------------------------------------------------------------------------
// Entity data structures for Table Storage operations
// -----------------------------------------------------------------------------

/// Represents a Table Storage entity with required PartitionKey and RowKey
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableEntity {
    #[serde(rename = "PartitionKey")]
    pub partition_key: String,
    #[serde(rename = "RowKey")]
    pub row_key: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: Option<String>,
    #[serde(flatten)]
    pub properties: HashMap<String, Value>,
}

/// Response from entity query operations
#[derive(Debug, Clone, Deserialize)]
pub struct EntityQueryResponse {
    #[serde(rename = "odata.metadata")]
    pub metadata: Option<String>,
    #[serde(rename = "value")]
    pub entities: Vec<TableEntity>,
    #[serde(rename = "odata.nextLink")]
    pub next_link: Option<String>,
}

/// Query options for entity operations
#[derive(Debug, Clone, Default)]
pub struct EntityQueryOptions {
    pub filter: Option<String>,
    pub select: Option<String>,
    pub top: Option<u32>,
}

/// ETag wrapper for conditional operations
#[derive(Debug, Clone)]
pub struct ETag(pub String);

impl From<String> for ETag {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ETag {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

// -----------------------------------------------------------------------------
// Table Management API (Control Plane)
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait TableManagementApi: Send + Sync + std::fmt::Debug {
    async fn create_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<Table>;

    async fn delete_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<()>;

    async fn get_table_acl(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<Vec<TableSignedIdentifier>>;

    async fn set_table_acl(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        signed_identifiers: &[TableSignedIdentifier],
    ) -> Result<()>;
}

// -----------------------------------------------------------------------------
// Table Storage API (Data Plane)
// -----------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait TableStorageApi: Send + Sync + std::fmt::Debug {
    async fn insert_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        entity: &TableEntity,
    ) -> Result<TableEntity>;

    async fn update_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
        etag: Option<ETag>,
    ) -> Result<TableEntity>;

    async fn merge_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
        etag: Option<ETag>,
    ) -> Result<TableEntity>;

    async fn delete_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        etag: Option<ETag>,
    ) -> Result<()>;

    async fn insert_or_replace_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
    ) -> Result<TableEntity>;

    async fn insert_or_merge_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
    ) -> Result<TableEntity>;

    async fn query_entities(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        options: Option<EntityQueryOptions>,
    ) -> Result<EntityQueryResponse>;

    async fn get_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        select: Option<String>,
    ) -> Result<TableEntity>;
}

// -----------------------------------------------------------------------------
// Table Management client (Control Plane)
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureTableManagementClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureTableManagementClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(client, endpoint, token_cache.config().clone()),
            token_cache,
        }
    }

    fn build_table_url(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Storage/storageAccounts/{}/tableServices/default/tables/{}",
            self.token_cache.config().subscription_id,
            resource_group_name,
            storage_account_name,
            table_name
        )
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl TableManagementApi for AzureTableManagementClient {
    async fn create_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<Table> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.build_table_url(resource_group_name, storage_account_name, table_name),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let table_properties = TableProperties {
            table_name: Some(table_name.to_string()),
            signed_identifiers: vec![],
        };

        let table = Table {
            id: None,
            name: Some(table_name.to_string()),
            properties: Some(table_properties),
            type_: None,
        };

        let body = serde_json::to_string(&table).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize table '{}'", table_name),
            },
        )?;
        let request_body = body.clone();

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "CreateTable", table_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Azure CreateTable: failed to read response body".to_string(),
            })?;

        let table: Table = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!("Azure CreateTable: JSON parse error. Body: {}", body),
                url: url.clone(),
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: Some(request_body),
            },
        )?;

        Ok(table)
    }

    async fn delete_table(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.build_table_url(resource_group_name, storage_account_name, table_name),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "DeleteTable", table_name)
            .await?;

        Ok(())
    }

    async fn get_table_acl(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
    ) -> Result<Vec<TableSignedIdentifier>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.build_table_url(resource_group_name, storage_account_name, table_name),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone());

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetTableAcl", table_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: "Azure GetTableAcl: failed to read response body".to_string(),
            })?;

        let table: Table = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!("Azure GetTableAcl: JSON parse error. Body: {}", body),
                url: url.clone(),
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(table
            .properties
            .map(|props| props.signed_identifiers)
            .unwrap_or_default())
    }

    async fn set_table_acl(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        signed_identifiers: &[TableSignedIdentifier],
    ) -> Result<()> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &self.build_table_url(resource_group_name, storage_account_name, table_name),
            Some(vec![("api-version", "2024-01-01".into())]),
        );

        let table_properties = TableProperties {
            table_name: Some(table_name.to_string()),
            signed_identifiers: signed_identifiers.to_vec(),
        };

        let table = Table {
            id: None,
            name: Some(table_name.to_string()),
            properties: Some(table_properties),
            type_: None,
        };

        let body = serde_json::to_string(&table).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize table ACL for '{}'", table_name),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url.clone())
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let _resp = self
            .base
            .execute_request(signed, "SetTableAcl", table_name)
            .await?;

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Table Storage client (Data Plane)
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct AzureTableStorageClient {
    pub client: Client,
    pub token_cache: AzureTokenCache,
    pub storage_account_key: String,
}

impl AzureTableStorageClient {
    pub fn new(
        client: Client,
        token_cache: AzureTokenCache,
        storage_account_key: String,
    ) -> Self {
        Self {
            client,
            token_cache,
            storage_account_key,
        }
    }

    /// Build the full URL for Table Storage data plane operations
    fn build_table_storage_url(
        &self,
        storage_account_name: &str,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
    ) -> Result<url::Url> {
        let base_url = if let Some(override_url) = self.token_cache.get_service_endpoint("table")
        {
            override_url.trim_end_matches('/').to_string()
        } else {
            format!("https://{}.table.core.windows.net", storage_account_name)
        };

        let mut url = url::Url::parse(&format!("{}{}", base_url, path))
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: format!("Invalid Table Storage URL: {}{}", base_url, path),
                errors: None,
            })?;

        if let Some(params) = query_params {
            let mut qp = url.query_pairs_mut();
            for (k, v) in params {
                qp.append_pair(k, &v);
            }
        }

        Ok(url)
    }

    /// URL encode a value for use in entity keys
    fn url_encode(&self, value: &str) -> String {
        urlencoding::encode(value).to_string()
    }

    /// Generate shared key signature for Azure Table Storage authentication
    fn generate_shared_key_signature(
        &self,
        method: &str,
        url: &url::Url,
        headers: &HashMap<String, String>,
        storage_account_name: &str,
        storage_account_key: &str,
    ) -> Result<String> {
        // Construct the string to sign according to Azure Table Storage specification
        // For Table Storage, we use x-ms-date preferentially
        let date = headers.get("x-ms-date").ok_or_else(|| {
            AlienError::new(ErrorData::RequestSignError {
                message: "x-ms-date header is required for shared key authentication".to_string(),
            })
        })?;

        let canonicalized_resource = self.canonicalize_resource(url, storage_account_name);

        // String to sign format for Table Storage:
        // VERB + "\n" +
        // Content-MD5 + "\n" +
        // Content-Type + "\n" +
        // Date + "\n" +
        // CanonicalizedResource
        let string_to_sign = format!(
            "{}\n{}\n{}\n{}\n{}",
            method.to_uppercase(),
            headers.get("Content-MD5").unwrap_or(&String::new()),
            headers.get("Content-Type").unwrap_or(&String::new()),
            date,
            canonicalized_resource
        );

        // Create HMAC-SHA256 signature
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;

        let decoded_key = BASE64
            .decode(storage_account_key)
            .into_alien_error()
            .context(ErrorData::RequestSignError {
                message: "Failed to decode storage account key".to_string(),
            })?;

        let mut mac = HmacSha256::new_from_slice(&decoded_key)
            .into_alien_error()
            .context(ErrorData::RequestSignError {
                message: "Failed to create HMAC".to_string(),
            })?;

        mac.update(string_to_sign.as_bytes());
        let signature = mac.finalize().into_bytes();
        let signature_b64 = BASE64.encode(signature);

        Ok(format!(
            "SharedKey {}:{}",
            storage_account_name, signature_b64
        ))
    }

    /// Canonicalize the resource string for shared key authentication
    /// For Table Storage, query parameters should NOT be included in the canonicalized resource
    fn canonicalize_resource(&self, url: &url::Url, storage_account_name: &str) -> String {
        let path = url.path();
        format!("/{}{}", storage_account_name, path)
    }

    /// Execute an HTTP request and handle errors using centralized Azure error mapping
    async fn execute_data_plane_request(
        &self,
        method: &str,
        url: &url::Url,
        body: &str,
        storage_account_name: &str,
        resource_group_name: &str,
        operation_name: &str,
        entity_identifier: &str,
        additional_headers: Option<HashMap<String, String>>,
    ) -> Result<reqwest::Response> {
        let headers = self
            .sign_request_with_shared_key(
                method,
                url,
                body,
                storage_account_name,
                resource_group_name,
            )
            .await?;

        let mut request_builder = match method {
            "GET" => self.client.get(url.to_string()),
            "POST" => self.client.post(url.to_string()),
            "PUT" => self.client.put(url.to_string()),
            "PATCH" => self.client.patch(url.to_string()),
            "DELETE" => self.client.delete(url.to_string()),
            _ => {
                return Err(AlienError::new(ErrorData::InvalidInput {
                    message: format!("Unsupported HTTP method: {}", method),
                    field_name: None,
                }))
            }
        };

        if !body.is_empty() {
            request_builder = request_builder.body(body.to_string());
        }

        // Add authentication and standard headers
        for (key, value) in headers {
            request_builder = request_builder.header(&key, &value);
        }

        // Add any additional headers (like If-Match, Prefer, etc.)
        if let Some(extra_headers) = additional_headers {
            for (key, value) in extra_headers {
                request_builder = request_builder.header(&key, &value);
            }
        }

        let resp = request_builder.send().await.into_alien_error().context(
            ErrorData::HttpRequestFailed {
                message: format!("Azure {}: failed to execute request", operation_name),
            },
        )?;

        let status = resp.status();
        if status.is_success() || status == StatusCode::CREATED || status == StatusCode::ACCEPTED {
            Ok(resp)
        } else {
            let error_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(create_azure_http_error_with_context(
                status,
                operation_name,
                "Azure Table Storage Entity",
                entity_identifier,
                &error_text,
                &url.to_string(),
                if body.is_empty() {
                    None
                } else {
                    Some(body.to_string())
                },
            ))
        }
    }

    /// Sign an HTTP request using shared key authentication
    async fn sign_request_with_shared_key(
        &self,
        method: &str,
        url: &url::Url,
        body: &str,
        storage_account_name: &str,
        _resource_group_name: &str, // No longer needed, but keeping for API compatibility
    ) -> Result<HashMap<String, String>> {
        let storage_account_key = &self.storage_account_key;

        let mut headers = HashMap::new();

        // Add required headers
        let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        headers.insert("x-ms-date".to_string(), date);
        headers.insert("x-ms-version".to_string(), "2020-12-06".to_string());
        headers.insert("Content-Length".to_string(), body.len().to_string());

        // Add OData headers for Table Storage
        headers.insert("DataServiceVersion".to_string(), "3.0;NetFx".to_string());
        headers.insert("MaxDataServiceVersion".to_string(), "3.0;NetFx".to_string());

        if !body.is_empty() {
            headers.insert("Content-Type".to_string(), "application/json".to_string());
        }

        // Generate signature
        let authorization = self.generate_shared_key_signature(
            method,
            url,
            &headers,
            storage_account_name,
            storage_account_key,
        )?;

        headers.insert("Authorization".to_string(), authorization);
        headers.insert(
            "Accept".to_string(),
            "application/json;odata=minimalmetadata".to_string(),
        );

        Ok(headers)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl TableStorageApi for AzureTableStorageClient {
    async fn insert_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        entity: &TableEntity,
    ) -> Result<TableEntity> {
        let url =
            self.build_table_storage_url(storage_account_name, &format!("/{}", table_name), None)?;

        let body = serde_json::to_string(entity).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize entity for table '{}'", table_name),
            },
        )?;

        let entity_identifier = format!("{}:{}", entity.partition_key, entity.row_key);
        let mut additional_headers = HashMap::new();
        additional_headers.insert("Prefer".to_string(), "return-content".to_string());

        let resp = self
            .execute_data_plane_request(
                "POST",
                &url,
                &body,
                storage_account_name,
                resource_group_name,
                "InsertEntity",
                &entity_identifier,
                Some(additional_headers),
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure InsertEntity: failed to read response body".to_string(),
                })?;

        let entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure InsertEntity: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 201,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(entity)
    }

    async fn update_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
        etag: Option<ETag>,
    ) -> Result<TableEntity> {
        let encoded_partition_key = self.url_encode(partition_key);
        let encoded_row_key = self.url_encode(row_key);

        let url = self.build_table_storage_url(
            storage_account_name,
            &format!(
                "/{}(PartitionKey='{}',RowKey='{}')",
                table_name, encoded_partition_key, encoded_row_key
            ),
            None,
        )?;

        let body = serde_json::to_string(entity).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize entity for update in table '{}'",
                    table_name
                ),
            },
        )?;

        let entity_identifier = format!("{}:{}", partition_key, row_key);
        let mut additional_headers = HashMap::new();

        // Add If-Match header for optimistic concurrency
        if let Some(etag) = etag {
            additional_headers.insert("If-Match".to_string(), etag.0);
        } else {
            additional_headers.insert("If-Match".to_string(), "*".to_string());
        }

        let resp = self
            .execute_data_plane_request(
                "PUT",
                &url,
                &body,
                storage_account_name,
                resource_group_name,
                "UpdateEntity",
                &entity_identifier,
                Some(additional_headers),
            )
            .await?;

        // Update operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure UpdateEntity: failed to read response body".to_string(),
                })?;

        let updated_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
            message: format!(
                "Azure UpdateEntity: JSON parse error. Body: {}",
                response_body
            ),
            url: url.to_string(),
            http_status: 200,
            http_request_text: Some(body),
            http_response_text: Some(response_body),
        })?;

        Ok(updated_entity)
    }

    async fn merge_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
        etag: Option<ETag>,
    ) -> Result<TableEntity> {
        let encoded_partition_key = self.url_encode(partition_key);
        let encoded_row_key = self.url_encode(row_key);

        let url = self.build_table_storage_url(
            storage_account_name,
            &format!(
                "/{}(PartitionKey='{}',RowKey='{}')",
                table_name, encoded_partition_key, encoded_row_key
            ),
            None,
        )?;

        let body = serde_json::to_string(entity).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize entity for merge in table '{}'",
                    table_name
                ),
            },
        )?;

        let entity_identifier = format!("{}:{}", partition_key, row_key);
        let mut additional_headers = HashMap::new();

        // Add If-Match header for optimistic concurrency
        if let Some(etag) = etag {
            additional_headers.insert("If-Match".to_string(), etag.0);
        } else {
            additional_headers.insert("If-Match".to_string(), "*".to_string());
        }

        let resp = self
            .execute_data_plane_request(
                "PATCH",
                &url,
                &body,
                storage_account_name,
                resource_group_name,
                "MergeEntity",
                &entity_identifier,
                Some(additional_headers),
            )
            .await?;

        // Merge operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure MergeEntity: failed to read response body".to_string(),
                })?;

        let merged_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure MergeEntity: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(merged_entity)
    }

    async fn delete_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        etag: Option<ETag>,
    ) -> Result<()> {
        let encoded_partition_key = self.url_encode(partition_key);
        let encoded_row_key = self.url_encode(row_key);

        let url = self.build_table_storage_url(
            storage_account_name,
            &format!(
                "/{}(PartitionKey='{}',RowKey='{}')",
                table_name, encoded_partition_key, encoded_row_key
            ),
            None,
        )?;

        let entity_identifier = format!("{}:{}", partition_key, row_key);
        let mut additional_headers = HashMap::new();

        // Add If-Match header for optimistic concurrency
        if let Some(etag) = etag {
            additional_headers.insert("If-Match".to_string(), etag.0);
        } else {
            additional_headers.insert("If-Match".to_string(), "*".to_string());
        }

        let _resp = self
            .execute_data_plane_request(
                "DELETE",
                &url,
                "",
                storage_account_name,
                resource_group_name,
                "DeleteEntity",
                &entity_identifier,
                Some(additional_headers),
            )
            .await?;

        Ok(())
    }

    async fn insert_or_replace_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
    ) -> Result<TableEntity> {
        let encoded_partition_key = self.url_encode(partition_key);
        let encoded_row_key = self.url_encode(row_key);

        let url = self.build_table_storage_url(
            storage_account_name,
            &format!(
                "/{}(PartitionKey='{}',RowKey='{}')",
                table_name, encoded_partition_key, encoded_row_key
            ),
            None,
        )?;

        let body = serde_json::to_string(entity).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize entity for insert or replace in table '{}'",
                    table_name
                ),
            },
        )?;

        let entity_identifier = format!("{}:{}", partition_key, row_key);

        let resp = self
            .execute_data_plane_request(
                "PUT",
                &url,
                &body,
                storage_account_name,
                resource_group_name,
                "InsertOrReplaceEntity",
                &entity_identifier,
                None,
            )
            .await?;

        // Insert or replace operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure InsertOrReplaceEntity: failed to read response body"
                        .to_string(),
                })?;

        let result_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure InsertOrReplaceEntity: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(result_entity)
    }

    async fn insert_or_merge_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        entity: &TableEntity,
    ) -> Result<TableEntity> {
        let encoded_partition_key = self.url_encode(partition_key);
        let encoded_row_key = self.url_encode(row_key);

        let url = self.build_table_storage_url(
            storage_account_name,
            &format!(
                "/{}(PartitionKey='{}',RowKey='{}')",
                table_name, encoded_partition_key, encoded_row_key
            ),
            None,
        )?;

        let body = serde_json::to_string(entity).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize entity for insert or merge in table '{}'",
                    table_name
                ),
            },
        )?;

        let entity_identifier = format!("{}:{}", partition_key, row_key);

        let resp = self
            .execute_data_plane_request(
                "PATCH",
                &url,
                &body,
                storage_account_name,
                resource_group_name,
                "InsertOrMergeEntity",
                &entity_identifier,
                None,
            )
            .await?;

        // Insert or merge operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure InsertOrMergeEntity: failed to read response body".to_string(),
                })?;

        let result_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure InsertOrMergeEntity: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: Some(body),
                http_response_text: Some(response_body),
            })?;

        Ok(result_entity)
    }

    async fn query_entities(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        options: Option<EntityQueryOptions>,
    ) -> Result<EntityQueryResponse> {
        let mut query_params = vec![];

        if let Some(opts) = options {
            if let Some(filter) = opts.filter {
                query_params.push(("$filter", filter));
            }
            if let Some(select) = opts.select {
                query_params.push(("$select", select));
            }
            if let Some(top) = opts.top {
                query_params.push(("$top", top.to_string()));
            }
        }

        let url = self.build_table_storage_url(
            storage_account_name,
            &format!("/{}", table_name),
            if query_params.is_empty() {
                None
            } else {
                Some(query_params)
            },
        )?;

        let resp = self
            .execute_data_plane_request(
                "GET",
                &url,
                "",
                storage_account_name,
                resource_group_name,
                "QueryEntities",
                table_name,
                None,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure QueryEntities: failed to read response body".to_string(),
                })?;

        let query_response: EntityQueryResponse = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure QueryEntities: JSON parse error. Body: {}",
                    response_body
                ),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(query_response)
    }

    async fn get_entity(
        &self,
        resource_group_name: &str,
        storage_account_name: &str,
        table_name: &str,
        partition_key: &str,
        row_key: &str,
        select: Option<String>,
    ) -> Result<TableEntity> {
        let encoded_partition_key = self.url_encode(partition_key);
        let encoded_row_key = self.url_encode(row_key);

        let query_params = if let Some(select) = select {
            Some(vec![("$select", select)])
        } else {
            None
        };

        let url = self.build_table_storage_url(
            storage_account_name,
            &format!(
                "/{}(PartitionKey='{}',RowKey='{}')",
                table_name, encoded_partition_key, encoded_row_key
            ),
            query_params,
        )?;

        let entity_identifier = format!("{}:{}", partition_key, row_key);

        let resp = self
            .execute_data_plane_request(
                "GET",
                &url,
                "",
                storage_account_name,
                resource_group_name,
                "GetEntity",
                &entity_identifier,
                None,
            )
            .await?;

        let response_body =
            resp.text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    message: "Azure GetEntity: failed to read response body".to_string(),
                })?;

        let entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Azure GetEntity: JSON parse error. Body: {}", response_body),
                url: url.to_string(),
                http_status: 200,
                http_request_text: None,
                http_response_text: Some(response_body),
            })?;

        Ok(entity)
    }
}

// For backward compatibility, keep the old AzureTableStorageClient name as an alias
// for the management client
pub type AzureTableStorageClientOld = AzureTableManagementClient;
