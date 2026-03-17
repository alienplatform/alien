#![cfg(test)]

use alien_bindings::{
    traits::{BindingsProviderApi, Function, FunctionInvokeRequest},
    BindingsProvider,
};

#[cfg(feature = "grpc")]
use alien_bindings::{grpc::run_grpc_server, providers::grpc_provider::GrpcBindingsProvider};
use alien_core::bindings::{self, FunctionBinding};

// Import cloud clients for creating test resources
#[cfg(feature = "aws")]
use alien_aws_clients::lambda::{
    AddPermissionRequest, Cors, CreateFunctionRequest, CreateFunctionUrlConfigRequest,
    FunctionCode, LambdaApi, LambdaClient,
};
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
    async fn get_function(&self) -> Arc<dyn Function>;
    fn provider_name(&self) -> &'static str;
    fn get_test_endpoint(&self) -> String;
}

// --- gRPC Provider Context ---
#[cfg(feature = "grpc")]
struct GrpcProviderTestContext {
    function: Arc<dyn Function>,
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

        // Mock function binding for gRPC - this will simulate a local HTTP endpoint
        let server_binding =
            FunctionBinding::lambda("test-function".to_string(), "us-east-1".to_string());

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
            .load_function(GRPC_BINDING_NAME)
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
    async fn get_function(&self) -> Arc<dyn Function> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "grpc"
    }
    fn get_test_endpoint(&self) -> String {
        "test-function".to_string()
    }
}

// --- AWS Provider Context ---
#[cfg(feature = "aws")]
struct AwsProviderTestContext {
    function: Arc<dyn Function>,
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
        let lambda_client = LambdaClient::new(reqwest::Client::new(), aws_config);

        let image_uri = env::var("ALIEN_TEST_AWS_LAMBDA_IMAGE")
            .expect("ALIEN_TEST_AWS_LAMBDA_IMAGE must be set in .env.test");
        let role_arn = env::var("ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN")
            .unwrap_or_else(|_| format!("arn:aws:iam::{}:role/lambda-execution-role", account_id));

