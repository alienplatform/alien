use crate::error::{map_cloud_client_error, ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_aws_clients::dynamodb::*;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

use super::{validate_key, validate_value};

const HASH_BUCKET_COUNT: u8 = 16;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorState {
    version: u8,
    prefix: String,
    bucket: u8,
    last_key: Option<String>,
}

/// AWS DynamoDB implementation of the KV trait.
///
/// Credential refresh is handled automatically by the underlying `AwsCredentialProvider`
/// inside `DynamoDbClient`.
pub struct AwsDynamodbKv {
    client: DynamoDbClient,
    table_name: String,
}

impl Debug for AwsDynamodbKv {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsDynamodbKv")
            .field("table_name", &self.table_name)
            .finish()
    }
}

impl AwsDynamodbKv {
    pub fn new(table_name: String, client: DynamoDbClient) -> Self {
        Self { client, table_name }
    }

    /// Creates a hash bucket for load distribution
    fn hash_bucket(&self, key: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let bucket_id = hasher.finish() % u64::from(HASH_BUCKET_COUNT);
        format!("bucket_{}", bucket_id)
    }

    /// Checks if an item has expired based on TTL
    fn is_expired(&self, ttl_epoch: Option<i64>) -> bool {
        if let Some(ttl_timestamp) = ttl_epoch {
            let now = Utc::now().timestamp();
            now >= ttl_timestamp
        } else {
            false
        }
    }

    fn encode_cursor(state: &CursorState) -> Result<String> {
        let json =
            serde_json::to_vec(state)
                .into_alien_error()
                .context(ErrorData::InvalidInput {
                    operation_context: "DynamoDB KV scan cursor encoding".to_string(),
                    details: "Failed to serialize cursor state".to_string(),
                    field_name: Some("cursor".to_string()),
                })?;
        Ok(BASE64_URL_SAFE_NO_PAD.encode(json))
    }

    fn decode_cursor(prefix: &str, cursor: &str) -> Result<CursorState> {
        let decoded = BASE64_URL_SAFE_NO_PAD
            .decode(cursor)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                operation_context: "DynamoDB KV scan cursor decoding".to_string(),
                details: "Invalid cursor encoding".to_string(),
                field_name: Some("cursor".to_string()),
            })?;
        let state: CursorState = serde_json::from_slice(&decoded)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                operation_context: "DynamoDB KV scan cursor decoding".to_string(),
                details: "Invalid cursor data".to_string(),
                field_name: Some("cursor".to_string()),
            })?;
        if state.version != 1 || state.prefix != prefix || state.bucket >= HASH_BUCKET_COUNT {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: "DynamoDB KV scan cursor validation".to_string(),
                details: "Cursor does not belong to this prefix scan".to_string(),
                field_name: Some("cursor".to_string()),
            }));
        }
        if state
            .last_key
            .as_ref()
            .is_some_and(|last_key| !last_key.starts_with(prefix))
        {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: "DynamoDB KV scan cursor validation".to_string(),
                details: "Cursor key does not match the scan prefix".to_string(),
                field_name: Some("cursor".to_string()),
            }));
        }
        Ok(state)
    }
}

impl Binding for AwsDynamodbKv {}

