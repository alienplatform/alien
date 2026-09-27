//! Namespace-scoped Kubernetes Pod log collection for pull deployments.
//!
//! The API stream runs inside the Operator. It reads only Pods carrying the
//! configured label in the installation namespace, then commits each bounded
//! OTLP batch and its resume cursor to the encrypted local database together.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_k8s_clients::{
    kubernetes::kubernetes_request_utils::KubernetesRequestSigner, KubernetesClient,
    KubernetesClientConfig, KubernetesClientConfigExt,
};
use chrono::{DateTime, SecondsFormat, Utc};
use k8s_openapi::api::core::v1::Pod;
use tokio::{task::JoinHandle, time};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{
    collector_logs::{pod_log_records_to_otlp, PodLogRecord},
    db::{OperatorDb, PodLogOffset},
    error::{ErrorData, Result},
    OperatorState,
};

const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_MAX_STREAMS: usize = 32;
const HARD_MAX_STREAMS: usize = 256;
const MAX_LINE_BYTES: usize = 256 * 1024;
const MAX_BATCH_RECORDS: usize = 100;
const MAX_BATCH_BYTES: usize = 256 * 1024;

/// Values set by the reviewed Helm chart or standalone Operator manifest.
#[derive(Debug, Clone)]
pub struct PodLogCollectionConfig {
    pub label_key: String,
    pub label_value: String,
    pub legacy_daemonset: Option<String>,
    pub max_streams: usize,
}

pub fn config_from_env() -> Result<Option<PodLogCollectionConfig>> {
    let key = env_value("OPERATOR_POD_LOG_LABEL_KEY")?;
    let value = env_value("OPERATOR_POD_LOG_LABEL_VALUE")?;
    let legacy_daemonset = env_value("OPERATOR_POD_LOG_LEGACY_DAEMONSET")?;
    let max_streams = match env_value("OPERATOR_POD_LOG_MAX_STREAMS")? {
        Some(value) => value.parse::<usize>().map_err(|_| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "OPERATOR_POD_LOG_MAX_STREAMS must be between 1 and {HARD_MAX_STREAMS}"
                ),
            })
        })?,
        None => DEFAULT_MAX_STREAMS,
    };
    if max_streams == 0 || max_streams > HARD_MAX_STREAMS {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "OPERATOR_POD_LOG_MAX_STREAMS must be between 1 and {HARD_MAX_STREAMS}"
            ),
        }));
    }
    match (key, value) {
        (None, None) if legacy_daemonset.is_none() => Ok(None),
        (Some(label_key), Some(label_value))
            if valid_label_key(&label_key) && valid_label_value(&label_value) =>
        {
            Ok(Some(PodLogCollectionConfig {
                label_key,
                label_value,
                legacy_daemonset,
                max_streams,
            }))
        }
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Pod log collection requires a valid, non-empty label key and value"
                .to_string(),
        })),
    }
}

fn env_value(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!("{name} must be valid UTF-8"),
            }))
        }
    }
}

fn valid_label_key(key: &str) -> bool {
    let (prefix, name) = key.split_once('/').unwrap_or(("", key));
    valid_label_component(name)
        && (prefix.is_empty() && !key.contains('/')
            || (!prefix.is_empty()
                && prefix.len() <= 253
                && prefix.split('.').all(|part| {
                    !part.is_empty()
                        && part.len() <= 63
                        && part
                            .bytes()
                            .next()
                            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                        && part
                            .bytes()
                            .last()
                            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                        && part.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                        })
                })))
}

fn valid_label_value(value: &str) -> bool {
    valid_label_component(value)
}

fn valid_label_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Source {
    pod_uid: String,
    pod_name: String,
    container: String,
    restart_count: i32,
    terminal: bool,
    initial_since: DateTime<Utc>,
}

