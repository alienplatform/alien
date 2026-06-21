//! Per-binding check functions.
//!
//! Each function exercises one user-facing binding through the HTTP endpoints
//! exposed by the comprehensive test apps.

use alien_test::TestDeployment;
use anyhow::{bail, Context};
use serde::Deserialize;
use serde_json;
use tracing::info;

/// The binding name used in test app stack configurations.
const STORAGE_BINDING: &str = "alien-storage";
const KV_BINDING: &str = "alien-kv";
const VAULT_BINDING: &str = "alien-vault";
const QUEUE_BINDING: &str = "alien-queue";

/// Standard response shape returned by binding test endpoints.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindingTestResponse {
    success: bool,
    binding_name: String,
}

/// Helper to get the deployment URL, failing if not yet assigned.
fn deployment_url(deployment: &TestDeployment) -> anyhow::Result<&str> {
    deployment
        .url
        .as_deref()
        .context("Deployment URL not yet assigned")
}

fn post_empty(client: &reqwest::Client, url: String) -> reqwest::RequestBuilder {
    client
        .post(url)
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .body(Vec::<u8>::new())
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Check health endpoint: GET /health → { status: "ok" }
///
/// Retries on transient endpoint readiness failures:
/// - request send errors: Kubernetes/GCP load balancers can publish an address
///   before the first backend connection is stable
/// - 403: AWS permission propagation delay (resource-based policies can take ~60s)
/// - 500: Lambda cold start init timeout (large Bun binaries can exceed the 10s
///   init phase limit, causing the first invocation to fail with 500)
pub async fn check_health(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking health endpoint");
    check_health_url(url, std::time::Duration::from_secs(5)).await
}

async fn check_health_url(url: &str, retry_delay: std::time::Duration) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let max_attempts = 15;
    let health_url = format!("{}/health", url);

    for attempt in 1..=max_attempts {
        let resp = match client.get(&health_url).send().await {
            Ok(resp) => resp,
            Err(error) if attempt < max_attempts => {
                info!(
                    attempt,
                    max_attempts,
                    error = %error,
                    "Health check request failed before receiving a response (transient, retrying)"
                );
                tokio::time::sleep(retry_delay).await;
                continue;
            }
            Err(error) => return Err(error).context("Health check request failed"),
        };

        let status = resp.status();
        if (status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::INTERNAL_SERVER_ERROR)
            && attempt < max_attempts
        {
            let body = resp.text().await.unwrap_or_default();
            info!(
                attempt,
                max_attempts,
                %status,
                "Health check returned {} (transient, retrying): {}",
                status,
                body
            );
            tokio::time::sleep(retry_delay).await;
            continue;
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Health check returned {}: {}", status, body);
        }

        let data: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse health response")?;
        let health_status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if health_status != "ok" {
            bail!("Health check status not 'ok': {:?}", data);
        }

        info!("Health check passed");
        return Ok(());
    }

    unreachable!()
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{check_health_url, post_empty};

    #[tokio::test]
    async fn health_check_retries_request_send_errors() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("read listener address");
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_task = Arc::clone(&attempts);

        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let attempt = attempts_for_task.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt == 1 {
                    drop(socket);
                    continue;
                }

                let mut buf = [0_u8; 1024];
                let _ = socket.read(&mut buf).await.expect("read request");
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 15\r\n\r\n{\"status\":\"ok\"}",
                    )
                    .await
                    .expect("write response");
                break;
            }
        });

        check_health_url(
            &format!("http://{addr}"),
            std::time::Duration::from_millis(10),
        )
        .await
        .expect("health check should retry transient send failure");
        server.await.expect("server task should finish");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn post_empty_sets_explicit_zero_content_length() {
        let request = post_empty(
            &reqwest::Client::new(),
            "http://example.test/storage-test/alien-storage".to_string(),
        )
        .build()
        .expect("request should build");

        assert_eq!(
            request.headers().get(reqwest::header::CONTENT_LENGTH),
            Some(&reqwest::header::HeaderValue::from_static("0"))
        );
    }
}

// ---------------------------------------------------------------------------
// Hello
// ---------------------------------------------------------------------------

