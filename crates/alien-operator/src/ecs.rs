//! Collection on Amazon ECS: logs of the log groups under a prefix, and the
//! services of one cluster as inventory.
//!
//! * Logs: `FilterLogEvents` polled per log group, capped in groups, events per
//!   poll and bytes per batch. The saved position is the last event time plus
//!   the ids of the events stored at that millisecond, committed to the local
//!   database together with the batch, so a restart neither repeats nor skips.
//! * Inventory: the services of the cluster named by `OPERATOR_ECS_CLUSTER`
//!   (desired and running count, rollout state). Nothing is reported without it.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use alien_aws_clients::{
    ecs::{EcsApi, EcsClient},
    AwsClientConfigExt, AwsCredentialProvider,
};
use alien_core::{
    HeartbeatBackend, ObservedHealth, ObservedInventoryBatch, ObservedResourceSample, Platform,
    ProviderLifecycleState,
};
use alien_error::{AlienError, Context};
use chrono::Utc;
use serde_json::{json, Value};
use tokio::time;
use tracing::{info, warn};

use crate::{
    collector_logs::{pod_log_records_to_otlp, PodLogRecord},
    db::PodLogOffset,
    error::{ErrorData, Result},
    pod_logs::{MAX_BATCH_BYTES, MAX_BATCH_RECORDS, MAX_LINE_BYTES},
    OperatorState,
};

const POLL_INTERVAL: Duration = Duration::from_secs(10);
const INVENTORY_INTERVAL: Duration = Duration::from_secs(60);
/// Most log groups followed; the rest are skipped with one warning.
const MAX_LOG_GROUPS: usize = 20;
/// Pages of `FilterLogEvents` read per group per poll.
const MAX_PAGES_PER_POLL: usize = 3;
const PAGE_LIMIT: u32 = MAX_BATCH_RECORDS as u32;
const MAX_SERVICES: usize = 200;
const SCOPE: &str = "ecs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcsConfig {
    pub cluster: Option<String>,
    pub log_group_prefix: Option<String>,
    /// The operator's own log group, never read back as input.
    pub own_log_group: Option<String>,
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// `None` when neither a cluster nor a log group prefix is configured.
pub fn config_from_env() -> Result<Option<EcsConfig>> {
    let config = EcsConfig {
        cluster: env_value("OPERATOR_ECS_CLUSTER"),
        log_group_prefix: env_value("OPERATOR_ECS_LOG_GROUP_PREFIX"),
        own_log_group: env_value("OPERATOR_ECS_OWN_LOG_GROUP"),
    };
    if let Some(prefix) = &config.log_group_prefix {
        if !prefix.starts_with('/') {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: "OPERATOR_ECS_LOG_GROUP_PREFIX must start with '/'".to_string(),
            }));
        }
    }
    Ok((config.cluster.is_some() || config.log_group_prefix.is_some()).then_some(config))
}

pub async fn run(state: Arc<OperatorState>, config: EcsConfig) {
    let clients = match clients().await {
        Ok(clients) => clients,
        Err(error) => {
            warn!(error = %error, "ECS collection is off: cannot create AWS clients");
            return;
        }
    };
    info!(
        cluster = config.cluster.as_deref().unwrap_or("-"),
        log_group_prefix = config.log_group_prefix.as_deref().unwrap_or("-"),
        "Starting ECS collection"
    );
    let mut handles = Vec::new();
    if let Some(prefix) = config.log_group_prefix.clone() {
        if state.config.is_telemetry_enabled() {
            let (state, logs, own) = (
                state.clone(),
                clients.logs.clone(),
                config.own_log_group.clone(),
            );
            handles.push(tokio::spawn(async move {
                run_logs(state, logs, prefix, own).await
            }));
        } else {
            warn!("ECS log collection is enabled but deployment telemetry is off");
        }
    }
    if let Some(cluster) = config.cluster.clone() {
        let (state, ecs) = (state.clone(), clients.ecs.clone());
        handles.push(tokio::spawn(async move {
            run_inventory(state, ecs, cluster).await
        }));
    }
    for handle in handles {
        let _ = handle.await;
    }
}

