use crate::error::ApiError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_TRACE_BYTES: usize = 100 * 1024;
pub const DEFAULT_PAGE_SIZE: usize = 50;
pub const MAX_PAGE_SIZE: usize = 100;

/// A durable AI trace accepted by the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
    /// Stable, caller-assigned identifier used for idempotency.
    pub trace_id: String,
    /// Agent or engine that produced the trace.
    pub agent: String,
    /// Final or current trace status.
    pub status: String,
    /// Model used by the agent.
    pub model: String,
    /// Time at which execution began.
    pub started_at: DateTime<Utc>,
    /// Time at which execution ended, when known.
    pub finished_at: Option<DateTime<Utc>>,
    /// Application-specific trace content.
    pub payload: Value,
}

impl Trace {
    pub fn validate(&self, encoded_len: usize) -> std::result::Result<(), ApiError> {
        for (field, value) in [
            ("traceId", self.trace_id.as_str()),
            ("agent", self.agent.as_str()),
            ("status", self.status.as_str()),
            ("model", self.model.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ApiError::invalid(format!("{field} must not be empty")));
            }
            if value.len() > 512 {
                return Err(ApiError::invalid(format!(
                    "{field} must not exceed 512 bytes"
                )));
            }
        }
        if encoded_len > MAX_TRACE_BYTES {
            return Err(ApiError::invalid(format!(
                "encoded trace must not exceed {MAX_TRACE_BYTES} bytes"
            )));
        }
        if self.started_at.timestamp_millis() < 0 {
            return Err(ApiError::invalid(
                "startedAt must be on or after 1970-01-01",
            ));
        }
        if self
            .finished_at
            .is_some_and(|finished| finished < self.started_at)
        {
            return Err(ApiError::invalid("finishedAt must not precede startedAt"));
        }
        Ok(())
    }
}

/// Trace plus database-managed commit metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredTrace {
    #[serde(flatten)]
    pub trace: Trace,
    pub content_hash: String,
    pub committed_at: DateTime<Utc>,
}

/// Response returned after an ingestion request becomes durable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedTrace {
    pub trace_id: String,
    pub content_hash: String,
    pub status: &'static str,
}

/// Filters and pagination accepted by the list endpoint.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceQuery {
    pub agent: Option<String>,
    pub status: Option<String>,
    pub model: Option<String>,
    pub started_after: Option<DateTime<Utc>>,
    pub started_before: Option<DateTime<Utc>>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

impl TraceQuery {
    pub fn page_size(&self) -> std::result::Result<usize, ApiError> {
        let limit = self.limit.unwrap_or(DEFAULT_PAGE_SIZE);
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(ApiError::invalid(format!(
                "limit must be between 1 and {MAX_PAGE_SIZE}"
            )));
        }
        if self
            .started_after
            .zip(self.started_before)
            .is_some_and(|(after, before)| after > before)
        {
            return Err(ApiError::invalid(
                "startedAfter must not follow startedBefore",
            ));
        }
        Ok(limit)
    }
}

/// A page of committed traces.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TracePage {
    pub traces: Vec<StoredTrace>,
    pub next_cursor: Option<String>,
}

/// Small queue message pointing at a staged trace object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestionPointer {
    pub trace_id: String,
    pub content_hash: String,
    pub staging_path: String,
}

/// Metadata retained when a queued trace cannot be committed.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestionFailure {
    pub trace_id: Option<String>,
    pub staging_path: Option<String>,
    pub reason: String,
    pub failed_at: DateTime<Utc>,
    pub queue_attempt: u32,
}
