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
const DEFAULT_CLUSTER_MAX_STREAMS: usize = 200;
/// Cluster discovery is heavier than a single namespace, so it runs less often.
const CLUSTER_DISCOVERY_INTERVAL: Duration = Duration::from_secs(30);
const CLUSTER_PAGE_SIZE: &str = "500";
/// When more containers are eligible than streams are allowed, the set read
/// moves on this often, so every container is read in turn.
const CLUSTER_ROTATION_SECONDS: i64 = 300;
const DEFAULT_EXCLUDED_NAMESPACES: [&str; 3] = ["kube-system", "kube-public", "kube-node-lease"];
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
    pub replacement_daemonset: Option<String>,
    pub max_streams: usize,
    /// Read Pods of the whole cluster instead of one label in the installation
    /// namespace. Unset keeps the namespace-scoped behavior.
    pub cluster: Option<ClusterLogScope>,
}

/// Which Pods a cluster-scope collection reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterLogScope {
    /// Only these namespaces; empty means every namespace not excluded.
    pub namespaces: Vec<String>,
    /// Namespaces never read.
    pub exclude_namespaces: Vec<String>,
    /// Kubernetes label selector Pods must match.
    pub pod_selector: Option<String>,
}

impl ClusterLogScope {
    fn includes(&self, namespace: &str) -> bool {
        (self.namespaces.is_empty() || self.namespaces.iter().any(|n| n == namespace))
            && !self.exclude_namespaces.iter().any(|n| n == namespace)
    }
}

fn env_list(name: &str) -> Result<Option<Vec<String>>> {
    Ok(env_value(name)?.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect()
    }))
}

/// The namespaces never read: the system ones unless they are included, plus
/// the installer's own list.
fn cluster_excluded_namespaces(extra: Vec<String>, include_system: bool) -> Vec<String> {
    let mut excluded: Vec<String> = if include_system {
        Vec::new()
    } else {
        DEFAULT_EXCLUDED_NAMESPACES
            .iter()
            .map(|n| n.to_string())
            .collect()
    };
    for namespace in extra {
        if !excluded.contains(&namespace) {
            excluded.push(namespace);
        }
    }
    excluded
}

/// `OPERATOR_POD_LOG_SCOPE=cluster` selects cluster scope.
/// `OPERATOR_POD_LOG_EXCLUDE_NAMESPACES` adds to the system namespaces that are
/// always left out; `OPERATOR_POD_LOG_INCLUDE_SYSTEM_NAMESPACES=true` turns
/// those defaults off.
fn cluster_scope_from_env() -> Result<Option<ClusterLogScope>> {
    match env_value("OPERATOR_POD_LOG_SCOPE")?.as_deref() {
        None | Some("namespace") => Ok(None),
        Some("cluster") => Ok(Some(ClusterLogScope {
            namespaces: env_list("OPERATOR_POD_LOG_NAMESPACES")?.unwrap_or_default(),
            exclude_namespaces: cluster_excluded_namespaces(
                env_list("OPERATOR_POD_LOG_EXCLUDE_NAMESPACES")?.unwrap_or_default(),
                env_value("OPERATOR_POD_LOG_INCLUDE_SYSTEM_NAMESPACES")?
                    .is_some_and(|value| value.eq_ignore_ascii_case("true")),
            ),
            pod_selector: env_value("OPERATOR_POD_LOG_POD_SELECTOR")?,
        })),
        Some(_) => Err(AlienError::new(ErrorData::ConfigurationError {
            message: "OPERATOR_POD_LOG_SCOPE must be `namespace` or `cluster`".to_string(),
        })),
    }
}

