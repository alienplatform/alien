use crate::{
    error::{AppError, AppResult, ErrorData, Result},
    models::{AcceptedTrace, IngestionFailure, IngestionPointer, Trace, TracePage, TraceQuery},
    store::{TraceReader, TraceWriter},
};
use alien_error::{Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use alien_sdk::{
    traits::{MessagePayload, Storage},
    Queue,
};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use bytes::Bytes;
use object_store::{path::Path, ObjectStore};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};

pub struct ApiState {
    storage: Arc<dyn Storage>,
    queue: Queue,
    reader: RwLock<Option<Arc<TraceReader>>>,
}

impl ApiState {
    pub fn new(storage: Arc<dyn Storage>, queue: Queue) -> Self {
        Self {
            storage,
            queue,
            reader: RwLock::new(None),
        }
    }

    async fn reader(&self) -> AppResult<Arc<TraceReader>> {
        if let Some(reader) = self.reader.read().await.as_ref() {
            return Ok(Arc::clone(reader));
        }
        let mut slot = self.reader.write().await;
        if let Some(reader) = slot.as_ref() {
            return Ok(Arc::clone(reader));
        }
        let object_store: Arc<dyn ObjectStore> = Arc::clone(&self.storage) as Arc<dyn ObjectStore>;
        let reader = Arc::new(TraceReader::open(object_store).await.map_err(|error| {
            // On a brand-new deployment the writer may not have created SlateDB's
            // first manifest yet. Keep this expected startup race friendly to callers,
            // while retaining the underlying detail in the service log.
            tracing::info!(code = %error.code, "trace store reader is not ready");
            crate::error::ApiError::not_ready()
        })?);
        *slot = Some(Arc::clone(&reader));
        Ok(reader)
    }
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

pub async fn ingest(
    State(state): State<Arc<ApiState>>,
    Json(trace): Json<Trace>,
) -> AppResult<(StatusCode, Json<AcceptedTrace>)> {
    let encoded =
        serde_json::to_vec(&trace)
            .into_alien_error()
            .context(ErrorData::SerializationFailed {
                operation: "encode submitted trace".to_string(),
            })?;
    trace.validate(encoded.len())?;

    let content_hash = hex::encode(Sha256::digest(&encoded));
    let staging_path = format!(
        "staging/v1/{}/{}.json",
        hex::encode(&trace.trace_id),
        content_hash
    );
    state
        .storage
        .put(
            &Path::from(staging_path.as_str()),
            Bytes::from(encoded).into(),
        )
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "stage trace".to_string(),
            path: staging_path.clone(),
        })?;

    let pointer = IngestionPointer {
        trace_id: trace.trace_id.clone(),
        content_hash: content_hash.clone(),
        staging_path,
    };
    let pointer = serde_json::to_value(pointer).into_alien_error().context(
        ErrorData::SerializationFailed {
            operation: "encode ingestion pointer".to_string(),
        },
    )?;
    state
        .queue
        .send(MessagePayload::Json(pointer))
        .await
        .context(ErrorData::QueueOperationFailed {
            operation: "enqueue trace".to_string(),
        })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(AcceptedTrace {
            trace_id: trace.trace_id,
            content_hash,
            status: "accepted",
        }),
    ))
}

pub async fn get_trace(
    State(state): State<Arc<ApiState>>,
    AxumPath(trace_id): AxumPath<String>,
) -> AppResult<Json<crate::models::StoredTrace>> {
    Ok(Json(state.reader().await?.get(&trace_id).await?))
}

pub async fn list_traces(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<TraceQuery>,
) -> AppResult<Json<TracePage>> {
    Ok(Json(state.reader().await?.list(&query).await?))
}

pub async fn open_writer(
    storage: Arc<dyn Storage>,
    queue: Queue,
) -> Result<(TraceWriter, Arc<dyn Storage>, Queue)> {
    let object_store: Arc<dyn ObjectStore> = Arc::clone(&storage) as Arc<dyn ObjectStore>;
    let writer = TraceWriter::open(object_store).await?;
    Ok((writer, storage, queue))
}

pub async fn run_writer(
    writer: TraceWriter,
    storage: Arc<dyn Storage>,
    queue: Queue,
) -> Result<()> {
    loop {
        let messages = queue
            .receive(10)
            .await
            .context(ErrorData::QueueOperationFailed {
                operation: "receive traces".to_string(),
            })?;
        if messages.is_empty() {
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }

        for message in messages {
            let receipt_handle = message.receipt_handle.clone();
            let pointer = decode_pointer(message.payload);
            let outcome = match pointer {
                Ok(pointer) => process_pointer(&writer, &storage, &pointer).await,
                Err(reason) => Err(ProcessError::Permanent(PermanentFailure {
                    trace_id: None,
                    staging_path: None,
                    reason,
                })),
            };

            match outcome {
                Ok(staging_path) => {
                    queue
                        .ack(&receipt_handle)
                        .await
                        .context(ErrorData::QueueOperationFailed {
                            operation: "ack committed trace".to_string(),
                        })?;
                    if let Err(error) = storage.delete(&Path::from(staging_path.as_str())).await {
                        tracing::warn!(path = %staging_path, %error, "failed to delete committed staging object");
                    }
                }
                Err(ProcessError::Permanent(failure)) => {
                    record_failure(&storage, &failure, message.attempt, &receipt_handle).await?;
                    queue
                        .ack(&receipt_handle)
                        .await
                        .context(ErrorData::QueueOperationFailed {
                            operation: "ack rejected trace".to_string(),
                        })?;
                }
                Err(ProcessError::Retryable(error)) => {
                    tracing::warn!(
                        trace_id = ?error.trace_id,
                        code = %error.error.code,
                        "trace ingestion will be retried"
                    );
                    queue
                        .nack(&receipt_handle)
                        .await
                        .context(ErrorData::QueueOperationFailed {
                            operation: "release trace for retry".to_string(),
                        })?;
                }
            }
        }
    }
}