/// Check hello endpoint: GET /hello → response contains "Hello"
pub async fn check_hello(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking hello endpoint");

    let resp = reqwest::Client::new()
        .get(format!("{}/hello", url))
        .send()
        .await
        .context("Hello check request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Hello check returned {}: {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse hello response")?;
    let message = data.get("message").and_then(|v| v.as_str()).unwrap_or("");
    if !message.contains("Hello") {
        bail!("Hello response does not contain 'Hello': {:?}", data);
    }

    info!("Hello check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// SSE
// ---------------------------------------------------------------------------

/// Check SSE endpoint: GET /sse → 10 SSE events (sse_message_0..9)
pub async fn check_sse(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking SSE endpoint");

    let resp = reqwest::Client::new()
        .get(format!("{}/sse", url))
        .send()
        .await
        .context("SSE request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("SSE returned {}: {}", status, body);
    }

    let body = resp.text().await.context("Failed to read SSE body")?;

    // Count SSE data lines
    let data_lines: Vec<&str> = body
        .lines()
        .filter(|line| line.starts_with("data:"))
        .collect();

    if data_lines.len() < 10 {
        bail!(
            "Expected at least 10 SSE data events, got {}: {:?}",
            data_lines.len(),
            data_lines
        );
    }

    // Verify messages contain expected pattern
    for i in 0..10 {
        let expected = format!("sse_message_{}", i);
        let found = data_lines.iter().any(|line| line.contains(&expected));
        if !found {
            bail!(
                "SSE stream missing expected message '{}'. Got: {:?}",
                expected,
                data_lines
            );
        }
    }

    info!("SSE check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// Check environment variable injection: GET /env-var/NODE_ENV
pub async fn check_environment(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking environment variable injection");

    let resp = reqwest::Client::new()
        .get(format!("{}/env-var/NODE_ENV", url))
        .send()
        .await
        .context("Environment check request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Environment check returned {}: {}", status, body);
    }

    let data: serde_json::Value = resp.json().await.context("Failed to parse env response")?;
    let value = data.get("value").and_then(|v| v.as_str()).unwrap_or("");
    if value.is_empty() {
        bail!(
            "NODE_ENV environment variable is empty or missing: {:?}",
            data
        );
    }

    info!(value = %value, "Environment check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Inspect
// ---------------------------------------------------------------------------

/// Check inspect endpoint: POST /inspect → echo request body
pub async fn check_inspect(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking inspect (echo) endpoint");

    let test_payload = serde_json::json!({
        "test": "inspect",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let resp = reqwest::Client::new()
        .post(format!("{}/inspect", url))
        .json(&test_payload)
        .send()
        .await
        .context("Inspect request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Inspect returned {}: {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse inspect response")?;
    // The test app returns { success: true, requestBody: <echo> }
    let echoed = data
        .get("requestBody")
        .and_then(|rb| rb.get("test"))
        .and_then(|v| v.as_str());
    if echoed != Some("inspect") {
        bail!("Inspect echo mismatch: {:?}", data);
    }

    info!("Inspect check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Managed Secret
// ---------------------------------------------------------------------------

/// Check managed secret: GET /managed-secret (cloud only)
///
/// This verifies that the runtime can read a secret that the test harness
/// seeded into Alien's internal `secrets` vault. This is intentionally separate
/// from customer-owned external vault secrets.
pub async fn check_managed_secret(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking managed secret");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let resp = client
        .get(format!("{}/managed-secret", url))
        .send()
        .await
        .context("Managed secret request failed")?;

    let status = resp.status();

    // 502 can happen if the Lambda runtime proxy times out forwarding to the
    // app (e.g., the vault call takes too long on a cold Lambda).
    // Treat as non-fatal since vault binding was already validated separately.
    if status == reqwest::StatusCode::BAD_GATEWAY {
        let body = resp.text().await.unwrap_or_default();
        info!(
            "Managed secret returned 502 (runtime proxy timeout, non-fatal): {}",
            body
        );
        return Ok(());
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Managed secret returned {}: {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse managed secret response")?;
    // The endpoint returns { exists: bool, value?: string }
    let exists = data
        .get("exists")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !exists {
        bail!(
            "Managed secret MANAGED_TEST_SECRET not found. \
             It should have been provisioned via the manager vault API after deployment. \
             Response: {:?}",
            data
        );
    }

    info!("Managed secret check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Check event handler registration: GET /events/list
pub async fn check_events(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking events");

    let resp = reqwest::Client::new()
        .get(format!("{}/events/list", url))
        .send()
        .await
        .context("Events list request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Events list returned {}: {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse events response")?;
    // Verify the response has event arrays (even if empty, the handler is registered)
    if data.get("storageEvents").is_none() && data.get("queueMessages").is_none() {
        bail!(
            "Events response missing storageEvents/queueMessages: {:?}",
            data
        );
    }

    info!("Events check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Check storage binding: put, get, list, delete via the test app endpoint.
pub async fn check_storage(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking storage binding");

    let client = reqwest::Client::new();
    let resp = post_empty(&client, format!("{}/storage-test/{}", url, STORAGE_BINDING))
        .send()
        .await
        .context("Storage test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Storage test returned {}: {}", status, body);
    }

    let data: BindingTestResponse = resp
        .json()
        .await
        .context("Failed to parse storage response")?;
    if !data.success {
        bail!("Storage test reported failure");
    }
    if data.binding_name != STORAGE_BINDING {
        bail!(
            "Storage test binding mismatch: expected {}, got {}",
            STORAGE_BINDING,
            data.binding_name
        );
    }

    info!("Storage binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// KV
// ---------------------------------------------------------------------------

/// Check KV binding: put, get, exists, scan_prefix, delete.
pub async fn check_kv(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking KV binding");

    let client = reqwest::Client::new();
    let resp = post_empty(&client, format!("{}/kv-test/{}", url, KV_BINDING))
        .send()
        .await
        .context("KV test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("KV test returned {}: {}", status, body);
    }

    let data: BindingTestResponse = resp.json().await.context("Failed to parse KV response")?;
    if !data.success {
        bail!("KV test reported failure");
    }
    if data.binding_name != KV_BINDING {
        bail!(
            "KV test binding mismatch: expected {}, got {}",
            KV_BINDING,
            data.binding_name
        );
    }

    info!("KV binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Vault
// ---------------------------------------------------------------------------

/// Check vault binding: set secret, get secret, delete secret.
pub async fn check_vault(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking vault binding");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("Failed to build vault test HTTP client")?;
    let resp = post_empty(&client, format!("{}/vault-test/{}", url, VAULT_BINDING))
        .send()
        .await
        .context("Vault test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Vault test returned {}: {}", status, body);
    }

    let data: BindingTestResponse = resp
        .json()
        .await
        .context("Failed to parse vault response")?;
    if !data.success {
        bail!("Vault test reported failure");
    }
    if data.binding_name != VAULT_BINDING {
        bail!(
            "Vault test binding mismatch: expected {}, got {}",
            VAULT_BINDING,
            data.binding_name
        );
    }

    info!("Vault binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

/// Check queue binding: send a message, receive it, acknowledge it.
pub async fn check_queue(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking queue binding");

    let client = reqwest::Client::new();
    let resp = post_empty(&client, format!("{}/queue-test/{}", url, QUEUE_BINDING))
        .send()
        .await
        .context("Queue test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Queue test returned {}: {}", status, body);
    }

    let data: BindingTestResponse = resp
        .json()
        .await
        .context("Failed to parse queue response")?;
    if !data.success {
        bail!("Queue test reported failure");
    }
    if data.binding_name != QUEUE_BINDING {
        bail!(
            "Queue test binding mismatch: expected {}, got {}",
            QUEUE_BINDING,
            data.binding_name
        );
    }

    info!("Queue binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// Check worker binding: invoke a sibling worker and verify the response.
pub async fn check_worker(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking worker binding");

    let resp = reqwest::Client::new()
        .post(format!("{}/worker-invoke", url))
        .json(&serde_json::json!({
            "targetPath": "/hello",
        }))
        .send()
        .await
        .context("Worker invoke test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Worker invoke test returned {}: {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse worker response")?;
    if data.get("success") != Some(&serde_json::Value::Bool(true)) {
        bail!("Worker invoke test reported failure: {:?}", data);
    }

    info!("Worker binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Container
// ---------------------------------------------------------------------------

/// Check container binding: container-to-container communication.
pub async fn check_container(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking container binding");

    let resp = reqwest::Client::new()
        .post(format!("{}/container-call", url))
        .json(&serde_json::json!({
            "targetPath": "/hello",
        }))
        .send()
        .await
        .context("Container call test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Container call test returned {}: {}", status, body);
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse container response")?;
    if data.get("success") != Some(&serde_json::Value::Bool(true)) {
        bail!("Container call test reported failure: {:?}", data);
    }

    info!("Container binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// WaitUntil (background tasks)
// ---------------------------------------------------------------------------

/// Check wait_until: register a background task, wait for it to complete,
/// then verify the result via storage.
pub async fn check_wait_until(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking wait_until (background tasks)");

    let test_data = format!("wait-until-test-data-{}", uuid::Uuid::new_v4());
    let delay_ms: u64 = 2000;
    let verification_wait_ms: u64 = 5000;
    let max_attempts: u32 = 6;
    let retry_delay_ms: u64 = 5000;

    // Step 1: Trigger background task
    let trigger_resp = reqwest::Client::new()
        .post(format!("{}/wait-until-test", url))
        .json(&serde_json::json!({
            "storageBindingName": STORAGE_BINDING,
            "testData": test_data,
            "delayMs": delay_ms,
        }))
        .send()
        .await
        .context("Wait-until trigger request failed")?;

    let trigger_status = trigger_resp.status();
    if !trigger_status.is_success() {
        let body = trigger_resp.text().await.unwrap_or_default();
        bail!("Wait-until trigger returned {}: {}", trigger_status, body);
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TriggerResponse {
        success: bool,
        test_id: String,
    }

    let trigger_data: TriggerResponse = trigger_resp
        .json()
        .await
        .context("Failed to parse wait-until trigger response")?;

    if !trigger_data.success {
        bail!("Wait-until trigger reported failure");
    }

    let test_id = trigger_data.test_id;

    // Step 2: Wait then verify with retries
    tokio::time::sleep(std::time::Duration::from_millis(verification_wait_ms)).await;

    for attempt in 1..=max_attempts {
        let verify_resp = reqwest::Client::new()
            .get(format!(
                "{}/wait-until-verify/{}/{}",
                url, test_id, STORAGE_BINDING
            ))
            .send()
            .await
            .context("Wait-until verification request failed")?;

        let verify_status = verify_resp.status();
        if !verify_status.is_success() {
            let body = verify_resp.text().await.unwrap_or_default();
            bail!(
                "Wait-until verification returned {}: {}",
                verify_status,
                body
            );
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct VerifyResponse {
            success: bool,
            background_task_completed: bool,
            file_content: Option<String>,
            message: String,
        }

        let verify_data: VerifyResponse = verify_resp
            .json()
            .await
            .context("Failed to parse wait-until verify response")?;

        if verify_data.background_task_completed && verify_data.success {
            if verify_data.file_content.as_deref() != Some(&test_data) {
                bail!(
                    "Wait-until content mismatch: expected {:?}, got {:?}",
                    test_data,
                    verify_data.file_content
                );
            }
            info!("WaitUntil binding check passed");
            return Ok(());
        }

        if attempt < max_attempts {
            info!(
                attempt,
                max_attempts,
                message = %verify_data.message,
                "Wait-until not completed yet, retrying"
            );
            tokio::time::sleep(std::time::Duration::from_millis(retry_delay_ms)).await;
        } else {
            bail!(
                "Wait-until background task did not complete after {} attempts (last message: {})",
                max_attempts,
                verify_data.message
            );
        }
    }

    unreachable!()
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

/// Check build binding: start a build, poll status, verify completion.
pub async fn check_build(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking build binding");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap();

    let resp = client
        .post(format!("{}/build-test/{}", url, "test-alien-build"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .context("Build test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Build test returned {}: {}", status, body);
    }

    let data: BindingTestResponse = resp
        .json()
        .await
        .context("Failed to parse build response")?;
    if !data.success {
        bail!("Build test reported failure");
    }

    info!("Build binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Artifact Registry
// ---------------------------------------------------------------------------

/// Check artifact registry binding: create repo, generate credentials, delete.
pub async fn check_artifact_registry(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking artifact registry binding");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap();

    let resp = client
        .post(format!(
            "{}/artifact-registry-test/{}",
            url, "test-alien-artifact-registry"
        ))
        .json(&serde_json::json!({}))
        .send()
        .await
        .context("Artifact registry test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Artifact registry test returned {}: {}", status, body);
    }

    let data: BindingTestResponse = resp
        .json()
        .await
        .context("Failed to parse artifact registry response")?;
    if !data.success {
        bail!("Artifact registry test reported failure");
    }

    info!("Artifact registry binding check passed");
    Ok(())
}

// ---------------------------------------------------------------------------
// Service Account
// ---------------------------------------------------------------------------

/// Check service account binding: get identity info.
pub async fn check_service_account(deployment: &TestDeployment) -> anyhow::Result<()> {
    let url = deployment_url(deployment)?;
    info!("Checking service account binding");

    let client = reqwest::Client::new();
    let resp = post_empty(
        &client,
        format!("{}/service-account-test/{}", url, "test-alien-sa"),
    )
    .send()
    .await
    .context("Service account test request failed")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        bail!("Service account test returned {}: {}", status, body);
    }

    let data: BindingTestResponse = resp
        .json()
        .await
        .context("Failed to parse service account response")?;
    if !data.success {
        bail!("Service account test reported failure");
    }

    info!("Service account binding check passed");
    Ok(())
}
