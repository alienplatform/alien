//! Local Trigger Service — platform-level event delivery for local deployments.
//!
//! Mirrors what cloud platforms do natively:
//! - AWS: SQS Event Source Mapping → Lambda
//! - GCP: Pub/Sub push subscription → Cloud Run
//! - Azure: Service Bus → Container Apps
//!
//! For the local platform, this service runs independently of the function and delivers
//! events via the runtime's `ControlGrpcServer::send_task()`. This ensures:
//! - At-least-once delivery for queue messages (ack only on handler success)
//! - Filesystem-level storage event watching via `notify` (no polling)
//! - Persistent cron state for catch-up after restarts
//!
//! ## Cron Expression Format
//!
//! Users write standard 5-field unix cron in `alien.ts` (e.g., `"0 * * * *"`).
//! The Rust `cron` crate expects 6 fields (seconds first). This service converts
//! by prepending `"0 "` (seconds = 0) before parsing.
//!
//! See docs/02-manager/10-deployment-protocol.md for the full protocol.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::grpc::control::{
    self, CronEvent as ProtoCronEvent, QueueMessage as ProtoQueueMessage,
    StorageEvent as ProtoStorageEvent, Task,
};
use alien_bindings::grpc::control_service::ControlGrpcServer;
use alien_bindings::traits::{BindingsProviderApi, MessagePayload, Queue};
use alien_core::FunctionTrigger;
use alien_error::{AlienError, Context, IntoAlienError};
use chrono::Utc;
use prost_types::Timestamp;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::error::ErrorData;
use crate::LocalBindingsProvider;

type Result<T> = crate::error::Result<T>;

/// Platform-level trigger service for local deployments.
///
/// Runs alongside the function process, managed by the infra controller.
/// Delivers events to the function via `control_server.send_task()`.
pub struct LocalTriggerService {
    triggers: Vec<FunctionTrigger>,
    bindings_provider: Arc<LocalBindingsProvider>,
    state_dir: PathBuf,
    shutdown_rx: broadcast::Receiver<()>,
}