struct PermanentFailure {
    trace_id: Option<String>,
    staging_path: Option<String>,
    reason: String,
}

struct RetryableFailure {
    trace_id: Option<String>,
    error: crate::error::Error,
}

enum ProcessError {
    Permanent(PermanentFailure),
    Retryable(RetryableFailure),
}

fn decode_pointer(payload: MessagePayload) -> std::result::Result<IngestionPointer, String> {
    match payload {
        MessagePayload::Json(value) => serde_json::from_value(value),
        MessagePayload::Text(value) => serde_json::from_str(&value),
    }
    .map_err(|error| format!("invalid ingestion pointer: {error}"))
}

async fn process_pointer(
    writer: &TraceWriter,
    storage: &Arc<dyn Storage>,
    pointer: &IngestionPointer,
) -> std::result::Result<String, ProcessError> {
    let path = Path::from(pointer.staging_path.as_str());
    let result = storage.get(&path).await.map_err(|error| {
        if matches!(error, object_store::Error::NotFound { .. }) {
            ProcessError::Permanent(PermanentFailure {
                trace_id: Some(pointer.trace_id.clone()),
                staging_path: Some(pointer.staging_path.clone()),
                reason: "staged trace object was not found".to_string(),
            })
        } else {
            ProcessError::Retryable(RetryableFailure {
                trace_id: Some(pointer.trace_id.clone()),
                error: error
                    .into_alien_error()
                    .context(ErrorData::StorageOperationFailed {
                        operation: "read staged trace".to_string(),
                        path: pointer.staging_path.clone(),
                    }),
            })
        }
    })?;
    let bytes = result.bytes().await.map_err(|error| {
        ProcessError::Retryable(RetryableFailure {
            trace_id: Some(pointer.trace_id.clone()),
            error: error
                .into_alien_error()
                .context(ErrorData::StorageOperationFailed {
                    operation: "read staged trace body".to_string(),
                    path: pointer.staging_path.clone(),
                }),
        })
    })?;
    let actual_hash = hex::encode(Sha256::digest(&bytes));
    if actual_hash != pointer.content_hash {
        return Err(ProcessError::Permanent(PermanentFailure {
            trace_id: Some(pointer.trace_id.clone()),
            staging_path: Some(pointer.staging_path.clone()),
            reason: "staged trace content hash does not match queue pointer".to_string(),
        }));
    }
    let trace: Trace = serde_json::from_slice(&bytes).map_err(|error| {
        ProcessError::Permanent(PermanentFailure {
            trace_id: Some(pointer.trace_id.clone()),
            staging_path: Some(pointer.staging_path.clone()),
            reason: format!("staged trace is invalid JSON: {error}"),
        })
    })?;
    if trace.trace_id != pointer.trace_id {
        return Err(ProcessError::Permanent(PermanentFailure {
            trace_id: Some(pointer.trace_id.clone()),
            staging_path: Some(pointer.staging_path.clone()),
            reason: "staged trace ID does not match queue pointer".to_string(),
        }));
    }
    trace.validate(bytes.len()).map_err(|error| {
        ProcessError::Permanent(PermanentFailure {
            trace_id: Some(pointer.trace_id.clone()),
            staging_path: Some(pointer.staging_path.clone()),
            reason: error.message,
        })
    })?;
    if let Err(error) = writer.commit(trace, pointer.content_hash.clone()).await {
        match error {
            AppError::Api(error) => {
                return Err(ProcessError::Permanent(PermanentFailure {
                    trace_id: Some(pointer.trace_id.clone()),
                    staging_path: Some(pointer.staging_path.clone()),
                    reason: format!("{}: {}", error.code, error.message),
                }));
            }
            AppError::Internal(error) => {
                return Err(ProcessError::Retryable(RetryableFailure {
                    trace_id: Some(pointer.trace_id.clone()),
                    error,
                }));
            }
        }
    }
    Ok(pointer.staging_path.clone())
}

async fn record_failure(
    storage: &Arc<dyn Storage>,
    failure: &PermanentFailure,
    attempt: u32,
    receipt_handle: &str,
) -> Result<()> {
    let failed_at = chrono::Utc::now();
    let receipt_hash = hex::encode(Sha256::digest(receipt_handle.as_bytes()));
    let path = format!(
        "failures/v1/{:016x}-{}.json",
        failed_at.timestamp_millis(),
        &receipt_hash[..16]
    );
    let body = serde_json::to_vec(&IngestionFailure {
        trace_id: failure.trace_id.clone(),
        staging_path: failure.staging_path.clone(),
        reason: failure.reason.clone(),
        failed_at,
        queue_attempt: attempt,
    })
    .into_alien_error()
    .context(ErrorData::SerializationFailed {
        operation: "encode ingestion failure".to_string(),
    })?;
    storage
        .put(&Path::from(path.as_str()), Bytes::from(body).into())
        .await
        .into_alien_error()
        .context(ErrorData::StorageOperationFailed {
            operation: "record ingestion failure".to_string(),
            path,
        })?;
    Ok(())
}
