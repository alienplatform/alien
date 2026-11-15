//! Log collection on a machine or Docker host, beside the Kubernetes Pod log
//! collector in `pod_logs.rs`.
//!
//! `OPERATOR_HOST_LOGS` is a comma list of sources, `journald` and `docker`.
//! Unknown values refuse to start. Both sources use the bounds and the queue of
//! the Pod log collector: a batch and its resume cursor are committed to the
//! local database together, and a full queue closes the source until it drains.
//!
//! * journald: one `journalctl -f -o json` child. The cursor is the journal's
//!   own `__CURSOR`, saved with each batch.
//! * Docker: one Engine API log follow per running container, reconnecting from
//!   the timestamp of the last stored line.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
    time::Duration,
};

use alien_error::{AlienError, Context, IntoAlienError};
use chrono::{DateTime, SecondsFormat, Utc};
use futures::StreamExt;
use serde_json::Value;
use tokio::{io::AsyncBufReadExt, task::JoinHandle, time};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{
    collector_logs::{pod_log_records_to_otlp, PodLogRecord},
    db::{OperatorDb, PodLogOffset},
    error::{ErrorData, Result},
    host::docker_client,
    pod_logs::{MAX_BATCH_BYTES, MAX_BATCH_RECORDS, MAX_LINE_BYTES, RECONNECT_DELAY},
    OperatorState,
};

const DOCKER_DISCOVERY_INTERVAL: Duration = Duration::from_secs(10);
/// Most containers followed at once; the rest are skipped with one warning.
const MAX_DOCKER_STREAMS: usize = 64;
/// Cursor keys in the shared offsets table.
const JOURNALD_KEY: &str = "host:journald";
const DOCKER_KEY_PREFIX: &str = "host:docker:";
/// Resource attribute values for host sources.
const JOURNAL_SCOPE: &str = "journald";
const DOCKER_SCOPE: &str = "docker";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostLogsConfig {
    pub journald: bool,
    pub docker: bool,
}

/// Parse `OPERATOR_HOST_LOGS`. Unset means Docker logs when the operator runs
/// in a container with the Docker socket mounted and was given an enrollment
/// token (a new install), and nothing otherwise: an install that predates this
/// collector collects nothing new after an upgrade.
pub fn parse_sources(
    value: Option<&str>,
    in_container: bool,
    socket_present: bool,
    enrolled_by_token: bool,
) -> Result<Option<HostLogsConfig>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(
            (in_container && socket_present && enrolled_by_token).then_some(HostLogsConfig {
                journald: false,
                docker: true,
            }),
        );
    };
    let mut config = HostLogsConfig::default();
    for source in value.split(',').map(str::trim) {
        match source {
            "journald" => config.journald = true,
            "docker" => config.docker = true,
            "none" if value == "none" => return Ok(None),
            other => {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "OPERATOR_HOST_LOGS has the unknown source '{other}'. Use a comma list of: journald, docker."
                    ),
                }))
            }
        }
    }
    Ok(Some(config))
}

/// `enrolled_by_token` is whether this start was given an enrollment token by
/// any route (flag, file or environment), as resolved by the command line.
pub fn config_from_env(enrolled_by_token: bool) -> Result<Option<HostLogsConfig>> {
    let value = std::env::var("OPERATOR_HOST_LOGS").ok();
    log_units_from_env()?;
    parse_sources(
        value.as_deref(),
        Path::new("/.dockerenv").exists(),
        Path::new(&crate::host::docker_socket_path()).exists(),
        enrolled_by_token,
    )
}

/// What the inventory reports. The collectors that log sources turned on decide
/// the containers and units; a machine enrolled by token reports the host
/// itself even with no log source.
pub fn inventory_config(
    logs: Option<HostLogsConfig>,
    enrolled_by_token: bool,
) -> Option<HostLogsConfig> {
    logs.or_else(|| enrolled_by_token.then_some(HostLogsConfig::default()))
}

/// `OPERATOR_HOST_LOG_UNITS`: ship the journal of these units only. Unset ships
/// the whole journal. A name with anything but letters, digits and `_ . @ - :`
/// refuses to start.
pub(crate) fn log_units_from_env() -> Result<Vec<String>> {
    parse_log_units(std::env::var("OPERATOR_HOST_LOG_UNITS").ok().as_deref())
}

pub(crate) fn parse_log_units(value: Option<&str>) -> Result<Vec<String>> {
    let mut units = Vec::new();
    for unit in value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
    {
        let valid = !unit.is_empty()
            && unit.len() <= 256
            && unit
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_.@-:".contains(c))
            && !unit.starts_with('-');
        if !valid {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "OPERATOR_HOST_LOG_UNITS has the invalid unit name '{unit}'. Use a comma list of unit names."
                ),
            }));
        }
        units.push(unit.to_string());
    }
    Ok(units)
}

