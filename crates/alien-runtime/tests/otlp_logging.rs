#![cfg(test)]
#![cfg(feature = "otlp")]

use alien_runtime::{flush_otlp_logs, init_tracing};
use std::{env, sync::Once, time::Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

static TRACING_INIT: Once = Once::new();

fn load_test_env() {
    // Load .env.test from the workspace root if it exists (local dev).
    // In CI, Axiom credentials come from GitHub secrets passed as env vars.
    let root = workspace_root::get_workspace_root();
    let _ = dotenvy::from_path(root.join(".env.test"));
}

/// Test OTLP logging integration with Axiom.
///
/// Note: This test is configured to run serially via nextest (see .config/nextest.toml)
/// because it modifies global tracing state and environment variables.
#[tokio::test]
async fn test_otlp_logging_to_axiom() {
    load_test_env();

    // Skip this test if OTLP environment is not configured
    let axiom_endpoint = match env::var("AXIOM_OTLP_ENDPOINT") {
        Ok(endpoint) => endpoint,
        Err(_) => {
            println!("Skipping OTLP test: AXIOM_OTLP_ENDPOINT not set");
            return;
        }
    };

    let axiom_token = env::var("AXIOM_TOKEN").expect("AXIOM_TOKEN must be set for OTLP test");
    let axiom_dataset = env::var("AXIOM_DATASET").expect("AXIOM_DATASET must be set for OTLP test");

    println!("🚀 Starting OTLP logging test with Axiom");
    println!("📡 Using Axiom endpoint: {}", axiom_endpoint);
    println!("🗃️ Using Axiom dataset: {}", axiom_dataset);

    // Create unique identifier for this test
    let test_id = Uuid::new_v4().simple();
    let expected_message = format!("OTLP_RUNTIME_TEST_MESSAGE_{}", test_id);

    println!("🆔 Test ID: {}", test_id);
    println!("💬 Expected message: {}", expected_message);

    // Set up OTLP environment variables
    env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", &axiom_endpoint);
    env::set_var(
        "OTEL_EXPORTER_OTLP_HEADERS",
        format!(
            "authorization=Bearer {},x-axiom-dataset={}",
            axiom_token, axiom_dataset
        ),
    );
    env::set_var("OTEL_SERVICE_NAME", "alien-runtime-test");
    env::set_var("OTEL_SERVICE_VERSION", "test-1.0.0");

    // Initialize tracing with OTLP
    TRACING_INIT.call_once(|| {
        init_tracing().expect("Failed to initialize tracing with OTLP");
    });

    println!("🔧 OTLP logging initialized");

    // Record the start time
    let test_start_time = chrono::Utc::now();

    // Generate test log messages
    info!("Test info message: {}", expected_message);
    warn!("Test warn message: {}", expected_message);
    error!("Test error message: {}", expected_message);

    println!("📝 Test log messages generated");

    // Flush OTLP logs to ensure they're sent
    println!("⏳ Flushing OTLP logs...");
    flush_otlp_logs().await.expect("Failed to flush OTLP logs");
    println!("✅ OTLP logs flushed");

    // Wait for logs to be ingested into Axiom
    println!("⏰ Waiting 15 seconds for logs to be ingested into Axiom...");
    tokio::time::sleep(Duration::from_secs(15)).await;

    // Query logs from Axiom using APL
    println!("📖 Querying logs from Axiom...");
    let http_client = reqwest::Client::new();

    // Create APL query to search for our test message within the test timeframe
    let apl_query = format!(
        "['{}'] | where body contains '{}' | limit 100",
        axiom_dataset, expected_message
    );

    // Use a wider time range to account for clock skew and ingestion delays:
    // - Start time: 5 minutes before test started
    // - End time: current time (after waiting for ingestion)
    let start_time = (test_start_time - chrono::Duration::minutes(5)).to_rfc3339();
    let end_time = chrono::Utc::now().to_rfc3339();

    println!("🔍 APL Query: {}", apl_query);
    println!("⏰ Time range: {} to {}", start_time, end_time);

    let query_payload = serde_json::json!({
        "apl": apl_query,
        "startTime": start_time,
        "endTime": end_time
    });

    let response = http_client
        .post("https://api.axiom.co/v1/datasets/_apl?format=tabular")
        .header("Authorization", format!("Bearer {}", axiom_token))
        .header("Content-Type", "application/json")
        .json(&query_payload)
        .send()
        .await
        .expect("Failed to send Axiom query");

    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        panic!("Axiom query failed with status {}: {}", status, error_text);
    }

    let query_result: serde_json::Value = response
        .json()
        .await
        .expect("Failed to parse Axiom response");

    println!("✅ Successfully retrieved query response from Axiom");

    // Verify our test message is in the logs
    println!("🔍 Analyzing Axiom query response...");

    let tables = query_result
        .get("tables")
        .and_then(|t| t.as_array())
        .expect("No tables found in Axiom response");

    if tables.is_empty() {
        panic!("No data tables found in Axiom response");
    }

    let table = &tables[0];
    let columns = table
        .get("columns")
        .and_then(|c| c.as_array())
        .expect("No columns found in Axiom table");

    // Find the message field in the table structure
    let fields = table
        .get("fields")
        .and_then(|f| f.as_array())
        .expect("No fields found in Axiom table");

    let message_field_index = fields.iter().position(|field| {
        field
            .get("name")
            .and_then(|n| n.as_str())
            .map(|name| {
                name.contains("message") || name.contains("@message") || name.contains("body")
            })
            .unwrap_or(false)
    });

    if let Some(msg_index) = message_field_index {
        if let Some(message_column) = columns.get(msg_index).and_then(|c| c.as_array()) {
            let messages: Vec<String> = message_column
                .iter()
                .filter_map(|msg| msg.as_str().map(|s| s.to_string()))
                .collect();

            println!("📊 Retrieved {} log entries", messages.len());

            // Print first few messages for debugging
            println!("📝 First few log entries:");
            for (i, message) in messages.iter().take(5).enumerate() {
                println!("  Entry {}: {}", i, message);
            }

            let found_message = messages.iter().any(|msg| msg.contains(&expected_message));
            println!("🔍 Looking for message containing: '{}'", expected_message);
            println!("🔍 Message found: {}", found_message);

            assert!(
                found_message,
                "Expected to find test message '{}' in Axiom logs, but it was not found. Available messages: {:?}",
                expected_message, messages
            );
            println!("✅ Successfully found test message in Axiom logs");
        } else {
            panic!("Could not extract message column data from Axiom response");
        }
    } else {
        println!("❌ No message field found in Axiom response");
        println!("📋 Available fields: {:?}", fields);
        println!("📋 Full query result: {:?}", query_result);
        panic!("No message field found in Axiom response");
    }

    println!("🎉 OTLP logging test completed successfully!");
}

