use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, PutOptions, ScanResult};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::firestore::{
    CollectionSelector, Direction, Document, FieldFilter, FieldFilterOperator, FieldReference,
    Filter, FirestoreApi, FirestoreClient, Order, QueryType, RunQueryRequest, StructuredQuery,
    Value,
};
use async_trait::async_trait;
use base64::{self, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

use super::{validate_key, validate_value};

/// Firestore document for KV storage
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KvDocument {
    value: String, // Base64-encoded binary data
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>, // For TTL policy
}

/// GCP Firestore implementation of the KV trait
pub struct GcpFirestoreKv {
    client: FirestoreClient,
    project_id: String,
    database_id: String,
    collection_name: String,
}

impl Debug for GcpFirestoreKv {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpFirestoreKv")
            .field("project_id", &self.project_id)
            .field("database_id", &self.database_id)
            .field("collection_name", &self.collection_name)
            .finish()
    }
}

impl GcpFirestoreKv {
    pub fn new(
        client: FirestoreClient,
        project_id: String,
        database_id: String,
        collection_name: String,
    ) -> Result<Self> {
        Ok(Self {
            client,
            project_id,
            database_id,
            collection_name,
        })
    }

    /// Checks if an item has expired based on TTL
    fn is_expired(&self, expires_at: Option<DateTime<Utc>>) -> bool {
        if let Some(expiry) = expires_at {
            Utc::now() >= expiry
        } else {
            false
        }
    }

    /// Converts a KV document to Firestore Document format
    fn kv_document_to_firestore(&self, _key: &str, kv_doc: &KvDocument) -> Document {
        let mut fields = HashMap::new();

        fields.insert(
            "value".to_string(),
            Value::StringValue(kv_doc.value.clone()),
        );
        fields.insert(
            "created_at".to_string(),
            Value::TimestampValue(kv_doc.created_at.to_rfc3339()),
        );

        if let Some(expires_at) = kv_doc.expires_at {
            fields.insert(
                "expires_at".to_string(),
                Value::TimestampValue(expires_at.to_rfc3339()),
            );
        }

        Document::builder().fields(fields).build()
    }

    /// Converts a KV document to Firestore Document format with name (for updates)
    fn kv_document_to_firestore_with_name(&self, key: &str, kv_doc: &KvDocument) -> Document {
        let mut fields = HashMap::new();

        fields.insert(
            "value".to_string(),
            Value::StringValue(kv_doc.value.clone()),
        );
        fields.insert(
            "created_at".to_string(),
            Value::TimestampValue(kv_doc.created_at.to_rfc3339()),
        );

        if let Some(expires_at) = kv_doc.expires_at {
            fields.insert(
                "expires_at".to_string(),
                Value::TimestampValue(expires_at.to_rfc3339()),
            );
        }

        Document::builder()
            .name(format!(
                "projects/{}/databases/{}/documents/{}/{}",
                self.project_id, self.database_id, self.collection_name, key
            ))
            .fields(fields)
            .build()
    }

