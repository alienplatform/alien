use crate::error::{map_cloud_client_error, ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_aws_clients::AwsClientConfig;
use alien_aws_clients::{dynamodb::*, AwsClientConfigExt as _};
use alien_core::AwsCredentials;
use alien_error::AlienError;
use async_trait::async_trait;
use base64::{prelude::BASE64_STANDARD, Engine};
use chrono::Utc;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{validate_key, validate_value};

/// How many seconds before expiry to refresh credentials (5 minutes).
const REFRESH_BUFFER_SECS: u64 = 300;
/// Default credential lifetime when expiry is unknown (50 minutes).
const DEFAULT_CREDENTIAL_LIFETIME_SECS: u64 = 3000;

/// AWS DynamoDB implementation of the KV trait with automatic IRSA credential refresh.
pub struct AwsDynamodbKv {
    client: Arc<RwLock<DynamoDbClient>>,
    table_name: String,
    region: String,
    endpoint_url: Option<String>,
    /// Original config with WebIdentity credentials for refreshing.
    /// None if using static AccessKeys (no refresh needed).
    original_config: Option<AwsClientConfig>,
    /// Unix timestamp (seconds) when current credentials expire.
    credentials_expiry: AtomicU64,
}

impl Debug for AwsDynamodbKv {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsDynamodbKv")
            .field("table_name", &self.table_name)
            .field("region", &self.region)
            .field("endpoint_url", &self.endpoint_url)
            .finish()
    }
}

impl AwsDynamodbKv {
    pub async fn new(table_name: String, config: AwsClientConfig) -> Result<Self> {
        let http_client = crate::http_client::create_http_client();

        // Check if we need credential refresh (IRSA/WebIdentity)
        let original_config = if matches!(config.credentials, AwsCredentials::WebIdentity { .. }) {
            Some(config.clone())
        } else {
            None
        };

        // Resolve credentials (exchanges web identity token for access keys if needed)
        let resolved_config = if matches!(config.credentials, AwsCredentials::WebIdentity { .. }) {
            config
                .get_web_identity_credentials()
                .await
                .map_err(|e| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: format!("Failed to resolve IRSA credentials: {}", e),
                        resource_id: None,
                    })
                })?
        } else {
            config
        };

        let region = resolved_config.region.clone();
        let endpoint_url = resolved_config
            .get_service_endpoint_option("dynamodb")
            .map(|u| u.to_string());
        let client = DynamoDbClient::new(http_client, resolved_config);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiry = now + DEFAULT_CREDENTIAL_LIFETIME_SECS;

        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            table_name,
            region,
            endpoint_url,
            original_config,
            credentials_expiry: AtomicU64::new(expiry),
        })
    }

    /// Ensures the DynamoDB client has fresh credentials.
    /// If using IRSA and credentials are about to expire, refreshes them.
    async fn ensure_fresh_client(&self) -> Result<()> {
        let original = match &self.original_config {
            Some(c) => c,
            None => return Ok(()), // Static credentials, no refresh needed
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiry = self.credentials_expiry.load(Ordering::Relaxed);

        if now + REFRESH_BUFFER_SECS < expiry {
            return Ok(()); // Credentials still valid
        }

        tracing::info!("Refreshing AWS credentials for DynamoDB (IRSA token exchange)");

        let refreshed = original.get_web_identity_credentials().await.map_err(|e| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("Failed to refresh IRSA credentials: {}", e),
                resource_id: None,
            })
        })?;

        let http_client = crate::http_client::create_http_client();
        let new_client = DynamoDbClient::new(http_client, refreshed);

        let mut client = self.client.write().await;
        *client = new_client;

        let new_expiry = now + DEFAULT_CREDENTIAL_LIFETIME_SECS;
        self.credentials_expiry.store(new_expiry, Ordering::Relaxed);

        tracing::info!("AWS credentials refreshed successfully for DynamoDB");
        Ok(())
    }

    /// Creates a hash bucket for load distribution
    fn hash_bucket(&self, key: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let bucket_id = hasher.finish() % 16; // 16 buckets for load distribution
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
}

impl Binding for AwsDynamodbKv {}

