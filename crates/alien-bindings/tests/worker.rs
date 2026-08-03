#![cfg(test)]

use alien_bindings::{
    traits::{BindingsProviderApi, Worker, WorkerInvokeRequest},
    BindingsProvider,
};

use alien_core::bindings::{self, WorkerBinding};

// Import cloud clients for creating test resources
#[cfg(feature = "aws")]
use alien_aws_clients::lambda::{
    AddPermissionRequest, Cors, CreateFunctionRequest, CreateFunctionUrlConfigRequest,
    FunctionCode, LambdaApi, LambdaClient,
};
#[cfg(feature = "aws")]
use alien_aws_clients::AwsCredentialProvider;
#[cfg(feature = "azure")]
use alien_azure_clients::authorization::{AuthorizationApi, AzureAuthorizationClient, Scope};
#[cfg(feature = "azure")]
use alien_azure_clients::container_apps::{AzureContainerAppsClient, ContainerAppsApi};
#[cfg(feature = "azure")]
use alien_azure_clients::long_running_operation::LongRunningOperationClient;
#[cfg(feature = "azure")]
use alien_azure_clients::managed_identity::{AzureManagedIdentityClient, ManagedIdentityApi};
#[cfg(feature = "azure")]
use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
#[cfg(feature = "azure")]
use alien_azure_clients::models::container_apps::{
    Configuration, Container as AzureContainer, ContainerApp, ContainerAppProperties, Ingress,
    ManagedServiceIdentity, ManagedServiceIdentityType, RegistryCredentials, Scale, Template,
    TrafficWeight, UserAssignedIdentities, UserAssignedIdentity,
};
#[cfg(feature = "azure")]
use alien_azure_clients::models::managed_identity::Identity;
use alien_azure_clients::AzureTokenCache;
#[cfg(feature = "gcp")]
use alien_gcp_clients::cloudrun::{
    CloudRunApi, CloudRunClient, Container, ContainerPort, RevisionTemplate, Service,
    TrafficTarget, TrafficTargetAllocationType,
};

use alien_client_core::{Error, ErrorData};
use async_trait::async_trait;
use rstest::rstest;
use std::path::PathBuf as StdPathBuf;
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::{Arc, Mutex},
};
use test_context::AsyncTestContext;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root::get_workspace_root;

fn load_test_env() {
    // Load .env.test from the workspace root
    let root: StdPathBuf = get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
}

#[async_trait]
pub trait FunctionTestContext: AsyncTestContext + Send + Sync {
    async fn get_function(&self) -> Arc<dyn Worker>;
    fn provider_name(&self) -> &'static str;
    fn get_test_endpoint(&self) -> String;
}

#[cfg(feature = "azure")]
#[path = "worker/azure.rs"]
mod azure;
#[cfg(feature = "azure")]
use azure::AzureProviderTestContext;

// --- gRPC Provider Context ---
// --- AWS Provider Context ---
#[cfg(feature = "aws")]
struct AwsProviderTestContext {
    function: Arc<dyn Worker>,
    function_name: String,
    lambda_client: LambdaClient,
    image_uri: String,
    role_arn: String,
    created_functions: Mutex<HashSet<String>>,
    created_function_urls: Mutex<HashSet<String>>,
}