        // Create a unique function name
        let function_name = format!("alien-test-function-{}", Uuid::new_v4().simple());

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
                        info!("✅ Function is ready!");
                        break;
                    }
                    if attempts >= max_attempts {
                        panic!(
                            "Function didn't become ready within {} attempts",
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

        // Create function URL for HTTP access
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
            .expect("Failed to create function URL");

        info!("✅ Created function URL: {}", url_response.function_url);

        // Add resource-based policy so the function URL (AuthType=NONE) is publicly
        // invocable.  Ignore ResourceConflict (policy already exists from a prior run)
        // but panic on any other error so permission failures are caught immediately.
        let permission_request = AddPermissionRequest::builder()
            .statement_id("AllowFunctionUrlInvoke".to_string())
            .action("lambda:InvokeFunctionUrl".to_string())
            .principal("*".to_string())
            .function_url_auth_type("NONE".to_string())
            .build();

        match lambda_client
            .add_permission(&function_name, permission_request)
            .await
        {
            Ok(_) => {
                info!("✅ Added public invocation permission for function URL");
            }
            Err(e) if matches!(e.error, Some(ErrorData::RemoteResourceConflict { .. })) => {
                info!("ℹ️ Function URL permission already exists, continuing");
            }
            Err(e) => panic!("Failed to add function URL permission: {:?}", e),
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

        // IAM resource-based policy propagation can take 10-60 seconds.
        tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;

        let binding = FunctionBinding::lambda(function_name.clone(), region.clone());

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
            .load_function(binding_name)
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

        // First clean up function URLs
        for function_name in &urls_to_cleanup {
            match self
                .lambda_client
                .delete_function_url_config(function_name, None)
                .await
            {
                Ok(_) => info!("✅ Function URL {} deleted successfully", function_name),
                Err(e) => {
                    if !matches!(
                        e,
                        Error {
                            error: Some(ErrorData::RemoteResourceNotFound { .. }),
                            ..
                        }
                    ) {
                        warn!(
                            "Failed to delete function URL {} during cleanup: {:?}",
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
                Ok(_) => info!("✅ Function {} deleted successfully", function_name),
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
    async fn get_function(&self) -> Arc<dyn Function> {
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
    function: Arc<dyn Function>,
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

        let binding = FunctionBinding::cloud_run(
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
            .load_function(binding_name)
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
    async fn get_function(&self) -> Arc<dyn Function> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "gcp"
    }
    fn get_test_endpoint(&self) -> String {
        format!("{}/{}", self.location, self.service_name)
    }
}

// --- Azure Provider Context ---
#[cfg(feature = "azure")]
struct AzureProviderTestContext {
    function: Arc<dyn Function>,
    resource_group_name: String,
    container_app_name: String,
    container_apps_client: AzureContainerAppsClient,
    authorization_client: AzureAuthorizationClient,
    managed_identity_client: AzureManagedIdentityClient,
    long_running_operation_client: LongRunningOperationClient,
    managed_environment_id: String,
    location: String,
    container_image: String,
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

        let client_config = alien_azure_clients::AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id,
            region: Some("eastus".to_string()),
            credentials: alien_azure_clients::AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            },
            service_overrides: None,
        };

        let container_apps_client =
            AzureContainerAppsClient::new(reqwest::Client::new(), client_config.clone());

        let authorization_client =
            AzureAuthorizationClient::new(reqwest::Client::new(), client_config.clone());

        let managed_identity_client =
            AzureManagedIdentityClient::new(reqwest::Client::new(), client_config.clone());

        let long_running_operation_client =
            LongRunningOperationClient::new(reqwest::Client::new(), client_config.clone());

        // Get the existing managed environment to retrieve its ID
        let managed_environment = container_apps_client.get_managed_environment(&resource_group_name, &managed_environment_name).await
            .expect("Failed to get existing managed environment. Make sure ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME points to an existing managed environment.");

        let managed_environment_id = managed_environment
            .id
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

            // Create managed identity
            let managed_identity = Identity {
                location: "eastus".to_string(),
                tags: Default::default(),
                properties: None,
                id: None,
                name: None,
                type_: None,
                system_data: None,
            };

            let created_identity = managed_identity_client
                .create_or_update_user_assigned_identity(
                    &resource_group_name,
                    &identity_name,
                    &managed_identity,
                )
                .await
                .expect("Failed to create managed identity");

            let principal_id = created_identity
                .properties
                .as_ref()
                .and_then(|p| p.principal_id.clone())
                .expect("Managed identity should have a principal ID");

            let identity_resource_id = created_identity
                .id
                .as_ref()
                .expect("Managed identity should have a resource ID")
                .clone();

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

            // Build ACR resource scope
            let acr_scope = Scope::Resource {
                resource_group_name: resource_group_name.clone(),
                resource_provider: "Microsoft.ContainerRegistry".to_string(),
                parent_resource_path: None,
                resource_type: "registries".to_string(),
                resource_name: acr_name.to_string(),
            };

            // Create role assignment
            let assignment_id = Uuid::new_v4().to_string();
            let acr_pull_role_definition_id = "7f951dda-4ed3-4680-a7ca-43fe172d538d"; // AcrPull built-in role
            let role_definition_full_id = format!(
                "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
                subscription_id, acr_pull_role_definition_id
            );

            let role_assignment = RoleAssignment {
                properties: Some(RoleAssignmentProperties {
                    principal_id: principal_id.to_string(),
                    role_definition_id: role_definition_full_id,
                    principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                    scope: Some(acr_scope.to_scope_string(&authorization_client.client_config)),
                    condition: None,
                    condition_version: None,
                    delegated_managed_identity_resource_id: None,
                    description: Some(
                        "AcrPull role for Container App managed identity".to_string(),
                    ),
                    created_by: None,
                    created_on: None,
                    updated_by: None,
                    updated_on: None,
                }),
                id: None,
                name: None,
                type_: None,
            };

            let full_assignment_id =
                authorization_client.build_role_assignment_id(&acr_scope, assignment_id);

            let role_assignment_result = authorization_client
                .create_or_update_role_assignment_by_id(
                    full_assignment_id.clone(),
                    &role_assignment,
                )
                .await;

            if let Err(e) = role_assignment_result {
                if matches!(e.error, Some(ErrorData::RemoteResourceNotFound { .. })) {
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

                let registries = vec![RegistryCredentials {
                    server: Some(acr_server.to_string()),
                    identity: Some(identity_resource_id.clone()),
                    ..Default::default()
                }];

                // Create managed identity configuration for the container app
                let mut user_assigned_identities = std::collections::HashMap::new();
                user_assigned_identities.insert(
                    identity_resource_id.clone(),
                    UserAssignedIdentity::default(),
                );

                let identity = Some(ManagedServiceIdentity {
                    type_: ManagedServiceIdentityType::UserAssigned,
                    user_assigned_identities: Some(UserAssignedIdentities(
                        user_assigned_identities,
                    )),
                    principal_id: None,
                    tenant_id: None,
                });

                info!("✅ Configured managed identity and registry credentials");

                (registries, identity)
            }
        } else {
            info!("ℹ️ Using public container image, no ACR authentication needed");
            (vec![], None)
        };

        // Create the Container App
        let container_app = ContainerApp {
            location: "eastus".to_string(),
            identity,
            properties: Some(ContainerAppProperties {
                environment_id: Some(managed_environment_id.clone()),
                template: Some(Template {
                    containers: vec![
                        AzureContainer {
                            name: Some("main".to_string()),
                            image: Some(container_image.clone()),
                            env: vec![],
                            resources: Some(alien_azure_clients::models::container_apps::ContainerResources {
                                cpu: Some(0.5),
                                memory: Some("1Gi".to_string()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }
                    ],
                    scale: Some(Scale {
                        min_replicas: Some(1),
                        max_replicas: 10,
                        rules: vec![],
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                configuration: Some(Configuration {
                    ingress: Some(Ingress {
                        external: true,
                        target_port: Some(8080),
                        traffic: vec![
                            TrafficWeight {
                                latest_revision: true,
                                weight: Some(100),
                                ..Default::default()
                            }
                        ],
                        transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
                        ..Default::default()
                    }),
                    registries,
                    active_revisions_mode: alien_azure_clients::models::container_apps::ConfigurationActiveRevisionsMode::Single,
                    ..Default::default()
                }),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: Some(managed_environment_id.clone()),
                running_status: None,
                workload_profile_name: None,
                provisioning_state: None,
                event_stream_endpoint: None,
            }),
            tags: Default::default(),
            id: None,
            name: None,
            type_: None,
            managed_by: None,
            system_data: None,
            extended_location: None,
        };

        let create_result = container_apps_client
            .create_or_update_container_app(
                &resource_group_name,
                &container_app_name,
                &container_app,
            )
            .await
            .expect("Failed to create test Container App");

        // Wait for the ARM operation to complete
        create_result
            .wait_for_operation_completion(
                &long_running_operation_client,
                "CreateContainerApp",
                &container_app_name,
            )
            .await
            .expect("Failed to wait for Container App creation");

        info!("✅ Created Container App: {}", container_app_name);

        // Wait for container app to be ready
        let mut attempts = 0;
        let max_attempts = 12; // Increased from 6 to 12
        loop {
            attempts += 1;

            match container_apps_client
                .get_container_app(&resource_group_name, &container_app_name)
                .await
            {
                Ok(app) => {
                    if let Some(props) = &app.properties {
                        if let Some(state) = &props.provisioning_state {
                            info!(
                                "📊 Container app provisioning state: {:?} (attempt {}/{})",
                                state, attempts, max_attempts
                            );

                            if *state == alien_azure_clients::models::container_apps::ContainerAppPropertiesProvisioningState::Succeeded {
                                info!("✅ Container app is ready!");
                                break;
                            }

                            if *state == alien_azure_clients::models::container_apps::ContainerAppPropertiesProvisioningState::Failed {
                                panic!("❌ Container app provisioning failed");
                            }
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
        let created_app = container_apps_client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
            .expect("Failed to get created container app");

        let app_url = created_app
            .properties
            .and_then(|props| props.configuration)
            .and_then(|config| config.ingress)
            .and_then(|ingress| ingress.fqdn)
            .map(|fqdn| format!("https://{}", fqdn))
            .expect("Container app should have a valid FQDN after creation");

        let binding = FunctionBinding::container_app(
            subscription_id.clone(),
            resource_group_name.clone(),
            container_app_name.clone(),
            app_url,
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert("AZURE_TENANT_ID".to_string(), client_config.tenant_id);

        // Extract credentials based on the type
        let (azure_client_id, azure_client_secret) = match &client_config.credentials {
            alien_azure_clients::AzureCredentials::ServicePrincipal {
                client_id,
                client_secret,
            } => (client_id.clone(), client_secret.clone()),
            alien_azure_clients::AzureCredentials::AccessToken { .. } => {
                panic!("AccessToken credentials not supported in function binding tests")
            }
            alien_azure_clients::AzureCredentials::WorkloadIdentity { client_id, .. } => {
                panic!("WorkloadIdentity credentials not fully supported in function binding tests, client_id: {}", client_id)
            }
        };

        env_map.insert("AZURE_CLIENT_ID".to_string(), azure_client_id);
        env_map.insert("AZURE_CLIENT_SECRET".to_string(), azure_client_secret);
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
            .load_function(binding_name)
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
            container_apps_client,
            authorization_client,
            managed_identity_client,
            long_running_operation_client,
            managed_environment_id,
            location: "eastus".to_string(),
            container_image,
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
                .authorization_client
                .delete_role_assignment_by_id(assignment_id.clone())
                .await
            {
                Ok(_) => info!("✅ Role assignment {} deleted successfully", assignment_id),
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
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
                .managed_identity_client
                .delete_user_assigned_identity(&self.resource_group_name, &identity_name)
                .await
            {
                Ok(_) => info!("✅ Managed identity {} deleted successfully", identity_name),
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
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
                .container_apps_client
                .delete_container_app(&self.resource_group_name, &container_app_name)
                .await
            {
                Ok(_) => info!(
                    "✅ Container app {} deleted successfully",
                    container_app_name
                ),
                Err(err) if matches!(err.error, Some(ErrorData::RemoteResourceNotFound { .. })) => {
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
    async fn get_function(&self) -> Arc<dyn Function> {
        self.function.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
    fn get_test_endpoint(&self) -> String {
        format!("{}/{}", self.resource_group_name, self.container_app_name)
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
    let get_request = FunctionInvokeRequest {
        method: "GET".to_string(),
        path: "/get".to_string(),
        headers: std::collections::BTreeMap::new(),
        body: Vec::new(),
        timeout: Some(Duration::from_secs(60)),
        target_function: test_endpoint.clone(),
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

    let post_request = FunctionInvokeRequest {
        method: "POST".to_string(),
        path: "/post".to_string(),
        headers: post_headers,
        body: b"{\"message\": \"Hello from Alien!\"}".to_vec(),
        timeout: Some(Duration::from_secs(60)),
        target_function: test_endpoint,
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

    info!("[{}] Function invoke test completed", provider_name);
}

/// Test getting function URL and making direct HTTP request
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

    // Get function URL
    let function_url = function
        .get_function_url()
        .await
        .expect(&format!(
            "[{}] Should be able to get function URL",
            provider_name
        ))
        .expect(&format!(
            "[{}] Function should have a valid URL",
            provider_name
        ));

    info!(
        "[{}] Function URL retrieved: {}",
        provider_name, function_url
    );

    // Validate URL format
    assert!(
        function_url.starts_with("http://") || function_url.starts_with("https://"),
        "[{}] Invalid URL format: {}",
        provider_name,
        function_url
    );

    // Make direct HTTP request to the function URL with retry for IAM propagation
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
            provider_name, attempt, max_retries, request_url, status,
            &body[..body.len().min(200)]
        );

        if status.is_success() || status.is_redirection() {
            info!(
                "[{}] Response body length: {} bytes",
                provider_name,
                body.len()
            );
            info!("[{}] Function HTTP access test completed", provider_name);
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
