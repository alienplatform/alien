use crate::{
    error::{ApiError, AppResult, ErrorData, Result},
    keys::{decode_cursor, index_keys, primary_key, query_prefix, timestamp_from_index_key},
    models::{StoredTrace, Trace, TracePage, TraceQuery},
};
use alien_error::{Context, IntoAlienError};
use object_store::ObjectStore;
use slatedb::{
    config::{DbReaderOptions, WriteOptions},
    db_cache::moka::{MokaCache, MokaCacheOptions},
    Db, DbReader, IsolationLevel,
};
use std::{sync::Arc, time::Duration};

const DATABASE_PATH: &str = "db/v1";
const CACHE_CAPACITY_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, PartialEq)]
pub enum CommitResult {
    Inserted,
    AlreadyExists,
}

pub struct TraceWriter {
    db: Db,
}

impl TraceWriter {
    pub async fn open(object_store: Arc<dyn ObjectStore>) -> Result<Self> {
        let db = Db::builder(DATABASE_PATH, object_store)
            .with_db_cache(Arc::new(MokaCache::new_with_opts(MokaCacheOptions {
                max_capacity: CACHE_CAPACITY_BYTES,
                time_to_live: None,
                time_to_idle: None,
            })))
            .build()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "open writer".to_string(),
            })?;
        Ok(Self { db })
    }

    pub async fn commit(&self, trace: Trace, content_hash: String) -> AppResult<CommitResult> {
        let key = primary_key(&trace.trace_id);
        let transaction = self
            .db
            .begin(IsolationLevel::SerializableSnapshot)
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "begin trace transaction".to_string(),
            })?;

        if let Some(existing) = transaction
            .get(key.as_bytes())
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "check trace idempotency".to_string(),
            })?
        {
            let existing: StoredTrace = serde_json::from_slice(&existing)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    operation: "decode existing trace".to_string(),
                })?;
            if existing.content_hash == content_hash {
                return Ok(CommitResult::AlreadyExists);
            }
            return Err(ApiError::conflict(&trace.trace_id).into());
        }

        let stored = StoredTrace {
            trace: trace.clone(),
            content_hash,
            committed_at: chrono::Utc::now(),
        };
        let value = serde_json::to_vec(&stored).into_alien_error().context(
            ErrorData::SerializationFailed {
                operation: "encode committed trace".to_string(),
            },
        )?;

        transaction
            .put(key.as_bytes(), &value)
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "buffer primary trace".to_string(),
            })?;
        for index_key in index_keys(&trace) {
            transaction
                .put(index_key.as_bytes(), key.as_bytes())
                .into_alien_error()
                .context(ErrorData::DatabaseOperationFailed {
                    operation: "buffer trace index".to_string(),
                })?;
        }
        transaction
            .commit_with_options(&WriteOptions {
                await_durable: true,
                ..WriteOptions::default()
            })
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "durably commit trace".to_string(),
            })?;
        Ok(CommitResult::Inserted)
    }
}

pub struct TraceReader {
    db: DbReader,
}

impl TraceReader {
    pub async fn open(object_store: Arc<dyn ObjectStore>) -> Result<Self> {
        let db = DbReader::builder(DATABASE_PATH, object_store)
            .with_options(DbReaderOptions {
                manifest_poll_interval: Duration::from_secs(1),
                ..DbReaderOptions::default()
            })
            .with_db_cache(Arc::new(MokaCache::new_with_opts(MokaCacheOptions {
                max_capacity: CACHE_CAPACITY_BYTES,
                time_to_live: None,
                time_to_idle: None,
            })))
            .build()
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "open reader".to_string(),
            })?;
        Ok(Self { db })
    }

    pub async fn get(&self, trace_id: &str) -> AppResult<StoredTrace> {
        let bytes = self
            .db
            .get(primary_key(trace_id))
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "read trace".to_string(),
            })?
            .ok_or_else(|| ApiError::not_found(trace_id))?;
        Ok(decode_stored_trace(&bytes)?)
    }

    pub async fn list(&self, query: &TraceQuery) -> AppResult<TracePage> {
        let limit = query.page_size()?;
        let prefix = query_prefix(query);
        let cursor = decode_cursor(query.cursor.as_deref())?;
        if cursor
            .as_deref()
            .is_some_and(|cursor| !cursor.starts_with(prefix.as_bytes()))
        {
            return Err(ApiError::invalid_cursor().into());
        }

        let mut iterator = self
            .db
            .scan_prefix(prefix.as_bytes())
            .await
            .into_alien_error()
            .context(ErrorData::DatabaseOperationFailed {
                operation: "scan trace index".to_string(),
            })?;
        let mut traces = Vec::with_capacity(limit);
        let mut last_returned_key = None;
        let mut has_more = false;

        while let Some(entry) = iterator.next().await.into_alien_error().context(
            ErrorData::DatabaseOperationFailed {
                operation: "read trace index page".to_string(),
            },
        )? {
            if cursor
                .as_deref()
                .is_some_and(|cursor| entry.key.as_ref() <= cursor)
            {
                continue;
            }
            let timestamp = timestamp_from_index_key(&entry.key, &prefix)?;
            if query
                .started_after
                .is_some_and(|after| timestamp < after.timestamp_millis())
            {
                continue;
            }
            if query
                .started_before
                .is_some_and(|before| timestamp > before.timestamp_millis())
            {
                break;
            }
            if traces.len() == limit {
                has_more = true;
                break;
            }
            let primary = self
                .db
                .get(&entry.value)
                .await
                .into_alien_error()
                .context(ErrorData::DatabaseOperationFailed {
                    operation: "read indexed trace".to_string(),
                })?
                .ok_or_else(|| {
                    crate::error::Error::new(ErrorData::DatabaseOperationFailed {
                        operation: "resolve dangling trace index".to_string(),
                    })
                })?;
            traces.push(decode_stored_trace(&primary)?);
            last_returned_key = Some(entry.key);
        }

        Ok(TracePage {
            traces,
            next_cursor: has_more.then(|| hex::encode(last_returned_key.unwrap_or_default())),
        })
    }
}

