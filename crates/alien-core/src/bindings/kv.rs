//! KV binding definitions for key-value storage across different platforms
//!
//! This module defines the binding parameters for different KV services:
//! - AWS DynamoDB
//! - GCP Firestore
//! - Azure Table Storage
//! - Redis (for Kubernetes/local)

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Represents a KV binding for key-value storage across platforms
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum KvBinding {
    /// AWS DynamoDB binding
    Dynamodb(DynamodbKvBinding),
    /// GCP Firestore binding
    Firestore(FirestoreKvBinding),
    /// Azure Table Storage binding
    TableStorage(TableStorageKvBinding),
    /// Redis binding (for Kubernetes/local)
    Redis(RedisKvBinding),
    /// Local development KV (for testing)
    #[serde(rename = "local-kv")]
    Local(LocalKvBinding),
}

/// AWS DynamoDB KV binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct DynamodbKvBinding {
    /// The DynamoDB table name
    pub table_name: BindingValue<String>,
    /// The AWS region where the table is located
    pub region: BindingValue<String>,
    /// Optional endpoint URL for local testing or custom endpoints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_url: Option<BindingValue<String>>,
}

/// GCP Firestore KV binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct FirestoreKvBinding {
    /// The GCP project ID
    pub project_id: BindingValue<String>,
    /// The Firestore database ID (default "(default)")
    pub database_id: BindingValue<String>,
    /// The collection name for KV storage
    pub collection_name: BindingValue<String>,
}

/// Azure Table Storage KV binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct TableStorageKvBinding {
    /// The Azure resource group name
    pub resource_group_name: BindingValue<String>,
    /// The storage account name
    pub account_name: BindingValue<String>,
    /// The table name
    pub table_name: BindingValue<String>,
}

/// Redis KV binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RedisKvBinding {
    /// Redis connection URL (e.g., "redis://localhost:6379" or "rediss://...")
    pub connection_url: BindingValue<String>,
    /// Optional key prefix for namespacing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_prefix: Option<BindingValue<String>>,
    /// Optional database number (0-15 for standard Redis)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<BindingValue<u8>>,
}

/// Local development KV binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalKvBinding {
    /// The base data directory for local storage
    pub data_dir: BindingValue<String>,
    /// Optional key prefix for namespacing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_prefix: Option<BindingValue<String>>,
}

impl KvBinding {
    /// Creates a DynamoDB KV binding
    pub fn dynamodb(
        table_name: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Dynamodb(DynamodbKvBinding {
            table_name: table_name.into(),
            region: region.into(),
            endpoint_url: None,
        })
    }

    /// Creates a DynamoDB KV binding with custom endpoint
    pub fn dynamodb_with_endpoint(
        table_name: impl Into<BindingValue<String>>,
        region: impl Into<BindingValue<String>>,
        endpoint_url: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Dynamodb(DynamodbKvBinding {
            table_name: table_name.into(),
            region: region.into(),
            endpoint_url: Some(endpoint_url.into()),
        })
    }

    /// Creates a Firestore KV binding
    pub fn firestore(
        project_id: impl Into<BindingValue<String>>,
        database_id: impl Into<BindingValue<String>>,
        collection_name: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Firestore(FirestoreKvBinding {
            project_id: project_id.into(),
            database_id: database_id.into(),
            collection_name: collection_name.into(),
        })
    }

    /// Creates an Azure Table Storage KV binding
    pub fn table_storage(
        resource_group_name: impl Into<BindingValue<String>>,
        account_name: impl Into<BindingValue<String>>,
        table_name: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::TableStorage(TableStorageKvBinding {
            resource_group_name: resource_group_name.into(),
            account_name: account_name.into(),
            table_name: table_name.into(),
        })
    }

    /// Creates a Redis KV binding
    pub fn redis(connection_url: impl Into<BindingValue<String>>) -> Self {
        Self::Redis(RedisKvBinding {
            connection_url: connection_url.into(),
            key_prefix: None,
            database: None,
        })
    }

    /// Creates a Redis KV binding with prefix and database
    pub fn redis_with_options(
        connection_url: impl Into<BindingValue<String>>,
        key_prefix: Option<impl Into<BindingValue<String>>>,
        database: Option<u8>,
    ) -> Self {
        Self::Redis(RedisKvBinding {
            connection_url: connection_url.into(),
            key_prefix: key_prefix.map(|p| p.into()),
            database: database.map(BindingValue::value),
        })
    }

    /// Creates a local KV binding
    pub fn local(data_dir: impl Into<BindingValue<String>>) -> Self {
        Self::Local(LocalKvBinding {
            data_dir: data_dir.into(),
            key_prefix: None,
        })
    }

    /// Creates a local KV binding with prefix
    pub fn local_with_prefix(
        data_dir: impl Into<BindingValue<String>>,
        key_prefix: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Local(LocalKvBinding {
            data_dir: data_dir.into(),
            key_prefix: Some(key_prefix.into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_dynamodb_binding() {
        let binding = KvBinding::dynamodb("my-table", "us-east-1");

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"dynamodb""#));

        let deserialized: KvBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_firestore_binding() {
        let binding = KvBinding::firestore("my-project", "(default)", "kv");

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"firestore""#));

        let deserialized: KvBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_table_storage_binding() {
        let binding = KvBinding::table_storage("myresourcegroup", "myaccount", "mytable");

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"tablestorage""#));

        let deserialized: KvBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_redis_binding() {
        let binding = KvBinding::redis("redis://localhost:6379");

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"redis""#));

        let deserialized: KvBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_local_binding() {
        let binding = KvBinding::local("/tmp/kv");

        let json = serde_json::to_string(&binding).unwrap();
        assert!(json.contains(r#""service":"local-kv""#));

        let deserialized: KvBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }

    #[test]
    fn test_binding_value_expressions() {
        let binding = KvBinding::Dynamodb(DynamodbKvBinding {
            table_name: BindingValue::expression(json!({"Ref": "MyTable"})),
            region: BindingValue::value("us-east-1".to_string()),
            endpoint_url: None,
        });

        let json = serde_json::to_string(&binding).unwrap();
        let deserialized: KvBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding, deserialized);
    }
}
