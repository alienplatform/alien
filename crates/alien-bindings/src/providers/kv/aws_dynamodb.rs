use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use aws_sdk_dynamodb::{primitives::Blob, types::AttributeValue};
use chrono::Utc;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use super::{validate_key, validate_value};

type DynamoDbItem = HashMap<String, AttributeValue>;

/// Minimal DynamoDB operations required by the KV binding.
#[async_trait]
pub trait DynamoDbKvClient: Debug + Send + Sync {
    /// Get an item by primary key.
    async fn get_item(
        &self,
        table_name: &str,
        key: DynamoDbItem,
        projection_expression: Option<&str>,
        expression_attribute_names: Option<HashMap<String, String>>,
    ) -> Result<Option<DynamoDbItem>>;

    /// Put an item, returning false only when an if-not-exists condition fails.
    async fn put_item(
        &self,
        table_name: &str,
        item: DynamoDbItem,
        condition_expression: Option<&str>,
    ) -> Result<bool>;

    /// Delete an item by primary key.
    async fn delete_item(&self, table_name: &str, key: DynamoDbItem) -> Result<()>;

    /// Query items by bucket and key prefix.
    async fn query_prefix(
        &self,
        table_name: &str,
        bucket: String,
        prefix: &str,
        limit: i32,
    ) -> Result<Vec<DynamoDbItem>>;
}

#[async_trait]
impl DynamoDbKvClient for aws_sdk_dynamodb::Client {
    async fn get_item(
        &self,
        table_name: &str,
        key: DynamoDbItem,
        projection_expression: Option<&str>,
        expression_attribute_names: Option<HashMap<String, String>>,
    ) -> Result<Option<DynamoDbItem>> {
        let mut request = self.get_item().table_name(table_name).set_key(Some(key));

        if let Some(projection_expression) = projection_expression {
            request = request.projection_expression(projection_expression);
        }
        if let Some(expression_attribute_names) = expression_attribute_names {
            request = request.set_expression_attribute_names(Some(expression_attribute_names));
        }

        let response =
            request
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to get DynamoDB item from table '{}'", table_name),
                    resource_id: Some(table_name.to_string()),
                })?;

        Ok(response.item)
    }

    async fn put_item(
        &self,
        table_name: &str,
        item: DynamoDbItem,
        condition_expression: Option<&str>,
    ) -> Result<bool> {
        let mut request = self.put_item().table_name(table_name).set_item(Some(item));

        if let Some(condition_expression) = condition_expression {
            request = request.condition_expression(condition_expression);
        }

        match request.send().await {
            Ok(_) => Ok(true),
            Err(error) if condition_expression.is_some() => {
                let is_condition_failure = error
                    .as_service_error()
                    .map(|service_error| service_error.is_conditional_check_failed_exception())
                    .unwrap_or(false);

                if is_condition_failure {
                    Ok(false)
                } else {
                    Err(error)
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "Failed to put DynamoDB item in table '{}'",
                                table_name
                            ),
                            resource_id: Some(table_name.to_string()),
                        })
                }
            }
            Err(error) => Err(error)
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to put DynamoDB item in table '{}'", table_name),
                    resource_id: Some(table_name.to_string()),
                }),
        }
    }

    async fn delete_item(&self, table_name: &str, key: DynamoDbItem) -> Result<()> {
        self.delete_item()
            .table_name(table_name)
            .set_key(Some(key))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete DynamoDB item from table '{}'", table_name),
                resource_id: Some(table_name.to_string()),
            })?;

        Ok(())
    }

    async fn query_prefix(
        &self,
        table_name: &str,
        bucket: String,
        prefix: &str,
        limit: i32,
    ) -> Result<Vec<DynamoDbItem>> {
        let mut expression_attribute_values = HashMap::new();
        expression_attribute_values.insert(":bucket".to_string(), AttributeValue::S(bucket));
        expression_attribute_values
            .insert(":prefix".to_string(), AttributeValue::S(prefix.to_string()));

        let response = self
            .query()
            .table_name(table_name)
            .key_condition_expression("pk = :bucket AND begins_with(sk, :prefix)")
            .set_expression_attribute_values(Some(expression_attribute_values))
            .limit(limit)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to query DynamoDB table '{}' by prefix '{}'",
                    table_name, prefix
                ),
                resource_id: Some(table_name.to_string()),
            })?;

        Ok(response.items.unwrap_or_default())
    }
}

/// AWS DynamoDB implementation of the KV trait.
pub struct AwsDynamodbKv {
    client: Arc<dyn DynamoDbKvClient>,
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
    pub fn new(table_name: String, client: Arc<dyn DynamoDbKvClient>) -> Self {
        Self { client, table_name }
    }