#[async_trait]
impl Kv for AwsDynamodbKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        validate_key(key)?;
        self.ensure_fresh_client().await?;

        let bucket = self.hash_bucket(key);
        let mut primary_key = HashMap::new();
        primary_key.insert("pk".to_string(), AttributeValue::s(bucket));
        primary_key.insert("sk".to_string(), AttributeValue::s(key.to_string()));

        let request = GetItemRequest::builder()
            .table_name(self.table_name.clone())
            .key(primary_key)
            .build();

        let client = self.client.read().await;
        let response = client.get_item(request).await.map_err(|e| {
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
                .and_then(|base64_value| BASE64_STANDARD.decode(base64_value).ok())
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
        self.ensure_fresh_client().await?;

        let bucket = self.hash_bucket(key);
        let options = options.unwrap_or_default();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::s(bucket));
        item.insert("sk".to_string(), AttributeValue::s(key.to_string()));
        item.insert(
            "value".to_string(),
            AttributeValue::b(BASE64_STANDARD.encode(&value)),
        );

        if let Some(ttl) = options.ttl {
            let expires_at = (Utc::now() + ttl).timestamp();
            item.insert("ttl".to_string(), AttributeValue::n(expires_at.to_string()));
        }

        let request = if options.if_not_exists {
            PutItemRequest::builder()
                .table_name(self.table_name.clone())
                .item(item)
                .condition_expression(
                    "attribute_not_exists(pk) AND attribute_not_exists(sk)".to_string(),
                )
                .build()
        } else {
            PutItemRequest::builder()
                .table_name(self.table_name.clone())
                .item(item)
                .build()
        };

        let client = self.client.read().await;
        match client.put_item(request).await {
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
        self.ensure_fresh_client().await?;

        let bucket = self.hash_bucket(key);
        let mut primary_key = HashMap::new();
        primary_key.insert("pk".to_string(), AttributeValue::s(bucket));
        primary_key.insert("sk".to_string(), AttributeValue::s(key.to_string()));

        let request = DeleteItemRequest::builder()
            .table_name(self.table_name.clone())
            .key(primary_key)
            .build();

        let client = self.client.read().await;
        client.delete_item(request).await.map_err(|e| {
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
        self.ensure_fresh_client().await?;

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
            .build();

        let client = self.client.read().await;
        let response = client.get_item(request).await.map_err(|e| {
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
        _cursor: Option<String>,
    ) -> Result<ScanResult> {
        validate_key(prefix)?; // Prefix follows same key validation rules
        self.ensure_fresh_client().await?;

        // For prefix scans with hash-based bucketing, we must query ALL buckets
        // since items with the same prefix can be distributed across different buckets
        let mut all_items = Vec::new();
        let mut total_fetched = 0;
        let limit = limit.unwrap_or(1000);

        let client = self.client.read().await;

        // For simplicity, we'll query all 16 buckets sequentially
        // In production, this could be parallelized for better performance
        for bucket_id in 0..16 {
            if total_fetched >= limit {
                break;
            }

            let bucket = format!("bucket_{}", bucket_id);
            let mut expression_attribute_values = HashMap::new();
            expression_attribute_values.insert(":bucket".to_string(), AttributeValue::s(bucket));
            expression_attribute_values
                .insert(":prefix".to_string(), AttributeValue::s(prefix.to_string()));

            // Build request for this bucket
            let request = QueryRequest::builder()
                .table_name(self.table_name.clone())
                .key_condition_expression("pk = :bucket AND begins_with(sk, :prefix)".to_string())
                .expression_attribute_values(expression_attribute_values)
                .limit((limit - total_fetched) as i32)
                .build();

            let response = client.query(request).await.map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to scan prefix '{}' in bucket {}", prefix, bucket_id),
                    Some(prefix.to_string()),
                )
            })?;

            // Process items from this bucket
            for item in response.items {
                if total_fetched >= limit {
                    break;
                }

                // Check TTL expiry
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
                        if let Ok(value) = BASE64_STANDARD.decode(base64_value) {
                            all_items.push((key.clone(), value));
                            total_fetched += 1;
                        }
                    }
                }
            }
        }

        // For simplicity, we're not implementing cursor-based pagination across buckets
        // In production, this would require more complex cursor state management
        Ok(ScanResult {
            items: all_items,
            next_cursor: None,
        })
    }
}
