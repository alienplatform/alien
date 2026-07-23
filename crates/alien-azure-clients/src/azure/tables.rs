use crate::azure::common::{
    create_azure_http_error_with_context, AzureClientBase, AzureRequestBuilder,
};
use crate::azure::error::safe_http_response_context;
use crate::azure::models::table::*;
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use url::Url;

#[cfg(feature = "test-utils")]
use mockall::automock;

fn table_diagnostic_url(request_url: &Url) -> String {
    let mut diagnostic_url = request_url.clone();
    diagnostic_url.set_query(None);
    diagnostic_url.set_fragment(None);

    let path = diagnostic_url.path().to_string();
    if let Some(entity_key_start) = path.find('(') {
        diagnostic_url.set_path(&format!(
            "{}(redacted-entity-key)",
            &path[..entity_key_start]
        ));
    }

    diagnostic_url.to_string()
}

async fn send_table_request(
    request: reqwest::RequestBuilder,
    operation_name: &str,
) -> Result<reqwest::Response> {
    request
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!("Azure {operation_name}: failed to execute request"),
        })
}

async fn read_table_response_body(
    response: reqwest::Response,
    operation_name: &str,
) -> Result<String> {
    response
        .text()
        .await
        .map_err(reqwest::Error::without_url)
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!("Azure {operation_name}: failed to read response body"),
        })
}

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
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
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
                message: "Azure CreateTable: JSON parse error".to_string(),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
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
                message: "Azure GetTableAcl: JSON parse error".to_string(),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
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
}

