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
pub(crate) const SCOPE: &str = "ecs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcsConfig {
    pub cluster: Option<String>,
    pub log_group_prefix: Option<String>,
    /// The operator's own log group, never read back as input.
    pub own_log_group: Option<String>,
    /// The operator found an enrolled identity when it started (a restart or a
    /// replaced task), as opposed to enrolling for the first time.
    pub enrolled_at_start: bool,
    /// How far back a group with no saved position is read when the operator
    /// started already enrolled. Saved positions live on the task's own disk and
    /// are lost when the task is replaced.
    pub log_lookback_ms: i64,
}

const DEFAULT_LOG_LOOKBACK_MINUTES: i64 = 15;
const MAX_LOG_LOOKBACK_MINUTES: i64 = 24 * 60;

/// Where a group with no saved position starts: the operator's own start on the
/// first-ever start, or a bounded lookback before now when it started already enrolled.
pub(crate) fn initial_log_start_ms(now_ms: i64, enrolled_at_start: bool, lookback_ms: i64) -> i64 {
    if enrolled_at_start {
        now_ms - lookback_ms
    } else {
        now_ms
    }
}

fn log_lookback_ms_from_env() -> Result<i64> {
    let minutes = match env_value("OPERATOR_ECS_LOG_LOOKBACK_MINUTES") {
        None => DEFAULT_LOG_LOOKBACK_MINUTES,
        Some(value) => value
            .parse::<i64>()
            .ok()
            .filter(|m| (0..=MAX_LOG_LOOKBACK_MINUTES).contains(m))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "OPERATOR_ECS_LOG_LOOKBACK_MINUTES must be a number of minutes from 0 to {MAX_LOG_LOOKBACK_MINUTES}"
                    ),
                })
            })?,
    };
    Ok(minutes * 60_000)
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
        enrolled_at_start: false,
        log_lookback_ms: log_lookback_ms_from_env()?,
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
            let (enrolled_at_start, lookback_ms) =
                (config.enrolled_at_start, config.log_lookback_ms);
            handles.push(tokio::spawn(async move {
                run_logs(state, logs, prefix, own, enrolled_at_start, lookback_ms).await
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

/// How far behind the newest stored event a late event may still arrive and be
/// stored. A group with several streams delivers events out of order.
const LATE_EVENT_WINDOW_MS: i64 = 30_000;
/// Ids kept for de-duplication inside the window. A group that writes more than
/// this within the window forgets its oldest ids and stops reading at or before
/// the newest forgotten time (`cutoff_ms`): a later event that old is not
/// collected, but nothing is read twice.
const MAX_SEEN_EVENTS: usize = 1000;
/// Ids sharing the newest millisecond are never forgotten before this many, so a
/// burst in one millisecond is moved past whole.
const MAX_SEEN_EVENTS_IN_BURST: usize = 10_000;

/// The saved position: the newest event time in milliseconds, and the time and id
/// of every stored event within the late-event window before it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LogPosition {
    pub timestamp_ms: i64,
    pub seen: Vec<(i64, String)>,
    /// Events at or before this time count as stored: ids this old were forgotten.
    pub cutoff_ms: i64,
    /// A read of the group that is not finished: where to go on.
    pub chain: Option<Chain>,
}

/// An unfinished read of a group: the query start, the page token and how many
/// events of that page were already taken. It is what lets a read of any number
/// of events in one millisecond end without remembering every id: the position
/// in the result is kept, not the events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Chain {
    pub start_ms: i64,
    pub token: Option<String>,
    pub skip: usize,
    /// The newest time whose ids were forgotten during this read. It becomes
    /// `cutoff_ms` only when the read has reached the end of the result, so the
    /// cutoff never passes a time that still has unread events.
    pub forgotten_ms: i64,
}

impl LogPosition {
    pub(crate) fn encode(&self) -> String {
        let seen: Vec<String> = self
            .seen
            .iter()
            .map(|(ts, id)| format!("{ts}:{id}"))
            .collect();
        let mut text = format!(
            "{}|{}|{}",
            self.timestamp_ms,
            seen.join(","),
            self.cutoff_ms
        );
        if let Some(chain) = &self.chain {
            let token: String = chain
                .token
                .as_deref()
                .unwrap_or_default()
                .bytes()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            text.push_str(&format!(
                "|{}~{}~{}~{}~{}",
                chain.start_ms,
                chain.skip,
                chain.forgotten_ms,
                u8::from(chain.token.is_some()),
                token
            ));
        }
        text
    }

    pub(crate) fn decode(text: &str) -> Option<Self> {
        let mut fields = text.splitn(4, '|');
        let timestamp = fields.next()?;
        let seen = fields.next()?;
        // A position saved before the cutoff existed has two fields.
        let cutoff_ms = match fields.next() {
            Some(cutoff) => cutoff.parse().ok()?,
            None => 0,
        };
        let mut parsed = Vec::new();
        for entry in seen.split(',').filter(|entry| !entry.is_empty()) {
            let (ts, id) = entry.split_once(':')?;
            parsed.push((ts.parse().ok()?, id.to_string()));
        }
        let chain = match fields.next() {
            Some(chain) => {
                let mut parts = chain.splitn(5, '~');
                let start_ms = parts.next()?.parse().ok()?;
                let skip = parts.next()?.parse().ok()?;
                let forgotten_ms = parts.next()?.parse().ok()?;
                let has_token = parts.next()? == "1";
                let hex = parts.next()?;
                let token = if has_token {
                    let bytes: Option<Vec<u8>> = (0..hex.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
                        .collect();
                    Some(String::from_utf8(bytes?).ok()?)
                } else {
                    None
                };
                Some(Chain {
                    start_ms,
                    token,
                    skip,
                    forgotten_ms,
                })
            }
            None => None,
        };
        Some(Self {
            timestamp_ms: timestamp.parse().ok()?,
            seen: parsed,
            cutoff_ms,
            chain,
        })
    }

    /// The `startTime` to ask for: the start of the window, or just after the
    /// newest forgotten time.
    pub(crate) fn start_time(&self) -> i64 {
        let window = self.timestamp_ms - LATE_EVENT_WINDOW_MS;
        if self.cutoff_ms > 0 {
            window.max(self.cutoff_ms + 1)
        } else {
            window
        }
    }

    /// Whether an event was stored before (or is older than anything kept).
    pub(crate) fn already_stored(&self, timestamp_ms: i64, event_id: &str) -> bool {
        // Inside a read the query start is the read's own, not the cutoff's.
        let start = self
            .chain
            .as_ref()
            .map_or(self.start_time(), |c| c.start_ms);
        timestamp_ms < start || self.seen.iter().any(|(_, id)| id == event_id)
    }

    /// The position after storing an event.
    pub(crate) fn advance(&mut self, timestamp_ms: i64, event_id: &str) {
        self.timestamp_ms = self.timestamp_ms.max(timestamp_ms);
        self.seen.push((timestamp_ms, event_id.to_string()));
        let floor = self.timestamp_ms - LATE_EVENT_WINDOW_MS;
        self.seen.retain(|(ts, _)| *ts >= floor);
        let newest = self
            .seen
            .iter()
            .filter(|(ts, _)| *ts == self.timestamp_ms)
            .count();
        let keep = MAX_SEEN_EVENTS.max(newest.min(MAX_SEEN_EVENTS_IN_BURST));
        if self.seen.len() > keep {
            self.seen.sort_by_key(|(ts, _)| *ts);
            let excess = self.seen.len() - keep;
            if let Some((forgotten, _)) = self.seen[..excess].last() {
                match &mut self.chain {
                    Some(chain) => chain.forgotten_ms = chain.forgotten_ms.max(*forgotten),
                    None => self.cutoff_ms = self.cutoff_ms.max(*forgotten),
                }
            }
            self.seen.drain(..excess);
        }
    }
}

/// One page of `FilterLogEvents`: the new records, the position after them and
/// the next page token.
pub(crate) struct ParsedPage {
    pub records: Vec<PodLogRecord>,
    pub position: LogPosition,
    pub next_token: Option<String>,
    /// The page was cut at the batch byte limit; read the rest of it next.
    pub cut: bool,
    /// Events of the page taken (stored or skipped, counting those skipped
    /// before): the index where the rest begins.
    pub taken: usize,
}

pub(crate) fn parse_events_page(group: &str, page: &Value, saved: &LogPosition) -> ParsedPage {
    parse_events_page_from(group, page, saved, 0)
}

/// Like `parse_events_page`, for a page whose first `skip` events were taken before.
pub(crate) fn parse_events_page_from(
    group: &str,
    page: &Value,
    saved: &LogPosition,
    skip: usize,
) -> ParsedPage {
    let mut position = saved.clone();
    let mut taken = skip;
    let mut records = Vec::new();
    let mut bytes = 0;
    let mut cut = false;
    for event in page
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .skip(skip)
    {
        let (Some(timestamp), Some(id), Some(message)) = (
            event.get("timestamp").and_then(Value::as_i64),
            event.get("eventId").and_then(Value::as_str),
            event.get("message").and_then(Value::as_str),
        ) else {
            taken += 1;
            continue;
        };
        if position.already_stored(timestamp, id) {
            taken += 1;
            continue;
        }
        if bytes >= MAX_BATCH_BYTES {
            // This event is not taken: the next read of the page begins here.
            cut = true;
            break;
        }
        taken += 1;
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
        taken,
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
        // Said when the prefix goes over the limit, not on every poll.
        if !OVER_GROUP_LIMIT_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            warn!(
                max = MAX_LOG_GROUPS,
                "More log groups under the prefix than the limit; the rest are not collected"
            );
        }
        names.truncate(MAX_LOG_GROUPS);
    } else {
        OVER_GROUP_LIMIT_WARNED.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(names)
}

/// Whether the warning about more log groups than the limit was given and the
/// count has not come back under it since.
static OVER_GROUP_LIMIT_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Groups for which a read that used its whole page scan without finding a
/// new event was already reported.
static UNPRODUCTIVE_GROUPS_WARNED: std::sync::Mutex<Vec<String>> =
    std::sync::Mutex::new(Vec::new());

/// Pages scanned in one poll that held no new event are not counted against
/// the page budget (a window of already-stored events must not use it up), but
/// they are bounded so one poll cannot read for ever.
const MAX_SCANNED_PAGES: usize = 100;

/// One queued batch: the records and the position saved with them. A batch
/// without records only moves the saved position (progress over pages that held
/// nothing new).
pub(crate) struct LogBatch {
    pub records: Vec<PodLogRecord>,
    pub position: LogPosition,
}

/// Read what is new in one group, from `position`, in batches. At most
/// `MAX_PAGES_PER_POLL` pages with new events are taken per call; pages that
/// hold only events stored before do not count, so a busy group's re-read
/// window can never keep a poll from reaching what is new.
pub(crate) async fn read_group(
    logs: &dyn EcsApi,
    group: &str,
    mut position: LogPosition,
) -> Result<Vec<LogBatch>> {
    let mut batches = Vec::new();
    let mut productive_pages = 0;
    let mut scanned = 0;
    let start = position.clone();
    // A read goes on where the saved one stopped; otherwise it starts at the
    // window. A token the service no longer accepts is given up once, and the
    // read starts again from its first page: events stored before whose ids
    // were forgotten may then repeat, none is skipped.
    let mut chain = position.chain.take().unwrap_or_else(|| Chain {
        start_ms: position.start_time(),
        ..Default::default()
    });
    // Only the token this call started with can be stale; the ones it was just given are fresh.
    let mut saved_token = chain.token.is_some();
    for _ in 0..MAX_SCANNED_PAGES {
        scanned += 1;
        position.chain = Some(chain.clone());
        let mut request = json!({
            "logGroupName": group,
            "startTime": chain.start_ms,
            "limit": PAGE_LIMIT,
        });
        if let Some(token) = &chain.token {
            request["nextToken"] = json!(token);
        }
        let page = match logs.call("FilterLogEvents", request).await {
            Ok(page) => page,
            Err(error)
                if chain.token.is_some()
                    && saved_token
                    && (error.code == "INVALID_INPUT"
                        || matches!(
                            error.error,
                            Some(alien_client_core::ErrorData::InvalidInput { .. })
                        )) =>
            {
                warn!(
                    group,
                    "A saved page token was refused; reading the group again from its start"
                );
                saved_token = false;
                chain.token = None;
                chain.skip = 0;
                continue;
            }
            Err(error) => {
                return Err(error).context(ErrorData::PodLogReadFailed {
                    message: format!("could not read log group {group}"),
                })
            }
        };
        saved_token = false;
        let parsed = parse_events_page_from(group, &page, &position, chain.skip);
        let mut after = parsed.position;
        // The parse updated the forgotten time inside the chain it was given.
        chain = after.chain.take().unwrap_or(chain);
        if parsed.cut {
            // The rest of this page is read next.
            chain.skip = parsed.taken;
            after.chain = Some(chain.clone());
        } else if let Some(next) = page.get("nextToken").and_then(Value::as_str) {
            chain.token = Some(next.to_string());
            chain.skip = 0;
            after.chain = Some(chain.clone());
        } else {
            // The end of the result: what was forgotten during the read now
            // bounds the next one.
            after.cutoff_ms = after.cutoff_ms.max(chain.forgotten_ms);
        }
        let productive = !parsed.records.is_empty();
        if productive {
            productive_pages += 1;
            batches.push(LogBatch {
                records: parsed.records,
                position: after.clone(),
            });
        }
        let finished = after.chain.is_none();
        position = after;
        if finished {
            break;
        }
        if productive_pages >= MAX_PAGES_PER_POLL {
            break;
        }
    }
    // Progress over pages that held nothing new is saved too: otherwise the next
    // poll would read the same pages again and a window of stored events would
    // keep it from ever reaching what is new.
    let last_saved = batches.last().map(|batch| batch.position.encode());
    if last_saved.as_deref() != Some(position.encode().as_str()) && position != start {
        batches.push(LogBatch {
            records: Vec::new(),
            position,
        });
    }
    if scanned == MAX_SCANNED_PAGES && productive_pages == 0 {
        let mut warned = UNPRODUCTIVE_GROUPS_WARNED
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !warned.iter().any(|name| name == group) {
            warned.push(group.to_string());
            warn!(
                group,
                pages = MAX_SCANNED_PAGES,
                "A poll read many pages of a log group without finding a new event; it continues from there at the next poll"
            );
        }
    }
    Ok(batches)
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
    let position = state
        .db
        .get_pod_log_offset(&key, "logs", 0)
        .await?
        .and_then(|offset| LogPosition::decode(&offset.timestamp))
        .unwrap_or(LogPosition {
            timestamp_ms: started_ms + LATE_EVENT_WINDOW_MS,
            ..Default::default()
        });
    let mut total = 0;
    for batch in read_group(logs, group, position).await? {
        total += batch.records.len();
        let offset = PodLogOffset {
            timestamp: batch.position.encode(),
            ordinal: 0,
        };
        if batch.records.is_empty() {
            state
                .db
                .save_pod_log_offset(&key, "logs", 0, &offset)
                .await?;
            continue;
        }
        let encoded = pod_log_records_to_otlp(batch.records, deployment_id)?;
        state
            .db
            .store_pod_log_batch(
                &key,
                "logs",
                0,
                &PodLogOffset {
                    timestamp: batch.position.encode(),
                    ordinal: 0,
                },
                &encoded,
            )
            .await?;
    }
    Ok(total)
}

async fn run_logs(
    state: Arc<OperatorState>,
    logs: Arc<dyn EcsApi>,
    prefix: String,
    own: Option<String>,
    enrolled_at_start: bool,
    log_lookback_ms: i64,
) {
    // The first-ever start begins at its own start. A start that finds an
    // enrolled identity (a replaced task lost its saved positions) reads back a
    // bounded lookback, de-duplicated by event id.
    let started_ms = initial_log_start_ms(
        Utc::now().timestamp_millis(),
        enrolled_at_start,
        log_lookback_ms,
    );
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
        let before = samples.len();
        samples.extend(
            described
                .get("services")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(service_sample),
        );
        // A listed service that came back as a failure, or not at all, is missing
        // from this batch, not gone: the batch must not replace the scope.
        let failed = described
            .get("failures")
            .and_then(Value::as_array)
            .is_some_and(|failures| !failures.is_empty());
        if failed || samples.len() - before < chunk.len() {
            complete = false;
        }
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
                    state
                        .db
                        .update_pending_observed_inventory_batches(|pending| {
                            crate::host::merge_batches(pending, batch)
                        })
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
            seen: vec![(1690, "a".into()), (1700, "b".into())],
            cutoff_ms: 1500,
            chain: Some(Chain {
                start_ms: 1600,
                token: Some("a|b,c~d é".into()),
                skip: 7,
                forgotten_ms: 1650,
            }),
        };
        assert_eq!(LogPosition::decode(&position.encode()), Some(position));
        let no_token = LogPosition {
            chain: Some(Chain {
                start_ms: 5,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(LogPosition::decode(&no_token.encode()), Some(no_token));
        assert_eq!(
            LogPosition::decode("5|"),
            Some(LogPosition {
                timestamp_ms: 5,
                ..Default::default()
            })
        );
        assert_eq!(LogPosition::decode("5|7:a|9").unwrap().cutoff_ms, 9);
        assert_eq!(LogPosition::decode("garbage"), None);
    }

    #[test]
    fn resume_drops_stored_events_and_keeps_new_ones_at_the_same_millisecond() {
        let saved = LogPosition {
            timestamp_ms: 100,
            seen: vec![(100, "a".into())],
            ..Default::default()
        };
        let parsed = parse_events_page(
            "/ecs/web",
            &page(
                &[
                    (99, "z", "late but new"),
                    (100, "a", "stored"),
                    (100, "b", "same ms"),
                    (101, "c", "new"),
                ],
                None,
            ),
            &saved,
        );
        let bodies: Vec<_> = parsed.records.iter().map(|r| r.body.as_str()).collect();
        assert_eq!(bodies, ["late but new", "same ms", "new"]);
        assert_eq!(parsed.position.timestamp_ms, 101);
        assert_eq!(parsed.records[1].timestamp_unix_nanos, 100_000_000);
        assert_eq!(parsed.records[0].container, "/ecs/web");
    }

    #[test]
    fn an_event_from_a_slower_stream_is_collected_inside_the_window_and_never_twice() {
        // Stream one is read up to t=10_000; stream two's event from t=4_000 arrives later.
        let first = parse_events_page(
            "/ecs/web",
            &page(&[(9_000, "a", "one"), (10_000, "b", "two")], None),
            &LogPosition::default(),
        );
        assert_eq!(first.records.len(), 2);
        let second = parse_events_page(
            "/ecs/web",
            &page(
                &[
                    (4_000, "late", "late"),
                    (9_000, "a", "one"),
                    (10_000, "b", "two"),
                ],
                None,
            ),
            &first.position,
        );
        let bodies: Vec<_> = second.records.iter().map(|r| r.body.as_str()).collect();
        assert_eq!(bodies, ["late"]);
        // Older than the window: not collected, and the query no longer asks for it.
        assert_eq!(second.position.start_time(), 10_000 - LATE_EVENT_WINDOW_MS);
        let too_late = parse_events_page(
            "/ecs/web",
            &page(&[(10_000 - LATE_EVENT_WINDOW_MS - 1, "x", "x")], None),
            &second.position,
        );
        assert!(too_late.records.is_empty());
    }

    #[test]
    fn the_id_list_is_pruned_to_the_window_and_capped() {
        let mut position = LogPosition::default();
        position.advance(1_000, "old");
        position.advance(1_000 + LATE_EVENT_WINDOW_MS + 5, "new");
        assert_eq!(position.seen.len(), 1);
        let mut busy = LogPosition::default();
        for i in 0..(MAX_SEEN_EVENTS as i64 + 50) {
            busy.advance(1_000_000 + i, &format!("e{i}"));
        }
        assert_eq!(busy.seen.len(), MAX_SEEN_EVENTS);
        // Full list: the query starts after the newest forgotten time, so nothing is read twice.
        assert_eq!(busy.start_time(), 1_000_000 + 50);
        assert!(busy.seen.iter().all(|(ts, _)| *ts >= busy.start_time()));
    }

    #[test]
    fn a_burst_in_one_millisecond_is_moved_past_without_reading_events_again() {
        let events: Vec<(i64, String, String)> = (0..1500)
            .map(|i| (5_000, format!("id{i}"), format!("m{i}")))
            .collect();
        let refs: Vec<(i64, &str, &str)> = events
            .iter()
            .map(|(t, i, m)| (*t, i.as_str(), m.as_str()))
            .collect();
        let first = parse_events_page("/ecs/web", &page(&refs, None), &LogPosition::default());
        assert_eq!(first.records.len(), 1500);
        // The same events come back from the query window: none is new.
        let saved = LogPosition::decode(&first.position.encode()).unwrap();
        assert!(saved.start_time() <= 5_000);
        let again = parse_events_page("/ecs/web", &page(&refs, None), &saved);
        assert!(again.records.is_empty(), "{} re-read", again.records.len());
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

    #[derive(Debug)]
    struct Partial;

    #[async_trait::async_trait]
    impl EcsApi for Partial {
        async fn call(&self, operation: &str, request: Value) -> alien_client_core::Result<Value> {
            Ok(match operation {
                "ListServices" => json!({"serviceArns": [
                    "arn:aws:ecs:r:1:service/c/a", "arn:aws:ecs:r:1:service/c/b", "arn:aws:ecs:r:1:service/c/c"]}),
                "DescribeServices" if request["services"].as_array().unwrap().len() == 3 => json!({
                    "services": [{"serviceName": "a", "serviceArn": "arn:aws:ecs:r:1:service/c/a", "status": "ACTIVE", "desiredCount": 1, "runningCount": 1}],
                    "failures": [{"arn": "arn:aws:ecs:r:1:service/c/b", "reason": "MISSING"}]
                }),
                other => panic!("unexpected {other}"),
            })
        }
    }

    #[tokio::test]
    async fn a_partial_describe_answer_is_not_a_complete_inventory() {
        let (samples, complete) = collect_services(&Partial, "c").await.unwrap();
        // Two of three came back (one answer, one failure, one silently absent).
        assert_eq!(samples.len(), 1);
        assert!(!complete);
    }

    /// A group with more events inside the late-event window than one poll may
    /// take. Each poll must move on to the events after what it stored.
    #[derive(Debug)]
    struct BusyGroup {
        total: usize,
    }

    #[async_trait::async_trait]
    impl EcsApi for BusyGroup {
        async fn call(&self, _op: &str, request: Value) -> alien_client_core::Result<Value> {
            let start = request["startTime"].as_i64().unwrap();
            let first: usize = request
                .get("nextToken")
                .and_then(Value::as_str)
                .map_or(0, |token| token.parse().unwrap());
            // Events are one per millisecond from t=1000 (all inside one 30 s window).
            let events: Vec<Value> = (0..self.total)
                .map(|i| (i, 1000 + i as i64))
                .filter(|(_, ts)| *ts >= start)
                .skip(first)
                .take(PAGE_LIMIT as usize)
                .map(|(i, ts)| json!({"timestamp": ts, "eventId": format!("e{i}"), "message": format!("m{i}"), "logStreamName": "s"}))
                .collect();
            let eligible = (0..self.total)
                .filter(|i| 1000 + *i as i64 >= start)
                .count();
            let mut page = json!({ "events": events });
            if first + PAGE_LIMIT as usize > 0 && first + (PAGE_LIMIT as usize) < eligible {
                page["nextToken"] = json!((first + PAGE_LIMIT as usize).to_string());
            }
            Ok(page)
        }
    }

    /// One page, no token, of events with lines big enough that the page
    /// holds several batches: every poll is cut at the batch limit.
    #[derive(Debug)]
    struct BigLines {
        total: usize,
        line: String,
    }

    #[async_trait::async_trait]
    impl EcsApi for BigLines {
        async fn call(&self, _op: &str, request: Value) -> alien_client_core::Result<Value> {
            let start = request["startTime"].as_i64().unwrap();
            // Forty seconds apart: older than the late-event window of the
            // newest event, so an event stored earlier is no longer in the
            // position's list of stored ids and only the page index tells that
            // it was taken.
            let events: Vec<Value> = (0..self.total)
                .map(|i| (i, 1000 + 40_000 * i as i64))
                .filter(|(_, ts)| *ts >= start)
                .map(|(i, ts)| json!({"timestamp": ts, "eventId": format!("e{i}"), "message": format!("{i}:{}", self.line), "logStreamName": "s"}))
                .collect();
            Ok(json!({ "events": events }))
        }
    }

    /// A page cut at the batch limit resumes after what it stored, not at its
    /// first event: with a page of large lines the same first events were
    /// stored again on every poll and the group never advanced.
    #[tokio::test]
    async fn a_page_larger_than_the_batch_limit_is_stored_once_over_several_polls() {
        // Two lines fill a batch; 12 events are 6 batches, two polls of 3 pages.
        let group = BigLines {
            total: 12,
            line: "x".repeat(MAX_BATCH_BYTES / 2),
        };
        // A replaced task's lookback: the start is before every event.
        let mut position = LogPosition {
            timestamp_ms: 1000,
            ..Default::default()
        };
        let mut delivered: Vec<String> = Vec::new();
        let mut positions = Vec::new();
        for _ in 0..6 {
            let batches = read_group(&group, "g", position.clone()).await.unwrap();
            for batch in &batches {
                delivered.extend(
                    batch
                        .records
                        .iter()
                        .map(|r| r.body.split(':').next().unwrap().to_string()),
                );
            }
            if let Some(last) = batches.last() {
                // Saved and read back, as a restart does.
                position = LogPosition::decode(&last.position.encode()).unwrap();
            }
            positions.push(position.encode());
        }
        let expected: Vec<String> = (0..12).map(|i| i.to_string()).collect();
        assert_eq!(delivered, expected, "each event once, in order");
        // The position moved on poll after poll until the page was done.
        assert_ne!(positions[0], positions[1]);
        assert!(position.chain.is_none(), "the read reached the end");
    }

    /// 150 pages of events older than the query start (already stored or too
    /// old), joined by tokens that are page numbers.
    #[derive(Debug)]
    struct OldPages;

    #[async_trait::async_trait]
    impl EcsApi for OldPages {
        async fn call(&self, _op: &str, request: Value) -> alien_client_core::Result<Value> {
            let page: usize = request
                .get("nextToken")
                .and_then(Value::as_str)
                .map_or(0, |token| token.parse().unwrap());
            let mut body = json!({"events": [
                {"timestamp": 1, "eventId": format!("old{page}"), "message": "m", "logStreamName": "s"}
            ]});
            if page + 1 < 150 {
                body["nextToken"] = json!((page + 1).to_string());
            }
            Ok(body)
        }
    }

    /// A poll that went through pages with nothing new saves how far it got, so
    /// the next poll goes on from there instead of reading the same pages.
    #[tokio::test]
    async fn progress_over_pages_with_nothing_new_is_saved() {
        let start = LogPosition {
            timestamp_ms: 1_000_000,
            ..Default::default()
        };
        let first = read_group(&OldPages, "g", start).await.unwrap();
        assert_eq!(first.len(), 1);
        assert!(first[0].records.is_empty());
        let saved = LogPosition::decode(&first[0].position.encode()).unwrap();
        assert_eq!(saved.chain.as_ref().unwrap().token.as_deref(), Some("100"));
        let second = read_group(&OldPages, "g", saved).await.unwrap();
        assert!(second.iter().all(|batch| batch.records.is_empty()));
        assert!(second
            .last()
            .is_none_or(|batch| batch.position.chain.is_none()));
    }

    #[tokio::test]
    async fn a_busy_group_is_read_to_the_end_over_several_polls() {
        let group = BusyGroup { total: 700 };
        let mut position = LogPosition {
            timestamp_ms: 1000,
            ..Default::default()
        };
        let mut stored = 0;
        for _ in 0..10 {
            let batches = read_group(&group, "g", position.clone()).await.unwrap();
            if let Some(last) = batches.last() {
                position = last.position.clone();
            }
            stored += batches.iter().map(|b| b.records.len()).sum::<usize>();
        }
        assert_eq!(stored, 700, "every event is stored exactly once");
    }

    /// Every event shares one millisecond; pages are cut by a token that is an
    /// index, like a real group's opaque cursor.
    #[derive(Debug)]
    struct OneMillisecond {
        total: usize,
        at_ms: i64,
    }

    #[async_trait::async_trait]
    impl EcsApi for OneMillisecond {
        async fn call(&self, _op: &str, request: Value) -> alien_client_core::Result<Value> {
            if request["startTime"].as_i64().unwrap() > self.at_ms {
                return Ok(json!({"events": []}));
            }
            let first: usize = request
                .get("nextToken")
                .and_then(Value::as_str)
                .map_or(0, |token| token.parse().unwrap());
            let end = (first + PAGE_LIMIT as usize).min(self.total);
            let events: Vec<Value> = (first..end)
                .map(|i| json!({"timestamp": self.at_ms, "eventId": format!("e{i}"), "message": format!("m{i}"), "logStreamName": "s"}))
                .collect();
            let mut page = json!({ "events": events });
            if end < self.total {
                page["nextToken"] = json!(end.to_string());
            }
            Ok(page)
        }
    }

    #[tokio::test]
    async fn more_events_than_ids_kept_in_one_millisecond_are_each_delivered_once_across_restarts()
    {
        let total = MAX_SEEN_EVENTS_IN_BURST + 500;
        let group = OneMillisecond {
            total,
            at_ms: 50_000,
        };
        let mut position = LogPosition::default();
        let mut delivered: Vec<String> = Vec::new();
        for poll in 0..400 {
            let batches = read_group(&group, "g", position.clone()).await.unwrap();
            if batches.is_empty() && poll > 5 {
                break;
            }
            for batch in &batches {
                delivered.extend(batch.records.iter().map(|r| r.body.clone()));
            }
            if let Some(last) = batches.last() {
                // Saved and read back, as a restart does.
                position = LogPosition::decode(&last.position.encode()).unwrap();
            }
        }
        let unique: std::collections::BTreeSet<_> = delivered.iter().collect();
        assert_eq!(
            delivered.len(),
            unique.len(),
            "an event was delivered twice"
        );
        assert_eq!(unique.len(), total, "an event was never delivered");
    }

    /// A group that refuses one saved page token, as a service does with an old one.
    #[derive(Debug)]
    struct RefusesTokens(OneMillisecond);

    #[async_trait::async_trait]
    impl EcsApi for RefusesTokens {
        async fn call(&self, op: &str, request: Value) -> alien_client_core::Result<Value> {
            if request.get("nextToken").and_then(Value::as_str) == Some("old") {
                return Err(alien_error::AlienError::new(
                    alien_client_core::ErrorData::InvalidInput {
                        message: "Invalid nextToken".to_string(),
                        field_name: None,
                    },
                ));
            }
            self.0.call(op, request).await
        }
    }

    #[tokio::test]
    async fn a_refused_page_token_reads_the_group_again_from_its_start_and_loses_nothing() {
        let group = RefusesTokens(OneMillisecond {
            total: 250,
            at_ms: 50_000,
        });
        // Saved mid-read, with a token the service no longer accepts.
        let saved = LogPosition {
            timestamp_ms: 50_000,
            chain: Some(Chain {
                start_ms: 49_000,
                token: Some("old".into()),
                skip: 0,
                forgotten_ms: 0,
            }),
            ..Default::default()
        };
        let batches = read_group(&group, "g", saved).await.unwrap();
        // Three pages are read per poll, from the first page.
        let delivered: usize = batches.iter().map(|b| b.records.len()).sum();
        assert_eq!(delivered, 250);
        assert!(batches.last().unwrap().position.chain.is_none());
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
    fn a_replaced_task_reads_back_a_bounded_lookback_and_a_first_start_begins_now() {
        let now = 10_000_000;
        assert_eq!(initial_log_start_ms(now, false, 900_000), now);
        assert_eq!(initial_log_start_ms(now, true, 900_000), now - 900_000);
        assert_eq!(initial_log_start_ms(now, true, 0), now);
    }

    #[test]
    fn the_lookback_setting_is_bounded() {
        // Parsed through the same rule as the environment value.
        for bad in ["-1", "abc", "100000"] {
            std::env::set_var("OPERATOR_ECS_LOG_LOOKBACK_MINUTES", bad);
            assert!(log_lookback_ms_from_env().is_err(), "{bad}");
        }
        std::env::set_var("OPERATOR_ECS_LOG_LOOKBACK_MINUTES", "30");
        assert_eq!(log_lookback_ms_from_env().unwrap(), 30 * 60_000);
        std::env::remove_var("OPERATOR_ECS_LOG_LOOKBACK_MINUTES");
        assert_eq!(log_lookback_ms_from_env().unwrap(), 15 * 60_000);
    }

    #[test]
    fn log_prefix_must_start_with_a_slash() {
        std::env::set_var("OPERATOR_ECS_LOG_GROUP_PREFIX", "ecs/");
        let result = config_from_env();
        std::env::remove_var("OPERATOR_ECS_LOG_GROUP_PREFIX");
        assert!(result.is_err());
    }
}