pub fn config_from_env() -> Result<Option<PodLogCollectionConfig>> {
    let key = env_value("OPERATOR_POD_LOG_LABEL_KEY")?;
    let value = env_value("OPERATOR_POD_LOG_LABEL_VALUE")?;
    let legacy_daemonset = env_value("OPERATOR_POD_LOG_LEGACY_DAEMONSET")?;
    let replacement_daemonset = env_value("OPERATOR_POD_LOG_REPLACEMENT_DAEMONSET")?;
    let cluster = cluster_scope_from_env()?;
    let default_streams = if cluster.is_some() {
        DEFAULT_CLUSTER_MAX_STREAMS
    } else {
        DEFAULT_MAX_STREAMS
    };
    let max_streams = match env_value("OPERATOR_POD_LOG_MAX_STREAMS")? {
        Some(value) => value.parse::<usize>().map_err(|_| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "OPERATOR_POD_LOG_MAX_STREAMS must be between 1 and {HARD_MAX_STREAMS}"
                ),
            })
        })?,
        None => default_streams,
    };
    if max_streams == 0 || max_streams > HARD_MAX_STREAMS {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "OPERATOR_POD_LOG_MAX_STREAMS must be between 1 and {HARD_MAX_STREAMS}"
            ),
        }));
    }
    if let Some(cluster) = cluster {
        return Ok(Some(PodLogCollectionConfig {
            label_key: String::new(),
            label_value: String::new(),
            legacy_daemonset: None,
            replacement_daemonset: None,
            max_streams,
            cluster: Some(cluster),
        }));
    }
    match (key, value) {
        (None, None) if legacy_daemonset.is_none() && replacement_daemonset.is_none() => Ok(None),
        (Some(label_key), Some(label_value))
            if valid_label_key(&label_key) && valid_label_value(&label_value) =>
        {
            Ok(Some(PodLogCollectionConfig {
                label_key,
                label_value,
                legacy_daemonset,
                replacement_daemonset,
                max_streams,
                cluster: None,
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
    /// Cluster scope: a saved cursor of this container instance wins over
    /// `initial_since`, which only applies to a container read for the first time.
    prefer_cursor: bool,
}

struct StreamTask {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

pub async fn run_loop(state: Arc<OperatorState>, config: PodLogCollectionConfig) {
    // New streams begin at operator start: a restart does not replay old logs.
    let started_at = Utc::now();
    let namespace = match state.config.namespace.as_deref() {
        Some(namespace) => namespace.to_string(),
        None if config.cluster.is_some() => String::new(),
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
    let mut coverage = None;
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
                    started_at,
                    &mut coverage,
                )
                .await
            }
            None => Ok(()),
        };
        if let Err(error) = result {
            // A failed cluster listing keeps the streams already running.
            if config.cluster.is_none() {
                stop_all(&mut tasks).await;
            }
            warn!(error = %error, "Pod log discovery failed; will retry");
        }
        let interval = if config.cluster.is_some() {
            CLUSTER_DISCOVERY_INTERVAL
        } else {
            DISCOVERY_INTERVAL
        };
        tokio::select! {
            _ = state.cancel.cancelled() => break,
            _ = time::sleep(interval) => {}
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
    started_at: DateTime<Utc>,
    coverage: &mut Option<(usize, usize)>,
) -> Result<()> {
    if let Some(cluster) = &config.cluster {
        return reconcile_cluster(
            state, client, config, cluster, tasks, completed, started_at, coverage,
        )
        .await;
    }
    let selector = format!("{}={}", config.label_key, config.label_value);
    let daemonset_names: Vec<_> = [
        config.legacy_daemonset.as_deref(),
        config.replacement_daemonset.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !daemonset_names.is_empty() {
        // Capture this time before querying the API. The database keeps the
        // first observation across discovery passes and Operator restarts.
        let observed_at = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
        if legacy_collector_running(client, namespace, &daemonset_names).await? {
            stop_all(tasks).await;
            completed.clear();
            state
                .db
                .observe_legacy_pod_log_collector(&selector, &observed_at)
                .await?;
            if !*legacy_was_running {
                info!(
                    daemonset_names = ?daemonset_names,
                    "Waiting for old node collector to stop before reading Pod logs"
                );
            }
            *legacy_was_running = true;
            return Ok(());
        }
        if *legacy_was_running {
            info!(
                daemonset_names = ?daemonset_names,
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
    // Cluster scope selects by namespace and selector before this point.
    let label_matches = config.cluster.is_some()
        || labels.and_then(|labels| labels.get(&config.label_key)) == Some(&config.label_value);
    if !label_matches
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
                prefer_cursor: config.cluster.is_some(),
            }
        })
        .collect()
}

/// Pods of the cluster, page by page. When the scope names namespaces, each is
/// listed on its own, so the operator never asks for (or holds) the Pods of
/// namespaces it will not read.
///
/// Trade-off: `resourceVersion=0` is served from the API server's watch cache,
/// which is cheap for the server, but older API servers then ignore `limit` and
/// send every Pod in one body. The cluster-wide listing runs every 30 s against
/// a small memory limit, so it uses consistent paged lists (no `resourceVersion`):
/// more work for the server, bounded memory here. A listing of one named
/// namespace is bounded by that namespace and keeps the cheap cached read.
async fn list_cluster_pods(
    client: &KubernetesClient,
    namespaces: &[String],
    selector: Option<&str>,
) -> Result<Vec<Pod>> {
    let base = client.get_base_url().trim_end_matches('/').to_string();
    if namespaces.is_empty() {
        return list_pods_at(client, &format!("{base}/api/v1/pods"), selector, false).await;
    }
    let mut pods = Vec::new();
    for namespace in namespaces {
        pods.extend(
            list_pods_at(
                client,
                &format!("{base}/api/v1/namespaces/{namespace}/pods"),
                selector,
                true,
            )
            .await?,
        );
    }
    Ok(pods)
}

async fn list_pods_at(
    client: &KubernetesClient,
    url: &str,
    selector: Option<&str>,
    from_cache: bool,
) -> Result<Vec<Pod>> {
    let failed = |message: &str| ErrorData::PodLogReadFailed {
        message: message.to_string(),
    };
    let mut pods = Vec::new();
    let mut continue_token: Option<String> = None;
    loop {
        let mut request = client
            .client()
            .get(url)
            .query(&[("limit", CLUSTER_PAGE_SIZE)]);
        if let Some(selector) = selector {
            request = request.query(&[("labelSelector", selector)]);
        }
        match &continue_token {
            Some(token) => request = request.query(&[("continue", token.as_str())]),
            None if from_cache => request = request.query(&[("resourceVersion", "0")]),
            None => {}
        }
        let response = request
            .sign_kubernetes_request(&client.auth_config())
            .context(failed("could not authenticate the Pod listing"))?
            .send()
            .await
            .into_alien_error()
            .context(failed("could not list Pods in the cluster"))?;
        if !response.status().is_success() {
            return Err(AlienError::new(failed(&format!(
                "listing Pods returned HTTP {}",
                response.status()
            ))));
        }
        let page: k8s_openapi::List<Pod> = response
            .json()
            .await
            .into_alien_error()
            .context(failed("the Pod listing could not be read"))?;
        pods.extend(page.items);
        continue_token = page.metadata.continue_.filter(|token| !token.is_empty());
        if continue_token.is_none() {
            return Ok(pods);
        }
    }
}

/// A terminal Pod is only worth reading when it finished after `since`.
fn finished_since(pod: &Pod, since: DateTime<Utc>) -> bool {
    pod.status
        .as_ref()
        .and_then(|status| status.container_statuses.as_ref())
        .is_some_and(|statuses| {
            statuses.iter().any(|status| {
                status
                    .state
                    .as_ref()
                    .and_then(|state| state.terminated.as_ref())
                    .and_then(|terminated| terminated.finished_at.as_ref())
                    .is_some_and(|finished| finished.0 >= since)
            })
        })
}

/// Pick the streams to run when more are eligible than allowed. Each namespace
/// gets an equal share of the streams (a namespace with fewer containers gives
/// its unused share to the others), so one busy namespace does not crowd out the
/// rest. Inside a namespace the window moves with `slot` by the size of its
/// share, so every container is read in turn.
fn choose_streams(eligible: Vec<(String, Source)>, max_streams: usize, slot: i64) -> Vec<Source> {
    if eligible.len() <= max_streams {
        return eligible.into_iter().map(|(_, source)| source).collect();
    }
    let mut by_namespace = std::collections::BTreeMap::<String, Vec<Source>>::new();
    for (namespace, source) in eligible {
        by_namespace.entry(namespace).or_default().push(source);
    }
    for sources in by_namespace.values_mut() {
        sources.sort();
    }
    // Hand out the streams one at a time, namespace by namespace.
    let mut share: std::collections::BTreeMap<&String, usize> = by_namespace
        .keys()
        .map(|namespace| (namespace, 0))
        .collect();
    let mut given = 0;
    while given < max_streams {
        let mut progressed = false;
        for (namespace, sources) in &by_namespace {
            let taken = share
                .get_mut(namespace)
                .expect("every namespace has a share");
            if given < max_streams && *taken < sources.len() {
                *taken += 1;
                given += 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    let mut chosen = Vec::with_capacity(max_streams);
    for (namespace, sources) in &by_namespace {
        let quota = share[namespace];
        let start = (slot.unsigned_abs() as usize).wrapping_mul(quota) % sources.len();
        chosen.extend(sources.iter().cycle().skip(start).take(quota).cloned());
    }
    chosen
}

async fn reconcile_cluster(
    state: &Arc<OperatorState>,
    client: &KubernetesClient,
    config: &PodLogCollectionConfig,
    cluster: &ClusterLogScope,
    tasks: &mut HashMap<Source, StreamTask>,
    completed: &mut HashSet<Source>,
    started_at: DateTime<Utc>,
    coverage: &mut Option<(usize, usize)>,
) -> Result<()> {
    let deployment_id = match state.db.get_deployment_id().await? {
        Some(id) => id,
        None => {
            stop_all(tasks).await;
            return Ok(());
        }
    };
    let pods =
        list_cluster_pods(client, &cluster.namespaces, cluster.pod_selector.as_deref()).await?;
    // The whole listing arrived: cursors of Pods that are gone can be dropped.
    let live: HashSet<String> = pods
        .iter()
        .filter_map(|pod| pod.metadata.uid.clone())
        .collect();
    if let Err(error) = state.db.prune_pod_log_offsets(&live).await {
        warn!(error = %error, "Could not prune log cursors of Pods that no longer exist");
    }
    let mut eligible = Vec::new();
    let mut namespaces = HashMap::<String, String>::new();
    for pod in &pods {
        let Some(namespace) = pod.metadata.namespace.as_deref() else {
            continue;
        };
        if !cluster.includes(namespace) {
            continue;
        }
        let terminal = pod
            .status
            .as_ref()
            .and_then(|status| status.phase.as_deref())
            .is_some_and(|phase| phase != "Running");
        if terminal && !finished_since(pod, started_at) {
            continue;
        }
        for source in selected_sources(pod, config, started_at) {
            namespaces.insert(source.pod_uid.clone(), namespace.to_string());
            eligible.push((namespace.to_string(), source));
        }
    }
    let known: HashSet<_> = eligible.iter().map(|(_, source)| source.clone()).collect();
    completed.retain(|source| known.contains(source));
    eligible.retain(|(_, source)| !completed.contains(source));
    let total = eligible.len() + completed.len();
    let slot = Utc::now().timestamp() / CLUSTER_ROTATION_SECONDS;
    let desired: BTreeSet<_> = choose_streams(eligible, config.max_streams, slot)
        .into_iter()
        .collect();
    // Report the coverage once per change, so the count is not repeated every
    // pass. A shortfall is a warning: those containers are read in turn.
    let collecting = desired.len() + completed.len();
    if coverage.replace((collecting, total)) != Some((collecting, total)) {
        if collecting < total {
            warn!(
                collecting,
                containers = total,
                max_streams = config.max_streams,
                "Collecting {collecting} of {total} containers; the rest are read in turn"
            );
        } else {
            info!(
                collecting,
                containers = total,
                "Collecting {collecting} of {total} containers"
            );
        }
    }
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
        let Some(namespace) = namespaces.get(&source.pod_uid).cloned() else {
            continue;
        };
        let task_cancel = state.cancel.child_token();
        let handle = tokio::spawn({
            let client = client.clone();
            let db = state.db.clone();
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

async fn legacy_collector_running(
    client: &KubernetesClient,
    namespace: &str,
    daemonset_names: &[&str],
) -> Result<bool> {
    let daemonsets = client
        .list_daemonsets(namespace, None, None)
        .await
        .context(ErrorData::PodLogReadFailed {
            message: "could not check the old collector DaemonSet".to_string(),
        })?;
    if daemonsets.items.iter().any(|daemonset| {
        daemonset
            .metadata
            .name
            .as_deref()
            .is_some_and(|name| daemonset_names.contains(&name))
    }) {
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
                owners.iter().any(|owner| {
                    owner.kind == "DaemonSet" && daemonset_names.contains(&owner.name.as_str())
                })
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
    // Cluster scope continues from the saved cursor of this container instance, so
    // output written while no stream was open is not lost. The start time only
    // applies to a container with no cursor. Namespace scope keeps its floor.
    let replay_time = Some(match (saved_time, source.prefer_cursor) {
        (Some(saved), true) => saved,
        (saved, false) => saved
            .unwrap_or_else(|| source.initial_since.clone())
            .max(source.initial_since.clone()),
        (None, true) => source.initial_since.clone(),
    });
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
                    // A live watch can end in the middle of a line. Reconnect from
                    // the last complete line instead of advancing past a fragment.
                    if source.terminal && !partial.is_empty() {
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
    use axum::{extract::Query, http::StatusCode, routing::get, Json, Router};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodStatus};
    use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
    use opentelemetry_proto::tonic::common::v1::any_value::Value;
    use prost::Message;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn both_collector_generations_block_cutover_until_their_pods_stop() {
        let phase = Arc::new(AtomicUsize::new(0));
        let daemonset_phase = phase.clone();
        let pod_phase = phase.clone();
        let app = Router::new()
            .route(
                "/apis/apps/v1/namespaces/demo/daemonsets",
                get(move |Query(query): Query<HashMap<String, String>>| {
                    let phase = daemonset_phase.load(Ordering::SeqCst);
                    async move {
                        assert!(!query.contains_key("fieldSelector"));
                        if phase == 5 {
                            return (StatusCode::FORBIDDEN, Json(serde_json::json!({
                                "apiVersion": "v1", "kind": "Status", "status": "Failure",
                                "reason": "Forbidden", "message": "access denied", "code": 403
                            })));
                        }
                        let name = match phase {
                            0 => "collector-logs",
                            1 => "collector-logs-v2",
                            _ => "other-release-logs-v2",
                        };
                        (StatusCode::OK, Json(serde_json::json!({
                            "apiVersion": "apps/v1", "kind": "DaemonSetList",
                            "metadata": {}, "items": [{"metadata": {"name": name}}]
                        })))
                    }
                }),
            )
            .route(
                "/api/v1/namespaces/demo/pods",
                get(move |Query(query): Query<HashMap<String, String>>| {
                    let phase = pod_phase.load(Ordering::SeqCst);
                    async move {
                        assert_eq!(query.get("labelSelector").map(String::as_str), Some(
                            "app.kubernetes.io/component in (log-collector,whitelabeled-log-collector)"
                        ));
                        let owner = match phase {
                            2 => "collector-logs",
                            3 => "collector-logs-v2",
                            _ => "other-release-logs-v2",
                        };
                        Json(serde_json::json!({
                            "apiVersion": "v1", "kind": "PodList", "metadata": {},
                            "items": [{"metadata": {
                                "name": "terminating-collector", "deletionTimestamp": "2026-09-30T00:00:00Z",
                                "ownerReferences": [{"apiVersion": "apps/v1", "kind": "DaemonSet",
                                    "name": owner, "uid": "collector-uid"}]
                            }}]
                        }))
                    }
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
        let names = ["collector-logs", "collector-logs-v2"];
        for state in 0..4 {
            phase.store(state, Ordering::SeqCst);
            assert!(
                legacy_collector_running(&client, "demo", &names)
                    .await
                    .unwrap(),
                "collector generation or terminating Pod must block cutover in state {state}"
            );
        }
        phase.store(4, Ordering::SeqCst);
        assert!(!legacy_collector_running(&client, "demo", &names)
            .await
            .unwrap());
        phase.store(5, Ordering::SeqCst);
        assert!(legacy_collector_running(&client, "demo", &names)
            .await
            .is_err());
        server.abort();
    }

    #[tokio::test]
    async fn terminal_pod_api_logs_queue_once_across_reader_restart() {
        const TERMINAL_LINES: &str = concat!(
            "2026-09-27T09:00:00.000000001Z {\"time\":\"2026-09-27T09:00:00Z\",\"level\":\"INFO\",\"msg\":\"first\"}\n",
            "2026-09-27T09:00:00.000000001Z {\"time\":\"2026-09-27T09:00:00Z\",\"level\":\"INFO\",\"msg\":\"second\"}",
        );
        const LIVE_PARTIAL: &str = "2026-09-27T09:00:00.000000002Z {\"time\":\"2026-09-27T09:00:00Z\",\"level\":\"INFO\",\"msg\":\"comp";
        const LIVE_COMPLETE: &str = "2026-09-27T09:00:00.000000002Z {\"time\":\"2026-09-27T09:00:00Z\",\"level\":\"INFO\",\"msg\":\"complete\"}\n";
        let live_calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/api/v1/namespaces/demo/pods/app-123/log",
            get(move |Query(query): Query<HashMap<String, String>>| {
                let live_calls = live_calls.clone();
                async move {
                    assert_eq!(query.get("container").map(String::as_str), Some("app"));
                    assert_eq!(query.get("timestamps").map(String::as_str), Some("true"));
                    assert!(query.contains_key("sinceTime"));
                    match query.get("follow").map(String::as_str) {
                        Some("false") => TERMINAL_LINES,
                        Some("true") if live_calls.fetch_add(1, Ordering::SeqCst) == 0 => {
                            LIVE_PARTIAL
                        }
                        Some("true") => LIVE_COMPLETE,
                        other => panic!("unexpected follow query: {other:?}"),
                    }
                }
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
            prefer_cursor: false,
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

        let live_source = Source {
            pod_uid: "uid-live".to_string(),
            terminal: false,
            ..source
        };
        read_stream_once(&client, &db, "demo", "dep_test", &live_source, &cancel)
            .await
            .expect("read interrupted live stream");
        assert!(db
            .get_pod_log_offset("uid-live", "app", 0)
            .await
            .unwrap()
            .is_none());
        assert_eq!(db.get_pending_telemetry(10).await.unwrap().len(), 1);
        read_stream_once(&client, &db, "demo", "dep_test", &live_source, &cancel)
            .await
            .expect("reconnect live stream");
        let queued = db.get_pending_telemetry(10).await.unwrap();
        assert_eq!(queued.len(), 2);
        let otlp = ExportLogsServiceRequest::decode(queued[1].2.as_slice())
            .expect("decode reconnected live logs");
        let record = &otlp.resource_logs[0].scope_logs[0].log_records[0];
        assert!(matches!(
            record.body.as_ref().and_then(|body| body.value.as_ref()),
            Some(Value::StringValue(body)) if body == "complete"
        ));
        server.abort();
    }

    #[test]
    fn only_selected_workload_pods_are_read() {
        let cutover = Utc::now();
        let config = PodLogCollectionConfig {
            label_key: "example.com/deployment".to_string(),
            label_value: "release-one".to_string(),
            legacy_daemonset: None,
            replacement_daemonset: None,
            max_streams: 32,
            cluster: None,
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

    fn source(namespace: &str, index: usize) -> (String, Source) {
        (
            namespace.to_string(),
            Source {
                pod_uid: format!("{namespace}-{index:03}"),
                pod_name: format!("pod-{index}"),
                container: "app".to_string(),
                restart_count: 0,
                terminal: false,
                initial_since: Utc::now(),
                prefer_cursor: false,
            },
        )
    }

    #[test]
    fn a_busy_namespace_does_not_crowd_out_the_others() {
        let mut eligible: Vec<_> = (0..30).map(|i| source("busy", i)).collect();
        eligible.extend((0..3).map(|i| source("quiet", i)));
        // In every window, not only the first: the quiet namespace is always served,
        // and a different slice of the busy one is read each time.
        let mut busy_seen = HashSet::new();
        for slot in 0..12 {
            let chosen = choose_streams(eligible.clone(), 10, slot);
            assert_eq!(chosen.len(), 10);
            assert_eq!(
                chosen
                    .iter()
                    .filter(|s| s.pod_uid.starts_with("quiet"))
                    .count(),
                3,
                "slot {slot}"
            );
            busy_seen.extend(
                chosen
                    .into_iter()
                    .filter(|s| s.pod_uid.starts_with("busy"))
                    .map(|s| s.pod_uid),
            );
        }
        assert_eq!(busy_seen.len(), 30);
    }

    #[test]
    fn over_the_cap_every_container_is_read_in_turn() {
        let eligible: Vec<_> = (0..25).map(|i| source("apps", i)).collect();
        let mut seen = HashSet::new();
        for slot in 0..5 {
            for chosen in choose_streams(eligible.clone(), 10, slot) {
                seen.insert(chosen.pod_uid);
            }
        }
        assert_eq!(seen.len(), 25);
        // Under the cap, nothing moves.
        assert_eq!(choose_streams(eligible.clone(), 25, 7).len(), 25);
    }

    #[test]
    fn cluster_scope_excludes_system_namespaces_and_honors_lists() {
        let scope = ClusterLogScope {
            namespaces: Vec::new(),
            exclude_namespaces: DEFAULT_EXCLUDED_NAMESPACES
                .iter()
                .map(|n| n.to_string())
                .collect(),
            pod_selector: None,
        };
        assert!(scope.includes("shop") && !scope.includes("kube-system"));
        let only = ClusterLogScope {
            namespaces: vec!["a".into(), "b".into()],
            exclude_namespaces: vec!["b".into()],
            pod_selector: None,
        };
        assert!(only.includes("a") && !only.includes("b") && !only.includes("c"));
    }

    #[test]
    fn cluster_scope_reads_pods_without_the_namespace_label_and_never_older_than_start() {
        let started = Utc::now();
        let config = PodLogCollectionConfig {
            label_key: String::new(),
            label_value: String::new(),
            legacy_daemonset: None,
            replacement_daemonset: None,
            max_streams: 200,
            cluster: Some(ClusterLogScope {
                namespaces: Vec::new(),
                exclude_namespaces: Vec::new(),
                pod_selector: None,
            }),
        };
        let mut pod = Pod::default();
        pod.metadata.uid = Some("uid".to_string());
        pod.metadata.name = Some("web".to_string());
        pod.metadata.creation_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                started - chrono::Duration::hours(3),
            ));
        pod.spec = Some(PodSpec {
            containers: vec![Container {
                name: "web".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        pod.status = Some(PodStatus {
            phase: Some("Running".to_string()),
            ..Default::default()
        });
        let selected = selected_sources(&pod, &config, started);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].initial_since, started);
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
            prefer_cursor: false,
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

    async fn fake_api(app: Router) -> (KubernetesClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind fake Kubernetes API");
        let address = listener.local_addr().expect("fake Kubernetes address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve fake API");
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
        (client, server)
    }

    fn pod_list(names: &[&str], next: Option<&str>) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "apiVersion": "v1", "kind": "PodList",
            "metadata": next.map(|token| serde_json::json!({"continue": token})).unwrap_or_else(|| serde_json::json!({})),
            "items": names.iter().map(|name| serde_json::json!({"metadata": {"name": name, "namespace": "x"}})).collect::<Vec<_>>()
        }))
    }

    #[tokio::test]
    async fn a_cluster_wide_listing_reads_every_page_with_consistent_lists() {
        let requests = Arc::new(std::sync::Mutex::new(Vec::<HashMap<String, String>>::new()));
        let seen = requests.clone();
        let app = Router::new().route(
            "/api/v1/pods",
            get(move |Query(query): Query<HashMap<String, String>>| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(query.clone());
                    match query.get("continue").map(String::as_str) {
                        None => pod_list(&["a", "b"], Some("page-2")),
                        Some("page-2") => pod_list(&["c"], Some("page-3")),
                        _ => pod_list(&["d"], None),
                    }
                }
            }),
        );
        let (client, server) = fake_api(app).await;
        let pods = list_cluster_pods(&client, &[], Some("app=web"))
            .await
            .unwrap();
        server.abort();
        let names: Vec<_> = pods
            .iter()
            .filter_map(|p| p.metadata.name.clone())
            .collect();
        assert_eq!(names, ["a", "b", "c", "d"]);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        // `resourceVersion=0` would let an older API server ignore `limit`.
        assert!(requests.iter().all(|r| !r.contains_key("resourceVersion")));
        assert!(requests.iter().all(|r| r.contains_key("limit")));
        assert!(requests
            .iter()
            .all(|r| r.get("labelSelector").map(String::as_str) == Some("app=web")));
    }

    #[tokio::test]
    async fn named_namespaces_are_listed_one_by_one_never_cluster_wide() {
        let app = Router::new()
            .route(
                "/api/v1/namespaces/a/pods",
                get(|| async { pod_list(&["in-a"], None) }),
            )
            .route(
                "/api/v1/namespaces/b/pods",
                get(|| async { pod_list(&["in-b"], None) }),
            )
            .route(
                "/api/v1/pods",
                get(|| async {
                    (
                        StatusCode::FORBIDDEN,
                        "cluster-wide listing must not be used",
                    )
                }),
            );
        let (client, server) = fake_api(app).await;
        let pods = list_cluster_pods(&client, &["a".into(), "b".into()], None)
            .await
            .unwrap();
        server.abort();
        let names: Vec<_> = pods
            .iter()
            .filter_map(|p| p.metadata.name.clone())
            .collect();
        assert_eq!(names, ["in-a", "in-b"]);
    }

    #[tokio::test]
    async fn cluster_scope_resumes_from_the_saved_cursor_and_namespace_scope_keeps_its_floor() {
        let since = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = since.clone();
        let app = Router::new().route(
            "/api/v1/namespaces/demo/pods/app-1/log",
            get(move |Query(query): Query<HashMap<String, String>>| {
                let seen = seen.clone();
                async move {
                    seen.lock()
                        .unwrap()
                        .push(query.get("sinceTime").cloned().unwrap_or_default());
                    ""
                }
            }),
        );
        let (client, server) = fake_api(app).await;
        let data_dir = tempfile::tempdir().unwrap();
        let db = OperatorDb::new(
            data_dir.path().to_str().unwrap(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .await
        .unwrap();
        // A cursor from before this operator process started.
        let cursor = PodLogOffset {
            timestamp: "2026-09-27T07:00:00Z".to_string(),
            ordinal: 1,
        };
        db.store_pod_log_batch("uid-1", "app", 0, &cursor, &[])
            .await
            .unwrap();
        let started = DateTime::parse_from_rfc3339("2026-09-27T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let source = |prefer_cursor| Source {
            pod_uid: "uid-1".into(),
            pod_name: "app-1".into(),
            container: "app".into(),
            restart_count: 0,
            terminal: true,
            initial_since: started,
            prefer_cursor,
        };
        let cancel = CancellationToken::new();
        read_stream_once(&client, &db, "demo", "dep", &source(true), &cancel)
            .await
            .unwrap();
        read_stream_once(&client, &db, "demo", "dep", &source(false), &cancel)
            .await
            .unwrap();
        server.abort();
        let since = since.lock().unwrap();
        assert!(
            since[0].starts_with("2026-09-27T06:59:59"),
            "cluster scope resumes at the cursor: {}",
            since[0]
        );
        assert!(
            since[1].starts_with("2026-09-27T07:59:59"),
            "namespace scope keeps its floor: {}",
            since[1]
        );
    }

    #[test]
    fn extra_exclusions_add_to_the_system_namespaces_unless_those_are_included() {
        let extra = vec!["monitoring".to_string(), "kube-system".to_string()];
        assert_eq!(
            cluster_excluded_namespaces(extra.clone(), false),
            [
                "kube-system",
                "kube-public",
                "kube-node-lease",
                "monitoring"
            ]
        );
        assert_eq!(
            cluster_excluded_namespaces(extra, true),
            ["monitoring", "kube-system"]
        );
        assert!(cluster_excluded_namespaces(Vec::new(), true).is_empty());
        assert_eq!(cluster_excluded_namespaces(Vec::new(), false).len(), 3);
    }
}
