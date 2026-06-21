#![cfg(test)]

use alien_bindings::{
    aws_sdk::lambda_client_from_alien_config,
    traits::{BindingsProviderApi, Worker, WorkerInvokeRequest},
    BindingsProvider,
};

#[cfg(feature = "grpc")]
use alien_bindings::{grpc::run_grpc_server, providers::grpc_provider::GrpcBindingsProvider};
use alien_core::bindings::{self, WorkerBinding};

#[cfg(feature = "aws")]
use alien_core::{AwsClientConfig, AwsCredentials};
#[cfg(feature = "azure")]
use alien_core::{AzureClientConfig, AzureCredentials};
#[cfg(feature = "gcp")]
use alien_core::{GcpClientConfig, GcpCredentials};
#[cfg(feature = "aws")]
use aws_sdk_lambda::{
    types::{Architecture, Cors, FunctionCode, FunctionUrlAuthType, InvokeMode, PackageType},
    Client as LambdaClient,
};
#[cfg(feature = "azure")]
use azure_core::{
    credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions},
    time::{Duration as AzureDuration, OffsetDateTime},
};
#[cfg(feature = "azure")]
use azure_identity::{ClientSecretCredential, ClientSecretCredentialOptions};
#[cfg(feature = "gcp")]
use google_cloud_auth::credentials::{self, Credentials};
#[cfg(feature = "gcp")]
use google_cloud_longrunning::model::Operation as CloudRunOperation;
#[cfg(feature = "gcp")]
use google_cloud_run_v2::{
    client::Services as CloudRunServices,
    model::{
        Container, ContainerPort, RevisionTemplate, Service, TrafficTarget,
        TrafficTargetAllocationType,
    },
};
#[cfg(feature = "azure")]
use reqwest::{Method, StatusCode};

use async_trait::async_trait;
use rstest::rstest;
use std::path::PathBuf as StdPathBuf;
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;
use test_context::AsyncTestContext;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root::get_workspace_root;

const GRPC_BINDING_NAME: &str = "test-grpc-function-binding";

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

// --- gRPC Provider Context ---
#[cfg(feature = "grpc")]
struct GrpcProviderTestContext {
    function: Arc<dyn Worker>,
    _server_handle:
        JoinHandle<Result<(), alien_error::AlienError<alien_bindings::error::ErrorData>>>,
    _temp_data_dir: TempDir,
}

#[cfg(feature = "grpc")]
impl AsyncTestContext for GrpcProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let temp_data_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for ALIEN_DATA_DIR (gRPC server)");

        // Mock worker binding for gRPC - this will simulate a local HTTP endpoint
        let server_binding =
            WorkerBinding::lambda("test-worker".to_string(), "us-east-1".to_string());

        let mut server_provider_env_map: HashMap<String, String> = env::vars().collect();
        let server_binding_json =
            serde_json::to_string(&server_binding).expect("Failed to serialize server binding");
        server_provider_env_map.insert(
            bindings::binding_env_var_name(GRPC_BINDING_NAME),
            server_binding_json,
        );
        server_provider_env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());

        let local_provider_for_server = Arc::new(
            BindingsProvider::from_env(server_provider_env_map)
                .await
                .expect("Failed to load bindings provider"),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind to random port");
        let addr = listener.local_addr().expect("Failed to get local address");
        drop(listener); // Release the port so the server can bind to it

        let server_addr_str = addr.to_string();
        let server_addr_for_spawn = server_addr_str.clone();

        let server_handle = tokio::spawn(async move {
            let handles = run_grpc_server(local_provider_for_server, &server_addr_for_spawn)
                .await
                .unwrap();

            // Wait for server to be ready
            handles
                .readiness_receiver
                .await
                .expect("Server should become ready");
            handles.server_task.await.unwrap()
        });

        tokio::time::sleep(std::time::Duration::from_millis(500)).await; // Allow server to start

        // Env map for the GrpcBindingsProvider (client-side)
        let mut service_provider_env_map: HashMap<String, String> = env::vars().collect();
        service_provider_env_map.insert(
            "ALIEN_BINDINGS_GRPC_ADDRESS".to_string(),
            server_addr_str.clone(),
        );
        service_provider_env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "grpc".to_string());

        let grpc_provider = GrpcBindingsProvider::new_with_env(service_provider_env_map)
            .expect("Failed to load bindings provider");

        let function = grpc_provider
            .load_worker(GRPC_BINDING_NAME)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Grpc function for binding '{}' using ALIEN_BINDINGS_GRPC_ADDRESS='{}': {:?}",
                    GRPC_BINDING_NAME, server_addr_str, e
                )
            });

        Self {
            function,
            _server_handle: server_handle,
            _temp_data_dir: temp_data_dir,
        }
    }

    async fn teardown(self) {
        self._server_handle.abort();
    }
}