    /// Converts a Firestore Document to KV document
    fn firestore_to_kv_document(&self, doc: &Document) -> Result<KvDocument> {
        let fields = doc.fields.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "gcp".to_string(),
                binding_name: "firestore".to_string(),
                field: "fields".to_string(),
                response_json: serde_json::to_string(doc).unwrap_or_default(),
            })
        })?;

        let value = match fields.get("value") {
            Some(Value::StringValue(v)) => v.clone(),
            _ => {
                return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: "firestore".to_string(),
                    field: "value".to_string(),
                    response_json: serde_json::to_string(doc).unwrap_or_default(),
                }))
            }
        };

        let created_at = match fields.get("created_at") {
            Some(Value::TimestampValue(t)) => DateTime::parse_from_rfc3339(t)
                .map_err(|_| {
                    AlienError::new(ErrorData::UnexpectedResponseFormat {
                        provider: "gcp".to_string(),
                        binding_name: "firestore".to_string(),
                        field: "created_at".to_string(),
                        response_json: serde_json::to_string(doc).unwrap_or_default(),
                    })
                })?
                .with_timezone(&Utc),
            _ => {
                return Err(AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: "firestore".to_string(),
                    field: "created_at".to_string(),
                    response_json: serde_json::to_string(doc).unwrap_or_default(),
                }))
            }
        };

        let expires_at = match fields.get("expires_at") {
            Some(Value::TimestampValue(t)) => Some(
                DateTime::parse_from_rfc3339(t)
                    .map_err(|_| {
                        AlienError::new(ErrorData::UnexpectedResponseFormat {
                            provider: "gcp".to_string(),
                            binding_name: "firestore".to_string(),
                            field: "expires_at".to_string(),
                            response_json: serde_json::to_string(doc).unwrap_or_default(),
                        })
                    })?
                    .with_timezone(&Utc),
            ),
            _ => None,
        };

        Ok(KvDocument {
            value,
            created_at,
            expires_at,
        })
    }
}

impl Binding for GcpFirestoreKv {}

#[async_trait]
impl Kv for GcpFirestoreKv {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        validate_key(key)?;

        let document_id = key;
        let document_path = format!("{}/{}", self.collection_name, document_id);