fn decode_stored_trace(bytes: &[u8]) -> Result<StoredTrace> {
    serde_json::from_slice(bytes)
        .into_alien_error()
        .context(ErrorData::SerializationFailed {
            operation: "decode committed trace".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use chrono::{TimeZone, Utc};
    use object_store::memory::InMemory;
    use serde_json::json;

    fn trace(id: &str, agent: &str, status: &str, model: &str, timestamp: i64) -> Trace {
        Trace {
            trace_id: id.to_string(),
            agent: agent.to_string(),
            status: status.to_string(),
            model: model.to_string(),
            started_at: Utc.timestamp_millis_opt(timestamp).unwrap(),
            finished_at: None,
            payload: json!({ "id": id }),
        }
    }

    #[tokio::test]
    async fn commit_is_idempotent_and_rejects_different_content() {
        let storage: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = TraceWriter::open(storage)
            .await
            .expect("writer should open");
        let original = trace("trace-1", "agent-a", "ok", "model-a", 1_000);

        let inserted = writer
            .commit(original.clone(), "hash-a".to_string())
            .await
            .expect("first commit should succeed");
        let duplicate = writer
            .commit(original, "hash-a".to_string())
            .await
            .expect("identical commit should succeed");
        let conflict = writer
            .commit(
                trace("trace-1", "agent-a", "failed", "model-a", 1_000),
                "hash-b".to_string(),
            )
            .await
            .expect_err("different content should conflict");

        assert_eq!(inserted, CommitResult::Inserted);
        assert_eq!(duplicate, CommitResult::AlreadyExists);
        assert!(matches!(
            conflict,
            AppError::Api(ApiError {
                code: "TRACE_CONFLICT",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn every_filter_combination_returns_only_matching_traces() {
        let storage: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = TraceWriter::open(Arc::clone(&storage))
            .await
            .expect("writer should open");
        for (trace, hash) in [
            (trace("one", "a", "ok", "m1", 1_000), "h1"),
            (trace("two", "a", "failed", "m2", 2_000), "h2"),
            (trace("three", "b", "ok", "m2", 3_000), "h3"),
        ] {
            writer
                .commit(trace, hash.to_string())
                .await
                .expect("trace should commit");
        }
        writer.db.flush().await.expect("writer should flush");
        let reader = TraceReader::open(storage)
            .await
            .expect("reader should open");

        for (agent, status, model, expected) in [
            (None, None, None, 3),
            (Some("a"), None, None, 2),
            (None, Some("ok"), None, 2),
            (None, None, Some("m2"), 2),
            (Some("a"), Some("ok"), None, 1),
            (Some("a"), None, Some("m2"), 1),
            (None, Some("ok"), Some("m2"), 1),
            (Some("a"), Some("failed"), Some("m2"), 1),
        ] {
            let page = reader
                .list(&TraceQuery {
                    agent: agent.map(str::to_string),
                    status: status.map(str::to_string),
                    model: model.map(str::to_string),
                    ..TraceQuery::default()
                })
                .await
                .expect("filtered query should succeed");
            assert_eq!(page.traces.len(), expected);
        }
    }

    #[tokio::test]
    async fn cursor_pages_without_duplicates() {
        let storage: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let writer = TraceWriter::open(Arc::clone(&storage))
            .await
            .expect("writer should open");
        for index in 0..3 {
            writer
                .commit(
                    trace(&format!("trace-{index}"), "a", "ok", "m", index),
                    format!("hash-{index}"),
                )
                .await
                .expect("trace should commit");
        }
        writer.db.flush().await.expect("writer should flush");
        let reader = TraceReader::open(storage)
            .await
            .expect("reader should open");
        let first = reader
            .list(&TraceQuery {
                limit: Some(2),
                ..TraceQuery::default()
            })
            .await
            .expect("first page should load");
        let second = reader
            .list(&TraceQuery {
                limit: Some(2),
                cursor: first.next_cursor.clone(),
                ..TraceQuery::default()
            })
            .await
            .expect("second page should load");

        assert_eq!(first.traces.len(), 2);
        assert_eq!(second.traces.len(), 1);
        assert_ne!(
            first.traces[1].trace.trace_id,
            second.traces[0].trace.trace_id
        );
    }
}