/// Test OTLP configuration from environment variables.
/// NOTE: This test is named with 'zz' prefix to run AFTER other OTLP tests,
/// because it modifies environment variables which could interfere with
/// tests that initialize tracing.
#[tokio::test]
async fn test_zz_otlp_configuration() {
    use alien_runtime::otlp::OtlpConfig;

    // Clear any existing OTLP environment variables
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    env::remove_var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT");
    env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
    env::remove_var("OTEL_SERVICE_NAME");
    env::remove_var("OTEL_SERVICE_VERSION");

    // Test 1: No configuration
    let config = OtlpConfig::from_env();
    assert!(
        config.is_none(),
        "Should return None when no OTLP endpoint is configured"
    );

    // Test 2: Basic configuration
    env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");

    let config = OtlpConfig::from_env().expect("Should have config");
    assert_eq!(config.endpoint, "http://localhost:4318");
    assert_eq!(config.service_name, "alien-runtime");
    assert!(config.headers.is_empty());

    // Test 3: Full configuration with headers
    env::set_var(
        "OTEL_EXPORTER_OTLP_HEADERS",
        "authorization=Bearer token123,x-custom=value",
    );
    env::set_var("OTEL_SERVICE_NAME", "test-service");
    env::set_var("OTEL_SERVICE_VERSION", "1.2.3");

    let config = OtlpConfig::from_env().expect("Should have config");
    assert_eq!(config.endpoint, "http://localhost:4318");
    assert_eq!(config.service_name, "test-service");
    assert_eq!(config.service_version, "1.2.3");
    assert_eq!(
        config.headers.get("authorization"),
        Some(&"Bearer token123".to_string())
    );
    assert_eq!(config.headers.get("x-custom"), Some(&"value".to_string()));

    // Test 4: Logs-specific endpoint takes precedence
    env::set_var(
        "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
        "http://logs.example.com:4318",
    );

    let config = OtlpConfig::from_env().expect("Should have config");
    assert_eq!(config.endpoint, "http://logs.example.com:4318");

    // Cleanup
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    env::remove_var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT");
    env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
    env::remove_var("OTEL_SERVICE_NAME");
    env::remove_var("OTEL_SERVICE_VERSION");
}