struct StreamTask {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

pub async fn run_loop(state: Arc<OperatorState>, config: PodLogCollectionConfig) {
    let namespace = match state.config.namespace.as_deref() {
        Some(namespace) => namespace.to_string(),
        None => {
            warn!("Pod log collection requires a Kubernetes namespace");
            state.cancel.cancelled().await;
            return;
        }
    };
    if !state.config.is_telemetry_enabled() {
        warn!("Pod log collection is enabled but deployment telemetry is off");
        state.cancel.cancelled().await;
        return;
    }

    let mut tasks = HashMap::<Source, StreamTask>::new();
    let mut completed = HashSet::<Source>::new();
    let mut client = None;
    let mut legacy_was_running = false;
    info!(namespace, label_key = %config.label_key, label_value = %config.label_value,
        max_streams = config.max_streams, "Starting Kubernetes Pod log collection");

    loop {
        if client.is_none() {
            match KubernetesClientConfig::try_incluster().await {
                Ok(client_config) => match KubernetesClient::new(client_config).await {
                    Ok(connected) => client = Some(connected),
                    Err(error) => {
                        warn!(error = %error, "Could not create Pod log Kubernetes client")
                    }
                },
                Err(error) => {
                    warn!(error = %error, "Could not load in-cluster Pod log credentials")
                }
            }
        }
        let result = match client.as_ref() {
            Some(client) => {
                reconcile(
                    &state,
                    client,
                    &namespace,
                    &config,
                    &mut legacy_was_running,
                    &mut tasks,
                    &mut completed,
                )
                .await
            }
            None => Ok(()),
        };
        if let Err(error) = result {
            stop_all(&mut tasks).await;
            warn!(error = %error, "Pod log discovery failed; will retry");
        }
        tokio::select! {
            _ = state.cancel.cancelled() => break,
            _ = time::sleep(DISCOVERY_INTERVAL) => {}
        }
    }
    stop_all(&mut tasks).await;
    info!("Kubernetes Pod log collection stopped");
}

async fn reconcile(
    state: &Arc<OperatorState>,
    client: &KubernetesClient,
    namespace: &str,
    config: &PodLogCollectionConfig,
    legacy_was_running: &mut bool,
    tasks: &mut HashMap<Source, StreamTask>,
    completed: &mut HashSet<Source>,
) -> Result<()> {
    let selector = format!("{}={}", config.label_key, config.label_value);
    if let Some(legacy_daemonset) = config.legacy_daemonset.as_deref() {
        // Capture this time before querying the API. The database keeps the
        // first observation across discovery passes and Operator restarts.
        let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
        if legacy_collector_running(client, namespace, legacy_daemonset).await? {
            stop_all(tasks).await;
            completed.clear();
            state
                .db
                .observe_legacy_pod_log_collector(&selector, &observed_at)
                .await?;
            if !*legacy_was_running {
                info!(
                    legacy_daemonset,
                    "Waiting for old node collector to stop before reading Pod logs"
                );
            }
            *legacy_was_running = true;
            return Ok(());
        }
        if *legacy_was_running {
            info!(
                legacy_daemonset,
                "Old node collector stopped; reading selected Pod logs"
            );
            *legacy_was_running = false;
        }
        state
            .db
            .mark_legacy_pod_log_collector_gone(&selector)
            .await?;
    }

    let deployment_id = match state.db.get_deployment_id().await? {
        Some(id) => id,
        None => {
            stop_all(tasks).await;
            return Ok(());
        }
    };
    let pods = client
        .list_pods(namespace, Some(selector.clone()), None)
        .await
        .context(ErrorData::PodLogReadFailed {
            message: "could not list selected Pods".to_string(),
        })?;
    let cutover = state
        .db
        .pod_log_cutover_at(&selector, "1970-01-01T00:00:00Z")
        .await?;
    let cutover = DateTime::parse_from_rfc3339(&cutover)
        .into_alien_error()
        .context(ErrorData::PodLogReadFailed {
            message: "stored Pod log handoff has an invalid timestamp".to_string(),
        })?
        .with_timezone(&Utc);
    let mut desired = BTreeSet::new();
    for pod in &pods.items {
        desired.extend(selected_sources(pod, config, cutover.clone()));
    }
    completed.retain(|source| desired.contains(source));
    desired.retain(|source| !completed.contains(source));
    if desired.len() > config.max_streams {
        warn!(
            selected = desired.len(),
            max_streams = config.max_streams,
            "Selected containers exceed Pod log stream limit; some logs are not being collected"
        );
    }
    let desired: BTreeSet<_> = desired.into_iter().take(config.max_streams).collect();
    let stale: Vec<_> = tasks
        .iter()
        .filter(|(source, task)| !desired.contains(*source) || task.handle.is_finished())
        .map(|(source, _)| source.clone())
        .collect();
    for source in stale {
        let task = tasks.remove(&source).expect("stale task is present");
        let finished = task.handle.is_finished();
        task.cancel.cancel();
        let _ = task.handle.await;
        if source.terminal && desired.contains(&source) && finished {
            completed.insert(source);
        }
    }
    for source in desired {
        if completed.contains(&source) || tasks.contains_key(&source) {
            continue;
        }
        let task_cancel = state.cancel.child_token();
        let handle = tokio::spawn({
            let client = client.clone();
            let db = state.db.clone();
            let namespace = namespace.to_string();
            let deployment_id = deployment_id.clone();
            let source = source.clone();
            let cancel = task_cancel.clone();
            async move {
                stream_source(client, db, namespace, deployment_id, source, cancel).await;
            }
        });
        tasks.insert(
            source,
            StreamTask {
                cancel: task_cancel,
                handle,
            },
        );
    }
    Ok(())
}

fn selected_sources(
    pod: &Pod,
    config: &PodLogCollectionConfig,
    cutover: DateTime<Utc>,
) -> Vec<Source> {
    let labels = pod.metadata.labels.as_ref();
    let phase = pod
        .status
        .as_ref()
        .and_then(|status| status.phase.as_deref());
    if labels.and_then(|labels| labels.get(&config.label_key)) != Some(&config.label_value)
        || labels
            .and_then(|labels| labels.get("alien.dev/log-collector-exclude"))
            .is_some_and(|value| value == "true")
        || !matches!(phase, Some("Running" | "Succeeded" | "Failed"))
    {
        return Vec::new();
    }
    let (Some(pod_uid), Some(pod_name), Some(spec)) = (
        pod.metadata.uid.as_ref(),
        pod.metadata.name.as_ref(),
        pod.spec.as_ref(),
    ) else {
        return Vec::new();
    };
    let initial_since = pod
        .metadata
        .creation_timestamp
        .as_ref()
        .map(|created| created.0.clone())
        .unwrap_or(cutover.clone())
        .max(cutover);
    spec.containers
        .iter()
        .map(|container| {
            let restart_count = pod
                .status
                .as_ref()
                .and_then(|status| status.container_statuses.as_ref())
                .and_then(|statuses| statuses.iter().find(|status| status.name == container.name))
                .map_or(0, |status| status.restart_count);
            Source {
                pod_uid: pod_uid.clone(),
                pod_name: pod_name.clone(),
                container: container.name.clone(),
                restart_count,
                terminal: phase != Some("Running"),
                initial_since: initial_since.clone(),
            }
        })
        .collect()
}

async fn legacy_collector_running(
    client: &KubernetesClient,
    namespace: &str,
    daemonset_name: &str,
) -> Result<bool> {
    let daemonsets = client
        .list_daemonsets(
            namespace,
            None,
            Some(format!("metadata.name={daemonset_name}")),
        )
        .await
        .context(ErrorData::PodLogReadFailed {
            message: "could not check the old collector DaemonSet".to_string(),
        })?;
    if daemonsets
        .items
        .iter()
        .any(|daemonset| daemonset.metadata.name.as_deref() == Some(daemonset_name))
    {
        return Ok(true);
    }
    let pods = client
        .list_pods(
            namespace,
            Some(
                "app.kubernetes.io/component in (log-collector,whitelabeled-log-collector)"
                    .to_string(),
            ),
            None,
        )
        .await
        .context(ErrorData::PodLogReadFailed {
            message: "could not check old collector Pods".to_string(),
        })?;
    Ok(pods.items.iter().any(|pod| {
        pod.metadata
            .owner_references
            .as_ref()
            .is_some_and(|owners| {
                owners
                    .iter()
                    .any(|owner| owner.kind == "DaemonSet" && owner.name == daemonset_name)
            })
    }))
}

async fn stop_all(tasks: &mut HashMap<Source, StreamTask>) {
    for task in tasks.values() {
        task.cancel.cancel();
    }
    for task in tasks.drain().map(|(_, task)| task) {
        let _ = task.handle.await;
    }
}

async fn stream_source(
    client: KubernetesClient,
    db: Arc<OperatorDb>,
    namespace: String,
    deployment_id: String,
    source: Source,
    cancel: CancellationToken,
) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        match read_stream_once(&client, &db, &namespace, &deployment_id, &source, &cancel).await {
            Ok(()) if source.terminal => return,
            Ok(()) => {}
            Err(error) => {
                if !cancel.is_cancelled() {
                    warn!(error = %error, pod = %source.pod_name, container = %source.container,
                        "Pod log stream failed; will reconnect");
                }
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = time::sleep(RECONNECT_DELAY) => {}
        }
    }
}