#[cfg(feature = "grpc")]
#[async_trait]
impl FunctionTestContext for GrpcProviderTestContext {
    async fn get_function(&self) -> Arc<dyn Worker> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "grpc"
    }
    fn get_test_endpoint(&self) -> String {
        "test-worker".to_string()
    }
}

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

        let aws_config = AwsClientConfig {
            account_id: account_id.clone(),
            region: region.clone(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: access_key.clone(),
                secret_access_key: secret_key.clone(),
                session_token: None,
            },
            service_overrides: None,
        };
        let lambda_client = lambda_client_from_alien_config(&aws_config)
            .await
            .expect("Failed to create AWS Lambda SDK client for worker test");

        let image_uri = env::var("ALIEN_TEST_AWS_LAMBDA_IMAGE")
            .expect("ALIEN_TEST_AWS_LAMBDA_IMAGE must be set in .env.test");
        let role_arn = env::var("ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN")
            .unwrap_or_else(|_| format!("arn:aws:iam::{}:role/lambda-execution-role", account_id));

        // Create a unique function name
        let function_name = format!("alien-test-worker-{}", Uuid::new_v4().simple());

        let _function_config = lambda_client
            .create_function()
            .function_name(&function_name)
            .role(&role_arn)
            .code(FunctionCode::builder().image_uri(&image_uri).build())
            .package_type(PackageType::Image)
            .description("Test function created by alien-bindings tests")
            .timeout(30)
            .memory_size(128)
            .publish(false)
            .architectures(Architecture::Arm64)
            .send()
            .await
            .expect("Failed to create test Lambda function");

        info!("✅ Created Lambda function: {}", function_name);

        // Wait for function to be ready
        let mut attempts = 0;
        let max_attempts = 30;
        loop {
            attempts += 1;
            match lambda_client
                .get_function_configuration()
                .function_name(&function_name)
                .send()
                .await
            {
                Ok(config) => {
                    if config
                        .state()
                        .is_some_and(|state| state.as_str() == "Active")
                        && config
                            .last_update_status()
                            .is_some_and(|status| status.as_str() == "Successful")
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

        let url_response = lambda_client
            .create_function_url_config()
            .function_name(&function_name)
            .auth_type(FunctionUrlAuthType::None)
            .cors(
                Cors::builder()
                    .allow_credentials(false)
                    .allow_headers("Content-Type")
                    .allow_headers("X-Amz-Date")
                    .allow_methods("GET")
                    .allow_methods("POST")
                    .allow_origins("*")
                    .max_age(300)
                    .build(),
            )
            .invoke_mode(InvokeMode::Buffered)
            .send()
            .await
            .expect("Failed to create worker URL");

        info!("✅ Created worker URL: {}", url_response.function_url());

        // Public Worker URLs require two resource-based policy statements:
        // 1. lambda:InvokeFunctionUrl with FunctionUrlAuthType=NONE
        // 2. lambda:InvokeWorker (the actual invoke permission)
        // Without both, unauthenticated HTTP requests return 403.
        for (sid, action, auth_type) in [
            (
                "AllowFunctionUrlInvoke",
                "lambda:InvokeFunctionUrl",
                Some(FunctionUrlAuthType::None),
            ),
            ("AllowPublicInvoke", "lambda:InvokeFunction", None),
        ] {
            let mut permission = lambda_client
                .add_permission()
                .function_name(&function_name)
                .statement_id(sid)
                .action(action)
                .principal("*");
            if let Some(auth_type) = auth_type {
                permission = permission.function_url_auth_type(auth_type);
            }
            match permission.send().await {
                Ok(_) => {
                    info!("✅ Added permission: {}", sid);
                }
                Err(e)
                    if e.as_service_error()
                        .and_then(|error| error.meta().code())
                        .is_some_and(|code| code == "ResourceConflictException") =>
                {
                    info!("ℹ️ Permission {} already exists, continuing", sid);
                }
                Err(e) => panic!("Failed to add permission {}: {:?}", sid, e),
            }
        }

        // Verify the resource-based policy was applied
        match lambda_client
            .get_policy()
            .function_name(&function_name)
            .send()
            .await
        {
            Ok(policy) => {
                let policy_str = policy.policy().unwrap_or_default();
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
                .delete_function_url_config()
                .function_name(function_name)
                .send()
                .await
            {
                Ok(_) => info!("✅ Worker URL {} deleted successfully", function_name),
                Err(e) => {
                    let is_not_found = e
                        .as_service_error()
                        .and_then(|error| error.meta().code())
                        .is_some_and(|code| code == "ResourceNotFoundException");
                    if !is_not_found {
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
                .delete_function()
                .function_name(&function_name)
                .send()
                .await
            {
                Ok(_) => info!("✅ Worker {} deleted successfully", function_name),
                Err(e) => {
                    let is_not_found = e
                        .as_service_error()
                        .and_then(|error| error.meta().code())
                        .is_some_and(|code| code == "ResourceNotFoundException");
                    if !is_not_found {
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
    cloudrun_client: CloudRunServices,
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

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: gcp_region.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json.clone(),
            },
            service_overrides: None,
            project_number: None,
        };

        let cloudrun_client = cloud_run_services_client_from_alien_config(&config)
            .await
            .expect("Failed to build official Cloud Run services client");

        // Create a unique service name
        let service_name = format!(
            "alien-test-svc-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
        );

        // Create the Cloud Run service
        let container = Container::new()
            .set_image("gcr.io/cloudrun/hello")
            .set_ports([ContainerPort::new().set_container_port(8080)]);

        let revision_template = RevisionTemplate::new().set_containers([container]);

        let traffic_target = TrafficTarget::new()
            .set_type(TrafficTargetAllocationType::Latest)
            .set_percent(100);

        let service = Service::new()
            .set_template(revision_template)
            .set_traffic([traffic_target])
            .set_invoker_iam_disabled(true); // Allow unauthenticated access for testing

        let create_operation = cloudrun_client
            .create_service()
            .set_parent(cloud_run_parent(&project_id, &gcp_region))
            .set_service_id(service_name.clone())
            .set_service(service)
            .send()
            .await
            .expect("Failed to create test Cloud Run service");

        info!("✅ Created Cloud Run service: {}", service_name);

        wait_for_cloud_run_operation(&cloudrun_client, create_operation, &service_name)
            .await
            .expect("Cloud Run service creation should complete");

        // Get the service to verify it was created and get its URL
        let created_service = cloudrun_client
            .get_service()
            .set_name(cloud_run_service_name(
                &project_id,
                &gcp_region,
                &service_name,
            ))
            .send()
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
                .delete_service()
                .set_name(cloud_run_service_name(
                    &self.project_id,
                    &self.location,
                    &service_name,
                ))
                .send()
                .await
            {
                Ok(_) => info!(
                    "✅ Service {} deletion initiated successfully",
                    service_name
                ),
                Err(error) => {
                    if official_gcp_error_is_not_found(&error) {
                        info!("🔍 Service {} was already deleted", service_name);
                    } else {
                        warn!(
                            "Failed to delete service {} during cleanup: {:?}",
                            service_name, error
                        );
                    }
                }
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

#[cfg(feature = "gcp")]
async fn cloud_run_services_client_from_alien_config(
    config: &GcpClientConfig,
) -> anyhow::Result<CloudRunServices> {
    let credentials = gcp_credentials_from_alien_config(config)?;
    CloudRunServices::builder()
        .with_credentials(credentials)
        .build()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to build official Cloud Run client: {error}"))
}

#[cfg(feature = "gcp")]
fn gcp_credentials_from_alien_config(config: &GcpClientConfig) -> anyhow::Result<Credentials> {
    match &config.credentials {
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<serde_json::Value>(json).map_err(|error| {
                anyhow::anyhow!("Failed to parse GCP service account key JSON: {error}")
            })?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    ["https://www.googleapis.com/auth/cloud-platform"],
                ))
                .build()
                .map_err(|error| {
                    anyhow::anyhow!("Failed to build GCP service account credentials: {error}")
                })
        }
        other => anyhow::bail!(
            "alien-bindings Cloud Run live test setup supports service-account-key credentials only, got {other:?}"
        ),
    }
}

#[cfg(feature = "gcp")]
async fn wait_for_cloud_run_operation(
    client: &CloudRunServices,
    mut operation: CloudRunOperation,
    service_name: &str,
) -> anyhow::Result<()> {
    for attempt in 1..=40 {
        if operation.done {
            if let Some(error) = operation.error() {
                anyhow::bail!("Cloud Run operation for service '{service_name}' failed: {error:?}");
            }
            return Ok(());
        }

        info!(
            "⏳ Waiting for Cloud Run operation on {} (attempt {}/40)",
            service_name, attempt
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        operation = client
            .get_operation()
            .set_name(operation.name.clone())
            .send()
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "Failed to poll Cloud Run operation for service '{service_name}': {error}"
                )
            })?;
    }

    anyhow::bail!("Cloud Run operation for service '{service_name}' did not finish within timeout")
}

#[cfg(feature = "gcp")]
fn cloud_run_parent(project_id: &str, region: &str) -> String {
    format!("projects/{project_id}/locations/{region}")
}

#[cfg(feature = "gcp")]
fn cloud_run_service_name(project_id: &str, region: &str, service_name: &str) -> String {
    format!(
        "{}/services/{}",
        cloud_run_parent(project_id, region),
        service_name
    )
}

#[cfg(feature = "gcp")]
fn official_gcp_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == google_cloud_gax::error::rpc::Code::NotFound)
        || error.http_status_code() == Some(404)
}

// --- Azure Provider Context ---
#[cfg(feature = "azure")]
struct AzureProviderTestContext {
    function: Arc<dyn Worker>,
    resource_group_name: String,
    container_app_name: String,
    management_client: AzureContainerAppsManagementTestClient,
    created_container_apps: Mutex<HashSet<String>>,
    created_managed_identities: Mutex<HashSet<String>>,
    created_role_assignments: Mutex<HashSet<String>>,
}

#[cfg(feature = "azure")]
impl AsyncTestContext for AzureProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-azure-function";

        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID must be set in .env.test");
        let tenant_id = env::var("AZURE_MANAGEMENT_TENANT_ID")
            .expect("AZURE_MANAGEMENT_TENANT_ID must be set in .env.test");
        let client_id = env::var("AZURE_MANAGEMENT_CLIENT_ID")
            .expect("AZURE_MANAGEMENT_CLIENT_ID must be set in .env.test");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET must be set in .env.test");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP must be set in .env.test");
        let managed_environment_name = env::var("ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME")
            .expect("ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME must be set in .env.test");
        let default_container_image =
            "mcr.microsoft.com/azuredocs/containerapps-helloworld:latest".to_string();
        let mut container_image = env::var("ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE")
            .unwrap_or_else(|_| default_container_image.clone());

        let client_config = AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id: tenant_id.clone(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
            },
            service_overrides: None,
        };

        let management_client = AzureContainerAppsManagementTestClient::new(client_config.clone())
            .expect("Failed to build Azure Container Apps management test client");

        // Get the existing managed environment to retrieve its ID
        let managed_environment = management_client.get_managed_environment(&resource_group_name, &managed_environment_name).await
            .expect("Failed to get existing managed environment. Make sure ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME points to an existing managed environment.");

        let managed_environment_id = managed_environment
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(String::from)
            .expect("Managed environment should have an ID");

        // Create a unique container app name
        let container_app_name = format!(
            "alien-test-app-{}",
            Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>()
        );

        // Initialize tracking collections
        let mut created_managed_identities = HashSet::new();
        let mut created_role_assignments = HashSet::new();

        // Create managed identity with ACR access if needed
        let (registries, identity) = if container_image.contains(".azurecr.io") {
            info!(
                "🔐 Setting up ACR authentication for container image: {}",
                container_image
            );
            let identity_name = format!("{}-identity", container_app_name);

            let created_identity = management_client
                .create_user_assigned_identity(&resource_group_name, &identity_name, "eastus")
                .await
                .expect("Failed to create managed identity");

            let principal_id = created_identity
                .get("properties")
                .and_then(|properties| properties.get("principalId"))
                .and_then(serde_json::Value::as_str)
                .map(String::from)
                .expect("Managed identity should have a principal ID");

            let identity_resource_id = created_identity
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(String::from)
                .expect("Managed identity should have a resource ID");

            info!(
                "✅ Created managed identity with principal ID: {}",
                principal_id
            );

            // Track the managed identity for cleanup
            created_managed_identities.insert(identity_name.clone());

            // Extract ACR name from container image and assign AcrPull role
            let acr_server = container_image.split('/').next().unwrap_or_default();
            let acr_name = acr_server.split('.').next().unwrap_or_default();

            info!(
                "🏷️ Assigning AcrPull role to managed identity for ACR: {}",
                acr_name
            );

            // Create role assignment
            let assignment_id = Uuid::new_v4().to_string();
            let acr_pull_role_definition_id = "7f951dda-4ed3-4680-a7ca-43fe172d538d"; // AcrPull built-in role
            let acr_scope = format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerRegistry/registries/{}",
                subscription_id, resource_group_name, acr_name
            );
            let role_definition_full_id = format!(
                "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                subscription_id, acr_pull_role_definition_id
            );

            let full_assignment_id = format!(
                "{}/providers/Microsoft.Authorization/roleAssignments/{}",
                acr_scope, assignment_id
            );

            let role_assignment_result = management_client
                .create_or_update_role_assignment_by_id(
                    &full_assignment_id,
                    &principal_id,
                    &role_definition_full_id,
                    &acr_scope,
                )
                .await;

            if let Err(e) = role_assignment_result {
                if e.status == Some(StatusCode::NOT_FOUND) {
                    warn!(
                        "ACR registry not found for image {}, falling back to public image",
                        container_image
                    );
                    container_image = default_container_image.clone();
                    (vec![], None)
                } else {
                    panic!("Failed to create role assignment: {:?}", e);
                }
            } else {
                info!("✅ Assigned AcrPull role to managed identity");

                // Track the role assignment for cleanup
                created_role_assignments.insert(full_assignment_id.clone());

                // Wait for role assignment to propagate
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

                let registries = vec![serde_json::json!({
                    "server": acr_server,
                    "identity": identity_resource_id,
                })];

                let identity = Some(serde_json::json!({
                    "type": "UserAssigned",
                    "userAssignedIdentities": {
                        identity_resource_id: {},
                    },
                }));

                info!("✅ Configured managed identity and registry credentials");

                (registries, identity)
            }
        } else {
            info!("ℹ️ Using public container image, no ACR authentication needed");
            (vec![], None)
        };

        // Create the Container App
        let container_app = azure_container_app_request(
            "eastus",
            &managed_environment_id,
            &container_image,
            registries,
            identity,
        );

        management_client
            .create_or_update_container_app(
                &resource_group_name,
                &container_app_name,
                &container_app,
            )
            .await
            .expect("Failed to create test Container App");

        info!("✅ Created Container App: {}", container_app_name);

        // Wait for container app to be ready
        let mut attempts = 0;
        let max_attempts = 12; // Increased from 6 to 12
        loop {
            attempts += 1;

            match management_client
                .get_container_app(&resource_group_name, &container_app_name)
                .await
            {
                Ok(app) => {
                    if let Some(state) = app
                        .get("properties")
                        .and_then(|properties| properties.get("provisioningState"))
                        .and_then(serde_json::Value::as_str)
                    {
                        info!(
                            "📊 Container app provisioning state: {} (attempt {}/{})",
                            state, attempts, max_attempts
                        );

                        if state == "Succeeded" {
                            info!("✅ Container app is ready!");
                            break;
                        }

                        if state == "Failed" {
                            panic!("❌ Container app provisioning failed");
                        }
                    }

                    if attempts >= max_attempts {
                        panic!("⚠️  Container app didn't become ready within timeout");
                    }

                    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                    // Increased from 10 to 15 seconds
                }
                Err(e) => {
                    panic!("Failed to get container app status: {:?}", e);
                }
            }
        }

        // Additional wait time for the container to start responding to HTTP requests
        info!("⏳ Waiting additional time for container to be ready for HTTP requests...");
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

        // Get the created app to get its URL
        let created_app = management_client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
            .expect("Failed to get created container app");

        let app_url = created_app
            .get("properties")
            .and_then(|properties| properties.get("configuration"))
            .and_then(|configuration| configuration.get("ingress"))
            .and_then(|ingress| ingress.get("fqdn"))
            .and_then(serde_json::Value::as_str)
            .map(|fqdn| format!("https://{}", fqdn))
            .expect("Container app should have a valid FQDN after creation");

        let binding = WorkerBinding::container_app(
            subscription_id.clone(),
            resource_group_name.clone(),
            container_app_name.clone(),
            app_url,
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert("AZURE_TENANT_ID".to_string(), tenant_id);
        env_map.insert("AZURE_CLIENT_ID".to_string(), client_id);
        env_map.insert("AZURE_CLIENT_SECRET".to_string(), client_secret);
        env_map.insert("AZURE_SUBSCRIPTION_ID".to_string(), subscription_id);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "azure".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load Azure bindings provider"),
        );
        let function = provider
            .load_worker(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Azure function for binding '{}' using container app '{}': {:?}",
                    binding_name, container_app_name, e
                )
            });

        let mut created_container_apps = HashSet::new();
        created_container_apps.insert(container_app_name.clone());

        Self {
            function,
            resource_group_name,
            container_app_name,
            management_client,
            created_container_apps: Mutex::new(created_container_apps),
            created_managed_identities: Mutex::new(created_managed_identities),
            created_role_assignments: Mutex::new(created_role_assignments),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Container Apps test cleanup...");

        // Cleanup role assignments first
        let role_assignments_to_cleanup = {
            let assignments = self.created_role_assignments.lock().unwrap();
            assignments.clone()
        };

        for assignment_id in role_assignments_to_cleanup {
            match self
                .management_client
                .delete_role_assignment_by_id(&assignment_id)
                .await
            {
                Ok(_) => info!("✅ Role assignment {} deleted successfully", assignment_id),
                Err(err) if err.status == Some(StatusCode::NOT_FOUND) => {
                    info!("🔍 Role assignment {} was already deleted", assignment_id);
                }
                Err(e) => {
                    warn!(
                        "Failed to delete role assignment {} during cleanup: {:?}",
                        assignment_id, e
                    );
                }
            }
        }

        // Cleanup managed identities
        let identities_to_cleanup = {
            let identities = self.created_managed_identities.lock().unwrap();
            identities.clone()
        };

        for identity_name in identities_to_cleanup {
            match self
                .management_client
                .delete_user_assigned_identity(&self.resource_group_name, &identity_name)
                .await
            {
                Ok(_) => info!("✅ Managed identity {} deleted successfully", identity_name),
                Err(err) if err.status == Some(StatusCode::NOT_FOUND) => {
                    info!("🔍 Managed identity {} was already deleted", identity_name);
                }
                Err(e) => {
                    warn!(
                        "Failed to delete managed identity {} during cleanup: {:?}",
                        identity_name, e
                    );
                }
            }
        }

        // Cleanup container apps
        let container_apps_to_cleanup = {
            let apps = self.created_container_apps.lock().unwrap();
            apps.clone()
        };

        for container_app_name in container_apps_to_cleanup {
            match self
                .management_client
                .delete_container_app(&self.resource_group_name, &container_app_name)
                .await
            {
                Ok(_) => info!(
                    "✅ Container app {} deleted successfully",
                    container_app_name
                ),
                Err(err) if err.status == Some(StatusCode::NOT_FOUND) => {
                    info!(
                        "🔍 Container app {} was already deleted",
                        container_app_name
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to delete container app {} during cleanup: {:?}",
                        container_app_name, e
                    );
                }
            }
        }

        info!("✅ Container Apps test cleanup completed");
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl FunctionTestContext for AzureProviderTestContext {
    async fn get_function(&self) -> Arc<dyn Worker> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
    fn get_test_endpoint(&self) -> String {
        format!("{}/{}", self.resource_group_name, self.container_app_name)
    }
}

#[cfg(feature = "azure")]
#[derive(Clone)]
struct AzureContainerAppsManagementTestClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

#[cfg(feature = "azure")]
impl AzureContainerAppsManagementTestClient {
    fn new(config: AzureClientConfig) -> anyhow::Result<Self> {
        Ok(Self {
            credential: azure_credential_from_config(&config)?,
            config,
            http_client: reqwest::Client::new(),
        })
    }

    async fn get_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> Result<serde_json::Value, AzureTestRestError> {
        let (_, _, body) = self
            .request(
                Method::GET,
                self.managed_environment_url(resource_group_name, environment_name),
                None,
                environment_name,
            )
            .await?;
        self.parse_json(&body, environment_name)
    }

    async fn create_user_assigned_identity(
        &self,
        resource_group_name: &str,
        identity_name: &str,
        location: &str,
    ) -> Result<serde_json::Value, AzureTestRestError> {
        let body = serde_json::json!({
            "location": location,
            "tags": {
                "Environment": "Test",
                "Application": "alien-test",
            },
        });
        let (status, headers, response_body) = self
            .request(
                Method::PUT,
                self.user_assigned_identity_url(resource_group_name, identity_name),
                Some(body.to_string()),
                identity_name,
            )
            .await?;
        self.wait_for_operation_if_needed(
            status,
            &headers,
            "CreateUserAssignedIdentity",
            identity_name,
        )
        .await?;
        self.parse_json(&response_body, identity_name)
    }

    async fn delete_user_assigned_identity(
        &self,
        resource_group_name: &str,
        identity_name: &str,
    ) -> Result<(), AzureTestRestError> {
        let (status, headers, _) = self
            .request(
                Method::DELETE,
                self.user_assigned_identity_url(resource_group_name, identity_name),
                None,
                identity_name,
            )
            .await?;
        self.wait_for_operation_if_needed(
            status,
            &headers,
            "DeleteUserAssignedIdentity",
            identity_name,
        )
        .await
    }

    async fn create_or_update_role_assignment_by_id(
        &self,
        role_assignment_id: &str,
        principal_id: &str,
        role_definition_id: &str,
        scope: &str,
    ) -> Result<(), AzureTestRestError> {
        let body = serde_json::json!({
            "properties": {
                "principalId": principal_id,
                "roleDefinitionId": role_definition_id,
                "principalType": "ServicePrincipal",
                "scope": scope,
                "description": "AcrPull role for Container App managed identity",
            },
        });
        self.request(
            Method::PUT,
            self.role_assignment_url(role_assignment_id),
            Some(body.to_string()),
            role_assignment_id,
        )
        .await?;
        Ok(())
    }

    async fn delete_role_assignment_by_id(
        &self,
        role_assignment_id: &str,
    ) -> Result<(), AzureTestRestError> {
        self.request(
            Method::DELETE,
            self.role_assignment_url(role_assignment_id),
            None,
            role_assignment_id,
        )
        .await?;
        Ok(())
    }

    async fn create_or_update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &serde_json::Value,
    ) -> Result<(), AzureTestRestError> {
        let (status, headers, _) = self
            .request(
                Method::PUT,
                self.container_app_url(resource_group_name, container_app_name),
                Some(container_app.to_string()),
                container_app_name,
            )
            .await?;
        self.wait_for_operation_if_needed(
            status,
            &headers,
            "CreateContainerApp",
            container_app_name,
        )
        .await
    }

    async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> Result<serde_json::Value, AzureTestRestError> {
        let (_, _, body) = self
            .request(
                Method::GET,
                self.container_app_url(resource_group_name, container_app_name),
                None,
                container_app_name,
            )
            .await?;
        self.parse_json(&body, container_app_name)
    }

    async fn delete_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> Result<(), AzureTestRestError> {
        let (status, headers, _) = self
            .request(
                Method::DELETE,
                self.container_app_url(resource_group_name, container_app_name),
                None,
                container_app_name,
            )
            .await?;
        self.wait_for_operation_if_needed(
            status,
            &headers,
            "DeleteContainerApp",
            container_app_name,
        )
        .await
    }

    fn base_url(&self) -> String {
        self.config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("management"))
            .map(String::as_str)
            .unwrap_or("https://management.azure.com")
            .trim_end_matches('/')
            .to_string()
    }

    fn managed_environment_url(&self, resource_group_name: &str, environment_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}?api-version=2025-01-01",
            self.base_url(), self.config.subscription_id, resource_group_name, environment_name
        )
    }

    fn container_app_url(&self, resource_group_name: &str, container_app_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}?api-version=2025-01-01",
            self.base_url(), self.config.subscription_id, resource_group_name, container_app_name
        )
    }

    fn user_assigned_identity_url(&self, resource_group_name: &str, identity_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}?api-version=2023-01-31",
            self.base_url(), self.config.subscription_id, resource_group_name, identity_name
        )
    }

    fn role_assignment_url(&self, role_assignment_id: &str) -> String {
        format!(
            "{}{}?api-version=2022-04-01",
            self.base_url(),
            role_assignment_id
        )
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<String>,
        resource_name: &str,
    ) -> Result<(StatusCode, reqwest::header::HeaderMap, String), AzureTestRestError> {
        let token = self.bearer_token().await?;
        let mut request = self
            .http_client
            .request(method, &url)
            .bearer_auth(token.token.secret());

        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }

        let response = request.send().await.map_err(|error| {
            AzureTestRestError::new(
                None,
                format!("Azure Container Apps ARM request failed for '{resource_name}': {error}"),
            )
        })?;
        let status = response.status();
        let headers = response.headers().clone();
        let text = response.text().await.map_err(|error| {
            AzureTestRestError::new(
                None,
                format!(
                    "Failed to read Azure Container Apps ARM response for '{resource_name}': {error}"
                ),
            )
        })?;

        if !status.is_success() {
            return Err(AzureTestRestError::new(
                Some(status),
                format!(
                    "Azure Container Apps ARM request for '{resource_name}' returned HTTP {}: {text}",
                    status.as_u16()
                ),
            ));
        }

        Ok((status, headers, text))
    }

    async fn bearer_token(&self) -> Result<AccessToken, AzureTestRestError> {
        self.credential
            .get_token(&["https://management.azure.com/.default"], None)
            .await
            .map_err(|error| {
                AzureTestRestError::new(None, format!("Failed to get Azure ARM token: {error}"))
            })
    }

    async fn wait_for_operation_if_needed(
        &self,
        status: StatusCode,
        headers: &reqwest::header::HeaderMap,
        operation_name: &str,
        resource_name: &str,
    ) -> Result<(), AzureTestRestError> {
        if status != StatusCode::ACCEPTED {
            return Ok(());
        }

        let operation_url = headers
            .get("azure-asyncoperation")
            .or_else(|| headers.get("location"))
            .ok_or_else(|| {
                AzureTestRestError::new(
                    None,
                    format!(
                        "Azure {operation_name} for '{resource_name}' returned 202 without operation URL"
                    ),
                )
            })?
            .to_str()
            .map_err(|error| {
                AzureTestRestError::new(
                    None,
                    format!("Failed to parse Azure operation URL header: {error}"),
                )
            })?
            .to_string();
        let retry_after = headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(10);

        for attempt in 1..=60 {
            info!(
                "⏳ Waiting for Azure {} on {} (attempt {}/60)",
                operation_name, resource_name, attempt
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(retry_after)).await;

            let (poll_status, _, body) = self
                .request(Method::GET, operation_url.clone(), None, resource_name)
                .await?;

            if poll_status == StatusCode::NO_CONTENT {
                return Ok(());
            }

            if body.trim().is_empty() {
                if poll_status == StatusCode::OK {
                    return Ok(());
                }
                continue;
            }

            let value = self.parse_json(&body, resource_name)?;
            match value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("succeeded") => return Ok(()),
                Some("failed") | Some("canceled") => {
                    return Err(AzureTestRestError::new(
                        Some(poll_status),
                        format!(
                            "Azure {operation_name} for '{resource_name}' failed: {}",
                            value
                                .get("error")
                                .map(ToString::to_string)
                                .unwrap_or_else(|| "no error details".to_string())
                        ),
                    ));
                }
                _ => {}
            }
        }

        Err(AzureTestRestError::new(
            None,
            format!("Azure {operation_name} for '{resource_name}' did not finish within timeout"),
        ))
    }

    fn parse_json(
        &self,
        body: &str,
        resource_name: &str,
    ) -> Result<serde_json::Value, AzureTestRestError> {
        serde_json::from_str(body).map_err(|error| {
            AzureTestRestError::new(
                None,
                format!("Failed to parse Azure ARM JSON for '{resource_name}': {error}: {body}"),
            )
        })
    }
}