pub async fn run_loop(state: Arc<OperatorState>, config: HostLogsConfig) {
    // New streams begin at operator start: a restart does not replay old logs.
    let started_at = Utc::now();
    if !state.config.is_telemetry_enabled() {
        warn!("Host log collection is enabled but deployment telemetry is off");
        state.cancel.cancelled().await;
        return;
    }
    info!(
        journald = config.journald,
        docker = config.docker,
        "Starting host log collection"
    );
    let mut handles = Vec::new();
    if config.journald {
        let state = state.clone();
        handles.push(tokio::spawn(async move {
            run_journald(state, started_at).await
        }));
    }
    if config.docker {
        let state = state.clone();
        handles.push(tokio::spawn(
            async move { run_docker(state, started_at).await },
        ));
    }
    for handle in handles {
        let _ = handle.await;
    }
    info!("Host log collection stopped");
}

// ---------------------------------------------------------------- journald

/// One journal entry as a log record, with the cursor to resume after it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct JournalEntry {
    pub cursor: String,
    pub record: PodLogRecord,
}

/// Parse one `journalctl -o json` line. Entries without a cursor are dropped;
/// entries written by `own_pid` are skipped so the operator's own output is not
/// fed back into the log it reads.
pub(crate) fn parse_journal_line(line: &str, own_pid: u32) -> Option<JournalEntry> {
    let value: Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    let cursor = object.get("__CURSOR")?.as_str()?.to_string();
    if object
        .get("_PID")
        .and_then(Value::as_str)
        .and_then(|pid| pid.parse::<u32>().ok())
        == Some(own_pid)
    {
        return None;
    }
    // MESSAGE is a string, or an array of bytes when it is not valid UTF-8.
    let body = match object.get("MESSAGE")? {
        Value::String(text) => text.clone(),
        Value::Array(bytes) => {
            let bytes: Vec<u8> = bytes
                .iter()
                .filter_map(|byte| byte.as_u64().and_then(|value| u8::try_from(value).ok()))
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => return None,
    };
    let micros: u64 = object.get("__REALTIME_TIMESTAMP")?.as_str()?.parse().ok()?;
    let unit = ["_SYSTEMD_UNIT", "SYSLOG_IDENTIFIER", "_COMM"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_string();
    let body = truncate_line(body);
    Some(JournalEntry {
        cursor,
        record: PodLogRecord {
            namespace: JOURNAL_SCOPE.to_string(),
            pod: unit.clone(),
            container: unit,
            timestamp_unix_nanos: micros.saturating_mul(1000),
            body,
        },
    })
}

fn truncate_line(mut body: String) -> String {
    if body.len() > MAX_LINE_BYTES {
        let mut end = MAX_LINE_BYTES;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
        body.push_str(" [truncated]");
    }
    body
}

/// The `journalctl` arguments: after the saved cursor, or when there is none
/// from `start_unix_secs`, the moment collection began. The start stays the
/// same across reader restarts until a cursor is saved, so entries that could
/// not be queued yet are read again, not skipped.
pub(crate) fn journalctl_args(
    saved_cursor: Option<&str>,
    units: &[String],
    start_unix_secs: i64,
) -> Vec<String> {
    // --all: without it journalctl prints a field over 4 KB as null, and the entry would be lost.
    // Only the fields the record uses are printed; the cursor and timestamps always are.
    let mut args = vec![
        "--follow".to_string(),
        "--output=json".to_string(),
        "--all".to_string(),
        "--output-fields=MESSAGE,_PID,_SYSTEMD_UNIT,SYSLOG_IDENTIFIER,_COMM".to_string(),
        "--no-pager".to_string(),
    ];
    for unit in units {
        args.push(format!("--unit={unit}"));
    }
    match saved_cursor {
        Some(cursor) => args.push(format!("--after-cursor={cursor}")),
        None => args.push(format!("--since=@{start_unix_secs}")),
    }
    args
}

/// Reader failures in a row, with no entry read, after which a saved cursor is given up.
const CURSOR_FAILURES_BEFORE_RESET: u32 = 5;
/// Longest raw journal line read whole. A longer one is cut here and read past.
const MAX_JOURNAL_LINE_BYTES: usize = 2 * 1024 * 1024;
/// Distinct journalctl error messages logged; each is logged once.
const MAX_LOGGED_STDERR_KINDS: usize = 8;

async fn run_journald(state: Arc<OperatorState>, _started_at: DateTime<Utc>) {
    let units = log_units_from_env().unwrap_or_default();
    let mut failures = 0u32;
    let mut start_unix_secs = Utc::now().timestamp();
    loop {
        let mut progressed = false;
        if let Err(error) =
            read_journal_once(&state, &units, start_unix_secs, &mut progressed).await
        {
            if !state.cancel.is_cancelled() {
                warn!(error = %error, "Journal reader failed; will restart");
            }
        }
        failures = if progressed { 0 } else { failures + 1 };
        if failures >= CURSOR_FAILURES_BEFORE_RESET {
            failures = 0;
            // The journal may no longer accept the saved cursor (rotated away, or corrupt).
            if matches!(
                state
                    .db
                    .get_pod_log_offset(JOURNALD_KEY, "journal", 0)
                    .await,
                Ok(Some(_))
            ) && state
                .db
                .delete_pod_log_offset(JOURNALD_KEY, "journal", 0)
                .await
                .is_ok()
            {
                start_unix_secs = Utc::now().timestamp();
                warn!("The saved journal position is not accepted by the journal; continuing from now");
            }
        }
        tokio::select! {
            _ = state.cancel.cancelled() => return,
            _ = time::sleep(RECONNECT_DELAY) => {}
        }
    }
}

/// One line read within a size bound.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BoundedLine {
    /// At most the bound's bytes of the line (without the newline).
    pub bytes: Vec<u8>,
    /// The line was longer than the bound; the rest was read and dropped.
    pub truncated: bool,
}

/// A line being read. The bytes read so far live here, not in the future, so a
/// read that is dropped (a timer won a `select!`) loses nothing: the next call
/// continues the same line.
#[derive(Debug, Default)]
pub(crate) struct PartialLine {
    bytes: Vec<u8>,
    truncated: bool,
    read_any: bool,
}

impl PartialLine {
    /// Read up to the next newline, keeping at most `max` bytes; the rest of a
    /// longer line is consumed so the next call starts at the next line.
    /// Cancel-safe: the only await is `fill_buf`, and bytes are moved into
    /// `self` before anything else can be awaited.
    pub(crate) async fn read<R>(
        &mut self,
        reader: &mut R,
        max: usize,
    ) -> std::io::Result<Option<BoundedLine>>
    where
        R: tokio::io::AsyncBufRead + Unpin,
    {
        loop {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                let done = std::mem::take(self);
                return Ok(done.read_any.then_some(BoundedLine {
                    bytes: done.bytes,
                    truncated: done.truncated,
                }));
            }
            self.read_any = true;
            let (chunk, found_newline) = match available.iter().position(|byte| *byte == b'\n') {
                Some(index) => (&available[..index], true),
                None => (available, false),
            };
            let room = max.saturating_sub(self.bytes.len());
            if chunk.len() > room {
                self.truncated = true;
            }
            self.bytes
                .extend_from_slice(&chunk[..chunk.len().min(room)]);
            let consumed = chunk.len() + usize::from(found_newline);
            reader.consume(consumed);
            if found_newline {
                let done = std::mem::take(self);
                return Ok(Some(BoundedLine {
                    bytes: done.bytes,
                    truncated: done.truncated,
                }));
            }
        }
    }
}

