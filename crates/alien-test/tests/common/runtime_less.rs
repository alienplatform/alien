//! Mixed runtime-less application checks.
//!
//! The fixture has a TypeScript Container and Rust Daemon that both register
//! `status` and use a direct KV binding. Each handler reports its own identity
//! and verifies the keys it seeded. A pass therefore requires both language
//! bindings, both pull receivers, and target-scoped routing.

use alien_test::TestDeployment;
use anyhow::{bail, Context};
use serde_json::Value;
use tracing::info;

const TYPESCRIPT_CONTAINER: &str = "typescript-container";
const RUST_DAEMON: &str = "rust-daemon";
const SEEDED_DOCUMENT_COUNT: u64 = 4;
const SEED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
const SEED_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Copy)]
struct TargetExpectation {
    resource: &'static str,
    role: &'static str,
    language: &'static str,
}

/// Prove both runtime-less processes and reject ambiguous untargeted routing.
#[allow(dead_code)]
pub async fn check_mixed_runtime_less(deployment: &TestDeployment) -> anyhow::Result<()> {
    check_target(
        deployment,
        TargetExpectation {
            resource: TYPESCRIPT_CONTAINER,
            role: "container",
            language: "typescript",
        },
    )
    .await?;
    check_target(
        deployment,
        TargetExpectation {
            resource: RUST_DAEMON,
            role: "daemon",
            language: "rust",
        },
    )
    .await?;
    check_untargeted_status_is_rejected(deployment).await
}

async fn check_target(
    deployment: &TestDeployment,
    expected: TargetExpectation,
) -> anyhow::Result<()> {
    info!(
        target = expected.resource,
        "Checking mixed runtime-less target"
    );

    let last_count = std::cell::Cell::new(0u64);
    let ready = super::poll_until(SEED_TIMEOUT, SEED_POLL_INTERVAL, || async {
        let result = deployment
            .invoke_command_on_target(expected.resource, "status", serde_json::json!({}))
            .await
            .map_err(|error| {
                anyhow::anyhow!("status → {} invocation failed: {error}", expected.resource)
            })?;

        require_string(&result, "resource", expected.resource)?;
        require_string(&result, "role", expected.role)?;
        require_string(&result, "language", expected.language)?;
        require_string(&result, "model", "pull")?;

        let own_documents = numeric_field(&result, "ownDocuments")?;
        last_count.set(own_documents);

        if own_documents == SEEDED_DOCUMENT_COUNT {
            return Ok(Some(()));
        }

        info!(
            target = expected.resource,
            own_documents, "Target has not fully seeded its index yet"
        );
        Ok(None)
    })
    .await?;

    if ready.is_none() {
        let own_documents = last_count.get();
        bail!(
            "{} never observed its complete index: expected {SEEDED_DOCUMENT_COUNT} documents, saw {own_documents}",
            expected.resource
        );
    }

    info!(
        target = expected.resource,
        "Mixed runtime-less target passed"
    );
    Ok(())
}

fn require_string(value: &Value, field: &str, expected: &str) -> anyhow::Result<()> {
    let actual = value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("status response missing string field '{field}': {value:?}"))?;
    if actual != expected {
        bail!("status response field '{field}' must be '{expected}', got '{actual}': {value:?}");
    }
    Ok(())
}

fn numeric_field(value: &Value, field: &str) -> anyhow::Result<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .with_context(|| format!("status response missing numeric field '{field}': {value:?}"))
}

async fn check_untargeted_status_is_rejected(deployment: &TestDeployment) -> anyhow::Result<()> {
    match deployment
        .invoke_command("status", serde_json::json!({}))
        .await
    {
        Ok(result) => bail!(
            "untargeted `status` must fail with two command-capable resources, but succeeded: {result:?}"
        ),
        Err(error) => {
            let mut rendered = error.to_string();
            let mut source = error.source();
            while let Some(cause) = source {
                rendered.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            if !rendered.contains("COMMAND_TARGET_AMBIGUOUS") {
                bail!(
                    "untargeted `status` must fail with COMMAND_TARGET_AMBIGUOUS, got: {rendered}"
                );
            }
            info!(%error, "Untargeted mixed runtime-less command correctly rejected");
            Ok(())
        }
    }
}