impl AzureTableStorageClient {
    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        Self {
            client,
            token_cache,
        }
    }

    /// Build the full URL for Table Storage data plane operations
    fn build_table_storage_url(
        &self,
        storage_account_name: &str,
        path: &str,
        query_params: Option<Vec<(&str, String)>>,
    ) -> Result<url::Url> {
        let base_url = if let Some(override_url) = self.token_cache.get_service_endpoint("table") {
            override_url.trim_end_matches('/').to_string()
        } else {
            format!("https://{}.table.core.windows.net", storage_account_name)
        };

        let mut url = url::Url::parse(&format!("{}{}", base_url, path))
            .into_alien_error()
            .context(ErrorData::InvalidClientConfig {
                message: format!("Invalid Table Storage URL for account '{storage_account_name}'"),
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

    /// Execute an HTTP request and handle errors using centralized Azure error mapping
    async fn execute_data_plane_request(
        &self,
        method: &str,
        url: &url::Url,
        body: &str,
        storage_account_name: &str,
        resource_group_name: &str,
        operation_name: &str,
        table_name: &str,
        additional_headers: Option<HashMap<String, String>>,
    ) -> Result<reqwest::Response> {
        let headers = self
            .bearer_auth_headers(method, body, storage_account_name, resource_group_name)
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

        let diagnostic_url = table_diagnostic_url(url);
        let resp = send_table_request(request_builder, operation_name).await?;

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
                table_name,
                &error_text,
                &diagnostic_url,
            ))
        }
    }

    /// Build Table Storage data-plane headers using Microsoft Entra auth.
    async fn bearer_auth_headers(
        &self,
        _method: &str,
        body: &str,
        _storage_account_name: &str,
        _resource_group_name: &str,
    ) -> Result<HashMap<String, String>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://storage.azure.com/.default")
            .await?;

        let mut headers = HashMap::new();

        headers.insert("x-ms-version".to_string(), "2020-12-06".to_string());
        headers.insert("Content-Length".to_string(), body.len().to_string());

        // Add OData headers for Table Storage
        headers.insert("DataServiceVersion".to_string(), "3.0;NetFx".to_string());
        headers.insert("MaxDataServiceVersion".to_string(), "3.0;NetFx".to_string());

        if !body.is_empty() {
            headers.insert("Content-Type".to_string(), "application/json".to_string());
        }

        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", bearer_token),
        );
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
                table_name,
                Some(additional_headers),
            )
            .await?;

        let response_body = read_table_response_body(resp, "InsertEntity").await?;

        let entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
                "Azure InsertEntity: JSON parse error",
                table_diagnostic_url(&url),
                StatusCode::CREATED,
            ))?;

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
                table_name,
                Some(additional_headers),
            )
            .await?;

        // Update operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body = read_table_response_body(resp, "UpdateEntity").await?;

        let updated_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
            "Azure UpdateEntity: JSON parse error",
            table_diagnostic_url(&url),
            StatusCode::OK,
        ))?;

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
                table_name,
                Some(additional_headers),
            )
            .await?;

        // Merge operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body = read_table_response_body(resp, "MergeEntity").await?;

        let merged_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
                "Azure MergeEntity: JSON parse error",
                table_diagnostic_url(&url),
                StatusCode::OK,
            ))?;

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
                table_name,
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

        let resp = self
            .execute_data_plane_request(
                "PUT",
                &url,
                &body,
                storage_account_name,
                resource_group_name,
                "InsertOrReplaceEntity",
                table_name,
                None,
            )
            .await?;

        // Insert or replace operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body = read_table_response_body(resp, "InsertOrReplaceEntity").await?;

        let result_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
                "Azure InsertOrReplaceEntity: JSON parse error",
                table_diagnostic_url(&url),
                StatusCode::OK,
            ))?;

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

        let resp = self
            .execute_data_plane_request(
                "PATCH",
                &url,
                &body,
                storage_account_name,
                resource_group_name,
                "InsertOrMergeEntity",
                table_name,
                None,
            )
            .await?;

        // Insert or merge operations may return 204 No Content
        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(entity.clone());
        }

        let response_body = read_table_response_body(resp, "InsertOrMergeEntity").await?;

        let result_entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
                "Azure InsertOrMergeEntity: JSON parse error",
                table_diagnostic_url(&url),
                StatusCode::OK,
            ))?;

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

        let response_body = read_table_response_body(resp, "QueryEntities").await?;

        let query_response: EntityQueryResponse = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
                "Azure QueryEntities: JSON parse error",
                table_diagnostic_url(&url),
                StatusCode::OK,
            ))?;

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

        let resp = self
            .execute_data_plane_request(
                "GET",
                &url,
                "",
                storage_account_name,
                resource_group_name,
                "GetEntity",
                table_name,
                None,
            )
            .await?;

        // The ETag lives in the response HEADER: at the minimalmetadata
        // accept level the body carries no `odata.etag` property, and
        // callers doing optimistic-concurrency updates need it.
        let etag_header = resp
            .headers()
            .get("ETag")
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string);

        let response_body = read_table_response_body(resp, "GetEntity").await?;

        let mut entity: TableEntity = serde_json::from_str(&response_body)
            .into_alien_error()
            .context(safe_http_response_context(
                "Azure GetEntity: JSON parse error",
                table_diagnostic_url(&url),
                StatusCode::OK,
            ))?;

        // Expose it under the odata property name (present natively only at
        // fullmetadata) so consumers have ONE place to look.
        if let Some(etag) = etag_header {
            entity
                .properties
                .entry("odata.etag".to_string())
                .or_insert(serde_json::Value::String(etag));
        }

        Ok(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{Method::GET, MockServer};

    #[test]
    fn table_diagnostic_urls_redact_entity_keys_and_query_values() {
        const PARTITION_KEY: &str = "PARTITION_KEY_SECRET_0123456789";
        const ROW_KEY: &str = "ROW_KEY_SECRET_0123456789";
        const FILTER_VALUE: &str = "FILTER_SECRET_0123456789";
        const SELECT_VALUE: &str = "SELECT_SECRET_0123456789";

        let url = Url::parse(&format!(
            "https://example.table.core.windows.net/Jobs(PartitionKey='{PARTITION_KEY}',RowKey='{ROW_KEY}')?$filter={FILTER_VALUE}&$select={SELECT_VALUE}"
        ))
        .unwrap();
        let diagnostic_url = table_diagnostic_url(&url);

        assert!(diagnostic_url.contains("example.table.core.windows.net"));
        assert!(diagnostic_url.contains("/Jobs(redacted-entity-key)"));
        assert!(!diagnostic_url.contains(PARTITION_KEY));
        assert!(!diagnostic_url.contains(ROW_KEY));
        assert!(!diagnostic_url.contains(FILTER_VALUE));
        assert!(!diagnostic_url.contains(SELECT_VALUE));
        assert!(!diagnostic_url.contains('?'));
    }

    #[tokio::test]
    async fn table_transport_errors_drop_entity_keys_and_query_values() {
        const PARTITION_KEY: &str = "PARTITION_TRANSPORT_SECRET_0123456789";
        const ROW_KEY: &str = "ROW_TRANSPORT_SECRET_0123456789";
        const FILTER_VALUE: &str = "FILTER_TRANSPORT_SECRET_0123456789";

        let server = MockServer::start_async().await;
        let request_url = format!(
            "{}/Jobs(PartitionKey='{PARTITION_KEY}',RowKey='{ROW_KEY}')?$filter={FILTER_VALUE}",
            server.base_url()
        );
        let redirect = server
            .mock_async(|when, then| {
                when.method(GET);
                then.status(302).header("Location", &request_url);
            })
            .await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(1))
            .build()
            .unwrap();

        let error = send_table_request(client.get(&request_url), "GetEntity")
            .await
            .expect_err("redirect loop should be a transport failure");
        let serialized = serde_json::to_string(&error).unwrap();

        redirect.assert_hits_async(2).await;
        assert!(serialized.contains("GetEntity"));
        assert!(!serialized.contains(PARTITION_KEY));
        assert!(!serialized.contains(ROW_KEY));
        assert!(!serialized.contains(FILTER_VALUE));
    }
}

// For backward compatibility, keep the old AzureTableStorageClient name as an alias
// for the management client
pub type AzureTableStorageClientOld = AzureTableManagementClient;
