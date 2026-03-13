//! Logs service - streams and queries logs from DeepStore via Agent Manager
//!
//! Uses the high-level deepstore-client crate for type-safe API calls.
//!
//! Architecture:
//! - Production: Agent Manager acts as auth proxy for DeepStore
//!   - Search/fetch: Agent Manager URL with Query JWT
//!   - SSE streaming: Control Plane URL with Query JWT (via query param)
//! - Local dev: Dev server implements compatible endpoints

use crate::tui::state::deployments::LogLine;
use chrono::{DateTime, Utc};
use deepstore_client::{DeepstoreClient, SearchParams};
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

/// Unique identifier for a log (to detect duplicates)
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct LogId {
    timestamp_nanos: i64,
    body: String,
    resource_id: String,
}

impl LogId {
    fn from_log_line(log: &LogLine) -> Self {
        Self {
            timestamp_nanos: log.timestamp.timestamp_nanos_opt().unwrap_or(0),
            body: log.content.clone(),
            resource_id: log.resource_id.clone(),
        }
    }
}

/// Internal state for log streaming/polling
struct LogState {
    /// New logs fetched since last poll
    pending_logs: VecDeque<LogLine>,
    /// Whether streaming is active
    is_streaming: bool,
    /// Last error message (for status display)
    last_error: Option<String>,
    /// Last timestamp we've seen (to avoid re-fetching)
    last_seen_timestamp: Option<DateTime<Utc>>,
    /// Recently seen log IDs (for deduplication within a sliding window)
    /// We keep a cache of recent log IDs to handle edge cases where logs
    /// have identical timestamps but arrive in different batches
    seen_log_ids: HashSet<LogId>,
}

impl Default for LogState {
    fn default() -> Self {
        Self {
            pending_logs: VecDeque::new(),
            is_streaming: false,
            last_error: None,
            last_seen_timestamp: None,
            seen_log_ids: HashSet::new(),
        }
    }
}

