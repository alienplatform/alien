#![cfg(all(test, feature = "gcp"))]
use alien_client_core::{ErrorData, Result};
use alien_gcp_clients::longrunning::Operation;
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use alien_gcp_clients::service_usage::{
    CheckIfServiceHasUsage, Service, ServiceUsageApi, ServiceUsageClient, State,
};

use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};

// fast to enable/disable, control plane only, no cost
const TEST_SERVICE_NAME: &str = "accessapproval.googleapis.com";

struct ServiceUsageTestContext {
    client: ServiceUsageClient,
    project_id: String,
    enabled_services: Mutex<HashSet<String>>,
}

impl AsyncTestContext for ServiceUsageTestContext {
    async fn setup() -> ServiceUsageTestContext {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .unwrap_or_else(|_| panic!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set"));

        // Parse project_id from service account
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: "us-central1".to_string(), // Not used for Service Usage but required
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
            project_number: None,
        };

        let client = ServiceUsageClient::new(Client::new(), config);

        ServiceUsageTestContext {
            client,
            project_id,
            enabled_services: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Service Usage test cleanup...");

        let services_to_disable = {
            let services = self.enabled_services.lock().unwrap();
            services.clone()
        };

        for service_name in services_to_disable {
            self.cleanup_service(&service_name).await;
        }

        info!("✅ Service Usage test cleanup completed");
    }
}

impl ServiceUsageTestContext {
    fn track_enabled_service(&self, service_name: &str) {
        let mut services = self.enabled_services.lock().unwrap();
        services.insert(service_name.to_string());
        info!("📝 Tracking service for cleanup: {}", service_name);
    }

    fn untrack_service(&self, service_name: &str) {
        let mut services = self.enabled_services.lock().unwrap();
        services.remove(service_name);
        info!(
            "✅ Service {} successfully cleaned up and untracked",
            service_name
        );
    }