        match self
            .client
            .get_document(self.database_id.clone(), document_path, None, None, None)
            .await
        {
            Ok(doc) => {
                let kv_doc = self.firestore_to_kv_document(&doc)?;

                // Check TTL expiry (logical expiry contract)
                if self.is_expired(kv_doc.expires_at) {
                    return Ok(None); // Logically expired
                }

                let value = base64::engine::general_purpose::STANDARD
                    .decode(&kv_doc.value)
                    .into_alien_error()
                    .context(ErrorData::KvOperationFailed {
                        operation: "get".to_string(),
                        key: key.to_string(),
                        reason: "Failed to decode base64 value".to_string(),
                    })?;

                Ok(Some(value))
            }
            Err(e) => {
                // Check if this is a "not found" error
                match &e.error {
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) => {
                        Ok(None) // Document doesn't exist
                    }
                    _ => Err(crate::error::map_cloud_client_error(
                        e,
                        "Failed to get Firestore document".to_string(),
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

        let encoded_value = base64::engine::general_purpose::STANDARD.encode(&value);
        let kv_doc = KvDocument {
            value: encoded_value,
            created_at: Utc::now(),
            expires_at: options.ttl.map(|d| Utc::now() + d),
        };

        let document = self.kv_document_to_firestore(key, &kv_doc);

        if options.if_not_exists {
            let document_id = key.to_string();
            match self
                .client
                .create_document(
                    self.database_id.clone(),
                    self.collection_name.clone(),
                    Some(document_id),
                    document,
                    None,
                )
                .await
            {
                Ok(_) => Ok(true),
                Err(e) => {
                    // Check if this is a conflict (document already exists)
                    match &e.error {
                        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. }) => {
                            Ok(false)
                        }
                        _ => Err(crate::error::map_cloud_client_error(
                            e,
                            "Failed to create Firestore document".to_string(),
                            Some(key.to_string()),
                        )),
                    }
                }
            }
        } else {
            let document_id = key;
            let document_path = format!("{}/{}", self.collection_name, document_id);
            let document_with_name = self.kv_document_to_firestore_with_name(key, &kv_doc);

            self.client
                .patch_document(
                    self.database_id.clone(),
                    document_path,
                    document_with_name,
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| {
                    crate::error::map_cloud_client_error(
                        e,
                        "Failed to patch Firestore document".to_string(),
                        Some(key.to_string()),
                    )
                })?;

            Ok(true)
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;

        let document_id = key;
        let document_path = format!("{}/{}", self.collection_name, document_id);

        self.client
            .delete_document(self.database_id.clone(), document_path, None)
            .await
            .map_err(|e| {
                crate::error::map_cloud_client_error(
                    e,
                    "Failed to delete Firestore document".to_string(),
                    Some(key.to_string()),
                )
            })?;

        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        validate_key(key)?;

        let document_id = key;
        let document_path = format!("{}/{}", self.collection_name, document_id);

        match self
            .client
            .get_document(self.database_id.clone(), document_path, None, None, None)
            .await
        {
            Ok(doc) => {
                let kv_doc = self.firestore_to_kv_document(&doc)?;

                // Check TTL expiry (logical expiry contract)
                Ok(!self.is_expired(kv_doc.expires_at))
            }
            Err(e) => {
                match &e.error {
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) => {
                        Ok(false) // Document doesn't exist
                    }
                    _ => Err(crate::error::map_cloud_client_error(
                        e,
                        "Failed to get Firestore document".to_string(),
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

        let collection_selector = CollectionSelector::builder()
            .collection_id(self.collection_name.clone())
            .build();

        let mut structured_query = StructuredQuery::builder()
            .from(vec![collection_selector])
            .order_by(vec![Order::builder()
                .field(
                    FieldReference::builder()
                        .field_path("__name__".to_string())
                        .build(),
                )
                .direction(Direction::Ascending)
                .build()])
            .build();

        // Add prefix filter
        if !prefix.is_empty() {
            let document_id_prefix = prefix;
            let prefix_filter = Filter::FieldFilter(
                FieldFilter::builder()
                    .field(
                        FieldReference::builder()
                            .field_path("__name__".to_string())
                            .build(),
                    )
                    .op(FieldFilterOperator::GreaterThanOrEqual)
                    .value(Value::ReferenceValue(format!(
                        "projects/{}/databases/{}/documents/{}/{}",
                        self.project_id, self.database_id, self.collection_name, document_id_prefix
                    )))
                    .build(),
            );

            structured_query.r#where = Some(prefix_filter);
        }

        if let Some(limit) = limit {
            structured_query.limit = Some(limit as i32);
        }

        if let Some(ref cursor) = cursor {
            // For simplicity, use offset-based pagination
            if let Ok(offset) = cursor.parse::<i32>() {
                structured_query.offset = Some(offset);
            }
        }

        let query_request = RunQueryRequest::builder()
            .parent(format!(
                "projects/{}/databases/{}/documents",
                self.project_id, self.database_id
            ))
            .query_type(QueryType::StructuredQuery(structured_query))
            .build();

        let query_responses = self
            .client
            .run_query(self.database_id.clone(), query_request)
            .await
            .map_err(|e| {
                crate::error::map_cloud_client_error(
                    e,
                    "Failed to run Firestore query".to_string(),
                    Some(prefix.to_string()),
                )
            })?;

        let items: Vec<(String, Vec<u8>)> = query_responses
            .iter()
            .filter_map(|response| {
                let doc = response.document.as_ref()?;
                let doc_name = doc.name.as_ref()?;

                // Extract document ID from document name
                let document_id = doc_name.split('/').last()?.to_string();

                // Document ID is now the key directly (no encoding needed)
                let key = document_id;

                // Check if key starts with prefix
                if !key.starts_with(prefix) {
                    return None;
                }

                let kv_doc = self.firestore_to_kv_document(doc).ok()?;

                // Check TTL expiry
                if self.is_expired(kv_doc.expires_at) {
                    return None; // Skip expired items
                }

                let value = base64::engine::general_purpose::STANDARD
                    .decode(&kv_doc.value)
                    .ok()?;
                Some((key, value))
            })
            .collect();

        let next_cursor = if items.len() == limit.unwrap_or(usize::MAX) {
            // Simple offset-based pagination
            let current_offset = cursor
                .as_ref()
                .and_then(|c| c.parse::<usize>().ok())
                .unwrap_or(0);
            Some((current_offset + items.len()).to_string())
        } else {
            None
        };

        Ok(ScanResult { items, next_cursor })
    }
}
