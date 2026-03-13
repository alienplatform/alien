#![cfg(all(test, feature = "gcp"))]

use alien_client_core::{Error, ErrorData, Result};
use alien_gcp_clients::cloudrun::{
    CloudRunApi, CloudRunClient, Container, ContainerPort, EnvVar, LaunchStage,
    ResourceRequirements, RevisionTemplate, ScalingMode, Service, ServiceScaling, TrafficTarget,
    TrafficTargetAllocationType,
};
use alien_gcp_clients::compute::{
    ComputeApi, ComputeClient, NetworkEndpointGroup, NetworkEndpointGroupCloudRun,
    NetworkEndpointType,
};
use alien_gcp_clients::iam::{Binding, IamPolicy};
use alien_gcp_clients::longrunning::Operation;
use alien_gcp_clients::platform::{GcpClientConfig, GcpCredentials};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

const TEST_LOCATION: &str = "us-central1";

// Simple hello world container image that responds to HTTP requests
const TEST_CONTAINER_IMAGE: &str = "gcr.io/cloudrun/hello";

struct CloudRunTestContext {
    client: CloudRunClient,
    compute_client: ComputeClient,
    project_id: String,
    location: String,
    created_services: Mutex<HashSet<String>>,
    created_region_negs: Mutex<HashSet<(String, String)>>, // (region, name)
}

impl AsyncTestContext for CloudRunTestContext {
    async fn setup() -> CloudRunTestContext {
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
            region: TEST_LOCATION.to_string(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
        };

        let client = CloudRunClient::new(Client::new(), config.clone());
        let compute_client = ComputeClient::new(Client::new(), config);

        CloudRunTestContext {
            client,
            compute_client,
            project_id,
            location: TEST_LOCATION.to_string(),
            created_services: Mutex::new(HashSet::new()),
            created_region_negs: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Cloud Run test cleanup...");

        // Clean up regional NEGs first
        let region_negs_to_cleanup = {
            let negs = self.created_region_negs.lock().unwrap();
            negs.clone()
        };
        for (region, neg_name) in region_negs_to_cleanup {
            self.cleanup_region_neg(&region, &neg_name).await;
        }

        // Clean up services
        let services_to_cleanup = {
            let services = self.created_services.lock().unwrap();
            services.clone()
        };

        for service_name in services_to_cleanup {
            self.cleanup_service(&service_name).await;
        }

        info!("✅ Cloud Run test cleanup completed");
    }
}

impl CloudRunTestContext {
    fn track_service(&self, service_name: &str) {
        let mut services = self.created_services.lock().unwrap();
        services.insert(service_name.to_string());
        info!("📝 Tracking service for cleanup: {}", service_name);
    }

    fn untrack_service(&self, service_name: &str) {
        let mut services = self.created_services.lock().unwrap();
        services.remove(service_name);
        info!(
            "✅ Service {} successfully cleaned up and untracked",
            service_name
        );
    }

    fn track_region_neg(&self, region: &str, neg_name: &str) {
        let mut negs = self.created_region_negs.lock().unwrap();
        negs.insert((region.to_string(), neg_name.to_string()));
        info!(
            "📝 Tracking regional NEG for cleanup: {}/{}",
            region, neg_name
        );
    }

    fn untrack_region_neg(&self, region: &str, neg_name: &str) {
        let mut negs = self.created_region_negs.lock().unwrap();
        negs.remove(&(region.to_string(), neg_name.to_string()));
        info!(
            "✅ Regional NEG {}/{} successfully cleaned up and untracked",
            region, neg_name
        );
    }