    async fn cleanup_service(&self, service_name: &str) {
        info!("🧹 Disabling service: {}", service_name);

        match self
            .client
            .disable_service(
                service_name.to_string(),
                Some(true),
                Some(CheckIfServiceHasUsage::Skip),
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Service {} disable operation initiated successfully",
                    service_name
                );
            }
            Err(infra_err) => match &infra_err.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!(
                        "🔍 Service {} was already disabled or doesn't exist",
                        service_name
                    );
                }
                _ => {
                    warn!(
                        "Failed to disable service {} during cleanup: {:?}",
                        service_name, infra_err
                    );
                }
            },
        }
    }

    async fn ensure_service_disabled(&self, service_name: &str) -> Result<()> {
        // Check current state and disable if enabled
        match self.client.get_service(service_name.to_string()).await {
            Ok(service) => {
                if service.state == Some(State::Enabled) {
                    info!("Service {} is enabled, disabling it first", service_name);
                    self.client
                        .disable_service(
                            service_name.to_string(),
                            Some(true),
                            Some(CheckIfServiceHasUsage::Skip),
                        )
                        .await?;
                    self.wait_for_service_state(service_name, State::Disabled, 120)
                        .await?;
                }
            }
            Err(_) => {
                // Service might not exist or be accessible, that's fine
            }
        }
        Ok(())
    }

    async fn wait_for_service_state(
        &self,
        service_name: &str,
        expected_state: State,
        timeout_seconds: u64,
    ) -> Result<()> {
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(alien_error::AlienError::new(ErrorData::Timeout {
                    message: format!(
                        "Timeout waiting for service {} to reach state {:?}",
                        service_name, expected_state
                    ),
                }));
            }

            match self.client.get_service(service_name.to_string()).await {
                Ok(service) => {
                    if service.state == Some(expected_state.clone()) {
                        info!(
                            "✅ Service {} reached expected state: {:?}",
                            service_name, expected_state
                        );
                        return Ok(());
                    }
                    info!(
                        "⏳ Service {} current state: {:?}, waiting for: {:?}",
                        service_name, service.state, expected_state
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    warn!("Error checking service state: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    fn create_invalid_client(&self) -> ServiceUsageClient {
        let invalid_config = GcpClientConfig {
                project_id: "fake-project-12345".to_string(),
                region: "us-central1".to_string(),
                credentials: GcpCredentials::ServiceAccountKey {
                    json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
                },
                service_overrides: None,
            project_number: None,
            };
        ServiceUsageClient::new(Client::new(), invalid_config)
    }

    async fn wait_for_operation(
        &self,
        operation_name: &str,
        timeout_seconds: u64,
    ) -> Result<Operation> {
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(alien_error::AlienError::new(ErrorData::Timeout {
                    message: format!(
                        "Timeout waiting for operation {} to complete",
                        operation_name
                    ),
                }));
            }

            match self.client.get_operation(operation_name.to_string()).await {
                Ok(operation) => {
                    if operation.done == Some(true) {
                        info!("✅ Operation {} completed!", operation_name);
                        return Ok(operation);
                    }
                    info!("⏳ Operation {} still running, waiting...", operation_name);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    // Check if this is a non-retryable error that indicates the operation is invalid
                    match &e.error {
                        Some(ErrorData::RemoteResourceNotFound { .. }) => {
                            // Operation was not found - it might have completed and been cleaned up
                            warn!(
                                "Operation {} not found, assuming it completed successfully",
                                operation_name
                            );
                            // Return a synthetic completed operation
                            return Ok(Operation {
                                name: Some(operation_name.to_string()),
                                done: Some(true),
                                metadata: None,
                                result: None,
                            });
                        }
                        Some(ErrorData::InvalidInput { message, .. }) => {
                            // Check for specific GCP error messages that indicate invalid operation
                            if message.contains("Invalid operation id")
                                || message.contains("Cannot parse")
                            {
                                warn!(
                                    "Operation {} has invalid ID, assuming it completed: {}",
                                    operation_name, message
                                );
                                // Return a synthetic completed operation
                                return Ok(Operation {
                                    name: Some(operation_name.to_string()),
                                    done: Some(true),
                                    metadata: None,
                                    result: None,
                                });
                            }
                        }
                        _ => {}
                    }

                    warn!("Error checking operation status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

#[test_context(ServiceUsageTestContext)]
#[tokio::test]
async fn test_get_service(ctx: &mut ServiceUsageTestContext) {
    println!("Testing get_service for: {}", TEST_SERVICE_NAME);

    let service = ctx
        .client
        .get_service(TEST_SERVICE_NAME.to_string())
        .await
        .expect("Failed to get service");

    assert!(service.name.is_some(), "Service should have a name");
    assert!(service.config.is_some(), "Service should have config");
    assert!(service.state.is_some(), "Service should have state");

    let service_name = service.name.as_ref().unwrap();
    assert!(
        service_name.contains(TEST_SERVICE_NAME),
        "Service name should contain the requested service"
    );
    // Note: GCP service names may not always contain the project ID in the name field
    // The project association is handled via the API endpoint used

    println!("✅ Successfully retrieved service: {}", service_name);
    println!("   Project ID: {}", ctx.project_id);
    println!("   State: {:?}", service.state);
    println!(
        "   Config title: {:?}",
        service.config.as_ref().and_then(|c| c.title.as_ref())
    );
}

#[test_context(ServiceUsageTestContext)]
#[tokio::test]
async fn test_enable_disable_service_cycle(ctx: &mut ServiceUsageTestContext) {
    println!(
        "🔄 Testing complete enable/disable cycle for: {}",
        TEST_SERVICE_NAME
    );

    // Ensure service starts in disabled state
    ctx.ensure_service_disabled(TEST_SERVICE_NAME)
        .await
        .expect("Failed to ensure service is disabled initially");

    // 1. Enable the service
    println!("📦 Enabling service...");
    let enable_operation = ctx
        .client
        .enable_service(TEST_SERVICE_NAME.to_string())
        .await
        .expect("Failed to enable service");

    assert!(
        enable_operation.name.is_some(),
        "Enable operation should have a name"
    );
    println!(
        "✅ Service enable operation initiated: {}",
        enable_operation.name.as_ref().unwrap()
    );

    // Track for cleanup
    ctx.track_enabled_service(TEST_SERVICE_NAME);

    // Wait for operation to complete
    let _completed_operation = ctx
        .wait_for_operation(enable_operation.name.as_ref().unwrap(), 120)
        .await
        .expect("Enable operation failed to complete");

    println!("✅ Enable operation completed successfully");

    // Verify service is enabled
    ctx.wait_for_service_state(TEST_SERVICE_NAME, State::Enabled, 60)
        .await
        .expect("Service failed to reach enabled state");

    // 2. Get the enabled service
    let enabled_service = ctx
        .client
        .get_service(TEST_SERVICE_NAME.to_string())
        .await
        .expect("Failed to get enabled service");

    assert_eq!(
        enabled_service.state,
        Some(State::Enabled),
        "Service should be enabled"
    );
    println!("✅ Verified service is enabled");

    // 3. Disable the service
    println!("🛑 Disabling service...");
    let disable_operation = ctx
        .client
        .disable_service(
            TEST_SERVICE_NAME.to_string(),
            Some(true),                         // disable dependent services
            Some(CheckIfServiceHasUsage::Skip), // skip usage check for faster test
        )
        .await
        .expect("Failed to disable service");

    assert!(
        disable_operation.name.is_some(),
        "Disable operation should have a name"
    );
    println!(
        "✅ Service disable operation initiated: {}",
        disable_operation.name.as_ref().unwrap()
    );

    // Wait for disable operation to complete
    ctx.wait_for_operation(disable_operation.name.as_ref().unwrap(), 120)
        .await
        .expect("Disable operation failed to complete");

    // Verify service is disabled
    ctx.wait_for_service_state(TEST_SERVICE_NAME, State::Disabled, 60)
        .await
        .expect("Service failed to reach disabled state");

    // Untrack since we've successfully disabled it
    ctx.untrack_service(TEST_SERVICE_NAME);

    println!("🎉 Complete enable/disable cycle completed successfully!");
}

#[test_context(ServiceUsageTestContext)]
#[tokio::test]
async fn test_error_translation_service_not_found(ctx: &mut ServiceUsageTestContext) {
    let non_existent_service = "non-existent-service.googleapis.com";

    let result = ctx
        .client
        .get_service(non_existent_service.to_string())
        .await;
    assert!(result.is_err(), "Expected error for non-existent service");

    match &result.unwrap_err().error {
            Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) => {
                assert!(resource_name.contains(non_existent_service), "Resource name should contain the service name");
                println!("✅ Correctly mapped 404 to RemoteResourceNotFound error");
            }
            Some(ErrorData::RemoteAccessDenied { resource_name, .. }) => {
                // GCP returns 403 "Not found or permission denied" for non-existent services
                assert!(resource_name.contains(non_existent_service), "Resource name should contain the service name");
                println!("✅ Correctly mapped 403 'Not found or permission denied' to RemoteAccessDenied error");
            }
            Some(ErrorData::InvalidInput { message, .. }) => {
                // Some services may return different error formats
                if message.contains("404") || message.contains("not found") || message.contains("403") {
                    println!("✅ Correctly mapped service not found to Generic error");
                } else {
                    panic!("Unexpected Generic error: {}", message);
                }
            }
            other => panic!("Expected RemoteResourceNotFound, RemoteAccessDenied, or specific Generic error, got: {:?}", other),
        }
}

#[test_context(ServiceUsageTestContext)]
#[tokio::test]
async fn test_get_non_existent_operation(ctx: &mut ServiceUsageTestContext) {
    let non_existent_operation = "operations/non-existent-operation-12345";

    let result = ctx
        .client
        .get_operation(non_existent_operation.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error when getting non-existent operation"
    );

    match &result.unwrap_err().error {
        Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) => {
                println!("✅ Correctly mapped 404 to RemoteResourceNotFound for operation: {}", resource_name);
            }
            Some(ErrorData::InvalidInput { message, .. }) => {
                if message.contains("404") || message.contains("not found") || message.contains("Cannot parse") || message.contains("does not match expected pattern") {
                    println!("✅ Correctly returned Generic error for malformed/non-existent operation: {}", message);
                } else {
                    panic!("Unexpected Generic error for operation: {}", message);
                }
            }
            other => panic!("Expected RemoteResourceNotFound or Generic error for non-existent operation, got: {:?}", other),
        }
}