    /// Creates a hash bucket for load distribution.
    fn hash_bucket(&self, key: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let bucket_id = hasher.finish() % 16;
        format!("bucket_{}", bucket_id)
    }

    /// Checks if an item has expired based on TTL.
    fn is_expired(&self, ttl_epoch: Option<i64>) -> bool {
        ttl_epoch.is_some_and(|ttl_timestamp| Utc::now().timestamp() >= ttl_timestamp)
    }

    fn primary_key(&self, key: &str) -> DynamoDbItem {
        let mut primary_key = HashMap::new();
        primary_key.insert("pk".to_string(), AttributeValue::S(self.hash_bucket(key)));
        primary_key.insert("sk".to_string(), AttributeValue::S(key.to_string()));
        primary_key
    }

    fn ttl_epoch(item: &DynamoDbItem) -> Option<i64> {
        item.get("ttl")
            .and_then(|ttl| ttl.as_n().ok())
            .and_then(|ttl| ttl.parse::<i64>().ok())
    }

    fn value_bytes(item: &DynamoDbItem, key: &str) -> Result<Vec<u8>> {
        item.get("value")
            .and_then(|value| value.as_b().ok())
            .map(|value| value.as_ref().to_vec())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Missing or invalid value attribute for key '{}'", key),
                    resource_id: Some(key.to_string()),
                })
            })
    }

    fn key_string(item: &DynamoDbItem) -> Option<String> {
        item.get("sk")
            .and_then(|key| key.as_s().ok())
            .map(ToString::to_string)
    }
}

impl Binding for AwsDynamodbKv {}

#[async_trait]
impl Kv for AwsDynamodbKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        validate_key(key)?;

        let response = self
            .client
            .get_item(&self.table_name, self.primary_key(key), None, None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get item with key '{}'", key),
                resource_id: Some(key.to_string()),
            })?;

        let Some(item) = response else {
            return Ok(None);
        };

        if self.is_expired(Self::ttl_epoch(&item)) {
            return Ok(None);
        }

        Ok(Some(Self::value_bytes(&item, key)?))
    }

    async fn put(&self, key: &str, value: Vec<u8>, options: Option<PutOptions>) -> Result<bool> {
        validate_key(key)?;
        validate_value(&value)?;

        let options = options.unwrap_or_default();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(self.hash_bucket(key)));
        item.insert("sk".to_string(), AttributeValue::S(key.to_string()));
        item.insert("value".to_string(), AttributeValue::B(Blob::new(value)));

        if let Some(ttl) = options.ttl {
            let expires_at = (Utc::now() + ttl).timestamp();
            item.insert("ttl".to_string(), AttributeValue::N(expires_at.to_string()));
        }

        let condition_expression = options
            .if_not_exists
            .then_some("attribute_not_exists(pk) AND attribute_not_exists(sk)");

        self.client
            .put_item(&self.table_name, item, condition_expression)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to put item with key '{}'", key),
                resource_id: Some(key.to_string()),
            })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;

        self.client
            .delete_item(&self.table_name, self.primary_key(key))
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete item with key '{}'", key),
                resource_id: Some(key.to_string()),
            })
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        validate_key(key)?;

        let mut expression_attribute_names = HashMap::new();
        expression_attribute_names.insert("#ttl".to_string(), "ttl".to_string());

        let response = self
            .client
            .get_item(
                &self.table_name,
                self.primary_key(key),
                Some("pk, #ttl"),
                Some(expression_attribute_names),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to check existence of item with key '{}'", key),
                resource_id: Some(key.to_string()),
            })?;

        let Some(item) = response else {
            return Ok(false);
        };

        Ok(!self.is_expired(Self::ttl_epoch(&item)))
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        _cursor: Option<String>,
    ) -> Result<ScanResult> {
        validate_key(prefix)?;

        let mut all_items = Vec::new();
        let mut total_fetched = 0;
        let limit = limit.unwrap_or(1000);

        for bucket_id in 0..16 {
            if total_fetched >= limit {
                break;
            }

            let bucket = format!("bucket_{}", bucket_id);
            let items = self
                .client
                .query_prefix(
                    &self.table_name,
                    bucket,
                    prefix,
                    (limit - total_fetched) as i32,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to scan prefix '{}'", prefix),
                    resource_id: Some(prefix.to_string()),
                })?;

            for item in items {
                if total_fetched >= limit {
                    break;
                }

                if self.is_expired(Self::ttl_epoch(&item)) {
                    continue;
                }

                let Some(key) = Self::key_string(&item) else {
                    continue;
                };
                let Ok(value) = Self::value_bytes(&item, &key) else {
                    continue;
                };

                all_items.push((key, value));
                total_fetched += 1;
            }
        }

        Ok(ScanResult {
            items: all_items,
            next_cursor: None,
        })
    }
}