#[cfg(feature = "aws")]
impl AsyncTestContext for AwsProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-aws-function";

        let region = env::var("AWS_MANAGEMENT_REGION").expect("AWS_MANAGEMENT_REGION must be set");
        let access_key = env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set");
        let secret_key = env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set");
        let account_id =
            env::var("AWS_MANAGEMENT_ACCOUNT_ID").expect("AWS_MANAGEMENT_ACCOUNT_ID must be set");

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id: account_id.clone(),
            region: region.clone(),
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key.clone(),
                secret_access_key: secret_key.clone(),
                session_token: None,
            },
            service_overrides: None,
        };
        let lambda_client = LambdaClient::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        let image_uri = env::var("ALIEN_TEST_AWS_LAMBDA_IMAGE")
            .expect("ALIEN_TEST_AWS_LAMBDA_IMAGE must be set in .env.test");
        let role_arn = env::var("ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN")
            .unwrap_or_else(|_| format!("arn:aws:iam::{}:role/lambda-execution-role", account_id));

        // Create a unique function name
        let function_name = format!("alien-test-worker-{}", Uuid::new_v4().simple());

        // Create the Lambda function
        let request = CreateFunctionRequest::builder()
            .function_name(function_name.clone())
            .role(role_arn.clone())
            .code(FunctionCode::builder().image_uri(image_uri.clone()).build())
            .description("Test function created by alien-bindings tests".to_string())
            .timeout(30)
            .memory_size(128)
            .publish(false)
            .architectures(vec!["arm64".to_string()])
            .build();

        let _function_config = lambda_client
            .create_function(request)
            .await
            .expect("Failed to create test Lambda function");

        info!("✅ Created Lambda function: {}", function_name);

        // Wait for function to be ready
        let mut attempts = 0;
        let max_attempts = 30;
        loop {
            attempts += 1;
            match lambda_client
                .get_function_configuration(&function_name, None)
                .await
            {
                Ok(config) => {
                    if config.state == Some("Active".to_string())
                        && config.last_update_status == Some("Successful".to_string())
                    {
                        info!("✅ Worker is ready!");
                        break;
                    }
                    if attempts >= max_attempts {
                        panic!(
                            "Worker didn't become ready within {} attempts",
                            max_attempts
                        );
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
                Err(e) => {
                    warn!("Failed to get function status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }
        }

        // Create worker URL for HTTP access
        let url_request = CreateFunctionUrlConfigRequest::builder()
            .auth_type("NONE".to_string())
            .cors(
                Cors::builder()
                    .allow_credentials(false)
                    .allow_headers(vec!["Content-Type".to_string(), "X-Amz-Date".to_string()])
                    .allow_methods(vec!["GET".to_string(), "POST".to_string()])
                    .allow_origins(vec!["*".to_string()])
                    .max_age(300)
                    .build(),
            )
            .invoke_mode("BUFFERED".to_string())
            .build();

        let url_response = lambda_client
            .create_function_url_config(&function_name, url_request)
            .await
            .expect("Failed to create worker URL");

        info!("✅ Created worker URL: {}", url_response.function_url);

        // Public Worker URLs require two resource-based policy statements:
        // 1. lambda:InvokeFunctionUrl with FunctionUrlAuthType=NONE
        // 2. lambda:InvokeWorker (the actual invoke permission)
        // Without both, unauthenticated HTTP requests return 403.
        let url_permission = AddPermissionRequest::builder()
            .statement_id("AllowFunctionUrlInvoke".to_string())
            .action("lambda:InvokeFunctionUrl".to_string())
            .principal("*".to_string())
            .function_url_auth_type("NONE".to_string())
            .build();

        let invoke_permission = AddPermissionRequest::builder()
            .statement_id("AllowPublicInvoke".to_string())
            .action("lambda:InvokeFunction".to_string())
            .principal("*".to_string())
            .build();

        for perm in [url_permission, invoke_permission] {
            let sid = perm.statement_id.clone();
            match lambda_client.add_permission(&function_name, perm).await {
                Ok(_) => {
                    info!("✅ Added permission: {}", sid);
                }
                Err(e) if matches!(e.error, Some(ErrorData::RemoteResourceConflict { .. })) => {
                    info!("ℹ️ Permission {} already exists, continuing", sid);
                }
                Err(e) => panic!("Failed to add permission {}: {:?}", sid, e),
            }
        }

        // Verify the resource-based policy was applied
        match lambda_client.get_policy(&function_name, None).await {
            Ok(policy) => {
                let policy_str = policy.policy.unwrap_or_default();
                info!("📋 Lambda resource policy: {}", policy_str);
            }
            Err(e) => {
                warn!("⚠️ Could not retrieve Lambda policy: {:?}", e);
            }
        }

        // IAM resource-based policy propagation can take up to ~2 minutes on AWS.
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

        let binding = WorkerBinding::lambda(function_name.clone(), region.clone());

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert("AWS_REGION".to_string(), region);
        env_map.insert("AWS_ACCESS_KEY_ID".to_string(), access_key);
        env_map.insert("AWS_SECRET_ACCESS_KEY".to_string(), secret_key);
        env_map.insert("AWS_ACCOUNT_ID".to_string(), account_id);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "aws".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load AWS bindings provider"),
        );
        let function = provider
            .load_worker(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load AWS function for binding '{}' using function '{}': {:?}",
                    binding_name, function_name, e
                )
            });

        let mut created_functions = HashSet::new();
        created_functions.insert(function_name.clone());
        let mut created_function_urls = HashSet::new();
        created_function_urls.insert(function_name.clone());

        Self {
            function,
            function_name,
            lambda_client,
            image_uri,
            role_arn,
            created_functions: Mutex::new(created_functions),
            created_function_urls: Mutex::new(created_function_urls),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Lambda test cleanup...");

        let functions_to_cleanup = {
            let functions = self.created_functions.lock().unwrap();
            functions.clone()
        };

        let urls_to_cleanup = {
            let urls = self.created_function_urls.lock().unwrap();
            urls.clone()
        };

        // First clean up worker URLs
        for function_name in &urls_to_cleanup {
            match self
                .lambda_client
                .delete_function_url_config(function_name, None)
                .await
            {
                Ok(_) => info!("✅ Worker URL {} deleted successfully", function_name),
                Err(e) => {
                    if !matches!(
                        e,
                        Error {
                            error: Some(ErrorData::RemoteResourceNotFound { .. }),
                            ..
                        }
                    ) {
                        warn!(
                            "Failed to delete worker URL {} during cleanup: {:?}",
                            function_name, e
                        );
                    }
                }
            }
        }

        // Then clean up functions
        for function_name in functions_to_cleanup {
            match self
                .lambda_client
                .delete_function(&function_name, None)
                .await
            {
                Ok(_) => info!("✅ Worker {} deleted successfully", function_name),
                Err(e) => {
                    if !matches!(
                        e,
                        Error {
                            error: Some(ErrorData::RemoteResourceNotFound { .. }),
                            ..
                        }
                    ) {
                        warn!(
                            "Failed to delete function {} during cleanup: {:?}",
                            function_name, e
                        );
                    }
                }
            }
        }

        info!("✅ Lambda test cleanup completed");
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl FunctionTestContext for AwsProviderTestContext {
    async fn get_function(&self) -> Arc<dyn Worker> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "aws"
    }
    fn get_test_endpoint(&self) -> String {
        self.function_name.clone()
    }
}

// --- GCP Provider Context ---
#[cfg(feature = "gcp")]
struct GcpProviderTestContext {
    function: Arc<dyn Worker>,
    service_name: String,
    location: String,
    cloudrun_client: CloudRunClient,
    project_id: String,
    created_services: Mutex<HashSet<String>>,
}

#[cfg(feature = "gcp")]
impl AsyncTestContext for GcpProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-gcp-function";

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .expect("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set in .env.test");

        // Parse project_id from service account
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let gcp_region = env::var("GOOGLE_MANAGEMENT_REGION")
            .expect("GOOGLE_MANAGEMENT_REGION must be set in .env.test");

        let config = alien_gcp_clients::GcpClientConfig {
            project_id: project_id.clone(),
            region: gcp_region.clone(),
            credentials: alien_gcp_clients::GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json.clone(),
            },
            service_overrides: None,
            project_number: None,
        };

        let cloudrun_client = CloudRunClient::new(reqwest::Client::new(), config);

        // Create a unique service name
        let service_name = format!(
            "alien-test-svc-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
        );

        // Create the Cloud Run service
        let container = Container::builder()
            .image("gcr.io/cloudrun/hello".to_string())
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
            .invoker_iam_disabled(true) // Allow unauthenticated access for testing
            .build();

        let _create_operation = cloudrun_client
            .create_service(gcp_region.clone(), service_name.clone(), service, None)
            .await
            .expect("Failed to create test Cloud Run service");

        info!("✅ Created Cloud Run service: {}", service_name);

        // Wait for service to be created
        tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;

        // Get the service to verify it was created and get its URL
        let created_service = cloudrun_client
            .get_service(gcp_region.clone(), service_name.clone())
            .await
            .expect("Failed to get created service");

        let service_url = created_service
            .urls
            .first()
            .expect("Service should have at least one URL")
            .clone();

        let binding = WorkerBinding::cloud_run(
            project_id.clone(),
            service_name.clone(),
            gcp_region.clone(),
            service_url,
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert(
            "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
            gcp_credentials_json,
        );
        env_map.insert("GCP_REGION".to_string(), gcp_region.clone());
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "gcp".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load GCP bindings provider"),
        );
        let function = provider
            .load_worker(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load GCP function for binding '{}' using service '{}': {:?}",
                    binding_name, service_name, e
                )
            });

        let mut created_services = HashSet::new();
        created_services.insert(service_name.clone());

        Self {
            function,
            service_name,
            location: gcp_region,
            cloudrun_client,
            project_id,
            created_services: Mutex::new(created_services),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Cloud Run test cleanup...");

        let services_to_cleanup = {
            let services = self.created_services.lock().unwrap();
            services.clone()
        };

        for service_name in services_to_cleanup {
            match self
                .cloudrun_client
                .delete_service(self.location.clone(), service_name.clone(), None, None)
                .await
            {
                Ok(_) => info!(
                    "✅ Service {} deletion initiated successfully",
                    service_name
                ),
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

        info!("✅ Cloud Run test cleanup completed");
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl FunctionTestContext for GcpProviderTestContext {
    async fn get_function(&self) -> Arc<dyn Worker> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "gcp"
    }
    fn get_test_endpoint(&self) -> String {
        format!("{}/{}", self.location, self.service_name)
    }
}

// --- Test implementations ---

/// Test function invoke functionality with various HTTP methods and payloads
#[rstest]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[tokio::test]
async fn test_function_invoke(#[case] ctx: impl FunctionTestContext) {
    let function = ctx.get_function().await;
    let provider_name = ctx.provider_name();
    let test_endpoint = ctx.get_test_endpoint();

    info!("[{}] Testing function invoke functionality", provider_name);

    // Test GET request
    let get_request = WorkerInvokeRequest {
        method: "GET".to_string(),
        path: "/get".to_string(),
        headers: std::collections::BTreeMap::new(),
        body: Vec::new(),
        timeout: Some(Duration::from_secs(60)),
        target_worker: test_endpoint.clone(),
    };

    let get_response = function
        .invoke(get_request)
        .await
        .expect(&format!("[{}] GET invoke should succeed", provider_name));

    info!(
        "[{}] GET invoke succeeded with status: {}",
        provider_name, get_response.status
    );
    assert!(
        get_response.status >= 200 && get_response.status < 600,
        "[{}] Invalid HTTP status: {}",
        provider_name,
        get_response.status
    );

    // Validate response structure
    info!(
        "[{}] GET response headers count: {}",
        provider_name,
        get_response.headers.len()
    );

    info!(
        "[{}] GET response body length: {} bytes",
        provider_name,
        get_response.body.len()
    );

    // Test POST request with payload
    let mut post_headers = std::collections::BTreeMap::new();
    post_headers.insert("Content-Type".to_string(), "application/json".to_string());
    post_headers.insert("X-Test-Header".to_string(), "alien-test".to_string());

    let post_request = WorkerInvokeRequest {
        method: "POST".to_string(),
        path: "/post".to_string(),
        headers: post_headers,
        body: b"{\"message\": \"Hello from Alien!\"}".to_vec(),
        timeout: Some(Duration::from_secs(60)),
        target_worker: test_endpoint,
    };

    let post_response = function
        .invoke(post_request)
        .await
        .expect(&format!("[{}] POST invoke should succeed", provider_name));

    info!(
        "[{}] POST invoke succeeded with status: {}",
        provider_name, post_response.status
    );
    assert!(
        post_response.status >= 200 && post_response.status < 600,
        "[{}] Invalid HTTP status: {}",
        provider_name,
        post_response.status
    );

    // Validate response structure
    info!(
        "[{}] POST response headers count: {}",
        provider_name,
        post_response.headers.len()
    );

    info!(
        "[{}] POST response body length: {} bytes",
        provider_name,
        post_response.body.len()
    );

    info!("[{}] Worker invoke test completed", provider_name);
}

/// Test getting worker URL and making direct HTTP request
#[rstest]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[tokio::test]
async fn test_function_http_access(#[case] ctx: impl FunctionTestContext) {
    let function = ctx.get_function().await;
    let provider_name = ctx.provider_name();

    info!("[{}] Testing function HTTP URL access", provider_name);

    // Get worker URL
    let function_url = function
        .get_worker_url()
        .await
        .expect(&format!(
            "[{}] Should be able to get worker URL",
            provider_name
        ))
        .expect(&format!(
            "[{}] Worker should have a valid URL",
            provider_name
        ));

    info!("[{}] Worker URL retrieved: {}", provider_name, function_url);

    // Validate URL format
    assert!(
        function_url.starts_with("http://") || function_url.starts_with("https://"),
        "[{}] Invalid URL format: {}",
        provider_name,
        function_url
    );

    // Make direct HTTP request to the worker URL with retry for IAM propagation
    let client = reqwest::Client::new();
    let request_url = format!("{}/", function_url.trim_end_matches('/'));

    let mut last_status = None;
    let mut last_body = String::new();
    let max_retries = 8;
    for attempt in 1..=max_retries {
        let response = client
            .get(&request_url)
            .header("User-Agent", "alien-function-test/1.0")
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .expect(&format!(
                "[{}] HTTP request to {} should succeed",
                provider_name, request_url
            ));

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        info!(
            "[{}] HTTP request attempt {}/{} to {} returned status: {} body: {}",
            provider_name,
            attempt,
            max_retries,
            request_url,
            status,
            &body[..body.len().min(200)]
        );

        if status.is_success() || status.is_redirection() {
            info!(
                "[{}] Response body length: {} bytes",
                provider_name,
                body.len()
            );
            info!("[{}] Worker HTTP access test completed", provider_name);
            return;
        }

        last_status = Some(status);
        last_body = body;
        if attempt < max_retries {
            let wait = Duration::from_secs(10);
            info!(
                "[{}] Got {}, waiting {:?} for propagation before retry...",
                provider_name, status, wait
            );
            tokio::time::sleep(wait).await;
        }
    }

    panic!(
        "[{}] HTTP request failed after {} retries with status: {} body: {}",
        provider_name,
        max_retries,
        last_status.unwrap(),
        last_body
    );
}
