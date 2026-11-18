//! Checks for the `sandbox-storage` app: a sandbox with persistent storage driven through the
//! Worker that holds its binding. Each Worker endpoint is one whole check, so the binding's
//! replication runs inside the request that needs it (on GCP the binding pulls commits from the
//! sandbox, and Cloud Run gives a Worker CPU only during a request).
//!
//! The abrupt kill goes around the binding: the test deletes the sandbox through the cloud API with
//! the target credentials while the writer runs, and compares the writer's last acknowledged round
//! with what a new sandbox on the same key restores.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alien_core::{GcpClientConfig, GcpCredentials, Platform};
use alien_gcp_clients::gcp::agent_platform::{AgentPlatformApi, AgentPlatformClient};
use alien_test::config::TestConfig;
use alien_test::TestContext;
use anyhow::{ensure, Context};
use futures::StreamExt;
use serde_json::{json, Value};

/// Design acceptance item 1: abrupt terminations per run, and the p95 loss allowed where the
/// binding pulls commits.
const KILL_RUNS: usize = 20;
const PULL_P95_LOSS_SECONDS: f64 = 5.0;
/// Seconds between the writer's rounds (`sleep 0.1` in the app's writer).
const ROUND_SECONDS: f64 = 0.1;

fn url(ctx: &TestContext) -> anyhow::Result<String> {
    Ok(ctx
        .deployment
        .url
        .as_deref()
        .context("the deployment has no public URL")?
        .trim_end_matches('/')
        .to_string())
}