impl LocalTriggerService {
    pub fn new(
        triggers: Vec<FunctionTrigger>,
        bindings_provider: Arc<LocalBindingsProvider>,
        state_dir: PathBuf,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            triggers,
            bindings_provider,
            state_dir,
            shutdown_rx,
        }
    }

    /// Run the trigger service. Spawns one task per trigger.
    /// Blocks until shutdown signal.
    pub async fn run(mut self) -> Result<()> {
        if self.triggers.is_empty() {
            return Ok(());
        }

        let control_server = wait_for_control_server().await?;

        info!(
            trigger_count = self.triggers.len(),
            "Starting local trigger service"
        );

        let mut handles: Vec<JoinHandle<()>> = Vec::new();

        for trigger in &self.triggers {
            match trigger {
                FunctionTrigger::Queue { queue } => {
                    let binding_name = queue.id.clone();
                    let provider = self.bindings_provider.clone();
                    let cs = control_server.clone();
                    let mut shutdown = self.shutdown_rx.resubscribe();

                    info!(queue = %binding_name, "Starting queue trigger poller");
                    handles.push(tokio::spawn(async move {
                        if let Err(e) =
                            poll_queue(&binding_name, &provider, &cs, &mut shutdown).await
                        {
                            error!(queue = %binding_name, error = %e, "Queue poller error");
                        }
                    }));
                }
                FunctionTrigger::Storage { storage, events } => {
                    let binding_name = storage.id.clone();
                    let event_types = events.clone();
                    let storage_path = self
                        .bindings_provider
                        .storage_manager()
                        .get_storage_path(&binding_name)
                        .context(ErrorData::TriggerServiceError {
                            trigger_type: "storage".to_string(),
                            trigger_id: binding_name.clone(),
                            reason: "Storage resource not found".to_string(),
                        })?;
                    let cs = control_server.clone();
                    let mut shutdown = self.shutdown_rx.resubscribe();

                    info!(storage = %binding_name, events = ?event_types, "Starting storage trigger watcher");
                    handles.push(tokio::spawn(async move {
                        if let Err(e) = watch_storage(
                            &binding_name,
                            &storage_path,
                            &event_types,
                            &cs,
                            &mut shutdown,
                        )
                        .await
                        {
                            error!(storage = %binding_name, error = %e, "Storage watcher error");
                        }
                    }));
                }
                FunctionTrigger::Schedule { cron } => {
                    let cron_expr = cron.clone();
                    let cs = control_server.clone();
                    let cron_state_dir = self.state_dir.clone();
                    let mut shutdown = self.shutdown_rx.resubscribe();

                    info!(cron = %cron_expr, "Starting cron trigger scheduler");
                    handles.push(tokio::spawn(async move {
                        if let Err(e) =
                            run_cron_scheduler(&cron_expr, &cron_state_dir, &cs, &mut shutdown)
                                .await
                        {
                            error!(cron = %cron_expr, error = %e, "Cron scheduler error");
                        }
                    }));
                }
            }
        }

        // Wait for shutdown signal
        self.shutdown_rx.recv().await.ok();

        info!("Shutting down local trigger service");
        for handle in handles {
            handle.abort();
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Wait for runtime to expose ControlGrpcServer
// ---------------------------------------------------------------------------

/// Wait for the runtime's control server AND for the application to register
/// its event handlers. The trigger service must not deliver events before the
/// application is ready — otherwise tasks arrive with "No handler found."
async fn wait_for_control_server() -> Result<Arc<ControlGrpcServer>> {
    // Wait for control server to exist
    let cs = {
        let mut cs_opt = None;
        for _ in 0..60 {
            if let Some(cs) = alien_runtime::get_control_server() {
                cs_opt = Some(cs);
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        cs_opt.ok_or_else(|| {
            AlienError::new(ErrorData::TriggerServiceError {
                trigger_type: "all".to_string(),
                trigger_id: String::new(),
                reason: "Timeout waiting for runtime ControlGrpcServer (30s)".to_string(),
            })
        })?
    };

    // Wait for application to register at least one event handler
    for _ in 0..60 {
        if cs.has_registered_handlers().await {
            return Ok(cs);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Proceed anyway — the app might not have handlers (just triggers in config)
    warn!(
        "Application has not registered event handlers after 30s, starting trigger polling anyway"
    );
    Ok(cs)
}

// ---------------------------------------------------------------------------
// Cron expression conversion
// ---------------------------------------------------------------------------

/// Convert a standard 5-field unix cron expression to the 6-field format
/// expected by the Rust `cron` crate (which adds seconds as the first field).
///
/// Users write `"0 * * * *"` (MIN HOUR DOM MON DOW) in alien.ts.
/// The `cron` crate expects `"0 0 * * * *"` (SEC MIN HOUR DOM MON DOW).
///
/// Cloud platforms handle this conversion in their own infra controllers:
/// - AWS CloudWatch: `cron(MIN HOUR DOM MON DOW YEAR)`
/// - GCP Cloud Scheduler: standard 5-field unix cron (no conversion needed)
fn to_cron_crate_format(user_cron: &str) -> String {
    let fields: Vec<&str> = user_cron.trim().split_whitespace().collect();
    if fields.len() == 5 {
        // Standard 5-field → prepend seconds=0
        format!("0 {}", user_cron)
    } else {
        // Already 6+ fields (user specified seconds), pass through
        user_cron.to_string()
    }
}

// ---------------------------------------------------------------------------
// Queue Polling
// ---------------------------------------------------------------------------

/// Poll a queue for messages and deliver to the function via send_task.
/// Acks only on successful handler completion (at-least-once delivery).
///
/// Uses the shared `LocalBindingsProvider` to access the queue — the same
/// provider the function runtime uses. This avoids sled lock contention
/// (sled only allows one open handle per database directory).
async fn poll_queue(
    binding_name: &str,
    provider: &LocalBindingsProvider,
    control_server: &ControlGrpcServer,
    shutdown: &mut broadcast::Receiver<()>,
) -> Result<()> {
    let queue =
        provider
            .load_queue(binding_name)
            .await
            .context(ErrorData::TriggerServiceError {
                trigger_type: "queue".to_string(),
                trigger_id: binding_name.to_string(),
                reason: "Failed to load queue binding".to_string(),
            })?;

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!(queue = %binding_name, "Queue poller shutting down");
                return Ok(());
            }
            result = poll_queue_once(binding_name, &*queue, control_server) => {
                match result {
                    Ok(0) => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    Ok(_) => {
                        // Got messages — immediately poll again
                    }
                    Err(e) => {
                        warn!(queue = %binding_name, error = %e, "Queue poll error, retrying in 2s");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        }
    }
}

async fn poll_queue_once(
    binding_name: &str,
    queue: &dyn Queue,
    control_server: &ControlGrpcServer,
) -> Result<usize> {
    let messages = queue
        .receive(binding_name, 10)
        .await
        .into_alien_error()
        .context(ErrorData::TriggerServiceError {
            trigger_type: "queue".to_string(),
            trigger_id: binding_name.to_string(),
            reason: "Failed to receive messages".to_string(),
        })?;
    let count = messages.len();

    for msg in messages {
        let payload_bytes = match &msg.payload {
            MessagePayload::Json(v) => v.to_string().into_bytes(),
            MessagePayload::Text(s) => s.clone().into_bytes(),
        };

        let task_id = uuid::Uuid::new_v4().to_string();
        let task = Task {
            task_id: task_id.clone(),
            payload: Some(control::task::Payload::QueueMessage(ProtoQueueMessage {
                id: task_id,
                source: binding_name.to_string(),
                payload: payload_bytes,
                receipt_handle: msg.receipt_handle.clone(),
                attempt_count: 1,
                timestamp: Some(now_timestamp()),
                attributes: std::collections::HashMap::new(),
            })),
        };

        match control_server
            .send_task(task, Duration::from_secs(300))
            .await
        {
            Ok(result) if result.success => {
                if let Err(e) = queue.ack(binding_name, &msg.receipt_handle).await {
                    warn!(queue = %binding_name, error = %e, "Failed to ack after successful handler");
                }
                debug!(queue = %binding_name, "Queue message processed and acked");
            }
            Ok(result) => {
                warn!(
                    queue = %binding_name,
                    error_code = ?result.error_code,
                    error_message = ?result.error_message,
                    "Queue handler returned error, message will be retried"
                );
            }
            Err(e) => {
                warn!(
                    queue = %binding_name,
                    error = %e,
                    "Failed to deliver queue message, will be retried"
                );
            }
        }
    }

    Ok(count)
}

// ---------------------------------------------------------------------------
// Storage Watching (filesystem events via notify)
// ---------------------------------------------------------------------------

/// Watch a storage directory for filesystem changes and deliver events.
async fn watch_storage(
    binding_name: &str,
    storage_path: &Path,
    event_types: &[String],
    control_server: &ControlGrpcServer,
    shutdown: &mut broadcast::Receiver<()>,
) -> Result<()> {
    use notify::{Event, EventKind, RecursiveMode, Watcher};

    let (fs_tx, mut fs_rx) = tokio::sync::mpsc::channel::<Event>(256);

    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        if let Ok(event) = result {
            let _ = fs_tx.blocking_send(event);
        }
    })
    .into_alien_error()
    .context(ErrorData::TriggerServiceError {
        trigger_type: "storage".to_string(),
        trigger_id: binding_name.to_string(),
        reason: "Failed to create filesystem watcher".to_string(),
    })?;

    std::fs::create_dir_all(storage_path)
        .into_alien_error()
        .context(ErrorData::LocalDirectoryError {
            path: storage_path.display().to_string(),
            operation: "create".to_string(),
            reason: "Failed to create storage directory for watching".to_string(),
        })?;

    watcher
        .watch(storage_path, RecursiveMode::Recursive)
        .into_alien_error()
        .context(ErrorData::TriggerServiceError {
            trigger_type: "storage".to_string(),
            trigger_id: binding_name.to_string(),
            reason: format!("Failed to watch directory: {}", storage_path.display()),
        })?;

    info!(
        storage = %binding_name,
        path = %storage_path.display(),
        "Watching storage directory for changes"
    );

    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!(storage = %binding_name, "Storage watcher shutting down");
                return Ok(());
            }
            Some(event) = fs_rx.recv() => {
                let event_type = match event.kind {
                    EventKind::Create(_) => "created",
                    EventKind::Remove(_) => "deleted",
                    _ => continue,
                };

                if !event_types.iter().any(|e| e == event_type) {
                    continue;
                }

                for path in &event.paths {
                    if path.is_dir() {
                        continue;
                    }

                    let relative = path
                        .strip_prefix(storage_path)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();

                    let size = if event_type == "created" {
                        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
                    } else {
                        0
                    };

                    debug!(
                        storage = %binding_name,
                        key = %relative,
                        event_type = %event_type,
                        "Storage event detected"
                    );

                    let task = Task {
                        task_id: uuid::Uuid::new_v4().to_string(),
                        payload: Some(control::task::Payload::StorageEvent(ProtoStorageEvent {
                            bucket: binding_name.to_string(),
                            key: relative,
                            event_type: event_type.to_string(),
                            size,
                            timestamp: Some(now_timestamp()),
                            content_type: String::new(),
                            etag: String::new(),
                            region: String::new(),
                            version_id: String::new(),
                            current_tier: String::new(),
                            metadata: std::collections::HashMap::new(),
                        })),
                    };

                    if let Err(e) = control_server
                        .send_task(task, Duration::from_secs(30))
                        .await
                    {
                        warn!(
                            storage = %binding_name,
                            error = %e,
                            "Failed to deliver storage event to handler"
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cron Scheduler
// ---------------------------------------------------------------------------

/// Run a cron scheduler with persistent last-fire tracking.
/// On restart, fires one catch-up event if a run was missed.
async fn run_cron_scheduler(
    cron_expression: &str,
    state_dir: &Path,
    control_server: &ControlGrpcServer,
    shutdown: &mut broadcast::Receiver<()>,
) -> Result<()> {
    use cron::Schedule;
    use std::str::FromStr;

    // Convert standard 5-field cron to 6-field format (seconds first)
    let crate_format = to_cron_crate_format(cron_expression);

    let schedule = Schedule::from_str(&crate_format).map_err(|e| {
        AlienError::new(ErrorData::InvalidCronExpression {
            expression: cron_expression.to_string(),
            reason: e.to_string(),
        })
    })?;

    let schedule_hash = format!("{:x}", simple_hash(cron_expression));
    let state_file = state_dir
        .join("cron")
        .join(&schedule_hash)
        .join("last_fired.json");

    // Check for missed run on startup
    if let Some(last_fired) = load_last_fired(&state_file) {
        if let Some(missed) = schedule.after(&last_fired).next() {
            if missed < Utc::now() {
                info!(
                    cron = %cron_expression,
                    missed = %missed,
                    "Firing catch-up cron event (missed during downtime)"
                );
                fire_cron_event(cron_expression, missed, control_server).await;
                save_last_fired(&state_file, &Utc::now());
            }
        }
    }

    // Normal schedule loop
    loop {
        let next = match schedule.upcoming(Utc).next() {
            Some(t) => t,
            None => {
                info!(cron = %cron_expression, "No more upcoming cron events");
                return Ok(());
            }
        };

        let delay = (next - Utc::now()).to_std().unwrap_or(Duration::ZERO);

        debug!(
            cron = %cron_expression,
            next = %next,
            delay_secs = delay.as_secs(),
            "Waiting for next cron event"
        );

        tokio::select! {
            _ = shutdown.recv() => {
                info!(cron = %cron_expression, "Cron scheduler shutting down");
                return Ok(());
            }
            _ = tokio::time::sleep(delay) => {
                info!(cron = %cron_expression, time = %next, "Firing cron event");
                fire_cron_event(cron_expression, next, control_server).await;
                save_last_fired(&state_file, &next);
            }
        }
    }
}

async fn fire_cron_event(
    schedule_name: &str,
    scheduled_time: chrono::DateTime<Utc>,
    control_server: &ControlGrpcServer,
) {
    let task = Task {
        task_id: uuid::Uuid::new_v4().to_string(),
        payload: Some(control::task::Payload::CronEvent(ProtoCronEvent {
            schedule_name: schedule_name.to_string(),
            scheduled_time: Some(Timestamp {
                seconds: scheduled_time.timestamp(),
                nanos: scheduled_time.timestamp_subsec_nanos() as i32,
            }),
        })),
    };

    if let Err(e) = control_server
        .send_task(task, Duration::from_secs(60))
        .await
    {
        warn!(
            schedule = %schedule_name,
            error = %e,
            "Failed to deliver cron event to handler"
        );
    }
}

// ---------------------------------------------------------------------------
// Cron persistence helpers
// ---------------------------------------------------------------------------

fn load_last_fired(state_file: &Path) -> Option<chrono::DateTime<Utc>> {
    let content = std::fs::read_to_string(state_file).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_last_fired(state_file: &Path, time: &chrono::DateTime<Utc>) {
    if let Some(parent) = state_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string(time).unwrap_or_default();
    let _ = std::fs::write(state_file, json);
}

fn simple_hash(input: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

fn now_timestamp() -> Timestamp {
    let now = Utc::now();
    Timestamp {
        seconds: now.timestamp(),
        nanos: now.timestamp_subsec_nanos() as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_field_cron_gets_seconds_prepended() {
        assert_eq!(to_cron_crate_format("0 * * * *"), "0 0 * * * *");
        assert_eq!(to_cron_crate_format("*/5 * * * *"), "0 */5 * * * *");
        assert_eq!(
            to_cron_crate_format("30 2 * * MON-FRI"),
            "0 30 2 * * MON-FRI"
        );
    }

    #[test]
    fn six_field_cron_passes_through() {
        assert_eq!(to_cron_crate_format("0 0 * * * *"), "0 0 * * * *");
        assert_eq!(to_cron_crate_format("*/10 * * * * *"), "*/10 * * * * *");
    }

    #[test]
    fn converted_cron_parses_successfully() {
        use cron::Schedule;
        use std::str::FromStr;

        let user_expressions = vec![
            "0 * * * *",    // every hour
            "*/5 * * * *",  // every 5 minutes
            "0 0 * * *",    // daily at midnight
            "0 12 * * MON", // Monday at noon
        ];

        for expr in user_expressions {
            let converted = to_cron_crate_format(expr);
            let result = Schedule::from_str(&converted);
            assert!(
                result.is_ok(),
                "Failed to parse '{}' → '{}': {:?}",
                expr,
                converted,
                result.err()
            );
        }
    }
}