struct Clients {
    ecs: Arc<dyn EcsApi>,
    logs: Arc<dyn EcsApi>,
}

async fn clients() -> Result<Clients> {
    let environment: std::collections::HashMap<String, String> = std::env::vars().collect();
    let config = alien_aws_clients::AwsClientConfig::from_env(&environment)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Cannot load AWS credentials for ECS collection".into(),
        })?;
    let credentials = AwsCredentialProvider::from_config(config).await.context(
        ErrorData::ConfigurationError {
            message: "Cannot create the AWS credential provider for ECS collection".into(),
        },
    )?;
    let http = reqwest::Client::new();
    Ok(Clients {
        ecs: Arc::new(EcsClient::new(http.clone(), credentials.clone())),
        logs: Arc::new(EcsClient::new_logs(http, credentials)),
    })
}

// -------------------------------------------------------------------- logs

/// The saved position: last event time in milliseconds and the ids of the
/// events already stored at exactly that time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LogPosition {
    pub timestamp_ms: i64,
    pub seen_ids: Vec<String>,
}

impl LogPosition {
    pub(crate) fn encode(&self) -> String {
        format!("{}|{}", self.timestamp_ms, self.seen_ids.join(","))
    }

    pub(crate) fn decode(text: &str) -> Option<Self> {
        let (timestamp, ids) = text.split_once('|')?;
        Some(Self {
            timestamp_ms: timestamp.parse().ok()?,
            seen_ids: ids
                .split(',')
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect(),
        })
    }

    /// Whether an event was stored before.
    pub(crate) fn already_stored(&self, timestamp_ms: i64, event_id: &str) -> bool {
        timestamp_ms < self.timestamp_ms
            || (timestamp_ms == self.timestamp_ms && self.seen_ids.iter().any(|id| id == event_id))
    }

    /// The position after storing an event.
    pub(crate) fn advance(&mut self, timestamp_ms: i64, event_id: &str) {
        if timestamp_ms > self.timestamp_ms {
            self.timestamp_ms = timestamp_ms;
            self.seen_ids.clear();
        }
        if timestamp_ms == self.timestamp_ms && !self.seen_ids.iter().any(|id| id == event_id) {
            self.seen_ids.push(event_id.to_string());
        }
    }
}

/// One page of `FilterLogEvents`: the new records, the position after them and
/// the next page token.
pub(crate) struct ParsedPage {
    pub records: Vec<PodLogRecord>,
    pub position: LogPosition,
    pub next_token: Option<String>,
    /// The page was cut at the batch byte limit; read again from `position`.
    pub cut: bool,
}

pub(crate) fn parse_events_page(group: &str, page: &Value, saved: &LogPosition) -> ParsedPage {
    let mut position = saved.clone();
    let mut records = Vec::new();
    let mut bytes = 0;
    let mut cut = false;
    for event in page
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(timestamp), Some(id), Some(message)) = (
            event.get("timestamp").and_then(Value::as_i64),
            event.get("eventId").and_then(Value::as_str),
            event.get("message").and_then(Value::as_str),
        ) else {
            continue;
        };
        if position.already_stored(timestamp, id) {
            continue;
        }
        if bytes >= MAX_BATCH_BYTES {
            cut = true;
            break;
        }
        position.advance(timestamp, id);
        let mut body = message.trim_end_matches('\n').to_string();
        if body.len() > MAX_LINE_BYTES {
            let mut end = MAX_LINE_BYTES;
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            body.truncate(end);
            body.push_str(" [truncated]");
        }
        bytes += body.len();
        records.push(PodLogRecord {
            namespace: SCOPE.to_string(),
            pod: event
                .get("logStreamName")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            container: group.to_string(),
            timestamp_unix_nanos: u64::try_from(timestamp)
                .unwrap_or(0)
                .saturating_mul(1_000_000),
            body,
        });
    }
    ParsedPage {
        records,
        position,
        next_token: if cut {
            None
        } else {
            page.get("nextToken")
                .and_then(Value::as_str)
                .map(str::to_string)
        },
        cut,
    }
}

