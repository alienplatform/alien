//! Cleanup utilities for tearing down test resources.

use tracing::{info, warn};

/// Destroy all deployments known to the manager.
///
/// This is a best-effort helper intended for test teardown. Errors on
/// individual deployments are logged but do not short-circuit the loop.
pub async fn cleanup_deployments(
    admin_token: &str,
    manager_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let http = reqwest::Client::builder()
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", admin_token))?,
            );
            h
        })
        .build()?;

    // List all deployments
    let list_url = format!("{}/v1/deployments", manager_url);
    let resp = http.get(&list_url).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to list deployments ({}): {}", status, body).into());
    }

    let body: serde_json::Value = resp.json().await?;
    let deployments = body
        .as_array()
        .or_else(|| body.get("deployments").and_then(|v| v.as_array()))
        .cloned()
        .unwrap_or_default();

    info!(count = deployments.len(), "cleaning up deployments");

    for dep in &deployments {
        let id = match dep.get("id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };

        let delete_url = format!("{}/v1/deployments/{}", manager_url, id);
        match http.delete(&delete_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(%id, "deployment deleted");
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                warn!(%id, %status, %body, "failed to delete deployment (continuing)");
            }
            Err(e) => {
                warn!(%id, error = %e, "failed to delete deployment (continuing)");
            }
        }
    }

    Ok(())
}

/// Stop and remove alien-agent Docker containers started during tests.
///
/// Looks for containers matching a label or name pattern and removes them.
/// Best-effort: errors are logged but do not fail the cleanup.
pub async fn cleanup_agent_containers(label: &str) -> Result<(), Box<dyn std::error::Error>> {
    info!(%label, "cleaning up alien-agent containers");

    // List containers with the test label
    let output = tokio::process::Command::new("docker")
        .args(["ps", "-a", "-q", "--filter", &format!("label={}", label)])
        .output()
        .await?;

    let container_ids: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();

    if container_ids.is_empty() {
        info!("no alien-agent containers to clean up");
        return Ok(());
    }

    info!(
        count = container_ids.len(),
        "removing alien-agent containers"
    );

    for id in &container_ids {
        match tokio::process::Command::new("docker")
            .args(["rm", "-f", id])
            .output()
            .await
        {
            Ok(out) if out.status.success() => {
                info!(container = %id, "container removed");
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                warn!(container = %id, %stderr, "failed to remove container (continuing)");
            }
            Err(e) => {
                warn!(container = %id, error = %e, "failed to remove container (continuing)");
            }
        }
    }

    Ok(())
}

/// Helm uninstall a test release. Best-effort: errors are logged but do not
/// fail the cleanup.
pub async fn cleanup_helm_release(
    release_name: &str,
    namespace: &str,
    kubeconfig: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(%release_name, %namespace, "cleaning up helm release");

    let mut cmd = tokio::process::Command::new("helm");
    cmd.args(["uninstall", release_name, "--namespace", namespace]);

    if let Some(kc) = kubeconfig {
        cmd.env("KUBECONFIG", kc);
    }

    match cmd.output().await {
        Ok(out) if out.status.success() => {
            info!(%release_name, "helm release uninstalled");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            warn!(%release_name, %stderr, "failed to uninstall helm release (continuing)");
        }
        Err(e) => {
            warn!(%release_name, error = %e, "failed to uninstall helm release (continuing)");
        }
    }

    Ok(())
}

/// Clean up all test-related resources. Combines deployment cleanup with
/// container and temp directory cleanup.
pub async fn cleanup_all(
    admin_token: &str,
    manager_url: &str,
    container_label: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Best-effort: run all cleanup steps regardless of individual failures
    if let Err(e) = cleanup_deployments(admin_token, manager_url).await {
        warn!(error = %e, "deployment cleanup failed (continuing)");
    }

    if let Some(label) = container_label {
        if let Err(e) = cleanup_agent_containers(label).await {
            warn!(error = %e, "container cleanup failed (continuing)");
        }
    }

    Ok(())
}
