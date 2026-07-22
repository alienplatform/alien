//! Integration tests for the `alien-ai-gateway` container launcher binary.
//!
//! These cover the passthrough paths (no cloud credentials needed). Credential
//! injection is exercised only by the end-to-end cloud tests; the lib's
//! `gateway_starts_and_serves_health` covers startup and health on empty bindings.

use std::process::Command;

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
