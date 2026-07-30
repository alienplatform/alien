//! Integration tests for the `alien-ai-gateway` container launcher binary.
//!
//! These cover the passthrough paths (no cloud credentials needed). Credential
//! injection is exercised only by the end-to-end cloud tests; the lib's
//! `gateway_starts_and_serves_health` covers startup and health on empty bindings.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

// With no ALIEN_*_BINDING for an AI resource, the launcher must exec the app
// unchanged and must NOT set ALIEN_AI_GATEWAY_URL.
#[test]
fn passthrough_execs_app_without_gateway_when_no_ai_binding() {
    let exe = env!("CARGO_BIN_EXE_alien-ai-gateway");
    // env_clear so a stray ALIEN_*_BINDING in the dev/CI env can't flip this into a
    // gateway-spawn path. /bin/sh is an absolute path, so no PATH is needed.
    let out = Command::new(exe)
        .env_clear()
        .args(["--", "/bin/sh", "-c", "printf '%s' \"gw=${ALIEN_AI_GATEWAY_URL:-unset}\""])
        .output()
        .expect("launcher should run");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "gw=unset");
}

// The SDK-spawn path: `--gateway-serve` with no pinned port must bind an ephemeral
// port and print exactly one machine-readable URL line to stdout, then serve a
// reachable gateway at that URL. This is the contract the TS side (gateway.ts)
// parses; it is the only test that drives the real binary end to end.
#[test]
fn gateway_serve_announces_a_reachable_ephemeral_url() {
    let exe = env!("CARGO_BIN_EXE_alien-ai-gateway");
    // env_clear so no stray ALIEN_AI_GATEWAY_PORT forces a fixed port and no
    // ALIEN_*_BINDING is present (empty bindings still serve /healthz).
    let mut child = Command::new(exe)
        .env_clear()
        .arg("--gateway-serve")
        .stdout(Stdio::piped())
        .spawn()
        .expect("gateway-serve should start");

    let mut line = String::new();
    let read = BufReader::new(child.stdout.take().expect("piped stdout")).read_line(&mut line);
    let url = read
        .ok()
        .and_then(|_| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .and_then(|v| v["aiGatewayUrl"].as_str().map(str::to_owned));
    // Probe reachability while the child is still alive.
    let reachable = url.as_deref().map(alien_ai_gateway::wait_until_ready_blocking);

    // Reap unconditionally, before any assertion, so a broken binary (crash before
    // printing, malformed output, missing field) can't leave the `--gateway-serve`
    // child orphaned on its `pending` future.
    let _ = child.kill();
    let _ = child.wait();

    let url = url.expect("gateway-serve must print a JSON line carrying aiGatewayUrl");
    assert!(url.starts_with("http://127.0.0.1:"), "expected a loopback URL, got {url:?}");
    assert!(!url.ends_with(":0"), "must report the OS-assigned port, not :0: {url:?}");
    assert_eq!(reachable, Some(true), "the announced gateway URL must be reachable: {url}");
}

// A malformed invocation (no `--` separator, no command) fails fast, non-zero.
#[test]
fn missing_command_fails_fast() {
    let exe = env!("CARGO_BIN_EXE_alien-ai-gateway");
    let out = Command::new(exe).output().expect("launcher should run");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("alien-ai-gateway"));
}

// A BYO-key (External) binding is not served by the gateway; the launcher must
// treat it as "no gateway" and exec the app directly (the SDK uses the key).
#[test]
fn external_binding_is_passthrough() {
    let exe = env!("CARGO_BIN_EXE_alien-ai-gateway");
    // env_clear so only the External binding under test is present.
    let out = Command::new(exe)
        .env_clear()
        .args(["--", "/bin/sh", "-c", "printf '%s' \"gw=${ALIEN_AI_GATEWAY_URL:-unset}\""])
        .env(
            "ALIEN_LLM_BINDING",
            r#"{"service":"external","provider":"openai","apiKey":"sk-x"}"#,
        )
        .output()
        .expect("launcher should run");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "gw=unset");
}