/// Read one whole line within a size bound (not cancel-safe; use `PartialLine`
/// when the read can be dropped).
#[cfg(test)]
pub(crate) async fn read_bounded_line<R>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Option<BoundedLine>>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    PartialLine::default().read(reader, max).await
}

/// The value of a string field near the start of a journal JSON line. journalctl
/// prints `__CURSOR` first, so it survives in the kept part of an oversize line.
pub(crate) fn leading_string_field(prefix: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let after = &prefix[prefix.find(&marker)? + marker.len()..];
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// A record for a journal entry that was too long to read, so the gap is visible.
fn oversize_entry(prefix: &str) -> Option<JournalEntry> {
    let cursor = leading_string_field(prefix, "__CURSOR")?;
    let now = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let unit =
        leading_string_field(prefix, "_SYSTEMD_UNIT").unwrap_or_else(|| "unknown".to_string());
    Some(JournalEntry {
        cursor,
        record: PodLogRecord {
            namespace: JOURNAL_SCOPE.to_string(),
            pod: unit.clone(),
            container: unit,
            timestamp_unix_nanos: u64::try_from(now).unwrap_or_default(),
            body: format!(
                "[journal entry over {} bytes dropped]",
                MAX_JOURNAL_LINE_BYTES
            ),
        },
    })
}

/// The journalctl errors already logged. It outlives each reader, so a
/// persistent failure is logged once, not on every restart of the reader.
static LOGGED_JOURNALCTL_ERRORS: std::sync::Mutex<Option<HashSet<String>>> =
    std::sync::Mutex::new(None);

/// True when this line is a new kind of error and should be logged.
fn first_time_seen(seen: &mut HashSet<String>, line: &str) -> bool {
    seen.len() < MAX_LOGGED_STDERR_KINDS && seen.insert(line.to_string())
}

/// Log each distinct journalctl error once, so a persistent failure does not
/// fill the log.
async fn log_journalctl_stderr(stderr: tokio::process::ChildStderr) {
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line: String = line.chars().take(300).collect();
        let new = {
            let mut guard = LOGGED_JOURNALCTL_ERRORS
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            first_time_seen(guard.get_or_insert_with(HashSet::new), &line)
        };
        if new {
            warn!(message = %line, "journalctl reported");
        }
    }
}