/// Test that alien.deployment_id attribute is included when ALIEN_DEPLOYMENT_ID env var is set.
///
/// Note: This test is configured to run serially via nextest (see .config/nextest.toml)
/// because it modifies global tracing state and environment variables.
#[tokio::test]
async fn test_alien_deployment_id_otlp_integration() {
    load_test_env();

    // Skip this test if OTLP environment is not configured
    let axiom_endpoint = match env::var("AXIOM_OTLP_ENDPOINT") {
        Ok(endpoint) => endpoint,
        Err(_) => {
            println!("Skipping ALIEN_DEPLOYMENT_ID OTLP test: AXIOM_OTLP_ENDPOINT not set");
            return;
        }
    };

    let axiom_token = env::var("AXIOM_TOKEN").expect("AXIOM_TOKEN must be set for OTLP test");
    let axiom_dataset = env::var("AXIOM_DATASET").expect("AXIOM_DATASET must be set for OTLP test");

    println!("🚀 Starting ALIEN_DEPLOYMENT_ID OTLP logging test");

    // Create unique identifier for this test
    let test_id = Uuid::new_v4().simple();
    let deployment_id = format!("test-deployment-{}", test_id);
    let expected_message = format!("DEPLOYMENT_ID_TEST_MESSAGE_{}", test_id);

    println!("🆔 Test Deployment ID: {}", deployment_id);
    println!("💬 Expected message: {}", expected_message);

    // Set up OTLP environment variables with deployment ID
    env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", &axiom_endpoint);
    env::set_var(
        "OTEL_EXPORTER_OTLP_HEADERS",
        format!(
            "authorization=Bearer {},x-axiom-dataset={}",
            axiom_token, axiom_dataset
        ),
    );
    env::set_var("OTEL_SERVICE_NAME", "alien-runtime-test-deployment");
    env::set_var("ALIEN_DEPLOYMENT_ID", &deployment_id);

    // Initialize tracing with OTLP (uses call_once to ensure only first test initializes)
    TRACING_INIT.call_once(|| {
        init_tracing().expect("Failed to initialize tracing with OTLP");
    });

    // Log a test message that should include the deployment_id attribute
    info!(
        test_id = %test_id,
        deployment_id = %deployment_id,
        message = %expected_message,
        "Testing alien.deployment_id resource attribute"
    );

    println!("✅ Test message logged with deployment ID");

    // Flush logs to ensure they are sent
    let flush_result = flush_otlp_logs().await;
    assert!(flush_result.is_ok(), "OTLP logs should flush successfully");

    println!("✅ OTLP logs flushed successfully");

    // Cleanup
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
    env::remove_var("OTEL_SERVICE_NAME");
    env::remove_var("ALIEN_DEPLOYMENT_ID");

    println!("🧹 Environment cleaned up");
    println!(
        "📝 NOTE: Check your Axiom dataset for logs with resource.alien.deployment_id = '{}'",
        deployment_id
    );
}

/// Test that OTLP logging returns None when not configured, and flush is safe to call.
/// NOTE: This test intentionally does NOT call init_tracing() to avoid polluting
/// the global subscriber state, which would prevent other tests from using OTLP.
#[tokio::test]
async fn test_zz_otlp_not_configured() {
    use alien_runtime::otlp::OtlpConfig;

    // Clear OTLP environment variables
    env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    env::remove_var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT");

    // Verify OTLP config returns None when not configured
    let config = OtlpConfig::from_env();
    assert!(
        config.is_none(),
        "OtlpConfig should be None when OTLP endpoint is not set"
    );

    // Test that flush works even without OTLP configured (should be a no-op)
    let flush_result = flush_otlp_logs().await;
    assert!(
        flush_result.is_ok(),
        "OTLP flush should succeed even when not configured"
    );
}
