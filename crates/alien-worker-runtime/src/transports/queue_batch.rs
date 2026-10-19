//! Bounded in-process collection. Enqueueing never acknowledges a delivery.

use std::{sync::Arc, time::Duration};

use alien_core::QueueBatchConfig;
use alien_error::{Context, IntoAlienError};
use alien_worker_protocol::{
    batch::validate_outcomes,
    control::{
        queue_delivery_outcome::Disposition, task::Payload, QueueBatch, QueueDelivery,
        QueueMessage, Task,
    },
    ControlGrpcServer,
};
use prost::Message;
use tokio::{
    sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::error::{ErrorData, Result};

pub(crate) const MAX_BATCH_BYTES: usize = 3 * 1024 * 1024;
const MAX_PENDING_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_MESSAGES: usize = 200;

pub(crate) fn batch_routing(
    environment: &std::collections::HashMap<String, String>,
) -> Result<(
    std::collections::HashMap<String, QueueBatchConfig>,
    std::collections::HashMap<String, String>,
)> {
    let Some(value) = environment.get("ALIEN_QUEUE_BATCH_CONFIG") else {
        return Ok(Default::default());
    };
    let sources: std::collections::HashMap<String, Option<QueueBatchConfig>> =
        serde_json::from_str(value)
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Invalid queue batch configuration".into(),
            })?;
    // Logical routes are authoritative. Short provider names are conveniences,
    // not identities: distinct accounts/namespaces may reuse the same name.
    let mut aliases: std::collections::HashMap<String, String> = sources
        .keys()
        .map(|source| (source.clone(), source.clone()))
        .collect();
    let mut short_aliases: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for (source, config) in &sources {
        if let Some(config) = config {
            config.validate().context(ErrorData::Other {
                message: "Invalid queue batch limits".into(),
            })?;
        }
        if let Some(binding) = environment.get(&alien_core::bindings::binding_env_var_name(source))
        {
            let binding: serde_json::Value = serde_json::from_str(binding)
                .into_alien_error()
                .context(ErrorData::Other {
                    message: "Invalid queue source binding".into(),
                })?;
            for key in ["queueUrl", "queueName", "topic", "subscription"] {
                if let Some(alias) = binding.get(key).and_then(serde_json::Value::as_str) {
                    if alias.contains('/')
                        && aliases
                            .insert(alias.to_string(), source.clone())
                            .is_some_and(|previous| previous != *source)
                    {
                        return Err(alien_error::AlienError::new(ErrorData::Other {
                            message: "Ambiguous qualified queue source bindings".into(),
                        }));
                    }
                    let short = alias.rsplit('/').next().unwrap_or(alias).to_string();
                    short_aliases
                        .entry(short)
                        .and_modify(|previous| {
                            if previous.as_ref() != Some(source) {
                                *previous = None;
                            }
                        })
                        .or_insert_with(|| Some(source.clone()));
                }
            }
        }
    }
    for (alias, source) in short_aliases {
        if let Some(source) = source {
            aliases.entry(alias).or_insert(source);
        }
    }
    Ok((
        sources
            .into_iter()
            .filter_map(|(source, config)| config.map(|config| (source, config)))
            .collect(),
        aliases,
    ))
}

pub(crate) fn batch_routing_from_env() -> Result<(
    std::collections::HashMap<String, QueueBatchConfig>,
    std::collections::HashMap<String, String>,
)> {
    batch_routing(&std::env::vars().collect())
}

struct Pending {
    cancelled: CancellationToken,
    delivery: QueueDelivery,
    deadline: Instant,
    response: oneshot::Sender<bool>,
    _bytes: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub struct QueueCollector {
    source: String,
    accepted_source: String,
    sender: mpsc::Sender<Pending>,
    bytes: Arc<Semaphore>,
}

/// Owns every collector task for a transport. Dropping aborts tasks and their
/// dispatched-task guards; orderly shutdown cancels and joins before returning.
pub struct QueueCollectors {
    pub clients: std::collections::HashMap<String, QueueCollector>,
    tasks: tokio::task::JoinSet<()>,
    shutdown: CancellationToken,
}

impl QueueCollectors {
    pub(crate) fn from_env(control: Arc<ControlGrpcServer>) -> Result<Self> {
        Self::from_environment(&std::env::vars().collect(), control)
    }