/// Posts to one of the Worker's check endpoints and returns its JSON answer.
async fn post(ctx: &TestContext, path: &str, body: Value) -> anyhow::Result<Value> {
    let response = reqwest::Client::new()
        .post(format!("{}{path}", url(ctx)?))
        .bearer_auth(&ctx.deployment.id)
        .json(&body)
        .timeout(Duration::from_secs(900))
        .send()
        .await
        .with_context(|| format!("POST {path}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .with_context(|| format!("read {path}"))?;
    ensure!(status.is_success(), "{path} answered {status}: {text}");
    serde_json::from_str(&text).with_context(|| format!("parse {path} answer: {text}"))
}

/// `name=value` lines of a check's stdout.
fn fields(answer: &Value) -> BTreeMap<String, String> {
    answer["stdout"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_string(), value.trim().to_string()))
        .collect()
}

fn ensure_ran(path: &str, answer: &Value) -> anyhow::Result<()> {
    ensure!(
        answer["exitCode"] == json!(0),
        "{path} exited {}: {}",
        answer["exitCode"],
        answer["stderr"]
    );
    Ok(())
}

/// A key no earlier run used, so runs never see each other's data.
fn fresh_key(ctx: &TestContext, purpose: &str) -> String {
    let suffix: u32 = rand::random();
    let id: String = ctx
        .deployment
        .id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    format!("{purpose}-{id}-{suffix:08x}")
}

/// Every acceptance check of the design the app can drive (items 1 to 6). The full round trip runs
/// under egress `none`, a smaller one and the isolation check under both policies.
pub async fn check_persistent_storage(ctx: &TestContext) -> anyhow::Result<()> {
    check_round_trip(ctx, "none", 10_000, 1024).await?;
    check_round_trip(ctx, "internet", 1_000, 16).await?;
    check_isolation(ctx, "none").await?;
    check_isolation(ctx, "internet").await?;
    check_takeover(ctx).await?;
    check_oversize(ctx).await?;
    check_kill_runs(ctx).await?;
    Ok(())
}

/// Items 2 and 5: an exact round trip of 10k files and 1 GiB (content, owner, mode, mtime, hard
/// links, symlinks; `tmp/` excluded), `readStorageFile` returns the synced bytes and
/// `deleteStorage` erases the key.
pub async fn check_round_trip(
    ctx: &TestContext,
    egress: &str,
    files: u32,
    mib: u32,
) -> anyhow::Result<()> {
    let key = fresh_key(ctx, "roundtrip");
    let prepared = post(
        ctx,
        "/s/prepare",
        json!({ "key": key, "files": files, "mib": mib, "egress": egress }),
    )
    .await?;
    ensure_ran("/s/prepare", &prepared)?;
    let written = fields(&prepared);
    eprintln!("prepared {key}: {written:?}, synced {}", prepared["synced"]);
    ensure!(
        prepared["synced"]["state"] == json!("replicating"),
        "the sync did not end replicating: {}",
        prepared["synced"]
    );

    let started = Instant::now();
    let verified = post(ctx, "/s/verify", json!({ "key": key, "egress": egress })).await?;
    ensure_ran("/s/verify", &verified)?;
    let restored = fields(&verified);
    eprintln!(
        "restored {key} in {} ms (request {:?}): {restored:?}",
        verified["restoredMs"],
        started.elapsed()
    );
    for name in ["entries", "bytes", "manifest"] {
        ensure!(
            written.get(name) == restored.get(name),
            "{name} differs after restore: wrote {:?}, restored {:?}",
            written.get(name),
            restored.get(name)
        );
    }
    ensure!(
        restored.get("git_fsck").map(String::as_str) == Some("ok"),
        "git fsck: {restored:?}"
    );
    ensure!(
        restored
            .get("sqlite")
            .is_some_and(|value| value.starts_with("ok 1000")),
        "SQLite after restore: {restored:?}"
    );
    ensure!(
        restored.get("hard_link").map(String::as_str) == Some("same"),
        "hard link split"
    );
    ensure!(
        restored.get("excluded").map(String::as_str) == Some("absent"),
        "tmp/ replicated"
    );

    let read = post(ctx, "/s/read", json!({ "key": key, "path": "linked-a" })).await?;
    ensure!(read["found"] == json!(true), "readStorageFile: {read}");
    let bytes = base64_decode(read["base64"].as_str().unwrap_or_default())?;
    ensure!(bytes == b"linked\n", "readStorageFile returned {bytes:?}");

    post(ctx, "/s/delete", json!({ "key": key })).await?;
    let gone = post(ctx, "/s/read", json!({ "key": key, "path": "linked-a" })).await?;
    ensure!(
        gone["found"] == json!(false),
        "the key outlived deleteStorage: {gone}"
    );
    Ok(())
}

/// Item 4: the command's user holds no store credentials and cannot reach the metadata server's
/// token, nor object storage under egress `none`; symlinks aimed out of the tree come back as
/// symlinks, not as the files they point at.
pub async fn check_isolation(ctx: &TestContext, egress: &str) -> anyhow::Result<()> {
    let key = fresh_key(ctx, "isolation");
    let answer = post(ctx, "/s/isolation", json!({ "key": key, "egress": egress })).await?;
    ensure_ran("/s/isolation", &answer)?;
    let seen = fields(&answer);
    eprintln!("isolation under {egress}: {seen:?}");
    ensure!(
        seen.get("uid").map(String::as_str) != Some("0"),
        "the command runs as root"
    );
    ensure!(
        seen.get("credential_env").map(String::as_str) == Some("0"),
        "credentials in the command's environment: {seen:?}"
    );
    // curl reports 000 when nothing answers.
    let token = seen.get("metadata_token").map(String::as_str).unwrap_or("");
    ensure!(token != "200", "the command read a metadata server token");
    if egress == "none" {
        let api = seen.get("storage_api").map(String::as_str).unwrap_or("");
        ensure!(
            api == "000" || api.is_empty(),
            "object storage answered under egress none: {api}"
        );
    }
    let escaped = post(
        ctx,
        "/s/read",
        json!({ "key": key, "path": "escape/root/etc/passwd" }),
    )
    .await?;
    ensure!(
        escaped["found"] == json!(false),
        "a symlink redirected the walk out of the tree: {escaped}"
    );
    post(ctx, "/s/delete", json!({ "key": key })).await?;
    Ok(())
}

/// Item 3: a second sandbox on the key takes it over; the first reports `stopped` and its later
/// writes never reach the store.
pub async fn check_takeover(ctx: &TestContext) -> anyhow::Result<()> {
    let key = fresh_key(ctx, "takeover");
    let answer = post(ctx, "/s/takeover", json!({ "key": key })).await?;
    eprintln!("takeover: {answer}");
    ensure!(
        answer["firstStatus"]["state"] == json!("stopped"),
        "the stale sandbox did not stop: {}",
        answer["firstStatus"]
    );
    ensure!(
        answer["owner"] == json!("second\n"),
        "the store holds {} instead of the new owner's write",
        answer["owner"]
    );
    post(ctx, "/s/delete", json!({ "key": key })).await?;
    Ok(())
}

/// Item 6: a tree above `maxSize` is refused whole; the key keeps its last good commit.
pub async fn check_oversize(ctx: &TestContext) -> anyhow::Result<()> {
    let key = fresh_key(ctx, "oversize");
    let answer = post(ctx, "/s/oversize", json!({ "key": key })).await?;
    eprintln!(
        "oversize: synced {} status {}",
        answer["synced"], answer["status"]
    );
    ensure_ran("/s/oversize", &answer)?;
    ensure!(
        answer["synced"]["state"] != json!("replicating") || answer["synced"]["error"].is_string(),
        "a sync above maxSize reported replicating: {}",
        answer["synced"]
    );
    let read = post(
        ctx,
        "/s/read",
        json!({ "key": key, "path": "oversize.bin" }),
    )
    .await?;
    ensure!(
        read["found"] == json!(false),
        "part of an oversize tree was published"
    );
    post(ctx, "/s/delete", json!({ "key": key })).await?;
    Ok(())
}

/// Item 1: abrupt terminations under the SQLite WAL writer, the rename loop and git commits.
/// Each run kills the sandbox from outside after a random 8 to 20 seconds; the loss is the
/// writer's last acknowledged round minus the restored one, in seconds.
pub async fn check_kill_runs(ctx: &TestContext) -> anyhow::Result<()> {
    ensure!(
        ctx.platform == Platform::Gcp,
        "the kill runs reach the sandbox through the GCP API only"
    );
    let killer = GcpKiller::new(ctx).await?;
    let mut losses = Vec::new();
    for run in 0..KILL_RUNS {
        let key = fresh_key(ctx, &format!("kill{run:02}"));
        let wait = Duration::from_secs(8 + u64::from(rand::random::<u8>() % 13));
        let (acked, sandbox_id) = writer_until_killed(ctx, &killer, &key, wait).await?;
        let restored = post(ctx, "/s/restored", json!({ "key": key })).await?;
        ensure_ran("/s/restored", &restored)?;
        let seen = fields(&restored);
        let last: u64 = seen
            .get("last")
            .and_then(|value| value.parse().ok())
            .with_context(|| format!("restore of {key} has no writer log: {seen:?}"))?;
        let loss = acked.saturating_sub(last) as f64 * ROUND_SECONDS;
        eprintln!(
            "kill run {run}: sandbox {sandbox_id} after {wait:?}, acked {acked}, restored {last}, \
             loss {loss:.1} s, {seen:?}"
        );
        ensure!(
            last <= acked + 1,
            "restored round {last} past the acknowledged {acked}"
        );
        ensure!(
            seen.get("sqlite").is_some_and(|value| value.trim() == "ok"),
            "run {run}: SQLite integrity after restore: {seen:?}"
        );
        ensure!(
            matches!(
                seen.get("git_fsck").map(String::as_str),
                Some("ok" | "none")
            ),
            "run {run}: git fsck after restore: {seen:?}"
        );
        losses.push(loss);
        post(ctx, "/s/delete", json!({ "key": key })).await?;
    }
    losses.sort_by(f64::total_cmp);
    let p50 = losses[losses.len() / 2];
    let p95 = losses[(losses.len() * 95).div_ceil(100) - 1];
    eprintln!(
        "kill runs: loss p50 {p50:.1} s, p95 {p95:.1} s over {}",
        losses.len()
    );
    ensure!(
        p95 <= PULL_P95_LOSS_SECONDS,
        "p95 loss {p95:.1} s is above {PULL_P95_LOSS_SECONDS} s"
    );
    Ok(())
}

/// Streams the writer's acknowledgements, deletes the sandbox after `wait`, and returns the last
/// acknowledged round once the Worker's stream ends.
async fn writer_until_killed(
    ctx: &TestContext,
    killer: &GcpKiller,
    key: &str,
    wait: Duration,
) -> anyhow::Result<(u64, String)> {
    let response = reqwest::Client::new()
        .post(format!("{}/s/writer", url(ctx)?))
        .bearer_auth(&ctx.deployment.id)
        .json(&json!({ "key": key, "rounds": 3000 }))
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .context("POST /s/writer")?;
    ensure!(
        response.status().is_success(),
        "/s/writer answered {}",
        response.status()
    );
    let mut body = response.bytes_stream();
    let mut buffer = String::new();
    let mut sandbox_id = None;
    let mut acked = 0u64;
    let mut kill_at = None;
    let mut killed = false;
    loop {
        if let (Some(at), Some(id)) = (kill_at, sandbox_id.as_deref()) {
            if !killed && Instant::now() >= at {
                killer.kill(id).await?;
                killed = true;
            }
        }
        let next = tokio::time::timeout(Duration::from_millis(200), body.next()).await;
        let chunk = match next {
            Err(_) => continue,
            Ok(None) => break,
            Ok(Some(chunk)) => chunk.context("read the writer stream")?,
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buffer.find('\n') {
            let line: String = buffer.drain(..=end).collect();
            let event: Value = serde_json::from_str(line.trim())
                .with_context(|| format!("writer line {line:?}"))?;
            if let Some(id) = event["sandboxId"].as_str() {
                sandbox_id = Some(id.to_string());
                kill_at = Some(Instant::now() + wait);
            } else if let Some(round) = event["ack"].as_u64() {
                // Rounds the Worker relays after the kill was sent were acknowledged before it.
                acked = acked.max(round);
            } else if event.get("end").is_some() {
                ensure!(killed, "the writer ended before the kill: {event}");
            }
        }
    }
    ensure!(killed, "the writer stream closed before the kill");
    let sandbox_id = sandbox_id.context("the writer stream named no sandbox")?;
    ensure!(acked > 0, "the writer acknowledged nothing before the kill");
    Ok((acked, sandbox_id))
}

/// Deletes one of the deployment's sandboxes through the Agent Platform API with the target
/// credentials, which the agent and the binding never see coming.
struct GcpKiller {
    client: Arc<AgentPlatformClient>,
    engine: String,
}

impl GcpKiller {
    async fn new(ctx: &TestContext) -> anyhow::Result<Self> {
        let target = TestConfig::from_env()
            .gcp_target
            .context("GOOGLE_TARGET_* is required for the kill runs")?;
        let credentials = target
            .credentials_json
            .context("GOOGLE_TARGET_SERVICE_ACCOUNT_KEY is required for the kill runs")?;
        let config = GcpClientConfig {
            project_id: target.project_id,
            region: target.region,
            credentials: GcpCredentials::ServiceAccountKey { json: credentials },
            service_overrides: None,
            project_number: None,
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .context("build the HTTP client")?;
        Ok(Self {
            client: Arc::new(AgentPlatformClient::new(http, config)),
            engine: sandbox_engine(ctx, "agents").await?,
        })
    }

    async fn kill(&self, sandbox_id: &str) -> anyhow::Result<()> {
        self.client
            .delete_sandbox(&self.engine, sandbox_id)
            .await
            .with_context(|| format!("delete sandbox {sandbox_id}"))
    }
}

/// The reasoning engine id of sandbox `resource_id`, from the deployment's stack state: the first
/// `…/reasoningEngines/<id>` name the resource's saved state holds.
async fn sandbox_engine(ctx: &TestContext, resource_id: &str) -> anyhow::Result<String> {
    let response = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("get_deployment failed: {error}"))?;
    let state = response
        .into_inner()
        .stack_state
        .context("deployment is missing stack_state")?;
    let resource = &state["resources"][resource_id];
    find_engine(resource)
        .with_context(|| format!("no reasoning engine in the state of '{resource_id}'"))
}

fn find_engine(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => text
            .split_once("/reasoningEngines/")
            .map(|(_, rest)| rest.split('/').next().unwrap_or(rest).to_string()),
        Value::Array(items) => items.iter().find_map(find_engine),
        Value::Object(map) => map.values().find_map(find_engine),
        _ => None,
    }
}

fn base64_decode(text: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|error| anyhow::anyhow!("base64: {error}"))
}
