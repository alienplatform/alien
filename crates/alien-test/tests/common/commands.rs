//! Command invocation tests.
//!
//! These tests exercise the command system by invoking commands on the deployed
//! test app via the manager API and verifying the responses.

use alien_test::TestDeployment;
use anyhow::bail;
use tracing::info;

/// Run all command checks against the deployment.
pub async fn check_commands(deployment: &TestDeployment) -> anyhow::Result<()> {
    check_command_echo(deployment).await?;
    check_command_small(deployment).await?;
    check_command_large_response(deployment).await?;
    check_command_medium_request(deployment).await?;
    check_command_large_request(deployment).await?;
    check_command_large_both(deployment).await?;
    Ok(())
}

/// Echo command: send params, expect them returned unchanged.
pub async fn check_command_echo(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking echo command");

    let test_params = serde_json::json!({
        "message": "test-echo-command",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let result = deployment
        .invoke_command("echo", test_params.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Echo command invocation failed: {}", e))?;

    if result != test_params {
        bail!(
            "Echo command did not return params unchanged.\nExpected: {}\nGot: {}",
            serde_json::to_string_pretty(&test_params)?,
            serde_json::to_string_pretty(&result)?
        );
    }

    info!("Echo command check passed");
    Ok(())
}

/// Small request, small response: test inline path for both directions.
pub async fn check_command_small(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking small request/response command");

    let test_params = serde_json::json!({
        "testType": "small-payload",
        "data": format!("test-data-{}", uuid::Uuid::new_v4()),
    });

    let result = deployment
        .invoke_command("cmd-test-small", test_params)
        .await
        .map_err(|e| anyhow::anyhow!("Small payload command invocation failed: {}", e))?;

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        bail!("Small payload command reported failure: {:?}", result);
    }

    let test_type = result
        .get("testType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if test_type != "cmd-small-payload" {
        bail!(
            "Small payload command returned unexpected testType: {}",
            test_type
        );
    }

    let has_hash = result
        .get("paramsHash")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_hash {
        bail!("Small payload command response missing paramsHash");
    }

    info!("Small request/response command check passed");
    Ok(())
}

/// Small request, large response: test storage-based response path (>48KB).
pub async fn check_command_large_response(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking large response command");

    let test_params = serde_json::json!({
        "testType": "large-payload",
        "data": format!("test-data-{}", uuid::Uuid::new_v4()),
    });

    let result = deployment
        .invoke_command("cmd-test-large-response", test_params)
        .await
        .map_err(|e| anyhow::anyhow!("Large response command invocation failed: {}", e))?;

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        bail!("Large response command reported failure: {:?}", result);
    }

    let test_type = result
        .get("testType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if test_type != "cmd-large-payload" {
        bail!(
            "Large response command returned unexpected testType: {}",
            test_type
        );
    }

    let bulk_data = result.get("bulkData").and_then(|v| v.as_array());
    match bulk_data {
        Some(arr) if arr.len() >= 1000 => {}
        Some(arr) => {
            bail!(
                "Large response command bulkData too small: {} items (expected >= 1000)",
                arr.len()
            );
        }
        None => {
            bail!("Large response command response missing or invalid bulkData array");
        }
    }

    info!("Large response command check passed");
    Ok(())
}

/// Generate medium JSON params (~50KB, >20KB KV limit but <150KB transport limit).
/// This exercises the auto-promote + re-inline path in build_envelope().
fn medium_params(test_type: &str) -> serde_json::Value {
    serde_json::json!({
        "testType": test_type,
        "data": format!("test-data-{}", uuid::Uuid::new_v4()),
        "bulkData": (0..3000).map(|i| format!("param-item-{}", i)).collect::<Vec<_>>(),
    })
}

/// Generate a large JSON params object (>150KB to trigger storage mode).
fn large_params(test_type: &str) -> serde_json::Value {
    serde_json::json!({
        "testType": test_type,
        "data": format!("test-data-{}", uuid::Uuid::new_v4()),
        "bulkData": (0..20000).map(|i| format!("param-item-{}", i)).collect::<Vec<_>>(),
    })
}

/// Medium request (~50KB), small response: test auto-promote + re-inline path.
/// Params exceed KV limit (20KB) but fit in transport limit (150KB), so server
/// auto-promotes to blob then re-inlines into the envelope on delivery.
pub async fn check_command_medium_request(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking medium request command (auto-promote + re-inline path)");

    let test_params = medium_params("medium-request");

    let result = deployment
        .invoke_command("cmd-test-medium-request", test_params)
        .await
        .map_err(|e| anyhow::anyhow!("Medium request command invocation failed: {}", e))?;

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        bail!("Medium request command reported failure: {:?}", result);
    }

    let test_type = result
        .get("testType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if test_type != "cmd-medium-request" {
        bail!(
            "Medium request command returned unexpected testType: {}",
            test_type
        );
    }

    let has_hash = result
        .get("paramsHash")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_hash {
        bail!("Medium request command response missing paramsHash");
    }

    info!("Medium request command check passed");
    Ok(())
}

/// Large request, small response: test storage-based request path (>150KB params).
pub async fn check_command_large_request(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking large request command");

    let test_params = large_params("large-request");

    let result = deployment
        .invoke_command("cmd-test-large-request", test_params)
        .await
        .map_err(|e| anyhow::anyhow!("Large request command invocation failed: {}", e))?;

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        bail!("Large request command reported failure: {:?}", result);
    }

    let test_type = result
        .get("testType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if test_type != "cmd-large-request" {
        bail!(
            "Large request command returned unexpected testType: {}",
            test_type
        );
    }

    let has_hash = result
        .get("paramsHash")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if !has_hash {
        bail!("Large request command response missing paramsHash");
    }

    info!("Large request command check passed");
    Ok(())
}

/// Large request, large response: test storage-based path in both directions.
pub async fn check_command_large_both(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking large request + large response command");

    let test_params = large_params("large-both");

    let result = deployment
        .invoke_command("cmd-test-large-both", test_params)
        .await
        .map_err(|e| anyhow::anyhow!("Large both command invocation failed: {}", e))?;

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        bail!("Large both command reported failure: {:?}", result);
    }

    let test_type = result
        .get("testType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if test_type != "cmd-large-both" {
        bail!(
            "Large both command returned unexpected testType: {}",
            test_type
        );
    }

    let bulk_data = result.get("bulkData").and_then(|v| v.as_array());
    match bulk_data {
        Some(arr) if arr.len() >= 1000 => {}
        Some(arr) => {
            bail!(
                "Large both command bulkData too small: {} items (expected >= 1000)",
                arr.len()
            );
        }
        None => {
            bail!("Large both command response missing or invalid bulkData array");
        }
    }

    info!("Large request + large response command check passed");
    Ok(())
}
