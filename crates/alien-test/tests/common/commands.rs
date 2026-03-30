//! Command invocation tests.
//!
//! These tests exercise the command system by invoking commands on the deployed
//! test app via the manager API and verifying the responses.

use alien_test::TestDeployment;
use anyhow::bail;
use tracing::info;

/// Run all command checks against the deployment.
///
/// `skip_large_payload`: set to true for push-mode cloud deployments where the
/// test manager uses local command storage that Lambda/Cloud Run can't access.
pub async fn check_commands(
    deployment: &TestDeployment,
    skip_large_payload: bool,
) -> anyhow::Result<()> {
    check_command_echo(deployment).await?;
    check_command_small_payload(deployment).await?;
    if skip_large_payload || deployment.platform == "local" {
        info!(
            "Skipping large payload command check (platform={}, skip_large_payload={})",
            deployment.platform, skip_large_payload
        );
    } else {
        check_command_large_payload(deployment).await?;
    }
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

/// Small payload command: test inline response path.
pub async fn check_command_small_payload(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking small payload command");

    let test_params = serde_json::json!({
        "testType": "small-payload",
        "data": format!("test-data-{}", uuid::Uuid::new_v4()),
    });

    let result = deployment
        .invoke_command("arc-test-small", test_params)
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
    if test_type != "arc-small-payload" {
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

    info!("Small payload command check passed");
    Ok(())
}

/// Large payload command: test storage-based response path (>48KB).
pub async fn check_command_large_payload(deployment: &TestDeployment) -> anyhow::Result<()> {
    info!("Checking large payload command");

    let test_params = serde_json::json!({
        "testType": "large-payload",
        "data": format!("test-data-{}", uuid::Uuid::new_v4()),
    });

    let result = deployment
        .invoke_command("arc-test-large", test_params)
        .await
        .map_err(|e| anyhow::anyhow!("Large payload command invocation failed: {}", e))?;

    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        bail!("Large payload command reported failure: {:?}", result);
    }

    let test_type = result
        .get("testType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if test_type != "arc-large-payload" {
        bail!(
            "Large payload command returned unexpected testType: {}",
            test_type
        );
    }

    let bulk_data = result.get("bulkData").and_then(|v| v.as_array());
    match bulk_data {
        Some(arr) if arr.len() >= 1000 => {}
        Some(arr) => {
            bail!(
                "Large payload command bulkData too small: {} items (expected >= 1000)",
                arr.len()
            );
        }
        None => {
            bail!("Large payload command response missing or invalid bulkData array");
        }
    }

    info!("Large payload command check passed");
    Ok(())
}