    pub(crate) fn from_environment(
        environment: &std::collections::HashMap<String, String>,
        control: Arc<ControlGrpcServer>,
    ) -> Result<Self> {
        let (configs, aliases) = batch_routing(environment)?;
        let mut collectors = Self::new(configs, control);
        for (alias, logical) in aliases {
            let Some(mut client) = collectors.clients.get(&logical).cloned() else {
                continue;
            };
            client.accepted_source = alias.clone();
            collectors.clients.insert(alias, client);
        }
        Ok(collectors)
    }

    pub fn new(
        configs: std::collections::HashMap<String, QueueBatchConfig>,
        control: Arc<ControlGrpcServer>,
    ) -> Self {
        let shutdown = CancellationToken::new();
        let mut tasks = tokio::task::JoinSet::new();
        let bytes = Arc::new(Semaphore::new(MAX_PENDING_BYTES));
        let clients = configs
            .into_iter()
            .map(|(source, config)| {
                let client = QueueCollector::spawn(
                    source.clone(),
                    config,
                    control.clone(),
                    shutdown.clone(),
                    &mut tasks,
                    bytes.clone(),
                );
                (source, client)
            })
            .collect();
        Self {
            clients,
            tasks,
            shutdown,
        }
    }

    pub fn stop(&self) {
        self.shutdown.cancel();
    }

    pub async fn join(mut self) -> Result<()> {
        self.stop();
        while let Some(result) = self.tasks.join_next().await {
            result.into_alien_error().context(ErrorData::Other {
                message: "Queue collector task failed".into(),
            })?;
        }
        Ok(())
    }
}

impl QueueCollector {
    fn spawn(
        source: String,
        config: QueueBatchConfig,
        control: Arc<ControlGrpcServer>,
        shutdown: CancellationToken,
        tasks: &mut tokio::task::JoinSet<()>,
        bytes: Arc<Semaphore>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(MAX_PENDING_MESSAGES);
        tasks.spawn(collect(receiver, source.clone(), config, control, shutdown));
        Self {
            sender,
            bytes,
            accepted_source: source.clone(),
            source,
        }
    }

