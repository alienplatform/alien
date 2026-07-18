//! Rust source-Container checks.
//!
//! Runs against the `container-rust` test app: one Rust SOURCE Container
//! (`indexer`) whose compiled binary is the image entrypoint, with a direct
//! in-process `index` KV binding and the app-owned pull command receiver.
//!
//! The `status` check is a handler-execution proof, designed to fail if:
//! - the container's pull receiver breaks (no answer at all),
//! - target routing regresses (a non-container identity in the answer),
//! - direct in-process bindings break inside the container (`documents`
//!   counts the KV documents the container seeded at startup — a real
//!   KV round-trip inside the compiled Linux binary, mirroring what
//!   `routing.rs` proves for the TypeScript Daemon).

use alien_test::TestDeployment;
use anyhow::{bail, Context};
use serde_json::Value;
use tracing::info;

/// Resource id and expectations from tests/e2e/test-apps/container-rust/alien.ts.
const CONTAINER_RESOURCE: &str = "indexer";
/// The container seeds exactly these documents into its `index` KV.
const SEEDED_DOCUMENT_COUNT: u64 = 4;

fn string_field<'a>(value: &'a Value, field: &str) -> anyhow::Result<&'a str> {
    value.get(field).and_then(|v| v.as_str()).with_context(|| {
        format!("container status response missing string field '{field}': {value:?}")
    })
}

/// `status` targeted at the Rust container must be answered by its pull
/// receiver, with a `documents` count proving the in-process KV round-trip.
#[allow(dead_code)]
pub async fn check_container_status(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking `status` targeted at the Rust container (pull)");

    let result = deployment
        .invoke_command_on_target(CONTAINER_RESOURCE, "status", serde_json::json!({}))
        .await
        .map_err(|e| anyhow::anyhow!("status → {CONTAINER_RESOURCE} invocation failed: {e}"))?;

    if string_field(&result, "resource")? != CONTAINER_RESOURCE {
        bail!("container status answered by wrong resource: {result:?}");
    }
    if string_field(&result, "role")? != "container" {
        bail!("container status not answered by the container receiver: {result:?}");
    }
    if string_field(&result, "model")? != "pull" {
        bail!("container status not delivered via the pull model: {result:?}");
    }
    let documents = result
        .get("documents")
        .and_then(|v| v.as_u64())
        .with_context(|| format!("container status response missing 'documents': {result:?}"))?;
    if documents != SEEDED_DOCUMENT_COUNT {
        bail!(
            "container KV round-trip broken: expected {SEEDED_DOCUMENT_COUNT} seeded documents, got {documents}"
        );
    }

    info!(documents, "Rust container status check passed");
    Ok(())
}