async fn read_journal_once(
    state: &OperatorState,
    units: &[String],
    start_unix_secs: i64,
    progressed: &mut bool,
) -> Result<()> {
    let db = &state.db;
    let deployment_id = db.get_deployment_id().await?.ok_or_else(|| {
        AlienError::new(ErrorData::PodLogReadFailed {
            message: "the operator has no deployment id yet".to_string(),
        })
    })?;
    let saved = db.get_pod_log_offset(JOURNALD_KEY, "journal", 0).await?;
    let mut command = tokio::process::Command::new("journalctl");
    // The child gets no environment: the operator's token and settings stay with the operator.
    command
        .env_clear()
        .args(journalctl_args(
            saved.as_ref().map(|s| s.timestamp.as_str()),
            units,
            start_unix_secs,
        ))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .into_alien_error()
        .context(ErrorData::PodLogReadFailed {
            message: "could not start journalctl; the operator needs it and membership of the systemd-journal group".to_string(),
        })?;
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(log_journalctl_stderr(stderr));
    }
    let stdout = child.stdout.take().ok_or_else(|| {
        AlienError::new(ErrorData::PodLogReadFailed {
            message: "journalctl has no output".to_string(),
        })
    })?;
    let mut reader = tokio::io::BufReader::new(stdout);
    let own_pid = std::process::id();
    let mut batch: Vec<PodLogRecord> = Vec::new();
    let mut batch_bytes = 0usize;
    let mut cursor: Option<String> = None;
    let mut partial = PartialLine::default();
    let mut flush_interval = time::interval(Duration::from_secs(1));
    flush_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = state.cancel.cancelled() => {
                flush_cursor_batch(db, &deployment_id, JOURNALD_KEY, "journal", &mut batch, cursor.as_deref()).await?;
                return Ok(());
            }
            _ = flush_interval.tick() => {
                flush_cursor_batch(db, &deployment_id, JOURNALD_KEY, "journal", &mut batch, cursor.as_deref()).await?;
                batch_bytes = 0;
            }
            line = partial.read(&mut reader, MAX_JOURNAL_LINE_BYTES) => {
                let line = line.into_alien_error().context(ErrorData::PodLogReadFailed {
                    message: "could not read the journal".to_string(),
                })?;
                let Some(line) = line else {
                    flush_cursor_batch(db, &deployment_id, JOURNALD_KEY, "journal", &mut batch, cursor.as_deref()).await?;
                    return Err(AlienError::new(ErrorData::PodLogReadFailed {
                        message: "journalctl exited".to_string(),
                    }));
                };
                *progressed = true;
                let text = String::from_utf8_lossy(&line.bytes);
                let entry = if line.truncated {
                    oversize_entry(&text)
                } else {
                    parse_journal_line(&text, own_pid)
                };
                // The cursor advances past entries that are skipped too.
                if let Some(entry) = entry {
                    batch_bytes += entry.record.body.len();
                    cursor = Some(entry.cursor);
                    batch.push(entry.record);
                } else if let Some(skipped) = leading_string_field(&text, "__CURSOR") {
                    cursor = Some(skipped);
                }
                if batch.len() >= MAX_BATCH_RECORDS || batch_bytes >= MAX_BATCH_BYTES {
                    flush_cursor_batch(db, &deployment_id, JOURNALD_KEY, "journal", &mut batch, cursor.as_deref()).await?;
                    batch_bytes = 0;
                }
            }
        }
    }
}