#[cfg(feature = "azure")]
fn azure_container_app_request(
    location: &str,
    managed_environment_id: &str,
    container_image: &str,
    registries: Vec<serde_json::Value>,
    identity: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut container_app = serde_json::json!({
        "location": location,
        "properties": {
            "environmentId": managed_environment_id,
            "managedEnvironmentId": managed_environment_id,
            "template": {
                "containers": [{
                    "name": "main",
                    "image": container_image,
                    "env": [],
                    "resources": {
                        "cpu": 0.5,
                        "memory": "1Gi",
                    },
                }],
                "scale": {
                    "minReplicas": 1,
                    "maxReplicas": 10,
                    "rules": [],
                },
            },
            "configuration": {
                "ingress": {
                    "external": true,
                    "targetPort": 8080,
                    "traffic": [{
                        "latestRevision": true,
                        "weight": 100,
                    }],
                    "transport": "Auto",
                },
                "registries": registries,
                "activeRevisionsMode": "Single",
            },
        },
        "tags": {
            "Environment": "Test",
            "Application": "alien-test",
        },
    });

    if let Some(identity) = identity {
        container_app["identity"] = identity;
    }

    container_app
}

#[cfg(feature = "azure")]
#[derive(Debug)]
struct AzureTestRestError {
    status: Option<StatusCode>,
    message: String,
}