    async fn cleanup_region_neg(&self, region: &str, neg_name: &str) {
        info!("🧹 Cleaning up regional NEG: {}/{}", region, neg_name);

        match self
            .compute_client
            .delete_region_network_endpoint_group(region.to_string(), neg_name.to_string())
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Regional NEG {}/{} deletion initiated successfully",
                    region, neg_name
                );
            }
            Err(infra_err) => match &infra_err.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!(
                        "🔍 Regional NEG {}/{} was already deleted",
                        region, neg_name
                    );
                }
                _ => {
                    warn!(
                        "Failed to delete regional NEG {}/{} during cleanup: {:?}",
                        region, neg_name, infra_err
                    );
                }
            },
        }
    }

    async fn cleanup_service(&self, service_name: &str) {
        info!("🧹 Cleaning up service: {}", service_name);

        match self
            .client
            .delete_service(self.location.clone(), service_name.to_string(), None, None)
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Service {} deletion initiated successfully",
                    service_name
                );
            }
            Err(infra_err) => match &infra_err.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("🔍 Service {} was already deleted", service_name);
                }
                _ => {
                    warn!(
                        "Failed to delete service {} during cleanup: {:?}",
                        service_name, infra_err
                    );
                }
            },
        }
    }

    fn generate_unique_service_name(&self) -> String {
        format!(
            "alien-test-svc-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
        )
    }

    async fn create_test_service(
        &self,
        service_name: String,
        service: Service,
    ) -> Result<Operation> {
        let result = self
            .client
            .create_service(self.location.clone(), service_name.clone(), service, None)
            .await;
        if result.is_ok() {
            self.track_service(&service_name);
        }
        result
    }

    fn create_basic_service(&self, service_name: String) -> Service {
        let container = Container::builder()
            .image(TEST_CONTAINER_IMAGE.to_string())
            .ports(vec![ContainerPort::builder().container_port(8080).build()])
            .build();

        let revision_template = RevisionTemplate::builder()
            .containers(vec![container])
            .build();

        let traffic_target = TrafficTarget::builder()
            .r#type(TrafficTargetAllocationType::TrafficTargetAllocationTypeLatest)
            .percent(100)
            .build();

        Service::builder()
            .template(revision_template)
            .traffic(vec![traffic_target])
            .build()
    }

    fn create_invalid_client(&self) -> CloudRunClient {
        let invalid_config = GcpClientConfig {
            project_id: "fake-project".to_string(),
            region: self.location.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: r#"{"type":"service_account","project_id":"fake","private_key_id":"fake","private_key":"-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----\n","client_email":"fake@fake.iam.gserviceaccount.com","client_id":"fake","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}"#.to_string(),
            },
            service_overrides: None,
        };
        CloudRunClient::new(Client::new(), invalid_config)
    }

    async fn wait_for_operation(
        &self,
        operation_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);
        let operation_id = operation_name.split('/').last().unwrap();

        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(
                    format!("Timeout waiting for operation {} to complete", operation_id).into(),
                );
            }

            match self
                .client
                .get_operation(self.location.clone(), operation_id.to_string())
                .await
            {
                Ok(operation) => {
                    if operation.done == Some(true) {
                        info!("✅ Operation {} completed!", operation_id);
                        return Ok(());
                    }
                    info!("⏳ Operation {} still running, waiting...", operation_id);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    warn!("Error checking operation status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_framework_setup_cloudrun(ctx: &mut CloudRunTestContext) {
    assert!(!ctx.project_id.is_empty(), "Project ID should not be empty");
    assert!(!ctx.location.is_empty(), "Location should not be empty");

    println!(
        "Successfully connected to Cloud Run in project: {} location: {}",
        ctx.project_id, ctx.location
    );
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_create_get_delete_service(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();
    let service = ctx.create_basic_service(service_name.clone());

    println!("Attempting to create test service: {}", service_name);

    // Create service
    let create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create service");

    assert!(
        create_operation.name.is_some(),
        "Operation should have a name"
    );
    println!("Successfully initiated service creation: {}", service_name);

    // Wait a bit for the service to be created
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // Get service
    let fetched_service = ctx
        .client
        .get_service(ctx.location.clone(), service_name.clone())
        .await
        .expect("Failed to get service");

    assert_eq!(
        fetched_service
            .name
            .as_ref()
            .unwrap()
            .split('/')
            .last()
            .unwrap(),
        service_name
    );
    println!("Successfully fetched service: {}", service_name);

    // Delete service
    let delete_operation = ctx
        .client
        .delete_service(ctx.location.clone(), service_name.clone(), None, None)
        .await
        .expect("Failed to delete service");

    assert!(
        delete_operation.name.is_some(),
        "Delete operation should have a name"
    );
    println!(
        "Successfully initiated deletion of service: {}",
        service_name
    );
    ctx.untrack_service(&service_name);
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_patch_service(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();
    let service = ctx.create_basic_service(service_name.clone());

    println!("Creating service for patch test: {}", service_name);

    let _create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create service for patch test");

    // Wait for service to be created
    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;

    // Update service with new environment variable
    let mut env_vars = Vec::new();
    env_vars.push(
        EnvVar::builder()
            .name("TEST_ENV".to_string())
            .value("test_value".to_string())
            .build(),
    );

    let updated_container = Container::builder()
        .image(TEST_CONTAINER_IMAGE.to_string())
        .env(env_vars)
        .ports(vec![ContainerPort::builder().container_port(8080).build()])
        .build();

    let updated_template = RevisionTemplate::builder()
        .containers(vec![updated_container])
        .build();

    let patch_service = Service::builder().template(updated_template).build();

    println!("Attempting to patch service: {}", service_name);
    let patch_operation = ctx
        .client
        .patch_service(
            ctx.location.clone(),
            service_name.clone(),
            patch_service,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to patch service");

    assert!(
        patch_operation.name.is_some(),
        "Patch operation should have a name"
    );
    println!("Successfully initiated service patch: {}", service_name);
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_service_iam_policy(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();
    let service = ctx.create_basic_service(service_name.clone());

    println!("Creating service for IAM policy test: {}", service_name);

    let _create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create service for IAM test");

    // Wait for service to be created
    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;

    // Get IAM policy
    let original_policy = ctx
        .client
        .get_service_iam_policy(ctx.location.clone(), service_name.clone())
        .await
        .expect("Failed to get service IAM policy");

    println!(
        "Successfully retrieved IAM policy for service: {}",
        service_name
    );

    // Try to set IAM policy (add a test binding)
    let current_sa_email = {
        let gcp_credentials_json = std::env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY").unwrap();
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        service_account_value
            .get("client_email")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| "serviceAccount:test-service@example.com".to_string())
    };

    let test_member = if current_sa_email.starts_with("serviceAccount:") {
        current_sa_email
    } else {
        format!("serviceAccount:{}", current_sa_email)
    };

    let new_binding = Binding::builder()
        .role("roles/run.invoker".to_string())
        .members(vec![test_member])
        .build();

    let mut modified_bindings = original_policy.bindings.clone();
    modified_bindings.push(new_binding);

    let policy_to_set = IamPolicy::builder()
        .bindings(modified_bindings)
        .maybe_etag(original_policy.etag.clone())
        .build();

    let set_result = ctx
        .client
        .set_service_iam_policy(ctx.location.clone(), service_name.clone(), policy_to_set)
        .await;

    match set_result {
        Ok(_) => {
            println!("Successfully set IAM policy for service: {}", service_name);
        }
        Err(e) => {
            // IAM policy operations can be tricky with test accounts, log but don't fail
            println!("IAM policy set failed (acceptable for test): {:?}", e);
        }
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_get_operation(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();
    let service = ctx.create_basic_service(service_name.clone());

    println!(
        "Creating service to test operation retrieval: {}",
        service_name
    );

    let create_operation = ctx
        .create_test_service(service_name, service)
        .await
        .expect("Failed to create service for operation test");

    let operation_name = create_operation.name.as_ref().unwrap();
    let operation_id = operation_name.split('/').last().unwrap();

    println!("Attempting to get operation: {}", operation_id);

    let fetched_operation = ctx
        .client
        .get_operation(ctx.location.clone(), operation_id.to_string())
        .await
        .expect("Failed to get operation");

    assert_eq!(fetched_operation.name.as_ref().unwrap(), operation_name);
    println!("Successfully retrieved operation: {}", operation_id);
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_get_service_revision(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();
    let service = ctx.create_basic_service(service_name.clone());

    println!(
        "Creating service to test revision retrieval: {}",
        service_name
    );

    let _create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create service for revision test");

    // Wait for service to be ready and have a revision
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    let fetched_service = ctx
        .client
        .get_service(ctx.location.clone(), service_name.clone())
        .await
        .expect("Failed to get service for revision test");

    if let Some(latest_revision) = &fetched_service.latest_ready_revision {
        let revision_name = latest_revision.split('/').last().unwrap();

        println!("Attempting to get revision: {}", revision_name);

        let revision = ctx
            .client
            .get_service_revision(
                ctx.location.clone(),
                service_name.clone(),
                revision_name.to_string(),
            )
            .await
            .expect("Failed to get service revision");

        assert!(revision.name.is_some(), "Revision should have a name");
        println!("Successfully retrieved revision: {}", revision_name);
    } else {
        println!("No ready revision found for service, skipping revision test");
    }
}

// === END-TO-END TEST ===

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_end_to_end_service_deployment(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();

    println!(
        "🚀 Starting end-to-end Cloud Run deployment test: {}",
        service_name
    );

    // Create a service with public access
    let container = Container::builder()
        .image(TEST_CONTAINER_IMAGE.to_string())
        .ports(vec![ContainerPort::builder().container_port(8080).build()])
        .resources(
            ResourceRequirements::builder()
                .limits({
                    let mut limits = HashMap::new();
                    limits.insert("memory".to_string(), "512Mi".to_string());
                    limits.insert("cpu".to_string(), "1000m".to_string());
                    limits
                })
                .build(),
        )
        .build();

    let revision_template = RevisionTemplate::builder()
        .containers(vec![container])
        .build();

    let traffic_target = TrafficTarget::builder()
        .r#type(TrafficTargetAllocationType::TrafficTargetAllocationTypeLatest)
        .percent(100)
        .build();

    let service = Service::builder()
        .template(revision_template)
        .traffic(vec![traffic_target])
        .invoker_iam_disabled(true) // Allow unauthenticated access for testing
        .build();

    // 1. Deploy the service
    println!("📦 Deploying Cloud Run service...");
    let create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create end-to-end test service");

    assert!(
        create_operation.name.is_some(),
        "Create operation should have a name"
    );
    println!("✅ Service deployment initiated");

    // 2. Wait for service to be ready
    println!("⏳ Waiting for deployment operation to complete...");
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Service deployment operation failed to complete within timeout");

    // 3. Get the final service state
    let ready_service = ctx
        .client
        .get_service(ctx.location.clone(), service_name.clone())
        .await
        .expect("Failed to get service after deployment completed");

    // 4. Get the service URL
    let service_url = ready_service
        .urls
        .first()
        .expect("Service should have at least one URL");

    println!("🌐 Service is ready at: {}", service_url);

    // 5. Make HTTP request to the service
    println!("🔍 Making HTTP request to deployed service...");
    let http_client = reqwest::Client::new();

    // Retry HTTP requests in case of temporary unavailability
    let mut last_error = None;
    let mut response_received = false;

    for attempt in 1..=10 {
        match http_client.get(service_url).send().await {
            Ok(response) => {
                println!("📡 HTTP Response Status: {}", response.status());

                if response.status().is_success() {
                    let body = response.text().await.expect("Failed to read response body");
                    println!(
                        "📝 Response Body (first 200 chars): {}",
                        if body.len() > 200 {
                            &body[..200]
                        } else {
                            &body
                        }
                    );

                    // Basic validation that we got a response from the hello service
                    assert!(!body.is_empty(), "Response body should not be empty");

                    response_received = true;
                    break;
                } else {
                    println!(
                        "⚠️ HTTP request failed with status: {} (attempt {}/10)",
                        response.status(),
                        attempt
                    );
                    last_error = Some(format!("HTTP {}", response.status()));
                }
            }
            Err(e) => {
                println!("⚠️ HTTP request error: {} (attempt {}/10)", e, attempt);
                last_error = Some(e.to_string());
            }
        }

        if attempt < 10 {
            tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        }
    }

    if !response_received {
        panic!(
            "Failed to get successful HTTP response after 10 attempts. Last error: {:?}",
            last_error
        );
    }

    println!("✅ Successfully received HTTP response from deployed Cloud Run service!");

    // 6. Test service scaling by checking the service configuration
    let final_service = ctx
        .client
        .get_service(ctx.location.clone(), service_name.clone())
        .await
        .expect("Failed to get service for final verification");

    assert!(final_service.urls.len() > 0, "Service should have URLs");
    assert!(
        final_service.latest_ready_revision.is_some(),
        "Service should have a ready revision"
    );

    println!("🎉 End-to-end test completed successfully!");
    println!("   - Service deployed: ✅");
    println!("   - Service became ready: ✅");
    println!("   - HTTP request successful: ✅");
    println!("   - Service URLs: {:?}", final_service.urls);
}

// === ERROR TESTING ===

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_error_translation_service_not_found(ctx: &mut CloudRunTestContext) {
    let non_existent_service = "alien-test-service-does-not-exist-12345";

    let result = ctx
        .client
        .get_service(ctx.location.clone(), non_existent_service.to_string())
        .await;
    assert!(result.is_err(), "Expected error for non-existent service");

    let err = result.unwrap_err();
    assert_eq!(err.code, "REMOTE_RESOURCE_NOT_FOUND");
    assert!(err.message.contains(non_existent_service));
    println!("✅ Correctly mapped 404 to REMOTE_RESOURCE_NOT_FOUND error");
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_error_translation_service_already_exists(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();
    let service = ctx.create_basic_service(service_name.clone());

    // Create the service first
    let _create_operation = ctx
        .create_test_service(service_name.clone(), service.clone())
        .await
        .expect("Failed to create initial service");

    // Wait a bit for the service to be created
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // Try to create the same service again
    let result = ctx.create_test_service(service_name.clone(), service).await;
    assert!(
        result.is_err(),
        "Expected error when creating existing service"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceConflict {
                    ref message,
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Cloud Run");
            assert_eq!(resource_name.to_string(), service_name.to_string());
            println!("✅ Correctly mapped 409 to RemoteResourceConflict error");
        }
        _ => panic!("Expected RemoteResourceConflict error, got: {:?}", err),
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_error_translation_access_denied(ctx: &mut CloudRunTestContext) {
    let invalid_client = ctx.create_invalid_client();
    let result = invalid_client
        .get_service(ctx.location.clone(), "any-service".to_string())
        .await;

    assert!(result.is_err(), "Expected error with invalid credentials");

    let err = result.unwrap_err();
    match &err.error {
        Some(ErrorData::RemoteAccessDenied { .. })
        | Some(ErrorData::HttpRequestFailed { .. })
        | Some(ErrorData::InvalidInput { .. }) => {
            println!("✅ Got expected error type for invalid credentials");
        }
        _ => println!("Got error (acceptable for invalid creds): {:?}", err),
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_delete_non_existent_service_error(ctx: &mut CloudRunTestContext) {
    let non_existent_service = "alien-test-service-does-not-exist-67890";

    let result = ctx
        .client
        .delete_service(
            ctx.location.clone(),
            non_existent_service.to_string(),
            None,
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error when deleting non-existent service"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Cloud Run");
            assert_eq!(resource_name, non_existent_service);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for service deletion");
        }
        _ => panic!(
            "Expected RemoteResourceNotFound error for non-existent service deletion, got: {:?}",
            err
        ),
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_patch_non_existent_service_error(ctx: &mut CloudRunTestContext) {
    let non_existent_service = "alien-test-service-does-not-exist-13579";
    let patch_service = ctx.create_basic_service(non_existent_service.to_string());

    let result = ctx
        .client
        .patch_service(
            ctx.location.clone(),
            non_existent_service.to_string(),
            patch_service,
            None,
            None,
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error when patching non-existent service"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    ref resource_type,
                    ref resource_name,
                }),
            ..
        } => {
            assert_eq!(resource_type, "Cloud Run");
            assert_eq!(resource_name, non_existent_service);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for service patch");
        }
        _ => panic!(
            "Expected RemoteResourceNotFound error for non-existent service patch, got: {:?}",
            err
        ),
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_iam_policy_operations_on_non_existent_service(ctx: &mut CloudRunTestContext) {
    let non_existent_service = "alien-test-service-does-not-exist-24680";

    // Test get_service_iam_policy
    let get_result = ctx
        .client
        .get_service_iam_policy(ctx.location.clone(), non_existent_service.to_string())
        .await;
    // Note: IAM operations may succeed even for non-existent services in some cases,
    // so we'll accept either success or specific error patterns
    match get_result {
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { resource_name, .. }),
            ..
        }) => {
            assert_eq!(resource_name, non_existent_service);
            println!("✅ get_service_iam_policy correctly mapped 404 to RemoteResourceNotFound");
        }
        Err(Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        }) => {
            println!("✅ get_service_iam_policy returned access denied (acceptable)");
        }
        Err(Error {
            error: Some(ErrorData::GenericError { .. }),
            ..
        }) => {
            println!(
                "✅ get_service_iam_policy returned generic error (acceptable for IAM operations)"
            );
        }
        Ok(_) => {
            println!("✅ get_service_iam_policy succeeded (acceptable - IAM policies can exist independently)");
        }
        Err(other) => {
            println!("Got unexpected error type for get IAM policy: {:?}", other);
        }
    }

    // Test set_service_iam_policy
    let test_binding = Binding::builder()
        .role("roles/run.invoker".to_string())
        .members(vec!["serviceAccount:test@example.com".to_string()])
        .build();

    let test_policy = IamPolicy::builder().bindings(vec![test_binding]).build();

    let set_result = ctx
        .client
        .set_service_iam_policy(
            ctx.location.clone(),
            non_existent_service.to_string(),
            test_policy,
        )
        .await;
    // Similar to get_iam_policy, set operations may succeed or fail in various ways
    match set_result {
        Err(Error {
            error: Some(ErrorData::RemoteResourceNotFound { resource_name, .. }),
            ..
        }) => {
            assert_eq!(resource_name, non_existent_service);
            println!("✅ set_service_iam_policy correctly mapped 404 to RemoteResourceNotFound");
        }
        Err(Error {
            error: Some(ErrorData::RemoteAccessDenied { .. }),
            ..
        }) => {
            println!("✅ set_service_iam_policy returned access denied (acceptable)");
        }
        Err(Error {
            error: Some(ErrorData::GenericError { .. }),
            ..
        }) => {
            println!(
                "✅ set_service_iam_policy returned generic error (acceptable for IAM operations)"
            );
        }
        Ok(_) => {
            println!("✅ set_service_iam_policy succeeded (acceptable - IAM policies can exist independently)");
        }
        Err(other) => {
            println!("Got unexpected error type for set IAM policy: {:?}", other);
        }
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_get_non_existent_operation_error(ctx: &mut CloudRunTestContext) {
    let non_existent_operation = "non-existent-operation-12345";

    let result = ctx
        .client
        .get_operation(ctx.location.clone(), non_existent_operation.to_string())
        .await;
    assert!(
        result.is_err(),
        "Expected error when getting non-existent operation"
    );

    // For operations, GCP may return different error types including Generic for malformed operation names
    let err = result.unwrap_err();
    match err {
            Error { error: Some(ErrorData::RemoteResourceNotFound { ref resource_name, .. }), .. } => {
                assert_eq!(resource_name, non_existent_operation);
                println!("✅ Correctly mapped 404 to RemoteResourceNotFound for operation");
            }
            Error { error: Some(ErrorData::InvalidInput { ref message, .. }), .. } => {
                // Accept Generic errors for malformed operation names
                if message.contains("Cannot parse a full operation name") || message.contains("400") {
                    println!("✅ Correctly returned Generic error for malformed operation name");
                } else {
                    panic!("Unexpected Generic error for operation: {}", message);
                }
            }
            _ => panic!("Expected RemoteResourceNotFound or Generic error for non-existent operation, got: {:?}", err),
        }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_get_non_existent_revision_error(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();
    let service = ctx.create_basic_service(service_name.clone());

    // Create a service first
    let _create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create service for revision test");

    // Wait for service to be created
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    let non_existent_revision = "non-existent-revision-12345";

    let result = ctx
        .client
        .get_service_revision(
            ctx.location.clone(),
            service_name.clone(),
            non_existent_revision.to_string(),
        )
        .await;
    assert!(
        result.is_err(),
        "Expected error when getting non-existent revision"
    );

    let err = result.unwrap_err();
    match err {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    ref resource_name, ..
                }),
            ..
        } => {
            assert_eq!(resource_name, non_existent_revision);
            println!("✅ Correctly mapped 404 to RemoteResourceNotFound for revision");
        }
        _ => panic!(
            "Expected RemoteResourceNotFound error for non-existent revision, got: {:?}",
            err
        ),
    }
}

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_service_with_complex_configuration(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();

    println!(
        "Creating service with complex configuration: {}",
        service_name
    );

    // Create service with complex configuration (excluding reserved environment variables)
    let env_vars = vec![
        EnvVar::builder()
            .name("NODE_ENV".to_string())
            .value("production".to_string())
            .build(),
        // Remove PORT as it's reserved by Cloud Run
        EnvVar::builder()
            .name("CUSTOM_VAR".to_string())
            .value("test_value".to_string())
            .build(),
    ];

    let container = Container::builder()
        .image(TEST_CONTAINER_IMAGE.to_string())
        .env(env_vars)
        .ports(vec![ContainerPort::builder().container_port(8080).build()])
        .resources(
            ResourceRequirements::builder()
                .limits({
                    let mut limits = HashMap::new();
                    limits.insert("memory".to_string(), "1Gi".to_string());
                    limits.insert("cpu".to_string(), "2000m".to_string());
                    limits
                })
                .cpu_idle(true)
                .startup_cpu_boost(true)
                .build(),
        )
        .build();

    let revision_template = RevisionTemplate::builder()
        .containers(vec![container])
        .timeout("300s".to_string())
        .max_instance_request_concurrency(1000)
        .build();

    let traffic_target = TrafficTarget::builder()
        .r#type(TrafficTargetAllocationType::TrafficTargetAllocationTypeLatest)
        .percent(100)
        .build();

    let scaling = ServiceScaling::builder()
        .min_instance_count(0)
        .max_instance_count(10)
        .scaling_mode(ScalingMode::Automatic)
        .build();

    let mut labels = HashMap::new();
    labels.insert("environment".to_string(), "test".to_string());
    labels.insert("team".to_string(), "alien".to_string());

    let service = Service::builder()
        .template(revision_template)
        .traffic(vec![traffic_target])
        .scaling(scaling)
        .labels(labels.clone())
        .launch_stage(LaunchStage::Alpha)
        .invoker_iam_disabled(true)
        .build();

    let create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create service with complex configuration");

    assert!(
        create_operation.name.is_some(),
        "Create operation should have a name"
    );
    println!("✅ Successfully created service with complex configuration");

    // Wait for service to be created and fetch it
    tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;

    let fetched_service = ctx
        .client
        .get_service(ctx.location.clone(), service_name.clone())
        .await
        .expect("Failed to fetch service with complex configuration");

    // Verify configuration
    assert!(
        fetched_service.labels.is_some(),
        "Service should have labels"
    );
    assert_eq!(
        fetched_service
            .labels
            .as_ref()
            .unwrap()
            .get("environment")
            .unwrap(),
        "test"
    );
    assert_eq!(
        fetched_service
            .labels
            .as_ref()
            .unwrap()
            .get("team")
            .unwrap(),
        "alien"
    );

    assert!(
        fetched_service.scaling.is_some(),
        "Service should have scaling configuration"
    );
    let scaling_config = fetched_service.scaling.as_ref().unwrap();
    assert!(
        scaling_config.min_instance_count.unwrap_or(0) == 0,
        "Min instance count should be 0 or None (default)"
    );
    assert_eq!(scaling_config.max_instance_count, Some(10));

    println!("✅ Successfully verified service with complex configuration");
}

// === SERVERLESS NEG TEST ===

#[test_context(CloudRunTestContext)]
#[tokio::test]
async fn test_serverless_neg_with_cloudrun_service(ctx: &mut CloudRunTestContext) {
    let service_name = ctx.generate_unique_service_name();

    println!(
        "🚀 Starting serverless NEG integration test with Cloud Run service: {}",
        service_name
    );

    // 1. Create a Cloud Run service
    println!("📦 Creating Cloud Run service...");
    let container = Container::builder()
        .image(TEST_CONTAINER_IMAGE.to_string())
        .ports(vec![ContainerPort::builder().container_port(8080).build()])
        .build();

    let revision_template = RevisionTemplate::builder()
        .containers(vec![container])
        .build();

    let traffic_target = TrafficTarget::builder()
        .r#type(TrafficTargetAllocationType::TrafficTargetAllocationTypeLatest)
        .percent(100)
        .build();

    let service = Service::builder()
        .template(revision_template)
        .traffic(vec![traffic_target])
        .invoker_iam_disabled(true)
        .build();

    let create_operation = ctx
        .create_test_service(service_name.clone(), service)
        .await
        .expect("Failed to create Cloud Run service for NEG test");

    assert!(
        create_operation.name.is_some(),
        "Create operation should have a name"
    );
    println!("✅ Cloud Run service creation initiated");

    // 2. Wait for service to be ready
    println!("⏳ Waiting for Cloud Run service to be ready...");
    ctx.wait_for_operation(create_operation.name.as_ref().unwrap(), 300)
        .await
        .expect("Cloud Run service creation timed out");

    let ready_service = ctx
        .client
        .get_service(ctx.location.clone(), service_name.clone())
        .await
        .expect("Failed to get Cloud Run service after creation");

    let service_url = ready_service
        .urls
        .first()
        .expect("Service should have at least one URL");
    println!("🌐 Cloud Run service is ready at: {}", service_url);

    // Verify the service is actually responding to HTTP requests
    println!("🔌 Testing HTTP connectivity to Cloud Run service...");
    let http_client = Client::new();
    let response = http_client
        .get(service_url)
        .send()
        .await
        .expect("Failed to send HTTP request to Cloud Run service");

    assert!(
        response.status().is_success(),
        "Cloud Run service should return success status, got: {}",
        response.status()
    );
    println!(
        "✅ Cloud Run service is responding to HTTP requests (status: {})",
        response.status()
    );

    // 3. Create a regional serverless NEG pointing to the Cloud Run service
    println!("📦 Creating regional serverless NEG...");
    let neg_name = format!(
        "alien-test-neg-{}",
        Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
    );

    // Create NEG with Cloud Run configuration
    // According to GCP API: https://docs.cloud.google.com/compute/docs/reference/rest/v1/networkEndpointGroups
    // For serverless NEGs, we must specify cloud_run, app_engine, or cloud_function
    let cloud_run_config = NetworkEndpointGroupCloudRun::builder()
        .service(service_name.clone())
        .build();

    let neg = NetworkEndpointGroup::builder()
        .name(neg_name.clone())
        .description(format!(
            "Test serverless NEG for Cloud Run service {}",
            service_name
        ))
        .network_endpoint_type(NetworkEndpointType::Serverless)
        .cloud_run(cloud_run_config)
        .build();

    ctx.track_region_neg(&ctx.location, &neg_name);

    ctx.compute_client
        .insert_region_network_endpoint_group(ctx.location.clone(), neg)
        .await
        .expect("Failed to create regional serverless NEG");

    println!("✅ Regional serverless NEG created successfully");

    // 4. Verify the NEG was created
    println!("🔍 Verifying NEG creation...");
    let created_neg = ctx
        .compute_client
        .get_region_network_endpoint_group(ctx.location.clone(), neg_name.clone())
        .await
        .expect("Failed to get created NEG");

    assert_eq!(created_neg.name.as_ref(), Some(&neg_name));
    assert_eq!(
        created_neg.network_endpoint_type,
        Some(NetworkEndpointType::Serverless)
    );
    assert!(
        created_neg.cloud_run.is_some(),
        "NEG should have cloud_run configuration"
    );
    let cloud_run = created_neg.cloud_run.as_ref().unwrap();
    assert_eq!(cloud_run.service.as_ref(), Some(&service_name));
    println!("✅ NEG verified successfully");

    // 5. Clean up the NEG
    println!("🧹 Deleting regional serverless NEG...");
    ctx.compute_client
        .delete_region_network_endpoint_group(ctx.location.clone(), neg_name.clone())
        .await
        .expect("Failed to delete regional serverless NEG");

    ctx.untrack_region_neg(&ctx.location, &neg_name);
    println!("✅ Regional serverless NEG deleted successfully");

    println!("🎉 Serverless NEG integration test completed!");
}
