//! Local Trigger Service — platform-level event delivery for local deployments.
//!
//! Mirrors what cloud platforms do natively:
//! - AWS: SQS Event Source Mapping → Lambda
//! - GCP: Pub/Sub push subscription → Cloud Run
//! - Azure: Service Bus → Container Apps
//!
//! For the local platform, this service runs independently of the worker and delivers
//! events via the runtime's `ControlGrpcServer::send_task()`. This ensures:
//! - At-least-once delivery for queue messages (ack only on handler success)
//! - Filesystem notifications plus reconciliation of committed storage objects
//! - Persistent cron state for catch-up after restarts
//!
//! ## Cron Expression Format
//!
//! Users write standard 5-field unix cron in `alien.ts` (e.g., `"0 * * * *"`).
//! The Rust `cron` crate expects 6 fields (seconds first). This service converts
//! by prepending `"0 "` (seconds = 0) before parsing.
//!
//! See docs/02-manager/10-deployment-protocol.md for the full protocol.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::traits::{BindingsProviderApi, MessagePayload, Queue};
use alien_core::WorkerTrigger;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_worker_protocol::control::{
    self, CronEvent as ProtoCronEvent, QueueMessage as ProtoQueueMessage,
    StorageEvent as ProtoStorageEvent, Task,
};
use alien_worker_protocol::ControlGrpcServer;
use alien_worker_runtime::transports::queue_batch::{QueueCollector, QueueCollectors};
use chrono::Utc;
use prost_types::Timestamp;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::error::ErrorData;
use crate::LocalBindingsProvider;

type Result<T> = crate::error::Result<T>;