#[cfg(feature = "azure")]
impl AzureTestRestError {
    fn new(status: Option<StatusCode>, message: String) -> Self {
        Self { status, message }
    }
}

#[cfg(feature = "azure")]
impl std::fmt::Display for AzureTestRestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

#[cfg(feature = "azure")]
impl std::error::Error for AzureTestRestError {}

#[cfg(feature = "azure")]
#[derive(Debug)]
struct StaticAzureAccessTokenCredential {
    token: String,
}

#[cfg(feature = "azure")]
#[async_trait]
impl TokenCredential for StaticAzureAccessTokenCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::Result<AccessToken> {
        if scopes.is_empty() {
            return Err(azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "no scopes specified",
            ));
        }

        Ok(AccessToken::new(
            self.token.clone(),
            OffsetDateTime::now_utc() + AzureDuration::days(365),
        ))
    }
}

#[cfg(feature = "azure")]
fn azure_credential_from_config(
    config: &AzureClientConfig,
) -> anyhow::Result<Arc<dyn TokenCredential>> {
    match &config.credentials {
        AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
            token: token.clone(),
        })),
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => ClientSecretCredential::new(
            &config.tenant_id,
            client_id.clone(),
            Secret::new(client_secret.clone()),
            Some(ClientSecretCredentialOptions::default()),
        )
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .map_err(|error| {
            anyhow::anyhow!("Failed to build Azure service principal credentials: {error}")
        }),
        other => anyhow::bail!(
            "alien-bindings Azure Container Apps live test setup supports service principal/access-token credentials only, got {other:?}"
        ),
    }
}

// --- Test implementations ---

/// Test function invoke functionality with various HTTP methods and payloads
#[rstest]
// #[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
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
// #[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
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