/// Names of the log groups under a prefix, capped, without the excluded one.
pub(crate) async fn list_log_groups(
    logs: &dyn EcsApi,
    prefix: &str,
    own: Option<&str>,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut request = json!({"logGroupNamePrefix": prefix, "limit": 50});
        if let Some(token) = &token {
            request["nextToken"] = json!(token);
        }
        let page =
            logs.call("DescribeLogGroups", request)
                .await
                .context(ErrorData::PodLogReadFailed {
                    message: "could not list log groups".to_string(),
                })?;
        for group in page
            .get("logGroups")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = group.get("logGroupName").and_then(Value::as_str) {
                if Some(name) != own {
                    names.push(name.to_string());
                }
            }
        }
        token = page
            .get("nextToken")
            .and_then(Value::as_str)
            .map(str::to_string);
        if token.is_none() || names.len() > MAX_LOG_GROUPS {
            break;
        }
    }
    if names.len() > MAX_LOG_GROUPS {
        warn!(
            max = MAX_LOG_GROUPS,
            "More log groups under the prefix than the limit; the rest are not collected"
        );
        names.truncate(MAX_LOG_GROUPS);
    }
    Ok(names)
}

/// Read what is new in one group and queue it. Returns the number of records.
pub(crate) async fn poll_group(
    state: &OperatorState,
    logs: &dyn EcsApi,
    deployment_id: &str,
    group: &str,
    started_ms: i64,
) -> Result<usize> {
    let key = format!("ecs:logs:{group}");
    let mut position = state
        .db
        .get_pod_log_offset(&key, "logs", 0)
        .await?
        .and_then(|offset| LogPosition::decode(&offset.timestamp))
        .unwrap_or(LogPosition {
            timestamp_ms: started_ms,
            seen_ids: Vec::new(),
        });
    let mut total = 0;
    let mut token: Option<String> = None;
    for _ in 0..MAX_PAGES_PER_POLL {
        let mut request = json!({
            "logGroupName": group,
            "startTime": position.timestamp_ms,
            "limit": PAGE_LIMIT,
        });
        if let Some(token) = &token {
            request["nextToken"] = json!(token);
        }
        let page =
            logs.call("FilterLogEvents", request)
                .await
                .context(ErrorData::PodLogReadFailed {
                    message: format!("could not read log group {group}"),
                })?;
        let parsed = parse_events_page(group, &page, &position);
        if !parsed.records.is_empty() {
            let count = parsed.records.len();
            let encoded = pod_log_records_to_otlp(parsed.records, deployment_id)?;
            state
                .db
                .store_pod_log_batch(
                    &key,
                    "logs",
                    0,
                    &PodLogOffset {
                        timestamp: parsed.position.encode(),
                        ordinal: 0,
                    },
                    &encoded,
                )
                .await?;
            total += count;
        }
        position = parsed.position;
        token = parsed.next_token;
        if token.is_none() && !parsed.cut {
            break;
        }
    }
    Ok(total)
}

async fn run_logs(
    state: Arc<OperatorState>,
    logs: Arc<dyn EcsApi>,
    prefix: String,
    own: Option<String>,
) {
    // A restart does not replay old logs: new groups begin at operator start.
    let started_ms = Utc::now().timestamp_millis();
    loop {
        let result = async {
            let deployment_id = state.db.get_deployment_id().await?.ok_or_else(|| {
                AlienError::new(ErrorData::PodLogReadFailed {
                    message: "the operator has no deployment id yet".to_string(),
                })
            })?;
            for group in list_log_groups(logs.as_ref(), &prefix, own.as_deref()).await? {
                if let Err(error) =
                    poll_group(&state, logs.as_ref(), &deployment_id, &group, started_ms).await
                {
                    warn!(error = %error, "ECS log poll failed; will retry");
                }
            }
            Result::Ok(())
        }
        .await;
        if let Err(error) = result {
            warn!(error = %error, "ECS log discovery failed; will retry");
        }
        tokio::select! {
            _ = state.cancel.cancelled() => return,
            _ = time::sleep(POLL_INTERVAL) => {}
        }
    }
}

// --------------------------------------------------------------- inventory

