//! Acceptance checks for persistent storage, through the `sandbox-storage` app's Worker (a sandbox
//! that declares persistent storage refuses remote access). Each check names the item of the
//! acceptance suite it covers.

use std::time::{Duration, Instant};

use alien_test::TestContext;
use anyhow::{bail, ensure, Context};
use base64::Engine;
use serde_json::{json, Value};

struct App {
    url: String,
    bearer: String,
    http: reqwest::Client,
}

impl App {
    fn new(ctx: &TestContext) -> anyhow::Result<Self> {
        let url = ctx
            .deployment
            .url
            .as_deref()
            .context("the deployment has no public URL")?
            .trim_end_matches('/')
            .to_string();
        Ok(Self {
            url,
            bearer: ctx.deployment.id.clone(),
            http: reqwest::Client::new(),
        })
    }

    async fn op(&self, op: &str, body: Value) -> anyhow::Result<Value> {
        let response = self
            .http
            .post(format!("{}/op/{op}", self.url))
            .bearer_auth(&self.bearer)
            .json(&body)
            .timeout(Duration::from_secs(300))
            .send()
            .await
            .with_context(|| format!("POST /op/{op}"))?;
        let status = response.status();
        let text = response.text().await.context("read response")?;
        ensure!(status.is_success(), "/op/{op} answered {status}: {text}");
        serde_json::from_str(&text).with_context(|| format!("parse /op/{op}: {text}"))
    }

    async fn create(&self, key: &str) -> anyhow::Result<(String, Duration)> {
        let started = Instant::now();
        let created = self.op("create", json!({ "key": key })).await?;
        let id = created["sandboxId"]
            .as_str()
            .context("create returned no sandboxId")?
            .to_string();
        eprintln!("sandbox {id} on key {key} in {:?}", started.elapsed());
        Ok((id, started.elapsed()))
    }

    async fn exec(&self, id: &str, command: &str, args: &[&str]) -> anyhow::Result<(i64, String)> {
        let run = self
            .op(
                "exec",
                json!({ "sandboxId": id, "command": command, "args": args, "timeoutMs": 280_000 }),
            )
            .await?;
        let code = run["exitCode"].as_i64().unwrap_or(-1);
        let stdout = run["stdout"].as_str().unwrap_or_default().to_string();
        if code != 0 {
            eprintln!(
                "{command} {args:?} exited {code}: {}",
                run["stderr"].as_str().unwrap_or_default()
            );
        }
        Ok((code, stdout))
    }

    async fn ok(&self, id: &str, command: &str, args: &[&str]) -> anyhow::Result<String> {
        let (code, stdout) = self.exec(id, command, args).await?;
        ensure!(code == 0, "{command} {args:?} exited {code}");
        Ok(stdout)
    }

    async fn python(&self, id: &str, code: &str) -> anyhow::Result<String> {
        self.ok(id, "python3", &["-c", code]).await
    }

    async fn status(&self, id: &str) -> anyhow::Result<Value> {
        Ok(self.op("status", json!({ "sandboxId": id })).await?["status"].clone())
    }

    async fn sync(&self, id: &str) -> anyhow::Result<Value> {
        self.op("sync", json!({ "sandboxId": id })).await
    }

    async fn terminate(&self, id: &str, discard: bool) -> anyhow::Result<()> {
        self.op("terminate", json!({ "sandboxId": id, "discard": discard }))
            .await
            .map(|_| ())
    }

