//! Target-scoped command routing checks.
//!
//! Runs against the `command-routing-ts` example app: one deployment with TWO
//! command-capable resources that both register a command named `status` —
//! Worker `api` (push model, SDK `command()` registrar) and Daemon
//! `indexer-daemon` (pull model, `createCommandReceiver()` lease loop).
//!
//! These checks are handler-execution proofs, designed to fail if:
//! - push delivery to the Worker breaks (the `api` handler never answers),
//! - the Daemon's pull receiver breaks or leases a command addressed to the
//!   other resource (the `role`/`model` fields in the answers would flip),
//! - target routing regresses to name-only routing (both targets would answer
//!   identically, or the untargeted ambiguous invoke would start succeeding),
//! - direct in-process bindings break inside the Daemon (its `documents`
//!   count comes from a `kv` binding it maintains itself, runtime-less).

use alien_test::TestDeployment;
use anyhow::{bail, Context};
use serde_json::Value;
use tracing::info;

/// Resource ids and expectations from examples/command-routing-ts/alien.ts.
const WORKER_RESOURCE: &str = "api";
const DAEMON_RESOURCE: &str = "indexer-daemon";
/// The daemon seeds exactly these documents into the shared `index` KV.
const SEEDED_DOCUMENT_COUNT: u64 = 4;

/// How long to wait for the daemon to seed its index after startup.
const INDEX_SEED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
const INDEX_SEED_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

fn string_field<'a>(value: &'a Value, field: &str, label: &str) -> anyhow::Result<&'a str> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .with_context(|| format!("{label} response missing string field '{field}': {value:?}"))
}

/// Run all target-routing checks against a command-routing-ts deployment.
#[allow(dead_code)]
pub async fn check_command_routing(deployment: &TestDeployment) -> anyhow::Result<()> {
    check_worker_target(deployment).await?;
    check_daemon_target(deployment).await?;
    check_untargeted_invoke_is_rejected(deployment).await?;
    check_cross_resource_search(deployment).await?;
    Ok(())
}

/// `status` targeted at the Worker must be answered by the Worker's push
/// handler — proven by the identity fields only that handler returns.
async fn check_worker_target(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking `status` targeted at the Worker (push)");

    let result = deployment
        .invoke_command_on_target(WORKER_RESOURCE, "status", serde_json::json!({}))
        .await
        .map_err(|e| anyhow::anyhow!("status → {WORKER_RESOURCE} invocation failed: {e}"))?;

    if string_field(&result, "resource", "worker status")? != WORKER_RESOURCE {
        bail!("worker status answered by wrong resource: {result:?}");
    }
    if string_field(&result, "role", "worker status")? != "worker" {
        bail!("worker status not answered by the Worker push handler: {result:?}");
    }
    if string_field(&result, "model", "worker status")? != "push" {
        bail!("worker status not delivered via the push model: {result:?}");
    }

    info!("Worker-targeted status check passed");
    Ok(())
}

/// `status` targeted at the Daemon must be answered by the Daemon's pull
/// receiver, and its `documents` count proves the Daemon's own in-process
/// `kv` binding round-trips (it writes the index and counts it back).
///
/// The daemon seeds the index shortly after startup, so retry until the
/// full seed set is visible.
async fn check_daemon_target(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking `status` targeted at the Daemon (pull receiver)");

    let last_seen = std::cell::Cell::new(0u64);
    let seeded = super::poll_until(INDEX_SEED_TIMEOUT, INDEX_SEED_POLL_INTERVAL, || async {
        let result = deployment
            .invoke_command_on_target(DAEMON_RESOURCE, "status", serde_json::json!({}))
            .await
            .map_err(|e| anyhow::anyhow!("status → {DAEMON_RESOURCE} invocation failed: {e}"))?;

        if string_field(&result, "resource", "daemon status")? != DAEMON_RESOURCE {
            bail!("daemon status answered by wrong resource: {result:?}");
        }
        if string_field(&result, "role", "daemon status")? != "daemon" {
            bail!("daemon status not answered by the Daemon receiver: {result:?}");
        }
        if string_field(&result, "model", "daemon status")? != "pull" {
            bail!("daemon status not delivered via the pull model: {result:?}");
        }

        let documents = result
            .get("documents")
            .and_then(|v| v.as_u64())
            .with_context(|| format!("daemon status missing numeric documents: {result:?}"))?;
        last_seen.set(documents);
        if documents >= SEEDED_DOCUMENT_COUNT {
            return Ok(Some(documents));
        }
        info!(documents, "Daemon index not fully seeded yet, retrying");
        Ok(None)
    })
    .await?;

    let Some(documents) = seeded else {
        bail!(
            "daemon never finished seeding its index: expected {SEEDED_DOCUMENT_COUNT} documents, last saw {}. \
             The daemon's in-process kv binding writes are not landing.",
            last_seen.get()
        );
    };
    info!(documents, "Daemon-targeted status check passed");
    Ok(())
}

/// With two command-capable resources in the deployment, an invoke WITHOUT an
/// explicit target must be rejected — single-target shorthand must not pick
/// one silently.
async fn check_untargeted_invoke_is_rejected(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!(
        "Checking that an untargeted `status` invoke is rejected (two command-capable resources)"
    );

    match deployment
        .invoke_command("status", serde_json::json!({}))
        .await
    {
        Ok(result) => bail!(
            "untargeted `status` invoke must fail when the deployment has two \
             command-capable resources, but it succeeded with: {result:?}"
        ),
        Err(error) => {
            info!(%error, "Untargeted ambiguous invoke correctly rejected");
            Ok(())
        }
    }
}

/// `search` targeted at the Worker reads the shared index the Daemon seeded:
/// two different resources, one KV, both using direct in-process bindings.
async fn check_cross_resource_search(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking cross-resource search (Worker reads the Daemon-seeded index)");

    // "resident" appears in exactly one seeded document: "daemons".
    let result = deployment
        .invoke_command_on_target(
            WORKER_RESOURCE,
            "search",
            serde_json::json!({ "term": "resident" }),
        )
        .await
        .map_err(|e| anyhow::anyhow!("search → {WORKER_RESOURCE} invocation failed: {e}"))?;

    if string_field(&result, "resource", "search")? != WORKER_RESOURCE {
        bail!("search answered by wrong resource: {result:?}");
    }
    let hits: Vec<&str> = result
        .get("hits")
        .and_then(|v| v.as_array())
        .with_context(|| format!("search response missing hits array: {result:?}"))?
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    if hits != ["daemons"] {
        bail!(
            "search for 'resident' should hit exactly the daemon-seeded 'daemons' document, got {hits:?}"
        );
    }

    info!("Cross-resource search check passed");
    Ok(())
}