/// One service: desired and running counts and the rollout state.
pub(crate) fn service_sample(service: &Value) -> Option<ObservedResourceSample> {
    let name = service.get("serviceName")?.as_str()?.to_string();
    let arn = service
        .get("serviceArn")
        .and_then(Value::as_str)
        .unwrap_or(&name)
        .to_string();
    let desired = service
        .get("desiredCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let running = service
        .get("runningCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let status = service
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    let rollout = service
        .get("deployments")
        .and_then(Value::as_array)
        .and_then(|deployments| {
            deployments
                .iter()
                .find(|deployment| {
                    deployment.get("status").and_then(Value::as_str) == Some("PRIMARY")
                })
                .and_then(|deployment| deployment.get("rolloutState").and_then(Value::as_str))
        })
        .unwrap_or("UNKNOWN");
    let (health, lifecycle) = match (status, rollout) {
        ("INACTIVE", _) | ("DRAINING", _) => {
            (ObservedHealth::Unknown, ProviderLifecycleState::Stopped)
        }
        (_, "FAILED") => (ObservedHealth::Unhealthy, ProviderLifecycleState::Running),
        (_, "IN_PROGRESS") => (ObservedHealth::Degraded, ProviderLifecycleState::Updating),
        _ if desired > 0 && running == 0 => {
            (ObservedHealth::Unhealthy, ProviderLifecycleState::Running)
        }
        _ if running < desired => (ObservedHealth::Degraded, ProviderLifecycleState::Running),
        _ => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
    };
    Some(ObservedResourceSample {
        deployment_id: None,
        raw_identity: format!("ecs:{arn}"),
        provider_kind: "ecs/Service".to_string(),
        display_name: name,
        namespace: None,
        region: None,
        scope: None,
        resource_type_hint: None,
        version: None,
        alien_resource_id: None,
        health,
        lifecycle,
        message: None,
        partial: false,
        provider_stale: false,
        counts: None,
        collection_issues: Vec::new(),
        labels: BTreeMap::new(),
        attributes: BTreeMap::from([
            ("status".to_string(), json!(status)),
            ("desiredCount".to_string(), json!(desired)),
            ("runningCount".to_string(), json!(running)),
            ("rolloutState".to_string(), json!(rollout)),
        ]),
        raw: Vec::new(),
        images: Vec::new(),
    })
}

/// The cluster's services, at most `MAX_SERVICES`. `complete` is false when the
/// list was cut.
pub(crate) async fn collect_services(
    ecs: &dyn EcsApi,
    cluster: &str,
) -> Result<(Vec<ObservedResourceSample>, bool)> {
    let mut arns: Vec<String> = Vec::new();
    let mut token: Option<String> = None;
    let mut complete = true;
    loop {
        let mut request = json!({"cluster": cluster, "maxResults": 100});
        if let Some(token) = &token {
            request["nextToken"] = json!(token);
        }
        let page =
            ecs.call("ListServices", request)
                .await
                .context(ErrorData::PodLogReadFailed {
                    message: "could not list ECS services".to_string(),
                })?;
        arns.extend(
            page.get("serviceArns")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|arn| arn.as_str().map(str::to_string)),
        );
        token = page
            .get("nextToken")
            .and_then(Value::as_str)
            .map(str::to_string);
        if token.is_none() {
            break;
        }
        if arns.len() >= MAX_SERVICES {
            complete = false;
            break;
        }
    }
    if arns.len() > MAX_SERVICES {
        arns.truncate(MAX_SERVICES);
        complete = false;
    }
    let mut samples = Vec::new();
    // DescribeServices takes at most ten services per call.
    for chunk in arns.chunks(10) {
        let described = ecs
            .call(
                "DescribeServices",
                json!({"cluster": cluster, "services": chunk}),
            )
            .await
            .context(ErrorData::PodLogReadFailed {
                message: "could not describe ECS services".to_string(),
            })?;
        samples.extend(
            described
                .get("services")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(service_sample),
        );
    }
    Ok((samples, complete))
}

async fn run_inventory(state: Arc<OperatorState>, ecs: Arc<dyn EcsApi>, cluster: String) {
    loop {
        match collect_services(ecs.as_ref(), &cluster).await {
            Ok((resources, complete)) => {
                let batch = ObservedInventoryBatch {
                    source_kind: "operator".to_string(),
                    inventory_scope: SCOPE.to_string(),
                    controller_platform: Platform::Aws,
                    backend: HeartbeatBackend::Aws,
                    observed_at: Utc::now(),
                    complete,
                    resources,
                };
                let result = async {
                    let pending = state.db.get_pending_observed_inventory_batches().await?;
                    let mut merged: Vec<_> = pending
                        .into_iter()
                        .filter(|existing| existing.inventory_scope != batch.inventory_scope)
                        .collect();
                    merged.push(batch);
                    state
                        .db
                        .set_pending_observed_inventory_batches(&merged)
                        .await?;
                    state.db.request_sync();
                    Result::Ok(())
                }
                .await;
                if let Err(error) = result {
                    warn!(error = %error, "Could not queue the ECS inventory");
                }
            }
            Err(error) => warn!(error = %error, "ECS inventory failed"),
        }
        tokio::select! {
            _ = state.cancel.cancelled() => return,
            _ = time::sleep(INVENTORY_INTERVAL) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(events: &[(i64, &str, &str)], next: Option<&str>) -> Value {
        let mut value = json!({
            "events": events.iter().map(|(ts, id, msg)| json!({
                "timestamp": ts, "eventId": id, "message": msg, "logStreamName": "web/abc"
            })).collect::<Vec<_>>()
        });
        if let Some(next) = next {
            value["nextToken"] = json!(next);
        }
        value
    }

    #[test]
    fn position_round_trips() {
        let position = LogPosition {
            timestamp_ms: 1700,
            seen_ids: vec!["a".into(), "b".into()],
        };
        assert_eq!(LogPosition::decode(&position.encode()), Some(position));
        assert_eq!(
            LogPosition::decode("5|"),
            Some(LogPosition {
                timestamp_ms: 5,
                seen_ids: vec![]
            })
        );
        assert_eq!(LogPosition::decode("garbage"), None);
    }

    #[test]
    fn resume_drops_stored_events_and_keeps_new_ones_at_the_same_millisecond() {
        let saved = LogPosition {
            timestamp_ms: 100,
            seen_ids: vec!["a".into()],
        };
        let parsed = parse_events_page(
            "/ecs/web",
            &page(
                &[
                    (99, "z", "old"),
                    (100, "a", "stored"),
                    (100, "b", "same ms"),
                    (101, "c", "new"),
                ],
                None,
            ),
            &saved,
        );
        let bodies: Vec<_> = parsed.records.iter().map(|r| r.body.as_str()).collect();
        assert_eq!(bodies, ["same ms", "new"]);
        assert_eq!(parsed.position.timestamp_ms, 101);
        assert_eq!(parsed.position.seen_ids, ["c"]);
        assert_eq!(parsed.records[0].timestamp_unix_nanos, 100_000_000);
        assert_eq!(parsed.records[0].container, "/ecs/web");
    }

    #[test]
    fn long_lines_are_cut_on_a_character_boundary() {
        let long = "é".repeat(MAX_LINE_BYTES);
        let parsed = parse_events_page(
            "/ecs/web",
            &page(&[(1, "a", &long)], Some("next")),
            &LogPosition::default(),
        );
        assert!(parsed.records[0].body.ends_with(" [truncated]"));
        assert_eq!(parsed.next_token.as_deref(), Some("next"));
    }

    #[test]
    fn a_page_is_cut_at_the_batch_byte_limit_and_resumes_from_the_position() {
        let line = "x".repeat(MAX_BATCH_BYTES / 2 + 1);
        let events = [
            (1, "a", line.as_str()),
            (2, "b", line.as_str()),
            (3, "c", "tail"),
        ];
        let first = parse_events_page(
            "/ecs/web",
            &page(&events, Some("next")),
            &LogPosition::default(),
        );
        assert!(first.cut);
        assert_eq!(first.records.len(), 2);
        assert_eq!(first.next_token, None);
        let second = parse_events_page("/ecs/web", &page(&events[1..], None), &first.position);
        let bodies: Vec<_> = second.records.iter().map(|r| r.body.as_str()).collect();
        assert_eq!(bodies, ["tail"]);
    }

    fn service(status: &str, rollout: &str, desired: i64, running: i64) -> Value {
        json!({
            "serviceName": "web", "serviceArn": "arn:aws:ecs:r:1:service/c/web",
            "status": status, "desiredCount": desired, "runningCount": running,
            "deployments": [{"status": "PRIMARY", "rolloutState": rollout}]
        })
    }

    #[test]
    fn service_health_follows_rollout_and_counts() {
        let health = |status, rollout, desired, running| {
            service_sample(&service(status, rollout, desired, running))
                .unwrap()
                .health
        };
        assert_eq!(health("ACTIVE", "COMPLETED", 2, 2), ObservedHealth::Healthy);
        assert_eq!(
            health("ACTIVE", "IN_PROGRESS", 2, 1),
            ObservedHealth::Degraded
        );
        assert_eq!(health("ACTIVE", "FAILED", 2, 0), ObservedHealth::Unhealthy);
        assert_eq!(
            health("ACTIVE", "COMPLETED", 2, 0),
            ObservedHealth::Unhealthy
        );
        assert_eq!(
            health("ACTIVE", "COMPLETED", 2, 1),
            ObservedHealth::Degraded
        );
        assert_eq!(
            health("INACTIVE", "COMPLETED", 0, 0),
            ObservedHealth::Unknown
        );
        let sample = service_sample(&service("ACTIVE", "COMPLETED", 3, 3)).unwrap();
        assert_eq!(sample.attributes["desiredCount"], json!(3));
        assert_eq!(sample.provider_kind, "ecs/Service");
    }

    #[derive(Debug)]
    struct Fake {
        calls: std::sync::Mutex<Vec<(String, Value)>>,
        services: usize,
    }

    #[async_trait::async_trait]
    impl EcsApi for Fake {
        async fn call(&self, operation: &str, request: Value) -> alien_client_core::Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push((operation.to_string(), request.clone()));
            Ok(match operation {
                "ListServices" => {
                    json!({"serviceArns": (0..self.services).map(|i| format!("arn:aws:ecs:r:1:service/c/s{i}")).collect::<Vec<_>>()})
                }
                "DescribeServices" => {
                    json!({"services": request["services"].as_array().unwrap().iter().map(|arn| {
                    let arn = arn.as_str().unwrap();
                    json!({"serviceName": arn.rsplit('/').next().unwrap(), "serviceArn": arn, "status": "ACTIVE", "desiredCount": 1, "runningCount": 1})
                }).collect::<Vec<_>>()})
                }
                "DescribeLogGroups" => json!({"logGroups": [
                    {"logGroupName": "/ecs/web"}, {"logGroupName": "/ecs/operator"}]}),
                other => panic!("unexpected {other}"),
            })
        }
    }

    #[tokio::test]
    async fn services_are_described_ten_at_a_time_and_capped() {
        let fake = Fake {
            calls: Default::default(),
            services: 25,
        };
        let (samples, complete) = collect_services(&fake, "c").await.unwrap();
        assert_eq!(samples.len(), 25);
        assert!(complete);
        let describes: Vec<usize> = fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(op, _)| op == "DescribeServices")
            .map(|(_, request)| request["services"].as_array().unwrap().len())
            .collect();
        assert_eq!(describes, [10, 10, 5]);

        let many = Fake {
            calls: Default::default(),
            services: MAX_SERVICES + 20,
        };
        let (samples, complete) = collect_services(&many, "c").await.unwrap();
        assert_eq!(samples.len(), MAX_SERVICES);
        assert!(!complete);
    }

    #[tokio::test]
    async fn the_operators_own_log_group_is_not_read() {
        let fake = Fake {
            calls: Default::default(),
            services: 0,
        };
        let groups = list_log_groups(&fake, "/ecs/", Some("/ecs/operator"))
            .await
            .unwrap();
        assert_eq!(groups, ["/ecs/web"]);
    }

    #[test]
    fn log_prefix_must_start_with_a_slash() {
        std::env::set_var("OPERATOR_ECS_LOG_GROUP_PREFIX", "ecs/");
        let result = config_from_env();
        std::env::remove_var("OPERATOR_ECS_LOG_GROUP_PREFIX");
        assert!(result.is_err());
    }
}