/// Store a batch with a cursor that is an opaque string (the journal cursor)
/// in the offsets table's text column.
async fn flush_cursor_batch(
    db: &OperatorDb,
    deployment_id: &str,
    key: &str,
    container: &str,
    batch: &mut Vec<PodLogRecord>,
    cursor: Option<&str>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let cursor = cursor.expect("a non-empty batch has a cursor");
    let encoded = pod_log_records_to_otlp(std::mem::take(batch), deployment_id)?;
    db.store_pod_log_batch(
        key,
        container,
        0,
        &PodLogOffset {
            timestamp: cursor.to_string(),
            ordinal: 0,
        },
        &encoded,
    )
    .await
}

// ------------------------------------------------------------------ Docker

/// Decides which lines of a re-read container log are new. Docker's `since`
/// has second resolution, so a reconnect replays some lines; this drops the ones
/// at or before the saved timestamp and ordinal.
#[derive(Debug, Clone, Default)]
pub(crate) struct DockerLineCursor {
    saved: Option<PodLogOffset>,
    last_time: Option<DateTime<Utc>>,
    ordinal: i64,
}

impl DockerLineCursor {
    pub(crate) fn new(saved: Option<PodLogOffset>) -> Self {
        Self {
            saved,
            last_time: None,
            ordinal: 0,
        }
    }

    /// The offset to store for this line, or `None` when it was stored before.
    pub(crate) fn accept(&mut self, timestamp: DateTime<Utc>) -> Option<PodLogOffset> {
        if self.last_time == Some(timestamp) {
            self.ordinal += 1;
        } else {
            self.last_time = Some(timestamp);
            self.ordinal = 1;
        }
        let stamp = timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true);
        if let Some(saved) = &self.saved {
            let saved_time = DateTime::parse_from_rfc3339(&saved.timestamp)
                .ok()
                .map(|time| time.with_timezone(&Utc));
            if saved_time.is_some_and(|saved_time| timestamp < saved_time)
                || (saved.timestamp == stamp && self.ordinal <= saved.ordinal)
            {
                return None;
            }
        }
        Some(PodLogOffset {
            timestamp: stamp,
            ordinal: self.ordinal,
        })
    }
}

/// Split a timestamped Docker log line (`<RFC 3339 nano> <message>`).
pub(crate) fn split_docker_line(line: &str) -> Option<(DateTime<Utc>, &str)> {
    let (timestamp, body) = line.split_once(' ')?;
    let timestamp = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Utc);
    Some((timestamp, body))
}

/// The Docker `since` value (whole seconds) for a reconnect: the saved
/// timestamp rounded down, else the operator start.
pub(crate) fn docker_since(saved: Option<&PodLogOffset>, started_at: DateTime<Utc>) -> i32 {
    let from = saved
        .and_then(|offset| DateTime::parse_from_rfc3339(&offset.timestamp).ok())
        .map(|time| time.with_timezone(&Utc))
        .unwrap_or(started_at);
    i32::try_from(from.timestamp().max(0)).unwrap_or(i32::MAX)
}

#[derive(Clone)]
struct DockerSource {
    id: String,
    name: String,
}