    /// An unavailable slot or expired request returns failure without waiting for admission.
    pub async fn deliver(&self, mut message: QueueMessage, deadline: Instant) -> bool {
        if message.source != self.accepted_source {
            return false;
        }
        message.source.clone_from(&self.source);
        let cancelled = CancellationToken::new();
        let _cancel_on_drop = cancelled.clone().drop_guard();
        let delivery = QueueDelivery {
            delivery_id: uuid::Uuid::new_v4().to_string(),
            message: Some(message),
        };
        let size = delivery.encoded_len() + 64;
        if size > MAX_BATCH_BYTES || deadline <= Instant::now() {
            return false;
        }
        let Ok(permit) = self.bytes.clone().try_acquire_many_owned(size as u32) else {
            return false;
        };
        let (response, receiver) = oneshot::channel();
        if self
            .sender
            .try_send(Pending {
                delivery,
                deadline,
                response,
                cancelled,
                _bytes: permit,
            })
            .is_err()
        {
            return false;
        }
        // Once dispatched, the actor owns the deadline and waits for host termination
        // before returning failure. Dropping this receiver never acknowledges work.
        receiver.await.unwrap_or(false)
    }
}

async fn collect(
    mut receiver: mpsc::Receiver<Pending>,
    source: String,
    config: QueueBatchConfig,
    control: Arc<ControlGrpcServer>,
    shutdown: CancellationToken,
) {
    let mut carry = None;
    loop {
        let first = match carry.take() {
            Some(pending) => pending,
            None => tokio::select! {
                _ = shutdown.cancelled() => break,
                pending = receiver.recv() => match pending { Some(pending) => pending, None => break },
            },
        };
        let mut deadline = first.deadline;
        let window = Instant::now() + Duration::from_secs(u64::from(config.max_window_seconds));
        let mut bytes = first.delivery.encoded_len() + 64;
        let mut pending = vec![first];
        while pending.len() < config.max_messages as usize {
            let next = tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep_until(window.min(deadline)) => break,
                next = receiver.recv() => match next { Some(next) => next, None => break },
            };
            let size = next.delivery.encoded_len() + 64;
            if bytes + size > MAX_BATCH_BYTES {
                carry = Some(next);
                break;
            }
            deadline = deadline.min(next.deadline);
            bytes += size;
            pending.push(next);
        }
        pending.retain(|entry| !entry.response.is_closed() && entry.deadline > Instant::now());
        if pending.is_empty() {
            continue;
        }
        // Expired or disconnected entries no longer constrain live deliveries.
        let deadline = pending
            .iter()
            .map(|entry| entry.deadline)
            .min()
            .expect("nonempty batch");
        let batch = QueueBatch {
            source: source.clone(),
            deliveries: pending.iter().map(|entry| entry.delivery.clone()).collect(),
        };
        let task = Task {
            task_id: uuid::Uuid::new_v4().to_string(),
            payload: Some(Payload::QueueBatch(batch.clone())),
        };
        let dispatched = std::sync::atomic::AtomicBool::new(false);
        let cancelled = futures_util::future::select_all(
            pending
                .iter()
                .map(|entry| Box::pin(entry.cancelled.cancelled())),
        );
        let outcome = tokio::select! {
            biased;
            _ = cancelled => {
                if dispatched.load(std::sync::atomic::Ordering::Relaxed) { control.cancel_application().await; }
                continue;
            },
            _ = shutdown.cancelled() => {
                if dispatched.load(std::sync::atomic::Ordering::Relaxed) { control.cancel_application().await; }
                return;
            },
            outcome = async {
                dispatched.store(true, std::sync::atomic::Ordering::Relaxed);
                control.send_task(task, deadline.saturating_duration_since(Instant::now())).await
            } => outcome,
        };
        let dispositions = outcome
            .ok()
            .and_then(|result| result.queue_batch)
            .and_then(|result| validate_outcomes(&batch, &result));
        for (index, entry) in pending.into_iter().enumerate() {
            let success = dispositions
                .as_ref()
                .is_some_and(|results| results[index] == Disposition::Success);
            // A disconnected HTTP request has no recipient and remains unacknowledged.
            let _ = entry.response.send(success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_worker_protocol::control::{
        control_service_server::ControlService, send_task_result_request, QueueBatchResult,
        QueueDeliveryOutcome, SendTaskResultRequest, WaitForTasksRequest,
    };
    use tokio_stream::StreamExt;
    use tonic::Request;

    #[tokio::test]
    async fn admission_bounds_bytes_across_sources_and_releases_cancelled_requests() {
        let control = Arc::new(ControlGrpcServer::new());
        let collectors = QueueCollectors::new(
            (0..4)
                .map(|i| {
                    (
                        i.to_string(),
                        QueueBatchConfig {
                            max_messages: 100,
                            max_window_seconds: 60,
                        },
                    )
                })
                .collect(),
            control,
        );
        let deadline = Instant::now() + Duration::from_secs(120);
        let message = |source: String, size| QueueMessage {
            source,
            payload: vec![0; size],
            ..Default::default()
        };
        assert!(
            !collectors.clients["0"]
                .deliver(message("0".into(), MAX_BATCH_BYTES), deadline)
                .await,
            "protobuf overhead must fit under the task bound"
        );
        let mut pending = Vec::new();
        for index in 0..3 {
            let source = index.to_string();
            let collector = collectors.clients[&source].clone();
            pending.push(tokio::spawn(async move {
                collector
                    .deliver(message(source, 2 * 1024 * 1024), deadline)
                    .await
            }));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while collectors.clients["0"].bytes.available_permits() >= 2 * 1024 * 1024 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("three requests admitted");
        assert!(
            pending.iter().all(|request| !request.is_finished()),
            "enqueue never acknowledges"
        );
        assert!(!tokio::time::timeout(
            Duration::from_secs(1),
            collectors.clients["3"].deliver(message("3".into(), 2 * 1024 * 1024), deadline)
        )
        .await
        .expect("saturation rejects immediately"));
        for request in pending {
            request.abort();
            assert!(request.await.expect_err("cancelled request").is_cancelled());
        }
        let bytes = collectors.clients["0"].bytes.clone();
        collectors
            .join()
            .await
            .expect("owned shutdown releases queued work");
        assert_eq!(bytes.available_permits(), MAX_PENDING_BYTES);
    }

    #[test]
    fn ambiguous_short_names_preserve_distinct_logical_and_qualified_routes() {
        let environment = std::collections::HashMap::from([
            (
                "ALIEN_QUEUE_BATCH_CONFIG".into(),
                r#"{"first":null,"second":null}"#.into(),
            ),
            (
                "ALIEN_FIRST_BINDING".into(),
                r#"{"queueName":"jobs","subscription":"projects/first/subscriptions/jobs"}"#.into(),
            ),
            (
                "ALIEN_SECOND_BINDING".into(),
                r#"{"queueName":"jobs","subscription":"projects/second/subscriptions/jobs"}"#
                    .into(),
            ),
        ]);
        let (_, aliases) = batch_routing(&environment).expect("distinct routes are valid");
        assert_eq!(aliases["first"], "first");
        assert_eq!(aliases["second"], "second");
        assert_eq!(aliases["projects/first/subscriptions/jobs"], "first");
        assert_eq!(aliases["projects/second/subscriptions/jobs"], "second");
        assert!(!aliases.contains_key("jobs"));
    }

    #[tokio::test(start_paused = true)]
    async fn expired_collection_entry_does_not_cancel_surviving_delivery() {
        let control = Arc::new(ControlGrpcServer::new());
        let mut tasks = control
            .wait_for_tasks(Request::new(WaitForTasksRequest::default()))
            .await
            .expect("subscribe")
            .into_inner();
        let collectors = QueueCollectors::new(
            std::collections::HashMap::from([(
                String::new(),
                QueueBatchConfig {
                    max_messages: 3,
                    max_window_seconds: 5,
                },
            )]),
            control.clone(),
        );
        let collector = collectors.clients[""].clone();
        let now = Instant::now();
        let expired = tokio::spawn({
            let collector = collector.clone();
            async move {
                collector
                    .deliver(QueueMessage::default(), now + Duration::from_secs(1))
                    .await
            }
        });
        let live = tokio::spawn(async move {
            collector
                .deliver(
                    QueueMessage {
                        id: "live".into(),
                        ..Default::default()
                    },
                    now + Duration::from_secs(60),
                )
                .await
        });
        let task = tasks.next().await.expect("stream").expect("live task");
        let Some(Payload::QueueBatch(batch)) = task.payload else {
            panic!("batch");
        };
        assert_eq!(batch.deliveries.len(), 1);
        assert_eq!(
            batch.deliveries[0].message.as_ref().expect("message").id,
            "live"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        control
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: task.task_id,
                result: Some(send_task_result_request::Result::QueueBatch(
                    QueueBatchResult {
                        outcomes: vec![QueueDeliveryOutcome {
                            delivery_id: batch.deliveries[0].delivery_id.clone(),
                            disposition: Disposition::Success as i32,
                        }],
                    },
                )),
            }))
            .await
            .expect("live result accepted");
        assert!(!expired.await.expect("expired waiter"));
        assert!(live.await.expect("live waiter"));
        collectors.join().await.expect("shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn window_flush_waits_for_result_and_keeps_delivery_outcomes_independent() {
        let control = Arc::new(ControlGrpcServer::new());
        let mut tasks = control
            .wait_for_tasks(Request::new(WaitForTasksRequest {
                application_id: "test".into(),
            }))
            .await
            .expect("subscribe")
            .into_inner();
        let collectors = QueueCollectors::new(
            std::collections::HashMap::from([(
                String::new(),
                QueueBatchConfig {
                    max_messages: 3,
                    max_window_seconds: 5,
                },
            )]),
            control.clone(),
        );
        let collector = collectors.clients.get("").expect("collector").clone();
        let deadline = Instant::now() + Duration::from_secs(60);
        let first = tokio::spawn({
            let collector = collector.clone();
            async move {
                collector
                    .deliver(
                        QueueMessage {
                            id: "same-id".into(),
                            ..Default::default()
                        },
                        deadline,
                    )
                    .await
            }
        });
        let second = tokio::spawn({
            let collector = collector.clone();
            async move {
                collector
                    .deliver(
                        QueueMessage {
                            id: "same-id".into(),
                            ..Default::default()
                        },
                        deadline,
                    )
                    .await
            }
        });
        assert!(tokio::time::timeout(Duration::from_secs(4), tasks.next())
            .await
            .is_err());
        assert!(!first.is_finished());
        assert!(!second.is_finished());
        let task = tasks.next().await.expect("window flush").expect("task");
        let Some(Payload::QueueBatch(batch)) = task.payload else {
            panic!("batch payload");
        };
        assert_eq!(batch.deliveries.len(), 2);
        assert_ne!(
            batch.deliveries[0].delivery_id,
            batch.deliveries[1].delivery_id
        );
        assert!(!first.is_finished());
        control
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: task.task_id,
                result: Some(send_task_result_request::Result::QueueBatch(
                    QueueBatchResult {
                        outcomes: batch
                            .deliveries
                            .iter()
                            .enumerate()
                            .map(|(index, delivery)| QueueDeliveryOutcome {
                                delivery_id: delivery.delivery_id.clone(),
                                disposition: if index == 0 {
                                    Disposition::Success
                                } else {
                                    Disposition::Retry
                                } as i32,
                            })
                            .collect(),
                    },
                )),
            }))
            .await
            .expect("result accepted");
        assert!(first.await.expect("first request"));
        assert!(!second.await.expect("second request"));
        collectors.join().await.expect("join collectors");
    }