/// Platform-level trigger service for local deployments.
///
/// Runs alongside the worker process, managed by the infra controller.
/// Delivers events to the worker via `control_server.send_task()`.
pub struct LocalTriggerService {
    timeout: Duration,
    triggers: Vec<WorkerTrigger>,
    bindings_provider: Arc<LocalBindingsProvider>,
    state_dir: PathBuf,
    control_server: Arc<ControlGrpcServer>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl LocalTriggerService {
    pub fn new(
        triggers: Vec<WorkerTrigger>,
        bindings_provider: Arc<LocalBindingsProvider>,
        state_dir: PathBuf,
        control_server: Arc<ControlGrpcServer>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            timeout: Duration::from_secs(180),
            triggers,
            bindings_provider,
            state_dir,
            control_server,
            shutdown_rx,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Run the trigger service. Spawns one task per trigger.
    /// Blocks until shutdown signal.
    pub async fn run(mut self) -> Result<()> {
        if self.triggers.is_empty() {
            return Ok(());
        }

        let control_server = self.control_server.clone();

        info!(
            trigger_count = self.triggers.len(),
            "Starting local trigger service"
        );

        let configs = self
            .triggers
            .iter()
            .filter_map(|trigger| match trigger {
                WorkerTrigger::Queue {
                    queue,
                    batch: Some(batch),
                } => Some((queue.id.clone(), batch.clone())),
                _ => None,
            })
            .collect();
        let collectors = QueueCollectors::new(configs, control_server.clone());
        let mut handles = JoinSet::new();

        for trigger in &self.triggers {
            match trigger {
                WorkerTrigger::Queue { queue, .. } => {
                    let binding_name = queue.id.clone();
                    let provider = self.bindings_provider.clone();
                    let cs = control_server.clone();
                    let mut shutdown = self.shutdown_rx.resubscribe();
                    let collector = collectors.clients.get(&binding_name).cloned();
                    let timeout = self.timeout;

                    info!(queue = %binding_name, "Starting queue trigger poller");
                    handles.spawn(async move {
                        if let Err(e) = poll_queue(
                            &binding_name,
                            &provider,
                            &cs,
                            &mut shutdown,
                            collector,
                            timeout,
                        )
                        .await
                        {
                            error!(queue = %binding_name, error = %e, "Queue poller error");
                        }
                    });
                }
                WorkerTrigger::Storage { storage, events } => {
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
                    handles.spawn(async move {
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
                    });
                }
                WorkerTrigger::Schedule { cron } => {
                    let cron_expr = cron.clone();
                    let cs = control_server.clone();
                    let cron_state_dir = self.state_dir.clone();
                    let mut shutdown = self.shutdown_rx.resubscribe();

                    info!(cron = %cron_expr, "Starting cron trigger scheduler");
                    handles.spawn(async move {
                        if let Err(e) =
                            run_cron_scheduler(&cron_expr, &cron_state_dir, &cs, &mut shutdown)
                                .await
                        {
                            error!(cron = %cron_expr, error = %e, "Cron scheduler error");
                        }
                    });
                }
            }
        }

        // Every exit path joins owned collectors; a failed poller must not
        // detach already-dispatched application work.
        let exit = tokio::select! {
            _ = self.shutdown_rx.recv() => Ok(()),
            result = handles.join_next() => Err(AlienError::new(ErrorData::TriggerServiceError {
                trigger_type: "all".to_string(), trigger_id: String::new(),
                reason: format!("Trigger task exited unexpectedly: {:?}", result),
            })),
        };

        info!("Shutting down local trigger service");
        collectors.stop();
        if exit.is_ok() {
            while let Some(joined) = handles.join_next().await {
                joined
                    .into_alien_error()
                    .context(ErrorData::TriggerServiceError {
                        trigger_type: "all".into(),
                        trigger_id: String::new(),
                        reason: "Trigger shutdown failed".into(),
                    })?;
            }
        } else {
            handles.shutdown().await;
        }
        collectors
            .join()
            .await
            .context(ErrorData::TriggerServiceError {
                trigger_type: "queue".into(),
                trigger_id: String::new(),
                reason: "Failed to stop queue collectors".into(),
            })?;

        exit
    }
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

/// Poll a queue for messages and deliver to the worker via send_task.
/// Acks only on successful handler completion (at-least-once delivery).
///
/// Uses the shared `LocalBindingsProvider` to access the queue — the same
/// provider the worker runtime uses. `LocalQueue` (multi-process WAL mode +
/// busy_timeout) allows concurrent opens across handles and processes, so this
/// sharing is a convenience rather than a lock-contention workaround.
async fn poll_queue(
    binding_name: &str,
    provider: &LocalBindingsProvider,
    control_server: &ControlGrpcServer,
    shutdown: &mut broadcast::Receiver<()>,
    collector: Option<QueueCollector>,
    timeout: Duration,
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

    if let Some(collector) = collector {
        return poll_batch_queue(
            binding_name,
            queue,
            control_server,
            shutdown,
            collector,
            timeout,
        )
        .await;
    }
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!(queue = %binding_name, "Queue poller shutting down");
                return Ok(());
            }
            result = poll_queue_once(binding_name, &*queue, control_server, timeout) => {
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

async fn poll_batch_queue(
    binding_name: &str,
    queue: Arc<dyn Queue>,
    control_server: &ControlGrpcServer,
    shutdown: &mut broadcast::Receiver<()>,
    collector: QueueCollector,
    timeout: Duration,
) -> Result<()> {
    let mut pending: JoinSet<alien_bindings::error::Result<()>> = JoinSet::new();
    let stop = CancellationToken::new();
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                stop.cancel();
                while let Some(joined) = pending.join_next().await {
                    joined.into_alien_error().context(ErrorData::TriggerServiceError {
                        trigger_type: "queue".into(), trigger_id: binding_name.into(), reason: "Delivery shutdown failed".into(),
                    })?.context(ErrorData::TriggerServiceError {
                        trigger_type: "queue".into(), trigger_id: binding_name.into(), reason: "Delivery release failed".into(),
                    })?;
                }
                return Ok(());
            },
            joined = pending.join_next(), if !pending.is_empty() => {
                let completion = joined.ok_or_else(|| AlienError::new(ErrorData::TriggerServiceError { trigger_type: "queue".into(), trigger_id: binding_name.into(), reason: "Queue task set unexpectedly empty".into() }))?.into_alien_error().context(ErrorData::TriggerServiceError {
                    trigger_type: "queue".into(), trigger_id: binding_name.into(), reason: "Queue delivery task failed".into(),
                })?;
                if let Err(error) = completion {
                    // No successful completion was confirmed. The lease makes
                    // this delivery available again; keep other intake alive.
                    tracing::error!(queue = %binding_name, %error, "Queue completion failed; delivery remains eligible for retry");
                }
            },
            _ = interval.tick(), if pending.len() < 100 => {
                let messages = match queue.receive(binding_name, (100 - pending.len()).min(10)).await {
                    Ok(messages) => messages,
                    Err(error) => {
                        tracing::error!(queue = %binding_name, %error, "Queue receive failed; retrying in 2s");
                        interval.reset_after(Duration::from_secs(2));
                        continue;
                    }
                };
                for message in messages {
                    let queue = queue.clone();
                    let collector = collector.clone();
                    let source = binding_name.to_string();
                    let control = control_server.clone();
                    let stop = stop.clone();
                    pending.spawn(async move {
                        queue.renew_lease(&source, &message.receipt_handle, 600).await?;
                        let success = {
                        let delivery = collector.deliver(ProtoQueueMessage {
                            id: uuid::Uuid::new_v4().to_string(), source: source.clone(),
                            payload: match message.payload { MessagePayload::Json(value) => value.to_string().into_bytes(), MessagePayload::Text(value) => value.into_bytes() },
                            receipt_handle: message.receipt_handle.clone(), attempt_count: message.attempt,
                            timestamp: Some(now_timestamp()), attributes: HashMap::new(),
                        }, tokio::time::Instant::now() + timeout);
                        tokio::pin!(delivery);
                        loop {
                            tokio::select! {
                                result = &mut delivery => break Some(result),
                                _ = stop.cancelled() => break None,
                                _ = tokio::time::sleep(Duration::from_secs(300)) => {
                                    if let Err(error) = queue.renew_lease(&source, &message.receipt_handle, 600).await {
                                        control.cancel_application().await;
                                        return Err(error);
                                    }
                                },
                            }
                        }
                        };
                        if success.is_none() { control.cancel_application().await; }
                        if success.unwrap_or(false) { queue.ack(&source, &message.receipt_handle).await?; }
                        else { queue.nack(&source, &message.receipt_handle).await?; }
                        Ok(())
                    });
                }
            },
        }
    }
}

async fn poll_queue_once(
    binding_name: &str,
    queue: &dyn Queue,
    control_server: &ControlGrpcServer,
    timeout: Duration,
) -> Result<usize> {
    let messages =
        queue
            .receive(binding_name, 1)
            .await
            .context(ErrorData::TriggerServiceError {
                trigger_type: "queue".to_string(),
                trigger_id: binding_name.to_string(),
                reason: "Failed to receive messages".to_string(),
            })?;
    let count = messages.len();

    for msg in messages {
        queue
            .renew_lease(binding_name, &msg.receipt_handle, 600)
            .await
            .context(ErrorData::TriggerServiceError {
                trigger_type: "queue".into(),
                trigger_id: binding_name.into(),
                reason: "Failed to establish delivery lease".into(),
            })?;
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
                attempt_count: msg.attempt,
                timestamp: Some(now_timestamp()),
                attributes: HashMap::new(),
            })),
        };

        let delivery = control_server.send_task(task, timeout);
        tokio::pin!(delivery);
        let result = loop {
            tokio::select! {
                result = &mut delivery => break result,
                _ = tokio::time::sleep(Duration::from_secs(300)) => {
                    if let Err(error) = queue.renew_lease(binding_name, &msg.receipt_handle, 600).await {
                        control_server.cancel_application().await;
                        return Err(error.context(ErrorData::TriggerServiceError {
                            trigger_type: "queue".into(), trigger_id: binding_name.into(), reason: "Delivery lease renewal failed".into(),
                        }));
                    }
                },
            }
        };
        let completion = if result.is_ok_and(|result| result.success) {
            queue.ack(binding_name, &msg.receipt_handle).await
        } else {
            queue.nack(binding_name, &msg.receipt_handle).await
        };
        completion.context(ErrorData::TriggerServiceError {
            trigger_type: "queue".into(),
            trigger_id: binding_name.into(),
            reason: "Queue completion failed".into(),
        })?;
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
        match result {
            Ok(event)
                if matches!(
                    event.kind,
                    EventKind::Create(_)
                        | EventKind::Remove(_)
                        | EventKind::Modify(
                            notify::event::ModifyKind::Name(_) | notify::event::ModifyKind::Data(_)
                        )
                ) =>
            {
                // Reconciliation covers dropped hints; never block watch registration.
                let _ = fs_tx.try_send(event);
            }
            Ok(_) => {}
            Err(error) => {
                warn!(%error, "Storage watcher error; periodic reconciliation remains active")
            }
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

    // notify reports canonicalized paths (on macOS `/var` is a symlink to
    // `/private/var`, so events arrive under `/private/var/...` while the
    // configured root says `/var/...`). Strip keys against the canonicalized
    // root, or event keys silently degrade to absolute filesystem paths.
    let canonical_storage_path =
        storage_path
            .canonicalize()
            .into_alien_error()
            .context(ErrorData::LocalDirectoryError {
                path: storage_path.display().to_string(),
                operation: "canonicalize".to_string(),
                reason: "Failed to canonicalize storage directory for watching".to_string(),
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

    // Recursive inotify registration races writes into new directories. Notifications only
    // wake reconciliation; committed objects remain pending until their handler succeeds.
    // A recovered Worker replays existing objects, matching at-least-once cloud delivery.
    //
    // `known` holds every committed object seen since start, with the version whose
    // creation event succeeded (`None` while that creation is still pending). Deletion is
    // derived from `known`, so an object whose creation handler failed still gets its
    // deletion event.
    let mut known: HashMap<PathBuf, Option<ObjectVersion>> = HashMap::new();
    let mut reconcile = tokio::time::interval(Duration::from_secs(1));
    reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shutdown.recv() => return Ok(()),
            _ = reconcile.tick() => {},
            _ = fs_rx.recv() => {},
        }
        // One scan covers every hint queued so far, so a write burst costs one scan
        // instead of one per hint, and the timer restarts after each scan. Only the hints
        // already queued are drained, so a steady stream of new ones cannot delay the scan.
        for _ in 0..fs_rx.len() {
            let _ = fs_rx.try_recv();
        }
        reconcile.reset();
        let root = canonical_storage_path.clone();
        let current = match tokio::task::spawn_blocking(move || storage_snapshot(&root))
            .await
            .into_alien_error()
            .context(ErrorData::TriggerServiceError {
                trigger_type: "storage".to_string(),
                trigger_id: binding_name.to_string(),
                reason: "Storage scan task failed".to_string(),
            })? {
            Ok(current) => current,
            Err(error) => {
                // Pending objects stay pending and the next tick rescans. Returning would
                // stop the Worker's queue and cron triggers along with this watcher.
                warn!(storage = %binding_name, %error, "Storage scan failed; retrying on the next reconciliation");
                continue;
            }
        };
        for path in current.keys() {
            known.entry(path.clone()).or_insert(None);
        }
        let changes = known
            .iter()
            .filter_map(|(path, delivered)| match current.get(path) {
                Some(version) if *delivered != Some(*version) => {
                    Some((path.clone(), "created", Some(*version)))
                }
                Some(_) => None,
                None => Some((path.clone(), "deleted", None)),
            })
            .collect::<Vec<_>>();
        for (path, event_type, version) in changes {
            if event_types.iter().any(|event| event == event_type) {
                let relative = path
                    .strip_prefix(&canonical_storage_path)
                    .into_alien_error()
                    .context(ErrorData::TriggerServiceError {
                        trigger_type: "storage".to_string(),
                        trigger_id: binding_name.to_string(),
                        reason: "Storage object is outside the watched root".to_string(),
                    })?
                    .to_string_lossy()
                    .to_string();
                let task = Task {
                    task_id: uuid::Uuid::new_v4().to_string(),
                    payload: Some(control::task::Payload::StorageEvent(ProtoStorageEvent {
                        bucket: binding_name.to_string(),
                        key: relative,
                        event_type: event_type.to_string(),
                        size: version.map_or(0, |(_, size)| size),
                        timestamp: Some(now_timestamp()),
                        ..Default::default()
                    })),
                };
                let result = tokio::select! {
                    _ = shutdown.recv() => return Ok(()),
                    result = control_server.send_task(task, Duration::from_secs(30)) => result,
                };
                match result {
                    Ok(result) if result.success => {}
                    result => {
                        warn!(storage = %binding_name, ?result, "Storage event failed; retaining for reconciliation");
                        continue;
                    }
                }
            }
            if version.is_some() {
                known.insert(path, version);
            } else {
                known.remove(&path);
            }
        }
    }
}