struct DockerStream {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

async fn run_docker(state: Arc<OperatorState>, started_at: DateTime<Utc>) {
    let mut streams = HashMap::<String, DockerStream>::new();
    let own_id_prefix = std::env::var("HOSTNAME")
        .ok()
        .filter(|name| !name.is_empty());
    let mut warned_cap = false;
    loop {
        match list_running(&own_id_prefix).await {
            Ok(sources) => {
                let wanted: HashSet<String> =
                    sources.iter().map(|source| source.id.clone()).collect();
                streams.retain(|id, stream| {
                    let keep = wanted.contains(id) && !stream.handle.is_finished();
                    if !keep {
                        stream.cancel.cancel();
                    }
                    keep
                });
                for source in sources {
                    if streams.contains_key(&source.id) {
                        continue;
                    }
                    if streams.len() >= MAX_DOCKER_STREAMS {
                        if !warned_cap {
                            warn!(max = MAX_DOCKER_STREAMS, "More running containers than the log stream limit; the rest are not collected");
                            warned_cap = true;
                        }
                        break;
                    }
                    let cancel = state.cancel.child_token();
                    let handle = tokio::spawn(stream_container(
                        state.clone(),
                        source.clone(),
                        started_at,
                        cancel.clone(),
                    ));
                    streams.insert(source.id, DockerStream { cancel, handle });
                }
            }
            Err(error) => warn!(error = %error, "Docker container discovery failed; will retry"),
        }
        tokio::select! {
            _ = state.cancel.cancelled() => break,
            _ = time::sleep(DOCKER_DISCOVERY_INTERVAL) => {}
        }
    }
    for stream in streams.values() {
        stream.cancel.cancel();
    }
    for (_, stream) in streams.drain() {
        let _ = stream.handle.await;
    }
}

async fn list_running(own_id_prefix: &Option<String>) -> Result<Vec<DockerSource>> {
    let docker = docker_client()?;
    let containers = docker
        .list_containers(Some(bollard::query_parameters::ListContainersOptions {
            all: false,
            ..Default::default()
        }))
        .await
        .into_alien_error()
        .context(ErrorData::PodLogReadFailed {
            message: "could not list Docker containers".to_string(),
        })?;
    Ok(containers
        .into_iter()
        .filter_map(|container| {
            let id = container.id?;
            // The operator's own container logs through its stdout, not here.
            if own_id_prefix
                .as_deref()
                .is_some_and(|own| !own.is_empty() && id.starts_with(own))
            {
                return None;
            }
            let name = container
                .names
                .and_then(|names| names.into_iter().next())
                .map(|name| name.trim_start_matches('/').to_string())
                .unwrap_or_else(|| id.chars().take(12).collect());
            Some(DockerSource { id, name })
        })
        .collect())
}

async fn stream_container(
    state: Arc<OperatorState>,
    source: DockerSource,
    started_at: DateTime<Utc>,
    cancel: CancellationToken,
) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        match read_container_once(&state, &source, started_at, &cancel).await {
            Ok(()) => {}
            Err(error) => {
                if !cancel.is_cancelled() {
                    warn!(error = %error, container = %source.name, "Container log stream failed; will reconnect");
                }
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = time::sleep(RECONNECT_DELAY) => {}
        }
        // A container that has stopped is dropped at the next discovery.
    }
}

async fn read_container_once(
    state: &OperatorState,
    source: &DockerSource,
    started_at: DateTime<Utc>,
    cancel: &CancellationToken,
) -> Result<()> {
    let db = &state.db;
    let deployment_id = db.get_deployment_id().await?.ok_or_else(|| {
        AlienError::new(ErrorData::PodLogReadFailed {
            message: "the operator has no deployment id yet".to_string(),
        })
    })?;
    let key = format!("{DOCKER_KEY_PREFIX}{}", source.id);
    let saved = db.get_pod_log_offset(&key, &source.name, 0).await?;
    let docker = docker_client()?;
    let mut stream = docker.logs(
        &source.id,
        Some(bollard::query_parameters::LogsOptions {
            follow: true,
            stdout: true,
            stderr: true,
            timestamps: true,
            since: docker_since(saved.as_ref(), started_at),
            tail: "all".to_string(),
            ..Default::default()
        }),
    );
    let mut cursor = DockerLineCursor::new(saved);
    let mut batch: Vec<PodLogRecord> = Vec::new();
    let mut batch_bytes = 0usize;
    let mut offset: Option<PodLogOffset> = None;
    let mut flush_interval = time::interval(Duration::from_secs(1));
    flush_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                flush_docker_batch(db, &deployment_id, &key, &source.name, &mut batch, offset.as_ref()).await?;
                return Ok(());
            }
            _ = flush_interval.tick() => {
                flush_docker_batch(db, &deployment_id, &key, &source.name, &mut batch, offset.as_ref()).await?;
                batch_bytes = 0;
            }
            item = stream.next() => {
                let Some(item) = item else {
                    flush_docker_batch(db, &deployment_id, &key, &source.name, &mut batch, offset.as_ref()).await?;
                    return Ok(());
                };
                let output = item.into_alien_error().context(ErrorData::PodLogReadFailed {
                    message: "Docker log stream ended with an error".to_string(),
                })?;
                let message = output.into_bytes();
                let text = String::from_utf8_lossy(&message);
                for line in text.split('\n').filter(|line| !line.is_empty()) {
                    let Some((timestamp, body)) = split_docker_line(line) else { continue };
                    let Some(next) = cursor.accept(timestamp) else { continue };
                    offset = Some(next);
                    let body = truncate_line(body.trim_end_matches('\r').to_string());
                    batch_bytes += body.len();
                    batch.push(PodLogRecord {
                        namespace: DOCKER_SCOPE.to_string(),
                        pod: source.name.clone(),
                        container: source.name.clone(),
                        timestamp_unix_nanos: timestamp.timestamp_nanos_opt().unwrap_or_default().max(0) as u64,
                        body,
                    });
                }
                if batch.len() >= MAX_BATCH_RECORDS || batch_bytes >= MAX_BATCH_BYTES {
                    flush_docker_batch(db, &deployment_id, &key, &source.name, &mut batch, offset.as_ref()).await?;
                    batch_bytes = 0;
                }
            }
        }
    }
}

