//! Pause and resume on an Azure sandbox binding keep memory: the same in-memory token and process,
//! and a counter that continues. Ignored: it needs a live group. Ignored: it needs the
//! `AZURE_TARGET_*` identity holding the data-plane role on the group `AZURE_RESOURCE_GROUP` and
//! `AZURE_SANDBOX_GROUP` name; `AZURE_SANDBOX_IMAGE` must already be built there if a reference.
//!
//! `AZURE_SANDBOX_AGENT=true` runs it against an image with the agent layered on, through the agent.
//!
//! ```text
//! cargo test -p alien-bindings --features azure --test azure_sandbox_pause_live -- --ignored --nocapture
//! ```

#![cfg(feature = "azure")]

use std::collections::{BTreeMap, HashMap};

use alien_bindings::traits::{CreateSandboxRequest, RunCommandRequest, Sandbox, SandboxState};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::ClientConfig;
use futures::StreamExt;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

const PROBE: &str = r#"
import http.server, json, os, secrets, threading, time
TOKEN = secrets.token_hex(16)
STATE = {"ticks": 0}
def tick():
    while True:
        time.sleep(1)
        STATE["ticks"] += 1
threading.Thread(target=tick, daemon=True).start()
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps({"pid": os.getpid(), "token": TOKEN, "ticks": STATE["ticks"]}).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args):
        pass
http.server.ThreadingHTTPServer(("127.0.0.1", 9999), Handler).serve_forever()
"#;

#[derive(Debug, serde::Deserialize)]
struct Reading {
    pid: u32,
    token: String,
    ticks: u64,
}

async fn shell(sandbox: &dyn Sandbox, id: &str, script: &str) -> (i32, String) {
    let mut stream = sandbox
        .run_command(
            id,
            RunCommandRequest {
                command: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), script.to_string()],
                cwd: None,
                env: BTreeMap::new(),
                timeout: std::time::Duration::from_secs(60),
            },
        )
        .await
        .expect("the command starts");
    let mut stdout = Vec::new();
    let mut code = None;
    while let Some(frame) = stream.next().await {
        match frame.expect("a frame decodes") {
            alien_bindings::traits::CommandOutput::Stdout { data, .. } => stdout.extend(data),
            alien_bindings::traits::CommandOutput::Exit { code: exit, .. } => code = Some(exit),
            _ => {}
        }
    }
    (
        code.expect("an exit frame arrives"),
        String::from_utf8_lossy(&stdout).to_string(),
    )
}

async fn read_probe(sandbox: &dyn Sandbox, id: &str) -> Reading {
    let (code, out) = shell(
        sandbox,
        id,
        "python3 -c \"import urllib.request; print(urllib.request.urlopen('http://127.0.0.1:9999/', timeout=5).read().decode())\"",
    )
    .await;
    assert_eq!(code, 0, "the in-memory probe answers: {out}");
    serde_json::from_str(out.trim()).expect("the probe answers JSON")
}

/// `pause`, wait `AZURE_PAUSE_SECONDS` (default 120), `resume`: the process, its in-memory token and
/// a continuing counter come back. Prints one JSON line with the readings.
#[tokio::test]
#[ignore = "needs a live Azure sandbox group; spends minutes of wall-clock"]
async fn pause_and_resume_keep_memory() {
    let region = std::env::var("AZURE_SANDBOX_REGION").unwrap_or_else(|_| "eastus".to_string());
    let image = std::env::var("AZURE_SANDBOX_IMAGE")
        .unwrap_or_else(|_| "docker.io/library/python:3.14-slim".to_string());
    let pause_seconds: u64 = std::env::var("AZURE_PAUSE_SECONDS")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .unwrap_or(120);
    // `AZURE_SANDBOX_AGENT=true` for an image the release layered the agent onto: commands and
    // files then travel through the agent, whose files live under its session root.
    let agent = std::env::var("AZURE_SANDBOX_AGENT").is_ok_and(|value| value == "true");
    let probe_path = if agent { "probe.py" } else { "/tmp/probe.py" };
    let binding = serde_json::json!({
        "agent": agent,
        "service": "sandbox-azure",
        "sandboxGroup": env("AZURE_SANDBOX_GROUP"),
        "dataPlaneEndpoint": format!("https://management.{region}.azuredevcompute.io"),
        "region": region.clone(),
        "resourceGroup": env("AZURE_RESOURCE_GROUP"),
        "diskImage": image,
        "egress": { "mode": "allow" },
    });
    let config = ClientConfig::Azure(Box::new({
        let mut cfg = alien_azure_clients::live_test::live_test_config("TARGET");
        cfg.region = Some(region.clone());
        cfg
    }));
    let provider =
        BindingsProvider::new(config, HashMap::from([("box".to_string(), binding)])).unwrap();
    let sandbox = provider.load_sandbox("box").await.expect("loads");

    let instance = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap_or_else(|error| panic!("create failed: {error}"));
    let id = instance.sandbox_id.clone();

    // A failed assertion must not leave the sandbox running: the body's panic is held until the
    // sandbox is terminated, then raised again.
    let outcome = futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(async {
        sandbox
            .write_files(
                &id,
                BTreeMap::from([(probe_path.to_string(), PROBE.as_bytes().to_vec())]),
            )
            .await
            .expect("the probe is written");
        let (code, out) = shell(
            sandbox.as_ref(),
            &id,
            &format!(
                "setsid python3 {probe_path} >/dev/null 2>&1 </dev/null & sleep 2; echo started"
            ),
        )
        .await;
        assert_eq!(code, 0, "the probe starts: {out}");
        let before = read_probe(sandbox.as_ref(), &id).await;

        sandbox.pause(&id).await.expect("pause");
        // `pause` only accepts the stop; wait until the sandbox reports it, and fail if it never
        // does: a sandbox that stayed running would pass every check below.
        let mut paused = false;
        for _ in 0..30 {
            let state = sandbox.get(&id).await.expect("get").map(|s| s.state);
            if state == Some(SandboxState::Paused) {
                paused = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        assert!(paused, "the sandbox never reported Paused");
        tokio::time::sleep(std::time::Duration::from_secs(pause_seconds)).await;
        sandbox.resume(&id).await.expect("resume");

        let after = read_probe(sandbox.as_ref(), &id).await;
        println!(
            "{}",
            serde_json::json!({
                "event": "azure-cycle",
                "pauseSeconds": pause_seconds,
                "samePid": before.pid == after.pid,
                "sameToken": before.token == after.token,
                "ticksBefore": before.ticks,
                "ticksAfter": after.ticks,
            })
        );
        assert_eq!(before.pid, after.pid, "the probe process was replaced");
        assert_eq!(before.token, after.token, "the in-memory token changed");
        assert!(after.ticks > before.ticks, "the counter did not continue");
    }))
    .await;

    sandbox.terminate(&id).await.expect("terminate");
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