#[async_trait]
impl Kv for AwsDynamodbKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        validate_key(key)?;

        let bucket = self.hash_bucket(key);
        let mut primary_key = HashMap::new();
        primary_key.insert("pk".to_string(), AttributeValue::s(bucket));
        primary_key.insert("sk".to_string(), AttributeValue::s(key.to_string()));

        let request = GetItemRequest::builder()
            .table_name(self.table_name.clone())
            .key(primary_key)
            // `Kv::put` followed by `Kv::get` must observe the write. DynamoDB
            // GetItem is eventually consistent by default, which can make a
            // freshly stored command payload appear missing during immediate
            // push dispatch.
            .consistent_read(true)
            .build();

        let response = self.client.get_item(request).await.map_err(|e| {
            map_cloud_client_error(
                e,
                format!("Failed to get item with key '{}'", key),
                Some(key.to_string()),
            )
        })?;

        if let Some(item) = response.item {
            // Check TTL expiry (logical expiry contract)
            if let Some(ttl_attr) = item.get("ttl") {
                if let Some(ttl_epoch) = ttl_attr.n.as_ref().and_then(|s| s.parse::<i64>().ok()) {
                    if self.is_expired(Some(ttl_epoch)) {
                        return Ok(None); // Logically expired
                    }
                }
            }

            let value = item
                .get("value")
                .and_then(|attr| attr.b.as_ref())
                .and_then(|base64_value| base64::prelude::BASE64_STANDARD.decode(base64_value).ok())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!("Missing or invalid value attribute for key '{}'", key),
                        resource_id: Some(key.to_string()),
                    })
                })?;

            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        validate_key(key)?;
        validate_value(&value)?;

        let bucket = self.hash_bucket(key);
        let options = options.unwrap_or_default();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::s(bucket));
        item.insert("sk".to_string(), AttributeValue::s(key.to_string()));
        item.insert(
            "value".to_string(),
            AttributeValue::b(base64::prelude::BASE64_STANDARD.encode(&value)),
        );

        if let Some(ttl) = options.ttl {
            let expires_at = (Utc::now() + ttl).timestamp();
            item.insert("ttl".to_string(), AttributeValue::n(expires_at.to_string()));
        }

        let request = if options.if_not_exists {
            // Expired rows count as ABSENT, exactly like the local provider's
            // atomic takeover: DynamoDB's background TTL sweeper can lag the
            // logical expiry by hours, and without the `#ttl <= :now` arm a
            // conditional put (e.g. a command lease takeover after the
            // previous holder died) would be blocked until the sweeper
            // physically deletes the row.
            let mut expression_attribute_names = HashMap::new();
            expression_attribute_names.insert("#ttl".to_string(), "ttl".to_string());
            let mut expression_attribute_values = HashMap::new();
            expression_attribute_values.insert(
                ":now".to_string(),
                AttributeValue::n(Utc::now().timestamp().to_string()),
            );
            PutItemRequest::builder()
                .table_name(self.table_name.clone())
                .item(item)
                .condition_expression(
                    "(attribute_not_exists(pk) AND attribute_not_exists(sk))                      OR (attribute_exists(#ttl) AND #ttl <= :now)"
                        .to_string(),
                )
                .expression_attribute_names(expression_attribute_names)
                .expression_attribute_values(expression_attribute_values)
                .build()
        } else {
            PutItemRequest::builder()
                .table_name(self.table_name.clone())
                .item(item)
                .build()
        };

        match self.client.put_item(request).await {
            Ok(_) => Ok(true),
            Err(e) => {
                // Check if this is a conditional check failure for if_not_exists
                if options.if_not_exists {
                    if let Some(alien_client_core::ErrorData::RemoteResourceConflict { .. }) =
                        &e.error
                    {
                        return Ok(false);
                    }
                }
                Err(map_cloud_client_error(
                    e,
                    format!("Failed to put item with key '{}'", key),
                    Some(key.to_string()),
                ))
            }
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;

        let bucket = self.hash_bucket(key);
        let mut primary_key = HashMap::new();
        primary_key.insert("pk".to_string(), AttributeValue::s(bucket));
        primary_key.insert("sk".to_string(), AttributeValue::s(key.to_string()));

        let request = DeleteItemRequest::builder()
            .table_name(self.table_name.clone())
            .key(primary_key)
            .build();

        self.client.delete_item(request).await.map_err(|e| {
            map_cloud_client_error(
                e,
                format!("Failed to delete item with key '{}'", key),
                Some(key.to_string()),
            )
        })?;

        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        validate_key(key)?;

        let bucket = self.hash_bucket(key);
        let mut primary_key = HashMap::new();
        primary_key.insert("pk".to_string(), AttributeValue::s(bucket));
        primary_key.insert("sk".to_string(), AttributeValue::s(key.to_string()));

        // Use expression attribute names to avoid reserved keyword 'ttl'
        let mut expression_attribute_names = HashMap::new();
        expression_attribute_names.insert("#ttl".to_string(), "ttl".to_string());

        let request = GetItemRequest::builder()
            .table_name(self.table_name.clone())
            .key(primary_key)
            .projection_expression("pk, #ttl".to_string()) // Get key and TTL for expiry check
            .expression_attribute_names(expression_attribute_names)
            .consistent_read(true)
            .build();

        let response = self.client.get_item(request).await.map_err(|e| {
            map_cloud_client_error(
                e,
                format!("Failed to check existence of item with key '{}'", key),
                Some(key.to_string()),
            )
        })?;

        if let Some(item) = response.item {
            // Check TTL expiry (logical expiry contract)
            if let Some(ttl_attr) = item.get("ttl") {
                if let Some(ttl_epoch) = ttl_attr.n.as_ref().and_then(|s| s.parse::<i64>().ok()) {
                    if self.is_expired(Some(ttl_epoch)) {
                        return Ok(false); // Logically expired
                    }
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<ScanResult> {
        validate_key(prefix)?;
        let limit = limit.unwrap_or(1000);
        if limit == 0 {
            return Ok(ScanResult {
                items: Vec::new(),
                next_cursor: cursor,
            });
        }

        let initial = cursor
            .as_deref()
            .map(|cursor| Self::decode_cursor(prefix, cursor))
            .transpose()?
            .unwrap_or(CursorState {
                version: 1,
                prefix: prefix.to_string(),
                bucket: 0,
                last_key: None,
            });
        let mut items = Vec::with_capacity(limit);
        let mut bucket_id = initial.bucket;
        let mut last_key = initial.last_key;

        while bucket_id < HASH_BUCKET_COUNT {
            let bucket = format!("bucket_{}", bucket_id);
            let mut expression_attribute_values = HashMap::new();
            expression_attribute_values
                .insert(":bucket".to_string(), AttributeValue::s(bucket.clone()));
            expression_attribute_values
                .insert(":prefix".to_string(), AttributeValue::s(prefix.to_string()));

            let exclusive_start_key = last_key.as_ref().map(|key| {
                HashMap::from([
                    ("pk".to_string(), AttributeValue::s(bucket.clone())),
                    ("sk".to_string(), AttributeValue::s(key.clone())),
                ])
            });
            let request = QueryRequest::builder()
                .table_name(self.table_name.clone())
                .key_condition_expression("pk = :bucket AND begins_with(sk, :prefix)".to_string())
                .expression_attribute_values(expression_attribute_values)
                .limit(i32::try_from(limit - items.len()).unwrap_or(i32::MAX))
                .maybe_exclusive_start_key(exclusive_start_key)
                .build();

            let response = self.client.query(request).await.map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to scan prefix '{}' in bucket {}", prefix, bucket_id),
                    Some(prefix.to_string()),
                )
            })?;

            for item in response.items {
                if let Some(ttl_attr) = item.get("ttl") {
                    if let Some(ttl_epoch) = ttl_attr.n.as_ref().and_then(|s| s.parse::<i64>().ok())
                    {
                        if self.is_expired(Some(ttl_epoch)) {
                            continue; // Skip expired items
                        }
                    }
                }

                if let (Some(key_attr), Some(value_attr)) = (item.get("sk"), item.get("value")) {
                    if let (Some(key), Some(base64_value)) =
                        (key_attr.s.as_ref(), value_attr.b.as_ref())
                    {
                        if let Ok(value) = base64::prelude::BASE64_STANDARD.decode(base64_value) {
                            items.push((key.clone(), value));
                        }
                    }
                }
            }

            let provider_last_key = response
                .last_evaluated_key
                .as_ref()
                .and_then(|key| key.get("sk"))
                .and_then(|value| value.s.clone());

            if items.len() == limit {
                let (next_bucket, next_key) = match provider_last_key {
                    Some(key) => (bucket_id, Some(key)),
                    None if bucket_id + 1 < HASH_BUCKET_COUNT => (bucket_id + 1, None),
                    None => {
                        return Ok(ScanResult {
                            items,
                            next_cursor: None,
                        });
                    }
                };
                return Ok(ScanResult {
                    items,
                    next_cursor: Some(Self::encode_cursor(&CursorState {
                        version: 1,
                        prefix: prefix.to_string(),
                        bucket: next_bucket,
                        last_key: next_key,
                    })?),
                });
            }

            if let Some(key) = provider_last_key {
                last_key = Some(key);
            } else {
                bucket_id += 1;
                last_key = None;
            }
        }

        Ok(ScanResult {
            items,
            next_cursor: None,
        })
    }
}