/// Modification time and size of a committed storage object.
type ObjectVersion = (std::time::SystemTime, u64);

fn storage_snapshot(root: &Path) -> Result<HashMap<PathBuf, ObjectVersion>> {
    let mut objects = HashMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .into_alien_error()
                    .context(ErrorData::LocalDirectoryError {
                        path: directory.display().to_string(),
                        operation: "scan".to_string(),
                        reason: "Failed to reconcile storage objects".to_string(),
                    })
            }
        };
        for entry in entries {
            let entry = entry
                .into_alien_error()
                .context(ErrorData::LocalDirectoryError {
                    path: directory.display().to_string(),
                    operation: "scan".to_string(),
                    reason: "Failed to read storage entry".to_string(),
                })?;
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .into_alien_error()
                        .context(ErrorData::LocalDirectoryError {
                            path: path.display().to_string(),
                            operation: "stat".to_string(),
                            reason: "Failed to inspect storage object".to_string(),
                        })
                }
            };
            if metadata.is_dir() {
                directories.push(path);
            } else if metadata.is_file() {
                // object_store stages key#N, then renames to the committed key.
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.rsplit_once('#').is_some_and(|(_, suffix)| {
                            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
                        })
                    })
                {
                    continue;
                }
                let modified = metadata.modified().into_alien_error().context(
                    ErrorData::LocalDirectoryError {
                        path: path.display().to_string(),
                        operation: "stat".to_string(),
                        reason: "Failed to read object modification time".to_string(),
                    },
                )?;
                objects.insert(path, (modified, metadata.len()));
            }
        }
    }
    Ok(objects)
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
    use alien_worker_protocol::control::{
        control_service_server::ControlService, send_task_result_request, SendTaskResultRequest,
        TaskSuccess, WaitForTasksRequest,
    };
    use futures_util::StreamExt;
    use std::collections::HashSet;
    use tonic::Request;

    #[derive(Debug)]
    struct RecoveringQueue {
        inner: alien_bindings::providers::queue::local::LocalQueue,
        fail_receive: std::sync::atomic::AtomicBool,
        fail_ack: std::sync::atomic::AtomicBool,
        completed: tokio::sync::Notify,
    }

    impl alien_bindings::traits::Binding for RecoveringQueue {}

    #[async_trait::async_trait]
    impl Queue for RecoveringQueue {
        async fn send(
            &self,
            queue: &str,
            message: MessagePayload,
        ) -> alien_bindings::error::Result<()> {
            self.inner.send(queue, message).await
        }
        async fn send_batch(
            &self,
            queue: &str,
            messages: Vec<MessagePayload>,
        ) -> alien_bindings::error::Result<Vec<alien_bindings::traits::QueueSendResult>> {
            self.inner.send_batch(queue, messages).await
        }
        async fn receive(
            &self,
            queue: &str,
            count: usize,
        ) -> alien_bindings::error::Result<Vec<alien_bindings::traits::QueueMessage>> {
            if self
                .fail_receive
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(AlienError::new(
                    alien_bindings::error::ErrorData::BindingSetupFailed {
                        binding_type: "queue".into(),
                        reason: "temporary receive failure".into(),
                    },
                ));
            }
            self.inner.receive(queue, count).await
        }
        async fn ack(&self, queue: &str, receipt: &str) -> alien_bindings::error::Result<()> {
            if self
                .fail_ack
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                // Make the uncompleted lease immediately eligible for retry.
                self.inner.nack(queue, receipt).await?;
                return Err(AlienError::new(
                    alien_bindings::error::ErrorData::BindingSetupFailed {
                        binding_type: "queue".into(),
                        reason: "temporary completion failure".into(),
                    },
                ));
            }
            self.inner.ack(queue, receipt).await?;
            self.completed.notify_one();
            Ok(())
        }
        async fn renew_lease(
            &self,
            queue: &str,
            receipt: &str,
            seconds: u32,
        ) -> alien_bindings::error::Result<()> {
            self.inner.renew_lease(queue, receipt, seconds).await
        }
        async fn nack(&self, queue: &str, receipt: &str) -> alien_bindings::error::Result<()> {
            self.inner.nack(queue, receipt).await
        }
        async fn purge(&self, queue: &str) -> alien_bindings::error::Result<()> {
            self.inner.purge(queue).await
        }
    }

    #[tokio::test]
    async fn local_batch_intake_recovers_from_receive_and_completion_errors() {
        let directory = tempfile::tempdir().expect("queue directory");
        let queue = Arc::new(RecoveringQueue {
            inner: alien_bindings::providers::queue::local::LocalQueue::new(
                directory.path().to_path_buf(),
            )
            .await
            .expect("queue"),
            fail_receive: std::sync::atomic::AtomicBool::new(true),
            fail_ack: std::sync::atomic::AtomicBool::new(true),
            completed: tokio::sync::Notify::new(),
        });
        queue
            .send("jobs", MessagePayload::Text("retryable".into()))
            .await
            .expect("send");
        let control = Arc::new(ControlGrpcServer::new());
        let mut stream = control
            .wait_for_tasks(Request::new(WaitForTasksRequest::default()))
            .await
            .expect("subscribe")
            .into_inner();
        let collectors = QueueCollectors::new(
            HashMap::from([(
                "jobs".into(),
                alien_core::QueueBatchConfig {
                    max_messages: 1,
                    max_window_seconds: 0,
                },
            )]),
            control.clone(),
        );
        let collector = collectors.clients["jobs"].clone();
        let (shutdown, mut receiver) = broadcast::channel(1);
        let poll_queue = queue.clone();
        let poll_control = control.clone();
        let poller = tokio::spawn(async move {
            poll_batch_queue(
                "jobs",
                poll_queue,
                &poll_control,
                &mut receiver,
                collector,
                Duration::from_secs(60),
            )
            .await
        });
        for attempt in [1, 2] {
            let task = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("polling recovered")
                .expect("stream")
                .expect("task");
            let Some(control::task::Payload::QueueBatch(batch)) = task.payload else {
                panic!("batch");
            };
            assert_eq!(batch.deliveries.len(), 1);
            assert_eq!(
                batch.deliveries[0]
                    .message
                    .as_ref()
                    .expect("message")
                    .attempt_count,
                attempt
            );
            control
                .send_task_result(Request::new(SendTaskResultRequest {
                    task_id: task.task_id,
                    result: Some(send_task_result_request::Result::QueueBatch(
                        control::QueueBatchResult {
                            outcomes: vec![control::QueueDeliveryOutcome {
                                delivery_id: batch.deliveries[0].delivery_id.clone(),
                                disposition: control::queue_delivery_outcome::Disposition::Success
                                    as i32,
                            }],
                        },
                    )),
                }))
                .await
                .expect("result");
        }
        // Submitting the application result does not confirm the queue ACK. Shutdown
        // may still cancel that delivery and release it for retry, so observe the
        // persisted completion before testing the recovered poller's shutdown.
        tokio::time::timeout(Duration::from_secs(5), queue.completed.notified())
            .await
            .expect("recovered delivery was durably acknowledged");
        shutdown.send(()).expect("shutdown");
        poller.await.expect("poller").expect("clean shutdown");
        collectors.join().await.expect("collectors");
        assert!(!queue.fail_receive.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!queue.fail_ack.load(std::sync::atomic::Ordering::SeqCst));
        assert!(queue
            .receive("jobs", 10)
            .await
            .expect("empty queue")
            .is_empty());
    }

    #[tokio::test]
    async fn local_batches_ack_independently_and_release_shutdown_delivery_after_app_stops() {
        let directory = tempfile::tempdir().expect("queue directory");
        let queue = Arc::new(
            alien_bindings::providers::queue::local::LocalQueue::new(
                directory.path().to_path_buf(),
            )
            .await
            .expect("queue"),
        );
        for value in ["complete", "retry"] {
            queue
                .send("jobs", MessagePayload::Text(value.into()))
                .await
                .expect("send");
        }
        let control = Arc::new(ControlGrpcServer::new());
        control.supervise_task_cancellation();
        let mut stream = control
            .wait_for_tasks(Request::new(WaitForTasksRequest::default()))
            .await
            .expect("subscribe")
            .into_inner();
        let collectors = QueueCollectors::new(
            HashMap::from([(
                "jobs".into(),
                alien_core::QueueBatchConfig {
                    max_messages: 2,
                    max_window_seconds: 1,
                },
            )]),
            control.clone(),
        );
        let collector = collectors.clients["jobs"].clone();
        let (shutdown, mut receiver) = broadcast::channel(1);
        let poll_control = control.clone();
        let poll_queue = queue.clone();
        let poller = tokio::spawn(async move {
            poll_batch_queue(
                "jobs",
                poll_queue,
                &poll_control,
                &mut receiver,
                collector,
                Duration::from_secs(900),
            )
            .await
        });
        let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("full batch")
            .expect("stream")
            .expect("task");
        let Some(control::task::Payload::QueueBatch(batch)) = first.payload else {
            panic!("typed batch");
        };
        assert_eq!(batch.source, "jobs");
        assert_eq!(batch.deliveries.len(), 2);
        assert!(queue
            .receive("jobs", 10)
            .await
            .expect("inflight hidden")
            .is_empty());
        let outcomes = batch
            .deliveries
            .iter()
            .map(|delivery| control::QueueDeliveryOutcome {
                delivery_id: delivery.delivery_id.clone(),
                disposition: if delivery.message.as_ref().expect("message").payload == b"complete" {
                    1
                } else {
                    2
                },
            })
            .collect();
        control
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: first.task_id,
                result: Some(send_task_result_request::Result::QueueBatch(
                    control::QueueBatchResult { outcomes },
                )),
            }))
            .await
            .expect("partial result");
        let second = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("undersized retry batch")
            .expect("stream")
            .expect("task");
        let Some(control::task::Payload::QueueBatch(batch)) = second.payload else {
            panic!("retry batch");
        };
        assert_eq!(batch.deliveries.len(), 1);
        let message = batch.deliveries[0].message.as_ref().expect("message");
        assert_eq!(message.payload, b"retry");
        assert_eq!(message.attempt_count, 2);
        shutdown.send(()).expect("shutdown");
        tokio::time::timeout(Duration::from_secs(5), control.wait_for_cancelled_task())
            .await
            .expect("host cancellation");
        assert!(
            !poller.is_finished(),
            "release must wait for the application stop barrier"
        );
        control.application_stopped();
        tokio::time::timeout(Duration::from_secs(5), poller)
            .await
            .expect("bounded shutdown")
            .expect("poller")
            .expect("release");
        collectors.join().await.expect("collectors joined");
        drop(queue);
        let reopened = alien_bindings::providers::queue::local::LocalQueue::new(
            directory.path().to_path_buf(),
        )
        .await
        .expect("restart queue");
        let surviving = reopened
            .receive("jobs", 10)
            .await
            .expect("restart delivery");
        assert_eq!(
            surviving.len(),
            1,
            "completed message must stay deleted across restart"
        );
        assert!(matches!(&surviving[0].payload, MessagePayload::Text(value) if value == "retry"));
        assert_eq!(surviving[0].attempt, 3);
        assert_ne!(surviving[0].receipt_handle, message.receipt_handle);
        reopened
            .ack("jobs", &surviving[0].receipt_handle)
            .await
            .expect("complete survivor");
        assert!(reopened
            .receive("jobs", 10)
            .await
            .expect("empty")
            .is_empty());
    }

    async fn acknowledge(server: &ControlGrpcServer, task: &Task) {
        server
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: task.task_id.clone(),
                result: Some(send_task_result_request::Result::Success(
                    TaskSuccess::default(),
                )),
            }))
            .await
            .expect("handler result accepted");
    }

    #[tokio::test]
    async fn storage_reconciles_new_directories_while_handler_is_busy_and_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let events = vec!["created".to_string(), "deleted".to_string()];
        let server = Arc::new(ControlGrpcServer::new());
        let mut stream = server
            .wait_for_tasks(Request::new(WaitForTasksRequest::default()))
            .await
            .unwrap()
            .into_inner();
        let (shutdown, mut receiver) = broadcast::channel(1);
        let task_root = root.clone();
        let task_server = server.clone();
        let task_events = events.clone();
        let watcher = tokio::spawn(async move {
            watch_storage(
                "objects",
                &task_root,
                &task_events,
                &task_server,
                &mut receiver,
            )
            .await
        });
        std::fs::write(root.join("first"), b"first").unwrap();
        let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("initial event")
            .unwrap()
            .unwrap();
        // Commit immediately into fresh directories while delivery awaits the first handler.
        // More hints than the 256-event buffer must not lose committed objects.
        for index in 0..300 {
            let folder = root.join(format!("new/{index}"));
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("object#1"), b"payload").unwrap();
            std::fs::rename(folder.join("object#1"), folder.join("object")).unwrap();
        }
        std::fs::write(root.join("unfinished#1"), b"unfinished").unwrap();
        acknowledge(&server, &first).await;
        let mut received = HashSet::new();
        while received.len() < 300 {
            let task = tokio::time::timeout(Duration::from_secs(10), stream.next())
                .await
                .expect("every committed object delivered")
                .unwrap()
                .unwrap();
            let Some(control::task::Payload::StorageEvent(event)) = task.payload.as_ref() else {
                panic!("expected storage task");
            };
            assert_eq!(event.bucket, "objects");
            assert_eq!(event.event_type, "created");
            assert_eq!(event.size, 7);
            assert!(event.key.ends_with("/object"));
            assert!(
                received.insert(event.key.clone()),
                "no duplicate successful delivery"
            );
            acknowledge(&server, &task).await;
        }
        std::fs::remove_file(root.join("first")).unwrap();
        let deleted = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("delete delivered")
            .unwrap()
            .unwrap();
        let Some(control::task::Payload::StorageEvent(event)) = deleted.payload.as_ref() else {
            panic!("expected storage deletion");
        };
        assert_eq!(event.key, "first");
        assert_eq!(event.event_type, "deleted");
        acknowledge(&server, &deleted).await;
        // A handler failure must not acknowledge the committed object.
        std::fs::write(root.join("retry"), b"retry").unwrap();
        let failed = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        server
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: failed.task_id.clone(),
                result: Some(send_task_result_request::Result::Error(
                    control::TaskError {
                        code: "retry".to_string(),
                        message: "temporary failure".to_string(),
                    },
                )),
            }))
            .await
            .unwrap();
        let retried = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Some(control::task::Payload::StorageEvent(event)) = retried.payload.as_ref() else {
            panic!("expected retried storage task");
        };
        assert_eq!(event.key, "retry");
        assert_ne!(failed.task_id, retried.task_id);
        acknowledge(&server, &retried).await;
        shutdown.send(()).unwrap();
        watcher.await.unwrap().unwrap();
        drop(stream);

        // A new runtime uses its own protocol server and replays the retained bucket.
        let replacement = Arc::new(ControlGrpcServer::new());
        let mut replacement_stream = replacement
            .wait_for_tasks(Request::new(WaitForTasksRequest::default()))
            .await
            .unwrap()
            .into_inner();
        let (shutdown, mut receiver) = broadcast::channel(1);
        let task_server = replacement.clone();
        let watcher = tokio::spawn(async move {
            watch_storage("objects", &root, &events, &task_server, &mut receiver).await
        });
        let replay = tokio::time::timeout(Duration::from_secs(5), replacement_stream.next())
            .await
            .expect("retained bucket replayed through replacement server")
            .unwrap()
            .unwrap();
        acknowledge(&replacement, &replay).await;
        shutdown.send(()).unwrap();
        watcher.await.unwrap().unwrap();
    }

    async fn next_storage_event(
        stream: &mut (impl futures_util::Stream<Item = std::result::Result<Task, tonic::Status>>
                  + Unpin),
    ) -> (Task, ProtoStorageEvent) {
        let task = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("storage event delivered")
            .unwrap()
            .unwrap();
        let Some(control::task::Payload::StorageEvent(event)) = task.payload.clone() else {
            panic!("expected storage task");
        };
        (task, event)
    }

    async fn fail(server: &ControlGrpcServer, task: &Task) {
        server
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: task.task_id.clone(),
                result: Some(send_task_result_request::Result::Error(
                    control::TaskError {
                        code: "failed".to_string(),
                        message: "handler failed".to_string(),
                    },
                )),
            }))
            .await
            .expect("handler result accepted");
    }

    #[tokio::test]
    async fn storage_deletion_is_delivered_after_failed_creation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let server = Arc::new(ControlGrpcServer::new());
        let mut stream = server
            .wait_for_tasks(Request::new(WaitForTasksRequest::default()))
            .await
            .unwrap()
            .into_inner();
        let (shutdown, mut receiver) = broadcast::channel(1);
        let task_root = root.clone();
        let task_server = server.clone();
        let watcher = tokio::spawn(async move {
            let events = ["created".to_string(), "deleted".to_string()];
            watch_storage("objects", &task_root, &events, &task_server, &mut receiver).await
        });

        std::fs::write(root.join("doomed"), b"doomed").unwrap();
        let (created, event) = next_storage_event(&mut stream).await;
        assert_eq!(
            (event.key.as_str(), event.event_type.as_str()),
            ("doomed", "created")
        );
        // The object disappears before its creation handler fails.
        std::fs::remove_file(root.join("doomed")).unwrap();
        fail(&server, &created).await;

        let (deleted, event) = next_storage_event(&mut stream).await;
        assert_eq!(
            (event.key.as_str(), event.event_type.as_str()),
            ("doomed", "deleted")
        );
        acknowledge(&server, &deleted).await;
        shutdown.send(()).unwrap();
        watcher.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn storage_scan_failure_keeps_watching_and_recovers() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("bucket");
        std::fs::create_dir(&root).unwrap();
        let server = Arc::new(ControlGrpcServer::new());
        let mut stream = server
            .wait_for_tasks(Request::new(WaitForTasksRequest::default()))
            .await
            .unwrap()
            .into_inner();
        let (shutdown, mut receiver) = broadcast::channel(1);
        let task_root = root.clone();
        let task_server = server.clone();
        let watcher = tokio::spawn(async move {
            let events = ["created".to_string()];
            watch_storage("objects", &task_root, &events, &task_server, &mut receiver).await
        });
        std::fs::write(root.join("before"), b"before").unwrap();
        let (task, event) = next_storage_event(&mut stream).await;
        assert_eq!(event.key, "before");
        acknowledge(&server, &task).await;

        // A file where the bucket directory belongs makes every scan fail.
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::write(&root, b"not a directory").unwrap();
        assert!(std::fs::read_dir(&root).is_err());
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(
            !watcher.is_finished(),
            "a failed scan must not end the trigger service"
        );

        std::fs::remove_file(&root).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("after"), b"after").unwrap();
        let (task, event) = next_storage_event(&mut stream).await;
        assert_eq!(
            (event.key.as_str(), event.event_type.as_str()),
            ("after", "created")
        );
        acknowledge(&server, &task).await;
        shutdown.send(()).unwrap();
        watcher.await.unwrap().unwrap();
    }

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
