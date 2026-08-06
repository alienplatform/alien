use crate::error::{ErrorData, Result};
use crate::traits::{Binding, Kv, KvEntry, PutCondition, PutOptions, ScanResult};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::firestore::{
    CollectionSelector, CompositeFilter, CompositeFilterOperator, Cursor, Direction, Document,
    FieldFilter, FieldFilterOperator, FieldReference, Filter, FirestoreApi, FirestoreClient, Order,
    QueryType, RunQueryRequest, StructuredQuery, Value,
};
use async_trait::async_trait;
use base64::{self, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

use super::{decode_version, encode_version, validate_key, validate_value};

/// Firestore document for KV storage
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KvDocument {
    value: String, // Base64-encoded binary data
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>, // For TTL policy
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorState {
    version: u8,
    prefix: String,
    last_document_name: String,
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

    fn document_name(&self, key: &str) -> String {
        format!(
            "projects/{}/databases/{}/documents/{}/{}",
            self.project_id, self.database_id, self.collection_name, key
        )
    }

    fn encode_cursor(state: &CursorState) -> Result<String> {
        let json =
            serde_json::to_vec(state)
                .into_alien_error()
                .context(ErrorData::InvalidInput {
                    operation_context: "Firestore KV scan cursor encoding".to_string(),
                    details: "Failed to serialize cursor state".to_string(),
                    field_name: Some("cursor".to_string()),
                })?;
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
    }

    fn decode_cursor(&self, prefix: &str, cursor: &str) -> Result<CursorState> {
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cursor)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                operation_context: "Firestore KV scan cursor decoding".to_string(),
                details: "Invalid cursor encoding".to_string(),
                field_name: Some("cursor".to_string()),
            })?;
        let state: CursorState = serde_json::from_slice(&decoded)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                operation_context: "Firestore KV scan cursor decoding".to_string(),
                details: "Invalid cursor JSON".to_string(),
                field_name: Some("cursor".to_string()),
            })?;
        if state.version != 1
            || state.prefix != prefix
            || !state
                .last_document_name
                .starts_with(&self.document_name(prefix))
        {
            return Err(AlienError::new(ErrorData::InvalidInput {
                operation_context: "Firestore KV scan cursor validation".to_string(),
                details: "Cursor does not belong to this prefix scan".to_string(),
                field_name: Some("cursor".to_string()),
            }));
        }
        Ok(state)
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
    /// Attempt to take over a logically-expired document after a
    /// conditional create conflict. Returns `Ok(true)` only when THIS
    /// caller replaced the expired document (an `updateTime` precondition
    /// makes the replace atomic — a racing taker bumps the update time and
    /// this patch loses); a live document, a lost race, or a deletion in
    /// between all resolve to `Ok(false)`.
    async fn try_take_over_expired(&self, key: &str, document: &Document) -> Result<bool> {
        use alien_client_core::ErrorData as CloudErrorData;
        use alien_gcp_clients::gcp::firestore::{Precondition, PreconditionType};

        let document_path = format!("{}/{}", self.collection_name, key);
        let existing = match self
            .client
            .get_document(
                self.database_id.clone(),
                document_path.clone(),
                None,
                None,
                None,
            )
            .await
        {
            Ok(existing) => existing,
            Err(e)
                if matches!(
                    e.error.as_ref(),
                    Some(CloudErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(false);
            }
            Err(e) => {
                return Err(crate::error::map_cloud_client_error(
                    e,
                    format!("Failed to read existing document for key '{}'", key),
                    Some(key.to_string()),
                ));
            }
        };

        let kv_doc = self.firestore_to_kv_document(&existing)?;
        if !self.is_expired(kv_doc.expires_at) {
            return Ok(false);
        }
        let Some(update_time) = existing.update_time.clone() else {
            return Ok(false);
        };

        match self
            .client
            .patch_document(
                self.database_id.clone(),
                document_path,
                document.clone(),
                None,
                None,
                Some(Precondition {
                    condition: PreconditionType::UpdateTime(update_time),
                }),
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => match e.error.as_ref() {
                // A lost race: the precondition mismatch maps to Conflict
                // (FAILED_PRECONDITION → RemoteResourceConflict in
                // map_gcp_error), and a deletion in between maps to NotFound.
                Some(CloudErrorData::RemoteResourceConflict { .. })
                | Some(CloudErrorData::RemoteResourceNotFound { .. }) => Ok(false),
                _ => Err(crate::error::map_cloud_client_error(
                    e,
                    format!("Failed to take over expired document for key '{}'", key),
                    Some(key.to_string()),
                )),
            },
        }
    }

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
    async fn get(&self, key: &str) -> Result<Option<KvEntry>> {
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

                let update_time = doc.update_time.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::UnexpectedResponseFormat {
                        provider: "gcp".to_string(),
                        binding_name: "firestore".to_string(),
                        field: "updateTime".to_string(),
                        response_json: serde_json::to_string(&doc).unwrap_or_default(),
                    })
                })?;
                Ok(Some(KvEntry {
                    key: key.to_string(),
                    value,
                    version: encode_version(
                        key,
                        update_time,
                        kv_doc
                            .expires_at
                            .map(|expires_at| expires_at.timestamp_millis()),
                    )?,
                }))
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

        if matches!(options.condition, PutCondition::Absent) {
            let document_id = key.to_string();
            match self
                .client
                .create_document(
                    self.database_id.clone(),
                    self.collection_name.clone(),
                    Some(document_id),
                    document.clone(),
                    None,
                )
                .await
            {
                Ok(_) => Ok(true),
                Err(e) => {
                    // Check if this is a conflict (document already exists)
                    match &e.error {
                        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. }) => {
                            // Expired documents count as ABSENT, matching the
                            // local provider's atomic takeover: Firestore's
                            // TTL deletion can lag the logical expiry by
                            // hours, and without a takeover a conditional
                            // put (e.g. a command lease after the previous
                            // holder died) stays blocked until then.
                            self.try_take_over_expired(key, &document).await
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
            let current_document = match &options.condition {
                PutCondition::Version(version) => {
                    let expected = decode_version(key, version)?;
                    if expected.expired {
                        return Ok(false);
                    }
                    Some(alien_gcp_clients::gcp::firestore::Precondition {
                        condition: alien_gcp_clients::gcp::firestore::PreconditionType::UpdateTime(
                            expected.backend_version,
                        ),
                    })
                }
                PutCondition::None => None,
                PutCondition::Absent => unreachable!("handled above"),
            };

            match self
                .client
                .patch_document(
                    self.database_id.clone(),
                    document_path,
                    document_with_name,
                    None,
                    None,
                    current_document,
                )
                .await
            {
                Ok(_) => Ok(true),
                Err(error)
                    if matches!(options.condition, PutCondition::Version(_))
                        && matches!(
                            error.error.as_ref(),
                            Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                                | Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) =>
                {
                    Ok(false)
                }
                Err(error) => Err(crate::error::map_cloud_client_error(
                    error,
                    "Failed to patch Firestore document".to_string(),
                    Some(key.to_string()),
                )),
            }
        }
    }

    async fn delete(&self, key: &str, if_version: Option<&str>) -> Result<bool> {
        validate_key(key)?;

        let document_id = key;
        let document_path = format!("{}/{}", self.collection_name, document_id);

        let current_document = if let Some(version) = if_version {
            let expected = decode_version(key, version)?;
            if expected.expired {
                return Ok(false);
            }
            Some(alien_gcp_clients::gcp::firestore::Precondition {
                condition: alien_gcp_clients::gcp::firestore::PreconditionType::UpdateTime(
                    expected.backend_version,
                ),
            })
        } else {
            None
        };

        match self
            .client
            .delete_document(self.database_id.clone(), document_path, current_document)
            .await
        {
            Ok(()) => Ok(true),
            Err(error)
                if if_version.is_some()
                    && matches!(
                        error.error.as_ref(),
                        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                            | Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
            {
                Ok(false)
            }
            Err(error) => Err(crate::error::map_cloud_client_error(
                error,
                "Failed to delete Firestore document".to_string(),
                Some(key.to_string()),
            )),
        }
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
        validate_key(prefix)?;

        let limit = limit.unwrap_or(1000);
        let cursor_state = cursor
            .as_deref()
            .map(|cursor| self.decode_cursor(prefix, cursor))
            .transpose()?;
        if limit == 0 {
            return Ok(ScanResult {
                items: Vec::new(),
                next_cursor: cursor,
            });
        }

        let collection_selector = CollectionSelector::builder()
            .collection_id(self.collection_name.clone())
            .build();

        let document_name_field = FieldReference::builder()
            .field_path("__name__".to_string())
            .build();
        let lower = self.document_name(prefix);
        let upper = self.document_name(&format!("{}~", prefix));
        let mut structured_query = StructuredQuery::builder()
            .from(vec![collection_selector])
            .order_by(vec![Order::builder()
                .field(document_name_field.clone())
                .direction(Direction::Ascending)
                .build()])
            .r#where(Filter::CompositeFilter(
                CompositeFilter::builder()
                    .op(CompositeFilterOperator::And)
                    .filters(vec![
                        Filter::FieldFilter(
                            FieldFilter::builder()
                                .field(document_name_field.clone())
                                .op(FieldFilterOperator::GreaterThanOrEqual)
                                .value(Value::ReferenceValue(lower))
                                .build(),
                        ),
                        Filter::FieldFilter(
                            FieldFilter::builder()
                                .field(document_name_field)
                                .op(FieldFilterOperator::LessThan)
                                .value(Value::ReferenceValue(upper))
                                .build(),
                        ),
                    ])
                    .build(),
            ))
            .limit(i32::try_from(limit).unwrap_or(i32::MAX))
            .build();

        if let Some(state) = cursor_state {
            structured_query.start_at = Some(
                Cursor::builder()
                    .values(vec![Value::ReferenceValue(state.last_document_name)])
                    .before(false)
                    .build(),
            );
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

        let documents = query_responses
            .iter()
            .filter_map(|response| response.document.as_ref())
            .collect::<Vec<_>>();
        let last_document_name = documents.last().and_then(|document| document.name.clone());
        let mut items = Vec::with_capacity(documents.len());
        for document in &documents {
            let Some(name) = &document.name else {
                continue;
            };
            let Some(key) = name.rsplit('/').next() else {
                continue;
            };
            let kv_doc = self.firestore_to_kv_document(document)?;
            if self.is_expired(kv_doc.expires_at) {
                continue;
            }
            let value = base64::engine::general_purpose::STANDARD
                .decode(&kv_doc.value)
                .into_alien_error()
                .context(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: "firestore".to_string(),
                    field: "value".to_string(),
                    response_json: serde_json::to_string(document).unwrap_or_default(),
                })?;
            let update_time = document.update_time.clone().ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: "firestore".to_string(),
                    field: "updateTime".to_string(),
                    response_json: serde_json::to_string(document).unwrap_or_default(),
                })
            })?;
            items.push(KvEntry {
                key: key.to_string(),
                value,
                version: encode_version(
                    key,
                    update_time,
                    kv_doc
                        .expires_at
                        .map(|expires_at| expires_at.timestamp_millis()),
                )?,
            });
        }

        let next_cursor = if documents.len() == limit {
            last_document_name
                .map(|last_document_name| {
                    Self::encode_cursor(&CursorState {
                        version: 1,
                        prefix: prefix.to_string(),
                        last_document_name,
                    })
                })
                .transpose()?
        } else {
            None
        };

        Ok(ScanResult { items, next_cursor })
    }
}
