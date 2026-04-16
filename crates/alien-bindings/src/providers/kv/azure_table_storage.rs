use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_azure_clients::tables::{
    AzureTableStorageClient, EntityQueryOptions, TableEntity, TableStorageApi,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

use super::{validate_key, validate_value};

/// Convert a KV operation to a Table Storage entity
/// This only base64 encodes the raw bytes when creating the properties map, not in memory
fn create_table_entity(
    partition_key: String,
    row_key: String,
    value: &[u8],
    expires_at: Option<DateTime<Utc>>,
) -> TableEntity {
    let mut properties = HashMap::new();

    // Base64 encode the raw bytes only when storing in the properties map
    // This keeps the original 32KB limit valid since we're not storing the encoded version in memory
    properties.insert("Value".to_string(), Value::String(BASE64.encode(value)));

    // Store creation timestamp
    properties.insert(
        "CreatedAt".to_string(),
        Value::String(Utc::now().to_rfc3339()),
    );

    // Store expiration timestamp if provided
    if let Some(expiry) = expires_at {
        properties.insert("ExpiresAt".to_string(), Value::String(expiry.to_rfc3339()));
    }

    TableEntity {
        partition_key,
        row_key,
        timestamp: None, // Azure will set this
        properties,
    }
}

/// Extract KV value from Table Storage entity
fn extract_value_from_entity(entity: &TableEntity) -> Result<Vec<u8>> {
    let value_str = entity
        .properties
        .get("Value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AlienError::new(ErrorData::InvalidInput {
                operation_context: "Azure Table Storage KV extract value".to_string(),
                details: "Entity missing Value property or not a string".to_string(),
                field_name: Some("Value".to_string()),
            })
        })?;

    // Decode base64 value
    BASE64
        .decode(value_str)
        .into_alien_error()
        .context(ErrorData::InvalidInput {
            operation_context: "Azure Table Storage KV extract value".to_string(),
            details: "Failed to decode base64 value".to_string(),
            field_name: Some("Value".to_string()),
        })
}

/// Check if entity has expired based on TTL
fn is_entity_expired(entity: &TableEntity) -> bool {
    if let Some(expires_at_value) = entity.properties.get("ExpiresAt") {
        if let Some(expires_at_str) = expires_at_value.as_str() {
            if let Ok(expires_at) = DateTime::parse_from_rfc3339(expires_at_str) {
                return Utc::now() > expires_at.with_timezone(&Utc);
            }
        }
    }
    false
}

/// Cursor state for pagination across partitions
#[derive(Serialize, Deserialize)]
struct CursorState {
    current_partition: u32,
    partition_continuation_token: Option<String>, // Azure's NextPartitionKey + NextRowKey combined
}

/// Azure Table Storage implementation of the KV trait
pub struct AzureTableStorageKv {
    client: AzureTableStorageClient,
    resource_group_name: String,
    account_name: String,
    table_name: String,
    num_partitions: u32,
}

impl Debug for AzureTableStorageKv {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureTableStorageKv")
            .field("resource_group_name", &self.resource_group_name)
            .field("account_name", &self.account_name)
            .field("table_name", &self.table_name)
            .field("num_partitions", &self.num_partitions)
            .finish()
    }
}

impl AzureTableStorageKv {
    pub fn new(
        client: AzureTableStorageClient,
        resource_group_name: String,
        account_name: String,
        table_name: String,
    ) -> Self {
        Self {
            client,
            resource_group_name,
            account_name,
            table_name,
            num_partitions: 16, // 16 partitions for load distribution
        }
    }

    /// Creates a hash bucket for load distribution
    fn hash_bucket(&self, key: &str) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish() as u32 % self.num_partitions
    }

    /// Splits key into partition key and row key
    fn split_key(&self, key: &str) -> (String, String) {
        // Use hash-based partitioning for load distribution
        let partition_key = format!("p{}", self.hash_bucket(key));
        (partition_key, key.to_string())
    }

    /// Combines partition key and row key back to original key
    fn combine_key(&self, _partition_key: &str, row_key: &str) -> String {
        row_key.to_string() // Row key contains the original key
    }

    /// Encodes cursor state as base64url JSON for safe HTTP transmission
    fn encode_cursor(&self, state: &CursorState) -> String {
        let json = serde_json::to_string(state).unwrap();
        BASE64.encode(json.as_bytes())
    }

    /// Decodes cursor state from base64url JSON
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
}

impl Binding for AzureTableStorageKv {}