    #[tokio::test(start_paused = true)]
    async fn full_batch_flushes_without_window_and_invalid_results_acknowledge_nothing() {
        let control = Arc::new(ControlGrpcServer::new());
        let mut tasks = control
            .wait_for_tasks(Request::new(WaitForTasksRequest {
                application_id: "test".into(),
            }))
            .await
            .expect("subscribe")
            .into_inner();
        let collectors = QueueCollectors::new(
            std::collections::HashMap::from([(
                String::new(),
                QueueBatchConfig {
                    max_messages: 1,
                    max_window_seconds: 30,
                },
            )]),
            control.clone(),
        );
        let collector = collectors.clients.get("").expect("collector").clone();
        let start = Instant::now();
        let request = tokio::spawn({
            let collector = collector.clone();
            async move {
                collector
                    .deliver(QueueMessage::default(), start + Duration::from_secs(60))
                    .await
            }
        });
        let task = tasks.next().await.expect("count flush").expect("task");
        assert_eq!(Instant::now(), start);
        control
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: task.task_id,
                result: Some(send_task_result_request::Result::QueueBatch(
                    QueueBatchResult { outcomes: vec![] },
                )),
            }))
            .await
            .expect("result accepted");
        assert!(!request.await.expect("request"));
        assert!(
            !collector
                .deliver(
                    QueueMessage {
                        payload: vec![0; MAX_BATCH_BYTES],
                        ..Default::default()
                    },
                    start + Duration::from_secs(60)
                )
                .await
        );
        assert!(!collector.deliver(QueueMessage::default(), start).await);
        assert_eq!(collector.bytes.available_permits(), MAX_PENDING_BYTES);
        collectors.join().await.expect("join collectors");
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn disconnected_dispatched_request_and_shutdown_wait_for_process_stop() {
        let control = Arc::new(ControlGrpcServer::new());
        control.supervise_task_cancellation();
        let mut tasks = control
            .wait_for_tasks(Request::new(WaitForTasksRequest {
                application_id: "test".into(),
            }))
            .await
            .expect("subscribe")
            .into_inner();
        let collectors = QueueCollectors::new(
            std::collections::HashMap::from([(
                "jobs".into(),
                QueueBatchConfig {
                    max_messages: 2,
                    max_window_seconds: 10,
                },
            )]),
            control.clone(),
        );
        let client = collectors.clients["jobs"].clone();
        let mut child = tokio::process::Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .expect("application");
        let deadline = Instant::now() + Duration::from_secs(30);
        let first = tokio::spawn({
            let client = client.clone();
            async move {
                client
                    .deliver(
                        QueueMessage {
                            source: "jobs".into(),
                            ..Default::default()
                        },
                        deadline,
                    )
                    .await
            }
        });
        let second = tokio::spawn(async move {
            client
                .deliver(
                    QueueMessage {
                        source: "jobs".into(),
                        ..Default::default()
                    },
                    deadline,
                )
                .await
        });
        let task = tasks.next().await.expect("dispatched").expect("task");
        assert!(matches!(task.payload, Some(Payload::QueueBatch(_))));
        first.abort();
        assert!(first
            .await
            .expect_err("request disconnected")
            .is_cancelled());
        tokio::time::timeout(Duration::from_secs(2), control.wait_for_cancelled_task())
            .await
            .expect("cancellation reaches host");
        let join = tokio::spawn(collectors.join());
        tokio::task::yield_now().await;
        assert!(
            !second.is_finished(),
            "failure response must wait for application stop"
        );
        assert!(
            !join.is_finished(),
            "collector shutdown must join dispatched work"
        );
        assert!(child.try_wait().expect("child status").is_none());
        child.kill().await.expect("stop application");
        control.application_stopped();
        assert!(!second.await.expect("remaining delivery"));
        tokio::time::timeout(Duration::from_secs(2), join)
            .await
            .expect("bounded join")
            .expect("collector task")
            .expect("collector shutdown");
        assert!(child.try_wait().expect("child reaped").is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn sources_have_distinct_windows_and_queued_cancellation_does_not_stop_host() {
        let control = Arc::new(ControlGrpcServer::new());
        let mut tasks = control
            .wait_for_tasks(Request::new(WaitForTasksRequest {
                application_id: "test".into(),
            }))
            .await
            .expect("subscribe")
            .into_inner();
        let collectors = QueueCollectors::new(
            std::collections::HashMap::from([
                (
                    "slow".into(),
                    QueueBatchConfig {
                        max_messages: 10,
                        max_window_seconds: 20,
                    },
                ),
                (
                    "fast".into(),
                    QueueBatchConfig {
                        max_messages: 1,
                        max_window_seconds: 0,
                    },
                ),
            ]),
            control.clone(),
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        let slow = collectors.clients["slow"].clone();
        assert!(
            !slow
                .deliver(
                    QueueMessage {
                        source: "fast".into(),
                        ..Default::default()
                    },
                    deadline
                )
                .await
        );
        let queued = tokio::spawn(async move {
            slow.deliver(
                QueueMessage {
                    source: "slow".into(),
                    ..Default::default()
                },
                deadline,
            )
            .await
        });
        tokio::task::yield_now().await;
        queued.abort();
        assert!(queued
            .await
            .expect_err("queued request disconnected")
            .is_cancelled());
        let fast = collectors.clients["fast"].clone();
        let active = tokio::spawn(async move {
            fast.deliver(
                QueueMessage {
                    source: "fast".into(),
                    ..Default::default()
                },
                deadline,
            )
            .await
        });
        let task = tasks
            .next()
            .await
            .expect("fast source dispatched")
            .expect("task");
        let Some(Payload::QueueBatch(batch)) = task.payload else {
            panic!("batch");
        };
        assert_eq!(batch.source, "fast");
        assert_eq!(batch.deliveries.len(), 1);
        assert_eq!(
            batch.deliveries[0]
                .message
                .as_ref()
                .expect("message")
                .source,
            "fast"
        );
        control
            .send_task_result(Request::new(SendTaskResultRequest {
                task_id: task.task_id,
                result: Some(send_task_result_request::Result::QueueBatch(
                    QueueBatchResult {
                        outcomes: vec![QueueDeliveryOutcome {
                            delivery_id: batch.deliveries[0].delivery_id.clone(),
                            disposition: Disposition::Success as i32,
                        }],
                    },
                )),
            }))
            .await
            .expect("completion");
        assert!(active.await.expect("fast request"));
        assert!(
            tokio::time::timeout(Duration::from_secs(21), tasks.next())
                .await
                .is_err(),
            "cancelled queued delivery must never dispatch"
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), control.wait_for_cancelled_task())
                .await
                .is_err(),
            "queued cancellation must not kill application"
        );
        collectors.join().await.expect("join");
    }
}