async fn flush_docker_batch(
    db: &OperatorDb,
    deployment_id: &str,
    key: &str,
    container: &str,
    batch: &mut Vec<PodLogRecord>,
    offset: Option<&PodLogOffset>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let offset = offset.expect("a non-empty batch has a cursor");
    let encoded = pod_log_records_to_otlp(std::mem::take(batch), deployment_id)?;
    db.store_pod_log_batch(key, container, 0, offset, &encoded)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_journalctl_error_is_new_once_across_reader_restarts() {
        let mut seen = HashSet::new();
        assert!(first_time_seen(&mut seen, "Permission denied"));
        assert!(!first_time_seen(&mut seen, "Permission denied"));
        assert!(first_time_seen(&mut seen, "No journal files"));
    }

    #[test]
    fn a_token_enrolled_machine_reports_the_host_without_a_log_source() {
        assert_eq!(inventory_config(None, false), None);
        assert_eq!(
            inventory_config(None, true),
            Some(HostLogsConfig::default())
        );
        let docker = HostLogsConfig {
            journald: false,
            docker: true,
        };
        assert_eq!(inventory_config(Some(docker), false), Some(docker));
        assert_eq!(inventory_config(Some(docker), true), Some(docker));
    }

    #[test]
    fn sources_parse_and_unknown_values_refuse_to_start() {
        assert_eq!(parse_sources(None, false, true, true).unwrap(), None);
        assert_eq!(parse_sources(None, true, false, true).unwrap(), None);
        assert_eq!(
            parse_sources(None, true, true, true).unwrap(),
            Some(HostLogsConfig {
                journald: false,
                docker: true
            })
        );
        // An install that was not enrolled with a token collects nothing new.
        assert_eq!(parse_sources(None, true, true, false).unwrap(), None);
        assert_eq!(
            parse_sources(Some("journald, docker"), false, false, false).unwrap(),
            Some(HostLogsConfig {
                journald: true,
                docker: true
            })
        );
        assert_eq!(parse_sources(Some("none"), true, true, true).unwrap(), None);
        let error = parse_sources(Some("journald,syslog"), false, false, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("syslog"), "{error}");
    }

    #[test]
    fn a_journal_line_becomes_a_record_with_its_cursor() {
        let line = r#"{"__CURSOR":"s=abc;i=1","__REALTIME_TIMESTAMP":"1700000000123456","_SYSTEMD_UNIT":"nginx.service","MESSAGE":"started","_PID":"700"}"#;
        let entry = parse_journal_line(line, 1).unwrap();
        assert_eq!(entry.cursor, "s=abc;i=1");
        assert_eq!(entry.record.pod, "nginx.service");
        assert_eq!(entry.record.timestamp_unix_nanos, 1_700_000_000_123_456_000);
        assert_eq!(entry.record.body, "started");
        // Bytes that are not text, the operator's own lines, and lines with no cursor.
        let bytes = r#"{"__CURSOR":"c2","__REALTIME_TIMESTAMP":"1","SYSLOG_IDENTIFIER":"kernel","MESSAGE":[104,105,255]}"#;
        assert_eq!(
            parse_journal_line(bytes, 1).unwrap().record.body,
            "hi\u{fffd}"
        );
        assert!(parse_journal_line(line, 700).is_none());
        assert!(parse_journal_line(r#"{"MESSAGE":"x"}"#, 1).is_none());
        assert!(parse_journal_line("not json", 1).is_none());
    }

    #[test]
    fn journalctl_resumes_after_the_saved_cursor_and_otherwise_starts_now() {
        // Without a cursor the start is fixed, so a retry reads the same entries again.
        assert!(journalctl_args(None, &[], 1700000000).contains(&"--since=@1700000000".to_string()));
        assert_eq!(
            journalctl_args(None, &[], 1700000000),
            journalctl_args(None, &[], 1700000000)
        );
        assert!(journalctl_args(None, &[], 5).contains(&"--all".to_string()));
        let narrowed = journalctl_args(None, &["nginx.service".to_string()], 5);
        assert!(narrowed.contains(&"--unit=nginx.service".to_string()));
        let resumed = journalctl_args(Some("s=abc;i=9"), &[], 5);
        assert!(resumed.contains(&"--after-cursor=s=abc;i=9".to_string()));
        assert!(!resumed.iter().any(|arg| arg.starts_with("--since")));
    }

    #[tokio::test]
    async fn a_read_dropped_mid_line_loses_no_bytes() {
        use tokio::io::AsyncWriteExt;
        let (mut writer, read_half) = tokio::io::duplex(64);
        let mut reader = tokio::io::BufReader::new(read_half);
        let mut partial = PartialLine::default();
        writer.write_all(b"{\"first\":").await.unwrap();
        // The timer wins: the read future is dropped with part of the line read.
        let timed_out =
            tokio::time::timeout(Duration::from_millis(50), partial.read(&mut reader, 1024)).await;
        assert!(timed_out.is_err());
        writer.write_all(b"1}\nnext\n").await.unwrap();
        let line = partial.read(&mut reader, 1024).await.unwrap().unwrap();
        assert_eq!(line.bytes, b"{\"first\":1}");
        let next = partial.read(&mut reader, 1024).await.unwrap().unwrap();
        assert_eq!(next.bytes, b"next");
    }

    #[tokio::test]
    async fn a_long_journal_line_is_cut_and_the_next_line_is_read_whole() {
        let data = format!("{}\nsecond\nlast", "x".repeat(50));
        let mut reader = tokio::io::BufReader::with_capacity(8, data.as_bytes());
        let first = read_bounded_line(&mut reader, 10).await.unwrap().unwrap();
        assert_eq!(first.bytes, b"xxxxxxxxxx");
        assert!(first.truncated);
        let second = read_bounded_line(&mut reader, 10).await.unwrap().unwrap();
        assert_eq!(
            (second.bytes.as_slice(), second.truncated),
            (&b"second"[..], false)
        );
        let last = read_bounded_line(&mut reader, 10).await.unwrap().unwrap();
        assert_eq!(last.bytes, b"last");
        assert!(read_bounded_line(&mut reader, 10).await.unwrap().is_none());
    }

    #[test]
    fn an_oversize_entry_keeps_its_cursor_and_says_it_was_dropped() {
        let prefix = r#"{"__CURSOR" : "s=abc;i=7","__REALTIME_TIMESTAMP":"1","_SYSTEMD_UNIT":"big.service","MESSAGE":"xxxx"#;
        let entry = oversize_entry(prefix).unwrap();
        assert_eq!(entry.cursor, "s=abc;i=7");
        assert_eq!(entry.record.pod, "big.service");
        assert!(entry.record.body.contains("dropped"));
        assert!(oversize_entry("no cursor here").is_none());
    }

    #[test]
    fn log_units_are_validated() {
        assert!(parse_log_units(None).unwrap().is_empty());
        assert_eq!(
            parse_log_units(Some("nginx.service, ssh@1.service")).unwrap(),
            vec!["nginx.service", "ssh@1.service"]
        );
        for bad in ["a b", "x;y", "--all", "a,,b", "$(id)"] {
            assert!(parse_log_units(Some(bad)).is_err(), "{bad}");
        }
    }

    fn at(seconds: i64, nanos: u32) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, nanos).unwrap()
    }

    #[test]
    fn a_docker_reconnect_drops_stored_lines_and_keeps_new_ones() {
        // First connection stores two lines at one timestamp and one later.
        let mut first = DockerLineCursor::new(None);
        let a = first.accept(at(100, 5)).unwrap();
        let b = first.accept(at(100, 5)).unwrap();
        assert_eq!((a.ordinal, b.ordinal), (1, 2));
        let c = first.accept(at(101, 0)).unwrap();
        // Docker replays from the whole second on reconnect.
        let mut second = DockerLineCursor::new(Some(c.clone()));
        assert!(second.accept(at(100, 5)).is_none());
        assert!(second.accept(at(100, 5)).is_none());
        assert!(second.accept(at(101, 0)).is_none());
        let next = second.accept(at(101, 0)).unwrap();
        assert_eq!(next.ordinal, 2);
        assert!(second.accept(at(102, 0)).is_some());
    }

    #[test]
    fn docker_lines_split_on_the_timestamp_and_since_uses_whole_seconds() {
        let (time, body) = split_docker_line("2024-05-01T10:00:00.123456789Z hello world").unwrap();
        assert_eq!(body, "hello world");
        assert_eq!(time.timestamp_subsec_nanos(), 123_456_789);
        assert!(split_docker_line("no timestamp here").is_none());
        let started = at(1_000, 0);
        assert_eq!(docker_since(None, started), 1_000);
        let saved = PodLogOffset {
            timestamp: at(2_000, 900).to_rfc3339_opts(SecondsFormat::Nanos, true),
            ordinal: 3,
        };
        assert_eq!(docker_since(Some(&saved), started), 2_000);
    }
}