#[async_trait]
impl Kv for AzureTableStorageKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        validate_key(key)?;

        let (partition_key, row_key) = self.split_key(key);

        match self
            .client
            .get_entity(
                &self.resource_group_name,
                &self.account_name,
                &self.table_name,
                &partition_key,
                &row_key,
                None,
            )
            .await
        {
            Ok(entity) => {
                // Check if TTL has expired (client-side filtering)
                if is_entity_expired(&entity) {
                    return Ok(None); // Expired
                }

                let value = extract_value_from_entity(&entity)?;
                Ok(Some(value))
            }
            Err(e) => {
                use alien_client_core::ErrorData as CloudErrorData;
                match e.error.as_ref() {
                    Some(CloudErrorData::RemoteResourceNotFound { .. }) => Ok(None),
                    _ => Err(crate::error::map_cloud_client_error(
                        e,
                        format!("Failed to get entity for key '{}'", key),
                        Some(key.to_string()),
                    )),
                }
            }
        }
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        validate_key(key)?;
        validate_value(&value)?;

        let options = options.unwrap_or_default();
        let (partition_key, row_key) = self.split_key(key);

        let expires_at = options.ttl.map(|d| Utc::now() + d);
        let entity =
            create_table_entity(partition_key.clone(), row_key.clone(), &value, expires_at);

        if options.if_not_exists {
            match self
                .client
                .insert_entity(
                    &self.resource_group_name,
                    &self.account_name,
                    &self.table_name,
                    &entity,
                )
                .await
            {
                Ok(_) => Ok(true),
                Err(e) => {
                    use alien_client_core::ErrorData as CloudErrorData;
                    match e.error.as_ref() {
                        Some(CloudErrorData::RemoteResourceConflict { .. }) => Ok(false),
                        _ => Err(crate::error::map_cloud_client_error(
                            e,
                            format!("Failed to insert entity for key '{}'", key),
                            Some(key.to_string()),
                        )),
                    }
                }
            }
        } else {
            // Insert Or Replace (upsert) - matches Azure REST API terminology
            self.client
                .insert_or_replace_entity(
                    &self.resource_group_name,
                    &self.account_name,
                    &self.table_name,
                    &partition_key,
                    &row_key,
                    &entity,
                )
                .await
                .map_err(|e| {
                    crate::error::map_cloud_client_error(
                        e,
                        format!("Failed to upsert entity for key '{}'", key),
                        Some(key.to_string()),
                    )
                })?;
            Ok(true)
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;

        let (partition_key, row_key) = self.split_key(key);

        // Delete entity, ignore if not found
        match self
            .client
            .delete_entity(
                &self.resource_group_name,
                &self.account_name,
                &self.table_name,
                &partition_key,
                &row_key,
                None, // No specific ETag constraint
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                use alien_client_core::ErrorData as CloudErrorData;
                match e.error.as_ref() {
                    Some(CloudErrorData::RemoteResourceNotFound { .. }) => Ok(()), // No error if key doesn't exist
                    _ => Err(crate::error::map_cloud_client_error(
                        e,
                        format!("Failed to delete entity for key '{}'", key),
                        Some(key.to_string()),
                    )),
                }
            }
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        validate_key(key)?;

        let (partition_key, row_key) = self.split_key(key);

        match self
            .client
            .get_entity(
                &self.resource_group_name,
                &self.account_name,
                &self.table_name,
                &partition_key,
                &row_key,
                None,
            )
            .await
        {
            Ok(entity) => {
                // Check TTL expiry
                Ok(!is_entity_expired(&entity))
            }
            Err(e) => {
                use alien_client_core::ErrorData as CloudErrorData;
                match e.error.as_ref() {
                    Some(CloudErrorData::RemoteResourceNotFound { .. }) => Ok(false),
                    _ => Err(crate::error::map_cloud_client_error(
                        e,
                        format!("Failed to check existence of entity for key '{}'", key),
                        Some(key.to_string()),
                    )),
                }
            }
        }
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        validate_key(prefix)?; // Prefix follows same key validation rules

        // For prefix scans with hash-based partitioning, must fan-out across ALL partitions
        // A RowKey-only filter forces expensive table-wide scans

        // Decode cursor to get partition progress and continuation tokens
        let cursor_state = cursor.as_ref().map(|c| self.decode_cursor(c)).transpose()?;

        let mut all_items = Vec::new();
        let mut total_fetched = 0;
        let limit = limit.unwrap_or(1000);

        // Start from the partition in cursor, or 0 if no cursor
        let start_partition = cursor_state.as_ref().map_or(0, |cs| cs.current_partition);

        for partition_id in start_partition..self.num_partitions {
            let partition_key = format!("p{}", partition_id);

            // Build filter with BOTH PartitionKey and RowKey conditions
            // Use a range query approach that's compatible with Azure Table Storage
            let prefix_end = format!("{}~", prefix); // Use tilde as it's after most printable chars
            let filter = format!(
                "(PartitionKey eq '{}') and (RowKey ge '{}') and (RowKey lt '{}')",
                partition_key, prefix, prefix_end
            );

            // Note: We'll do TTL filtering client-side to avoid OData syntax issues
            let filter_with_ttl = filter;

            let query_options = EntityQueryOptions {
                filter: Some(filter_with_ttl),
                select: None,
                top: Some((limit - total_fetched) as u32),
            };

            let response = self
                .client
                .query_entities(
                    &self.resource_group_name,
                    &self.account_name,
                    &self.table_name,
                    Some(query_options),
                )
                .await
                .map_err(|e| {
                    crate::error::map_cloud_client_error(
                        e,
                        format!("Failed to query entities with prefix '{}'", prefix),
                        Some(prefix.to_string()),
                    )
                })?;

            // Process entities from this partition
            for entity in response.entities {
                if total_fetched >= limit {
                    break;
                }

                // Additional client-side TTL check for precision
                if is_entity_expired(&entity) {
                    continue; // Skip expired
                }

                let key = self.combine_key(&entity.partition_key, &entity.row_key);
                let value = extract_value_from_entity(&entity)?;

                all_items.push((key, value));
                total_fetched += 1;
            }

            // If we hit the limit or have more data in this partition, encode cursor and return
            if total_fetched >= limit || response.next_link.is_some() {
                let next_cursor = self.encode_cursor(&CursorState {
                    current_partition: partition_id,
                    partition_continuation_token: response.next_link,
                });
                return Ok(ScanResult {
                    items: all_items,
                    next_cursor: Some(next_cursor),
                });
            }
        }

        // Scanned all partitions without hitting limit
        Ok(ScanResult {
            items: all_items,
            next_cursor: None,
        })
    }
}