/// Service for streaming and querying logs
#[derive(Clone)]
pub struct LogsService {
    client: Arc<DeepstoreClient>,
    database_id: String,
    state: Arc<RwLock<LogState>>,
    /// Handle to streaming task (for cancellation)
    stream_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl LogsService {
    /// Create a new logs service using DeepstoreClient
    pub fn new(client: DeepstoreClient, database_id: String) -> Self {
        Self {
            client: Arc::new(client),
            database_id,
            state: Arc::new(RwLock::new(LogState::default())),
            stream_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Start streaming logs via SSE
    ///
    /// Connects to the control plane SSE endpoint and spawns a background task
    /// to receive draft notifications and fetch documents.
    pub async fn start_streaming(&self) -> Result<(), String> {
        // Check if already streaming
        {
            let state = self.state.read().await;
            if state.is_streaming {
                debug!("Log streaming already active");
                return Ok(());
            }
        }

        debug!("Starting log streaming via DeepStore client");

        // Mark as streaming before spawning task
        {
            let mut state = self.state.write().await;
            state.is_streaming = true;
            state.last_error = None;
        }

        // Fetch initial history
        let history_end = Utc::now();
        let history_start = history_end - chrono::Duration::minutes(5);

        match self
            .search_logs_internal(String::new(), history_start, history_end, Some(100))
            .await
        {
            Ok(logs) => {
                let count = logs.len();
                debug!(count, "Fetched initial log history");
                let mut state = self.state.write().await;

                // Add logs and track what we've seen
                for log in logs {
                    let log_id = LogId::from_log_line(&log);
                    if state.seen_log_ids.insert(log_id) {
                        // Update last seen timestamp
                        if state.last_seen_timestamp.is_none()
                            || log.timestamp > state.last_seen_timestamp.unwrap()
                        {
                            state.last_seen_timestamp = Some(log.timestamp);
                        }
                        state.pending_logs.push_back(log);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to fetch initial log history: {}", e);
                // Continue to streaming anyway
            }
        }

        // For now, streaming is implemented via polling instead of true SSE
        // This keeps the implementation simple while we validate the flow
        // TODO: Implement true SSE streaming using eventsource-client
        let service = self.clone();
        let handle = tokio::spawn(async move {
            debug!("Log streaming task started (polling mode)");

            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                interval.tick().await;

                // Poll for new logs, starting from last seen timestamp
                let end = Utc::now();
                let start = {
                    let state = service.state.read().await;
                    state
                        .last_seen_timestamp
                        .unwrap_or(end - chrono::Duration::seconds(10))
                };

                match service
                    .search_logs_internal(String::new(), start, end, Some(100))
                    .await
                {
                    Ok(logs) => {
                        if !logs.is_empty() {
                            let total_fetched = logs.len();
                            let mut state = service.state.write().await;
                            let mut new_count = 0;

                            // Deduplicate and add new logs
                            for log in logs {
                                let log_id = LogId::from_log_line(&log);
                                if state.seen_log_ids.insert(log_id) {
                                    // Update last seen timestamp
                                    if state.last_seen_timestamp.is_none()
                                        || log.timestamp > state.last_seen_timestamp.unwrap()
                                    {
                                        state.last_seen_timestamp = Some(log.timestamp);
                                    }
                                    state.pending_logs.push_back(log);
                                    new_count += 1;
                                }
                            }

                            if new_count > 0 {
                                debug!(new_count, total_fetched, "Added new logs (deduplicated)");
                            }

                            // Trim seen_log_ids cache to prevent unbounded growth
                            // Keep last 10,000 IDs (represents ~10-30 minutes of logs in typical cases)
                            const MAX_SEEN_IDS: usize = 10_000;
                            if state.seen_log_ids.len() > MAX_SEEN_IDS {
                                // Clear old entries (simple strategy: clear half when threshold reached)
                                // This is fine because we also track last_seen_timestamp
                                state.seen_log_ids.clear();
                                state.seen_log_ids.reserve(MAX_SEEN_IDS / 2);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Polling for logs failed: {}", e);
                        let mut state = service.state.write().await;
                        state.last_error = Some(e);
                    }
                }
            }
        });

        // Store handle
        {
            let mut handle_lock = self.stream_handle.write().await;
            *handle_lock = Some(handle);
        }

        Ok(())
    }

    /// Stop streaming logs
    pub async fn stop_streaming(&self) {
        // Abort streaming task
        {
            let mut handle_lock = self.stream_handle.write().await;
            if let Some(handle) = handle_lock.take() {
                handle.abort();
            }
        }

        // Update state
        {
            let mut state = self.state.write().await;
            state.is_streaming = false;
        }

        debug!("Stopped log streaming");
    }

    /// Poll for new logs
    ///
    /// Returns and removes logs from the internal buffer.
    /// Call this periodically in the TUI event loop.
    pub async fn poll_new_logs(&self) -> Vec<LogLine> {
        let mut state = self.state.write().await;
        state.pending_logs.drain(..).collect()
    }

    /// Check if streaming is active
    pub async fn is_streaming(&self) -> bool {
        self.state.read().await.is_streaming
    }

    /// Get last error message
    pub async fn last_error(&self) -> Option<String> {
        self.state.read().await.last_error.clone()
    }

    /// Search logs
    pub async fn search_logs(
        &self,
        query: String,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        max_hits: Option<usize>,
    ) -> Result<Vec<LogLine>, String> {
        self.search_logs_internal(query, start_time, end_time, max_hits)
            .await
    }

    /// Internal search implementation using DeepstoreClient
    async fn search_logs_internal(
        &self,
        query: String,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        max_hits: Option<usize>,
    ) -> Result<Vec<LogLine>, String> {
        let params = SearchParams {
            database_id: self.database_id.clone(),
            query: if query.is_empty() {
                "*".to_string()
            } else {
                query
            },
            start_time,
            end_time,
            max_hits,
            sort_by: Some("timestamp_nanos".to_string()), // Ascending (oldest first) for chronological order
            ..Default::default()
        };

        debug!(
            database_id = %params.database_id,
            query = %params.query,
            start = %start_time,
            end = %end_time,
            "Searching logs via DeepstoreClient"
        );

        let result = self
            .client
            .search(params)
            .await
            .map_err(|e| format!("Search failed: {}", e))?;

        debug!(
            num_hits = result.num_hits,
            hits_count = result.hits.len(),
            "Search completed successfully"
        );

        let logs = result
            .hits
            .into_iter()
            .filter_map(|hit| self.document_to_log_line(hit))
            .collect();

        Ok(logs)
    }

    /// Convert DeepStore document to LogLine
    fn document_to_log_line(&self, doc: serde_json::Value) -> Option<LogLine> {
        // Get deployment_id from resource_attributes or _scope
        let deployment_id = doc
            .get("resource_attributes")
            .and_then(|ra| ra.get("alien.agent_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                // Fallback: extract from _scope
                // Format can be:
                //  - Platform: "ws_xxx/prj_xxx/ag_xxx" (3 parts)
                //  - Dev (old): "ws_xxx/prj_xxx" (2 parts, no deployment)
                doc.get("_scope")
                    .and_then(|v| v.as_str())
                    .and_then(|scope| {
                        let parts: Vec<&str> = scope.split('/').collect();
                        if parts.len() >= 3 {
                            // ws/prj/deployment format
                            Some(parts[2].to_string())
                        } else {
                            // Old format without deployment, return empty string (will be filled by active deployment)
                            Some(String::new())
                        }
                    })
            })
            .or_else(|| {
                // For dev server: scope field is directly available
                doc.get("scope").and_then(|v| v.as_str()).and_then(|scope| {
                    let parts: Vec<&str> = scope.split('/').collect();
                    if parts.len() >= 3 {
                        // ws/prj/deployment format
                        Some(parts[2].to_string())
                    } else {
                        // Old format without deployment, return empty string (will be filled by active deployment)
                        Some(String::new())
                    }
                })
            })
            .unwrap_or_else(|| String::new()); // If no scope at all, use empty string

        let resource_id = doc
            .get("resource_id")
            .and_then(|v| v.as_str())
            .or_else(|| {
                doc.get("resource_attributes")
                    .and_then(|ra| ra.get("service.name"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("unknown")
            .to_string();

        let content = doc
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let level = doc
            .get("level")
            .and_then(|v| v.as_str())
            .or_else(|| doc.get("severity_text").and_then(|v| v.as_str()))
            .map(|s| s.to_string());

        let timestamp_nanos = doc.get("timestamp_nanos").and_then(|v| v.as_i64())?;

        let timestamp = DateTime::from_timestamp_nanos(timestamp_nanos);

        let mut log = LogLine::new(deployment_id, resource_id, content).with_timestamp(timestamp);
        if let Some(lvl) = level {
            log = log.with_level(lvl);
        }
        Some(log)
    }
}