async fn read_stream_once(
    client: &KubernetesClient,
    db: &OperatorDb,
    namespace: &str,
    deployment_id: &str,
    source: &Source,
    cancel: &CancellationToken,
) -> Result<()> {
    let saved = db
        .get_pod_log_offset(&source.pod_uid, &source.container, source.restart_count)
        .await?;
    let saved_time = saved
        .as_ref()
        .map(|offset| DateTime::parse_from_rfc3339(&offset.timestamp))
        .transpose()
        .into_alien_error()
        .context(ErrorData::PodLogReadFailed {
            message: "stored Pod log cursor has an invalid timestamp".to_string(),
        })?
        .map(|value| value.with_timezone(&Utc));
    let replay_time = Some(
        saved_time
            .unwrap_or_else(|| source.initial_since.clone())
            .max(source.initial_since.clone()),
    );
    let url = format!(
        "{}/api/v1/namespaces/{}/pods/{}/log",
        client.get_base_url().trim_end_matches('/'),
        urlencoding::encode(namespace),
        urlencoding::encode(&source.pod_name)
    );
    let mut request = client.client().get(&url).query(&[
        ("container", source.container.as_str()),
        ("follow", if source.terminal { "false" } else { "true" }),
        ("timestamps", "true"),
    ]);
    if let Some(timestamp) = replay_time {
        let since = timestamp
            .checked_sub_signed(chrono::Duration::nanoseconds(1))
            .unwrap_or(timestamp)
            .to_rfc3339_opts(SecondsFormat::Nanos, true);
        request = request.query(&[("sinceTime", since)]);
    }
    let mut response = request
        .sign_kubernetes_request(&client.auth_config())
        .context(ErrorData::PodLogReadFailed {
            message: "could not authenticate Pod log request".to_string(),
        })?
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::PodLogReadFailed {
            message: "Kubernetes Pod log request failed".to_string(),
        })?;
    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::PodLogReadFailed {
            message: format!(
                "Kubernetes returned HTTP {} for the selected Pod",
                response.status()
            ),
        }));
    }

    let mut batch = Vec::new();
    let mut batch_bytes = 0;
    let mut partial = Vec::new();
    let mut truncated = false;
    let mut last_time = None;
    let mut ordinal = 0_i64;
    let mut offset = saved.clone();
    let mut flush_interval = time::interval(Duration::from_secs(1));
    flush_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                flush_batch(db, deployment_id, source, &mut batch, offset.as_ref()).await?;
                return Ok(());
            },
            _ = flush_interval.tick() => {
                flush_batch(db, deployment_id, source, &mut batch, offset.as_ref()).await?;
                batch_bytes = 0;
            }
            chunk = response.chunk() => {
                let chunk = chunk.into_alien_error().context(ErrorData::PodLogReadFailed {
                    message: "Kubernetes Pod log stream ended with an error".to_string(),
                })?;
                let Some(chunk) = chunk else {
                    if !partial.is_empty() {
                        if let Some(record) = parse_line(
                            &partial, truncated, namespace, source, replay_time,
                            saved.as_ref(), &mut last_time, &mut ordinal, &mut offset,
                        ) {
                            batch.push(record);
                        }
                    }
                    flush_batch(db, deployment_id, source, &mut batch, offset.as_ref()).await?;
                    return Ok(());
                };
                for byte in chunk.iter().copied() {
                    if byte == b'\n' {
                        if let Some(record) = parse_line(
                            &partial, truncated, namespace, source, replay_time,
                            saved.as_ref(), &mut last_time, &mut ordinal, &mut offset,
                        ) {
                            batch_bytes += record.body.len();
                            batch.push(record);
                        }
                        partial.clear();
                        truncated = false;
                        if batch.len() >= MAX_BATCH_RECORDS || batch_bytes >= MAX_BATCH_BYTES {
                            flush_batch(db, deployment_id, source, &mut batch, offset.as_ref()).await?;
                            batch_bytes = 0;
                        }
                    } else if partial.len() < MAX_LINE_BYTES {
                        partial.push(byte);
                    } else {
                        truncated = true;
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_line(
    line: &[u8],
    truncated: bool,
    namespace: &str,
    source: &Source,
    replay_time: Option<DateTime<Utc>>,
    saved: Option<&PodLogOffset>,
    last_time: &mut Option<DateTime<Utc>>,
    ordinal: &mut i64,
    offset: &mut Option<PodLogOffset>,
) -> Option<PodLogRecord> {
    let text = String::from_utf8_lossy(line);
    let (timestamp, body) = text.split_once(' ')?;
    let timestamp = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Utc);
    if *last_time == Some(timestamp) {
        *ordinal += 1;
    } else {
        *last_time = Some(timestamp);
        *ordinal = 1;
    }
    if replay_time.is_some_and(|replay| timestamp < replay)
        || saved.is_some_and(|saved| {
            saved.timestamp == timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
                && *ordinal <= saved.ordinal
        })
    {
        return None;
    }
    if truncated {
        warn!(pod = %source.pod_name, container = %source.container,
            max_bytes = MAX_LINE_BYTES, "Truncated oversized Pod log line");
    }
    *offset = Some(PodLogOffset {
        timestamp: timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true),
        ordinal: *ordinal,
    });
    Some(PodLogRecord {
        namespace: namespace.to_string(),
        pod: source.pod_name.clone(),
        container: source.container.clone(),
        timestamp_unix_nanos: timestamp.timestamp_nanos_opt().unwrap_or_default().max(0) as u64,
        body: if truncated {
            format!("{body} [truncated]")
        } else {
            body.to_string()
        },
    })
}

async fn flush_batch(
    db: &OperatorDb,
    deployment_id: &str,
    source: &Source,
    batch: &mut Vec<PodLogRecord>,
    offset: Option<&PodLogOffset>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let offset = offset.expect("a non-empty Pod log batch has a cursor");
    let encoded = pod_log_records_to_otlp(std::mem::take(batch), deployment_id)?;
    db.store_pod_log_batch(
        &source.pod_uid,
        &source.container,
        source.restart_count,
        offset,
        &encoded,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_k8s_clients::KubernetesClientConfig;
    use axum::{extract::Query, routing::get, Router};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodStatus};
    use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
    use prost::Message;

    #[tokio::test]
    async fn terminal_pod_api_logs_queue_once_across_reader_restart() {
        let app = Router::new().route(
            "/api/v1/namespaces/demo/pods/app-123/log",
            get(|Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(query.get("container").map(String::as_str), Some("app"));
                assert_eq!(query.get("follow").map(String::as_str), Some("false"));
                assert_eq!(query.get("timestamps").map(String::as_str), Some("true"));
                assert!(query.contains_key("sinceTime"));
                "2026-09-27T09:00:00.000000001Z {\"time\":\"2026-09-27T09:00:00Z\",\"level\":\"INFO\",\"msg\":\"first\"}\n2026-09-27T09:00:00.000000001Z {\"time\":\"2026-09-27T09:00:00Z\",\"level\":\"INFO\",\"msg\":\"second\"}"
            }),
        );
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind fake Kubernetes API");
        let address = listener.local_addr().expect("fake Kubernetes address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve fake Kubernetes API");
        });
        let client = KubernetesClient::new(KubernetesClientConfig::Manual {
            server_url: format!("http://{address}"),
            certificate_authority_data: None,
            insecure_skip_tls_verify: None,
            client_certificate_data: None,
            client_key_data: None,
            token: Some("test-token".to_string()),
            username: None,
            password: None,
            namespace: Some("demo".to_string()),
            additional_headers: HashMap::new(),
        })
        .await
        .expect("create fake Kubernetes client");
        let data_dir = tempfile::tempdir().expect("temporary encrypted state");
        let db = OperatorDb::new(
            data_dir.path().to_str().unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .await
        .expect("open encrypted state");
        let source = Source {
            pod_uid: "uid-123".to_string(),
            pod_name: "app-123".to_string(),
            container: "app".to_string(),
            restart_count: 0,
            terminal: true,
            initial_since: DateTime::parse_from_rfc3339("2026-09-27T08:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        };
        let cancel = CancellationToken::new();
        read_stream_once(&client, &db, "demo", "dep_test", &source, &cancel)
            .await
            .expect("read completed Pod logs");
        let queued = db
            .get_pending_telemetry(10)
            .await
            .expect("read queued logs");
        assert_eq!(queued.len(), 1);
        let otlp = ExportLogsServiceRequest::decode(queued[0].2.as_slice())
            .expect("decode queued Pod logs");
        let records = &otlp.resource_logs[0].scope_logs[0].log_records;
        assert_eq!(records.len(), 2);
        assert_eq!(
            db.get_pod_log_offset("uid-123", "app", 0)
                .await
                .unwrap()
                .unwrap()
                .ordinal,
            2
        );
        read_stream_once(&client, &db, "demo", "dep_test", &source, &cancel)
            .await
            .expect("replay completed Pod logs");
        assert_eq!(db.get_pending_telemetry(10).await.unwrap().len(), 1);
        server.abort();
    }

    #[test]
    fn only_selected_workload_pods_are_read() {
        let cutover = Utc::now();
        let config = PodLogCollectionConfig {
            label_key: "example.com/deployment".to_string(),
            label_value: "release-one".to_string(),
            legacy_daemonset: None,
            max_streams: 32,
        };
        let mut pod = Pod::default();
        pod.metadata.uid = Some("uid-one".to_string());
        pod.metadata.name = Some("api-123".to_string());
        pod.metadata.labels = Some(std::collections::BTreeMap::from([(
            config.label_key.clone(),
            config.label_value.clone(),
        )]));
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "api".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        pod.status = Some(PodStatus {
            phase: Some("Running".to_string()),
            ..Default::default()
        });
        let selected = selected_sources(&pod, &config, cutover);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].pod_uid, "uid-one");
        assert_eq!(selected[0].container, "api");

        pod.metadata
            .labels
            .as_mut()
            .unwrap()
            .insert(config.label_key.clone(), "other-release".to_string());
        assert!(selected_sources(&pod, &config, cutover).is_empty());
        pod.metadata
            .labels
            .as_mut()
            .unwrap()
            .insert(config.label_key.clone(), config.label_value.clone());
        pod.metadata.labels.as_mut().unwrap().insert(
            "alien.dev/log-collector-exclude".to_string(),
            "true".to_string(),
        );
        assert!(selected_sources(&pod, &config, cutover).is_empty());
        pod.metadata
            .labels
            .as_mut()
            .unwrap()
            .remove("alien.dev/log-collector-exclude");
        pod.status.as_mut().unwrap().phase = Some("Succeeded".to_string());
        assert!(selected_sources(&pod, &config, cutover)
            .iter()
            .all(|source| source.terminal));
        pod.status.as_mut().unwrap().phase = Some("Failed".to_string());
        assert!(selected_sources(&pod, &config, cutover)
            .iter()
            .all(|source| source.terminal));
        pod.status.as_mut().unwrap().phase = Some("Pending".to_string());
        assert!(selected_sources(&pod, &config, cutover).is_empty());
    }

    #[test]
    fn parses_timestamped_lines_and_replay_ordinals() {
        let source = Source {
            pod_uid: "uid".to_string(),
            pod_name: "app-123".to_string(),
            container: "app".to_string(),
            restart_count: 0,
            terminal: false,
            initial_since: Utc::now(),
        };
        let mut last_time = None;
        let mut ordinal = 0;
        let mut offset = None;
        let first = parse_line(
            b"2026-09-27T00:00:00.000000001Z first",
            false,
            "demo",
            &source,
            None,
            None,
            &mut last_time,
            &mut ordinal,
            &mut offset,
        )
        .expect("first line");
        assert_eq!(first.body, "first");
        let saved = offset.clone().expect("cursor");
        let replay_time = DateTime::parse_from_rfc3339(&saved.timestamp)
            .expect("timestamp")
            .with_timezone(&Utc);
        last_time = None;
        ordinal = 0;
        assert!(parse_line(
            b"2026-09-27T00:00:00.000000001Z first",
            false,
            "demo",
            &source,
            Some(replay_time),
            Some(&saved),
            &mut last_time,
            &mut ordinal,
            &mut offset,
        )
        .is_none());
        let next = parse_line(
            b"2026-09-27T00:00:00.000000001Z second",
            false,
            "demo",
            &source,
            Some(replay_time),
            Some(&saved),
            &mut last_time,
            &mut ordinal,
            &mut offset,
        )
        .expect("second same-timestamp line");
        assert_eq!(next.body, "second");
        assert_eq!(offset.expect("cursor").ordinal, 2);
    }
}