    async fn read_storage_file(
        &self,
        key: &str,
        path: &str,
    ) -> anyhow::Result<Result<Vec<u8>, String>> {
        let read = self
            .op("read-storage-file", json!({ "key": key, "path": path }))
            .await?;
        if let Some(refused) = read["refused"].as_str() {
            return Ok(Err(refused.to_string()));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(read["base64"].as_str().context("no base64")?)
            .context("decode")?;
        Ok(Ok(bytes))
    }

    async fn delete_storage(&self, key: &str) -> anyhow::Result<Result<(), String>> {
        let deleted = self.op("delete-storage", json!({ "key": key })).await?;
        match deleted["refused"].as_str() {
            Some(refused) => Ok(Err(refused.to_string())),
            None => Ok(Ok(())),
        }
    }

    async fn manifest(&self, id: &str) -> anyhow::Result<Value> {
        let out = self.ok(id, "python3", &["/opt/app/manifest.py"]).await?;
        serde_json::from_str(&out).context("parse manifest")
    }
}

fn unique_key(label: &str) -> String {
    format!(
        "{label}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    )
}

/// Items 2, 4 and 5: an exact round trip of the fixture, `sync` then terminate loses nothing,
/// `readStorageFile` returns the synced bytes, a symlink out of the tree stays a symlink, and
/// `deleteStorage` erases the key. Item 3: a sandbox whose key another one claimed is refused.
/// Item 6 (`maxSize`): a tree over the ceiling is refused whole.
pub async fn check_round_trip_and_lifecycle(ctx: &TestContext) -> anyhow::Result<()> {
    let app = App::new(ctx)?;
    let key = unique_key("rt");
    let files: u32 = std::env::var("ALIEN_E2E_STORAGE_FILES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2000);
    let big_mib: u32 = std::env::var("ALIEN_E2E_STORAGE_BIG_MIB")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(64);

    let (first, _) = app.create(&key).await?;
    let populated = Instant::now();
    app.ok(
        &first,
        "python3",
        &[
            "/opt/app/populate.py",
            &files.to_string(),
            &big_mib.to_string(),
        ],
    )
    .await?;
    let before = app.manifest(&first).await?;
    let synced_at = Instant::now();
    let synced = app.sync(&first).await?;
    eprintln!(
        "sync after populate: {synced} ({:?} after the write began, sync {:?})",
        populated.elapsed(),
        synced_at.elapsed()
    );
    ensure!(synced.get("refused").is_none(), "sync refused: {synced}");

    // Item 5: the synced bytes are readable without a sandbox.
    let notes = app.read_storage_file(&key, "notes.txt").await?;
    ensure!(
        notes.as_deref() == Ok(b"persistent storage acceptance\n".as_slice()),
        "readStorageFile returned {notes:?}"
    );
    let excluded = app.read_storage_file(&key, "scratch/local-only").await?;
    ensure!(
        excluded.is_err(),
        "an excluded file reached the store: {excluded:?}"
    );
    // Item 4: the walk stores a symlink as a link, never what it points at.
    let escape = app.read_storage_file(&key, "escape").await?;
    ensure!(
        !matches!(&escape, Ok(bytes) if !bytes.is_empty()),
        "readStorageFile followed a symlink out of the tree: {escape:?}"
    );

    app.terminate(&first, false).await?;
    let (second, restore) = app.create(&key).await?;
    eprintln!("restore of {files} files + {big_mib} MiB took {restore:?} (create included)");
    let after = app.manifest(&second).await?;
    ensure!(
        before["checks"]["sqlite"] == "ok" && after["checks"]["sqlite"] == "ok",
        "SQLite integrity: before {} after {}",
        before["checks"]["sqlite"],
        after["checks"]["sqlite"]
    );
    ensure!(
        after["checks"]["gitFsck"] == 0,
        "git fsck after restore: {}",
        after["checks"]["gitFsckOutput"]
    );
    compare_manifests(&before["entries"], &after["entries"])?;

    // Item 4: a command uid reaches neither the agent's grant nor object storage.
    let probe = app
        .python(
            &second,
            r#"
import json, os, socket, urllib.request
out = {}
for path in ["/proc/1/environ", "/proc/1/mem", "/opt/alien"]:
    try:
        if os.path.isdir(path):
            out[path] = sorted(os.listdir(path))
        else:
            open(path, "rb").read(64)
            out[path] = "readable"
    except Exception as error:
        out[path] = type(error).__name__
region = os.environ.get("AWS_REGION", "us-east-2")
try:
    urllib.request.urlopen(f"https://s3.{region}.amazonaws.com/", timeout=5)
    out["s3"] = "reached"
except Exception as error:
    out["s3"] = type(error).__name__ + ": " + str(error)[:120]
try:
    socket.create_connection(("169.254.169.254", 80), timeout=3)
    out["imds"] = "reached"
except Exception as error:
    out["imds"] = type(error).__name__
print(json.dumps(out))
"#,
        )
        .await?;
    eprintln!("command-uid probe: {probe}");
    let probe: Value = serde_json::from_str(&probe).context("parse probe")?;
    ensure!(
        probe["/proc/1/environ"] != "readable",
        "a command read the agent's environment"
    );
    ensure!(probe["s3"] != "reached", "a command reached S3");
    ensure!(
        probe["imds"] != "reached",
        "a command reached instance metadata"
    );

    // Item 6: a tree over maxSize is refused whole; the key keeps its last commit.
    app.python(
        &second,
        "f=open('/workspace/over.bin','wb'); f.truncate(int(2.1*(1<<30))); f.close()",
    )
    .await?;
    let over = app.sync(&second).await?;
    let over_status = app.status(&second).await?;
    eprintln!("sync over maxSize: {over}; status {over_status}");
    ensure!(
        over.get("refused").is_some() || over_status["state"] == "behind",
        "a tree over maxSize was not refused: {over} / {over_status}"
    );
    let still = app.read_storage_file(&key, "over.bin").await?;
    ensure!(still.is_err(), "a part of the over-size commit landed");
    app.ok(&second, "rm", &["-f", "/workspace/over.bin"])
        .await?;
    let recovered = app.sync(&second).await?;
    ensure!(
        recovered.get("refused").is_none(),
        "sync after removing the file: {recovered}"
    );

    // Item 3: a new sandbox claims the key; the old one's commit is refused and it stops.
    app.ok(&second, "sh", &["-c", "echo late > /workspace/late.txt"])
        .await?;
    let (third, _) = app.create(&key).await?;
    let stale = app.sync(&second).await?;
    let stale_status = app.status(&second).await?;
    eprintln!("stale sandbox sync: {stale}; status {stale_status}");
    ensure!(
        stale_status["state"] == "stopped",
        "the stale sandbox is not stopped: {stale_status}"
    );
    let late = app.read_storage_file(&key, "late.txt").await?;
    let in_third = app
        .exec(&third, "test", &["-e", "/workspace/late.txt"])
        .await?
        .0;
    eprintln!(
        "late write: store {:?}, third sandbox exit {in_third}",
        late.as_ref().map(|b| b.len())
    );
    let late_landed = late.is_ok();
    // The claim restores HEAD as of the claim, so a late write either landed before it (and the
    // claimer sees it) or never lands.
    ensure!(
        late_landed == (in_third == 0),
        "the store and the claiming sandbox disagree about the stale write"
    );
    app.terminate(&second, true).await.ok();

    // Item 5: deleteStorage is refused while held, then erases the key.
    let held = app.delete_storage(&key).await?;
    eprintln!("deleteStorage while held: {held:?}");
    ensure!(
        held.is_err(),
        "deleteStorage succeeded while a sandbox held the key"
    );
    app.terminate(&third, false).await?;
    app.delete_storage(&key)
        .await?
        .map_err(|refused| anyhow::anyhow!("deleteStorage refused: {refused}"))?;
    let gone = app.read_storage_file(&key, "notes.txt").await?;
    ensure!(gone.is_err(), "a deleted key still reads: {gone:?}");
    let (fourth, _) = app.create(&key).await?;
    let listing = app.ok(&fourth, "ls", &["-A", "/workspace"]).await?;
    ensure!(
        listing.trim().is_empty(),
        "a deleted key restored files: {listing}"
    );
    app.terminate(&fourth, true).await?;
    app.delete_storage(&key).await?.ok();
    Ok(())
}

fn compare_manifests(before: &Value, after: &Value) -> anyhow::Result<()> {
    let before = before.as_object().context("manifest entries")?;
    let after = after.as_object().context("manifest entries")?;
    let mut differences = Vec::new();
    for (path, entry) in before {
        if path == "scratch" || path.starts_with("scratch/") {
            continue;
        }
        match after.get(path) {
            None => differences.push(format!("{path}: missing after restore")),
            Some(restored) if restored != entry => {
                differences.push(format!("{path}: {entry} -> {restored}"))
            }
            Some(_) => {}
        }
    }
    for path in after.keys() {
        if !before.contains_key(path) && path != "scratch" {
            differences.push(format!("{path}: appeared after restore"));
        }
    }
    if after.contains_key("scratch/local-only") {
        differences.push("scratch/local-only: an excluded file was restored".to_string());
    }
    eprintln!(
        "round trip: {} entries compared, {} differences",
        before.len(),
        differences.len()
    );
    if !differences.is_empty() {
        bail!(
            "{} differences after restore, first: {:#?}",
            differences.len(),
            &differences[..differences.len().min(20)]
        );
    }
    Ok(())
}

/// Item 1: abrupt terminations under an SQLite WAL writer and a rename loop. Each run starts the
/// image command's writer, waits, terminates without a final commit (`discard`), restores the key
/// in a new sandbox and measures the seconds of writes lost; integrity holds on every restore.
pub async fn check_abrupt_terminations(ctx: &TestContext) -> anyhow::Result<()> {
    let app = App::new(ctx)?;
    let runs: usize = std::env::var("ALIEN_E2E_STORAGE_KILL_RUNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5);
    let key = unique_key("kill");
    let (mut current, _) = app.create(&key).await?;
    app.ok(
        &current,
        "sh",
        &["-c", "mkdir -p /workspace/kill && touch /workspace/kill/on"],
    )
    .await?;
    let mut losses = Vec::new();
    for run in 0..runs {
        tokio::time::sleep(Duration::from_secs(8 + (run as u64 % 5))).await;
        let newest = app
            .python(
                &current,
                "import sqlite3,time; print(sqlite3.connect('/workspace/kill/w.db').execute('select max(ts) from t').fetchone()[0])",
            )
            .await?;
        let killed_after: f64 = newest.trim().parse().context("newest row")?;
        app.terminate(&current, true).await?;
        let (next, _) = app.create(&key).await?;
        let restored = app
            .python(
                &next,
                r#"
import json, sqlite3
db = sqlite3.connect('/workspace/kill/w.db')
print(json.dumps({"ok": db.execute("pragma integrity_check").fetchone()[0],
                  "max": db.execute("select max(ts) from t").fetchone()[0],
                  "renamed": open('/workspace/kill/renamed').read().strip()}))
"#,
            )
            .await?;
        let restored: Value = serde_json::from_str(&restored).context("parse restored")?;
        ensure!(
            restored["ok"] == "ok",
            "run {run}: integrity {}",
            restored["ok"]
        );
        ensure!(
            restored["renamed"]
                .as_str()
                .is_some_and(|v| v.parse::<f64>().is_ok()),
            "run {run}: the renamed file is torn: {}",
            restored["renamed"]
        );
        let lost = (killed_after - restored["max"].as_f64().unwrap_or(0.0)).max(0.0);
        eprintln!("kill run {run}: lost {lost:.2} s, integrity ok");
        losses.push(lost);
        current = next;
    }
    app.terminate(&current, true).await?;
    app.delete_storage(&key).await?.ok();
    losses.sort_by(f64::total_cmp);
    let p = |q: f64| losses[((losses.len() as f64 - 1.0) * q).round() as usize];
    eprintln!(
        "kill runs: {} runs, lost p50 {:.2} s, p95 {:.2} s, max {:.2} s",
        losses.len(),
        p(0.5),
        p(0.95),
        losses[losses.len() - 1]
    );
    ensure!(p(0.95) <= 3.0, "p95 loss {:.2} s exceeds 3 s", p(0.95));
    Ok(())
}
