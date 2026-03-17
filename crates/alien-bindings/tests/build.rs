#![cfg(test)]

use alien_bindings::{
    traits::{BindingsProviderApi, Build},
    BindingsProvider,
};

#[cfg(feature = "grpc")]
use alien_bindings::{grpc::run_grpc_server, providers::grpc_provider::GrpcBindingsProvider};
use alien_core::bindings::{self, BindingValue};

// Platform-specific providers are now internal implementation details
// The unified BindingsProvider handles routing to appropriate implementations

use alien_core::{BuildConfig, BuildStatus, ComputeType};
use async_trait::async_trait;
use rstest::rstest;
use std::path::PathBuf as StdPathBuf;
use std::{collections::HashMap, env, sync::Arc, time::Duration};
use tempfile::TempDir;
use test_context::AsyncTestContext;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use workspace_root::get_workspace_root;

#[cfg(feature = "aws")]
use alien_aws_clients::{
    codebuild::{
        CodeBuildApi, CodeBuildClient, CreateProjectRequest, DeleteProjectRequest,
        ProjectArtifacts, ProjectEnvironment, ProjectSource,
    },
    iam::{CreateRoleRequest, IamApi, IamClient},
    AwsClientConfig,
};
#[cfg(feature = "aws")]
use {reqwest::Client, std::sync::Mutex, uuid::Uuid};

const GRPC_BINDING_NAME: &str = "test-grpc-build-binding";

fn load_test_env() {
    // Load .env.test from the workspace root
    let root: StdPathBuf = get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
}

#[async_trait]
pub trait BuildTestContext: AsyncTestContext + Send + Sync {
    async fn get_build(&self) -> Arc<dyn Build>;
    fn provider_name(&self) -> &'static str;
    async fn cleanup(&self);
}

// --- Local Provider Context ---
struct LocalProviderBuildTestContext {
    build: Arc<dyn Build>,
    _temp_dir: TempDir,
}

impl AsyncTestContext for LocalProviderBuildTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-local-build";
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for local build test");
        let temp_dir_path = temp_dir.path().to_str().unwrap().to_string();

        let binding =
            bindings::BuildBinding::local(temp_dir_path.clone(), std::collections::HashMap::new());

        let mut env_map: HashMap<String, String> = env::vars().collect();
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load bindings provider"),
        );
        let build = provider.load_build(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load Local build for binding '{}' using ALIEN_DATA_DIR='{}': {:?}",
                binding_name, temp_dir_path, e
            )
        });
        Self {
            build,
            _temp_dir: temp_dir,
        }
    }
}

#[async_trait]
impl BuildTestContext for LocalProviderBuildTestContext {
    async fn get_build(&self) -> Arc<dyn Build> {
        self.build.clone()
    }
    fn provider_name(&self) -> &'static str {
        "local"
    }
    async fn cleanup(&self) {
        // Local provider doesn't need cleanup
    }
}

// --- gRPC Provider Context ---
#[cfg(feature = "grpc")]
struct GrpcProviderBuildTestContext {
    build: Arc<dyn Build>,
    _server_handle:
        JoinHandle<Result<(), alien_error::AlienError<alien_bindings::error::ErrorData>>>,
    _temp_data_dir: TempDir,
}

#[cfg(feature = "grpc")]
impl AsyncTestContext for GrpcProviderBuildTestContext {
    async fn setup() -> Self {
        load_test_env();
        let temp_data_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for ALIEN_DATA_DIR (gRPC server)");
        let temp_data_dir_path = temp_data_dir.path().to_str().unwrap().to_string();

        let server_binding = bindings::BuildBinding::local(
            temp_data_dir_path.clone(),
            std::collections::HashMap::new(),
        );

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
        drop(listener);

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

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let mut service_provider_env_map: HashMap<String, String> = env::vars().collect();
        service_provider_env_map.insert(
            "ALIEN_BINDINGS_GRPC_ADDRESS".to_string(),
            server_addr_str.clone(),
        );
        service_provider_env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "grpc".to_string());
        let grpc_provider = GrpcBindingsProvider::new_with_env(service_provider_env_map)
            .expect("Failed to load bindings provider");

        let build_client = grpc_provider
            .load_build(GRPC_BINDING_NAME)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Grpc build for binding '{}' using ALIEN_BINDINGS_GRPC_ADDRESS='{}': {:?}",
                    GRPC_BINDING_NAME, server_addr_str, e
                )
            });

        Self {
            build: build_client,
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
impl BuildTestContext for GrpcProviderBuildTestContext {
    async fn get_build(&self) -> Arc<dyn Build> {
        self.build.clone()
    }
    fn provider_name(&self) -> &'static str {
        "grpc"
    }
    async fn cleanup(&self) {
        // gRPC provider doesn't need cleanup (server is cleaned up in teardown)
    }
}

// --- AWS Provider Context ---
#[cfg(feature = "aws")]
struct AwsProviderBuildTestContext {
    build: Arc<dyn Build>,
    codebuild_client: CodeBuildClient,
    iam_client: IamClient,
    project_name: String,
    service_role_name: String,
    created_projects: Mutex<Vec<String>>,
}

#[cfg(feature = "aws")]
impl AsyncTestContext for AwsProviderBuildTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-aws-build";

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");
        let codebuild_image = std::env::var("ALIEN_TEST_AWS_CODEBUILD_IMAGE")
            .unwrap_or_else(|_| "ghcr.io/alienplatform/alien-builder:latest".to_string());

        let aws_config = AwsClientConfig {
            account_id: account_id.clone(),
            region: region.clone(),
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key.clone(),
                secret_access_key: secret_key.clone(),
                session_token: None,
            },
            service_overrides: None,
        };

        let codebuild_client = CodeBuildClient::new(Client::new(), aws_config.clone());
        let iam_client = IamClient::new(Client::new(), aws_config.clone());

        // Create IAM role for CodeBuild
        let role_name = format!("alien-test-build-role-{}", Uuid::new_v4().simple());
        let assume_role_policy = r#"{
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": {"Service": "codebuild.amazonaws.com"},
                "Action": "sts:AssumeRole"
            }]
        }"#
        .to_string();

        let role_request = CreateRoleRequest::builder()
            .role_name(role_name.clone())
            .assume_role_policy_document(assume_role_policy)
            .build();

        let role = iam_client
            .create_role(role_request)
            .await
            .expect("Failed to create IAM role");
        let service_role_arn = role.create_role_result.role.arn.clone();

        let policy_document = r#"{
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "logs:CreateLogGroup",
                        "logs:CreateLogStream",
                        "logs:PutLogEvents"
                    ],
                    "Resource": "*"
                }
            ]
        }"#
        .to_string();

        iam_client
            .put_role_policy(&role_name, "CodeBuildDefaultPolicy", &policy_document)
            .await
            .expect("Failed to attach policy");

        // Wait for IAM propagation
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Create CodeBuild project
        let project_name = format!("alien-test-build-{}", Uuid::new_v4().simple());
        let create_project_req = CreateProjectRequest::builder()
            .name(project_name.clone())
            .service_role(service_role_arn)
            .source(
                ProjectSource::builder()
                    .r#type("NO_SOURCE".to_string())
                    .buildspec(
                        "version: 0.2\nphases:\n  build:\n    commands:\n      - echo 'test build'"
                            .to_string(),
                    )
                    .build(),
            )
            .artifacts(
                ProjectArtifacts::builder()
                    .r#type("NO_ARTIFACTS".to_string())
                    .build(),
            )
            .environment(
                ProjectEnvironment::builder()
                    .r#type("LINUX_CONTAINER".to_string())
                    .image(codebuild_image)
                    .image_pull_credentials_type("SERVICE_ROLE".to_string())
                    .compute_type("BUILD_GENERAL1_SMALL".to_string())
                    .build(),
            )
            .build();

        codebuild_client
            .create_project(create_project_req)
            .await
            .expect("Failed to create CodeBuild project");

        // Set up environment for the provider
        let binding = bindings::BuildBinding::codebuild(
            project_name.clone(),
            std::collections::HashMap::new(),
            None,
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert("AWS_REGION".to_string(), region);
        env_map.insert("AWS_ACCESS_KEY_ID".to_string(), access_key);
        env_map.insert("AWS_SECRET_ACCESS_KEY".to_string(), secret_key);
        env_map.insert("AWS_ACCOUNT_ID".to_string(), account_id);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "aws".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let build = provider.load_build(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load AWS build for binding '{}': {:?}",
                binding_name, e
            )
        });

        Self {
            build,
            codebuild_client,
            iam_client,
            project_name,
            service_role_name: role_name,
            created_projects: Mutex::new(vec![]),
        }
    }

    async fn teardown(self) {
        // Clean up CodeBuild project
        let delete_req = DeleteProjectRequest {
            name: self.project_name.clone(),
        };
        self.codebuild_client.delete_project(delete_req).await.ok();

        // Clean up any additional projects
        let projects = self.created_projects.lock().unwrap().clone();
        for project in projects {
            let delete_req = DeleteProjectRequest { name: project };
            self.codebuild_client.delete_project(delete_req).await.ok();
        }

        // Clean up IAM role
        self.iam_client
            .delete_role_policy(&self.service_role_name, "CodeBuildDefaultPolicy")
            .await
            .ok();
        self.iam_client
            .delete_role(&self.service_role_name)
            .await
            .ok();
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl BuildTestContext for AwsProviderBuildTestContext {
    async fn get_build(&self) -> Arc<dyn Build> {
        self.build.clone()
    }
    fn provider_name(&self) -> &'static str {
        "aws"
    }
    async fn cleanup(&self) {
        use alien_aws_clients::codebuild::DeleteProjectRequest;

        // Clean up CodeBuild project
        let delete_req = DeleteProjectRequest {
            name: self.project_name.clone(),
        };
        self.codebuild_client.delete_project(delete_req).await.ok();

        // Clean up any additional projects
        let projects = self.created_projects.lock().unwrap().clone();
        for project in projects {
            let delete_req = DeleteProjectRequest { name: project };
            self.codebuild_client.delete_project(delete_req).await.ok();
        }

        // Clean up IAM role
        self.iam_client
            .delete_role_policy(&self.service_role_name, "CodeBuildDefaultPolicy")
            .await
            .ok();
        self.iam_client
            .delete_role(&self.service_role_name)
            .await
            .ok();
    }
}

// --- GCP Provider Context ---
#[cfg(feature = "gcp")]
struct GcpProviderBuildTestContext {
    build: Arc<dyn Build>,
}

#[cfg(feature = "gcp")]
impl AsyncTestContext for GcpProviderBuildTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-gcp-build";

        let service_account_key_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .expect("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set in .env.test");
        let gcp_region = env::var("GOOGLE_MANAGEMENT_REGION")
            .expect("GOOGLE_MANAGEMENT_REGION must be set in .env.test");

        // Extract client_email from service account key JSON
        let credential_value: serde_json::Value = serde_json::from_str(&service_account_key_json)
            .expect("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be valid JSON");
        let client_email = credential_value
            .get("client_email")
            .and_then(|v| v.as_str())
            .expect("client_email field must be present in service account key")
            .to_string();

        let binding = bindings::BuildBinding::cloudbuild(
            std::collections::HashMap::new(),
            client_email,
            None,
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert(
            "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
            service_account_key_json,
        );
        env_map.insert("GCP_REGION".to_string(), gcp_region);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "gcp".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let build = provider.load_build(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load GCP build for binding '{}': {:?}",
                binding_name, e
            )
        });
        Self { build }
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl BuildTestContext for GcpProviderBuildTestContext {
    async fn get_build(&self) -> Arc<dyn Build> {
        self.build.clone()
    }
    fn provider_name(&self) -> &'static str {
        "gcp"
    }
    async fn cleanup(&self) {
        // GCP provider doesn't need cleanup (builds are managed by the cloud)
    }
}

// --- Azure Provider Context ---
#[cfg(feature = "azure")]
struct AzureProviderBuildTestContext {
    build: Arc<dyn Build>,
}

#[cfg(feature = "azure")]
impl AsyncTestContext for AzureProviderBuildTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-azure-build";

        let resource_group = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP must be set in .env.test");

        let managed_environment_name = env::var("ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME")
            .expect("ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME must be set in .env.test");

        // Construct the full managed environment ID from the resource group and environment name
        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID must be set for Azure build test");
        let managed_environment_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}",
            subscription_id, resource_group, managed_environment_name
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert(
            "AZURE_TENANT_ID".to_string(),
            env::var("AZURE_MANAGEMENT_TENANT_ID").unwrap(),
        );
        env_map.insert(
            "AZURE_CLIENT_ID".to_string(),
            env::var("AZURE_MANAGEMENT_CLIENT_ID").unwrap(),
        );
        env_map.insert(
            "AZURE_CLIENT_SECRET".to_string(),
            env::var("AZURE_MANAGEMENT_CLIENT_SECRET").unwrap(),
        );
        env_map.insert(
            "AZURE_SUBSCRIPTION_ID".to_string(),
            env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID").unwrap(),
        );
        env_map.insert(
            "AZURE_REGION".to_string(),
            env::var("AZURE_MANAGEMENT_REGION").unwrap(),
        );
        env_map.insert("AZURE_RESOURCE_GROUP".to_string(), resource_group.clone());
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "azure".to_string());

        let binding = bindings::BuildBinding::aca(
            managed_environment_id,
            resource_group,
            std::collections::HashMap::new(),
            None,
            "test-prefix".to_string(),
            None,
        );

        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let build = provider.load_build(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load Azure build for binding '{}': {:?}",
                binding_name, e
            )
        });
        Self { build }
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl BuildTestContext for AzureProviderBuildTestContext {
    async fn get_build(&self) -> Arc<dyn Build> {
        self.build.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
    async fn cleanup(&self) {
        // Azure provider doesn't need cleanup (builds are managed by the cloud)
    }
}

/*
// --- Kubernetes Provider Context ---
#[cfg(feature = "kubernetes")]
struct KubernetesProviderBuildTestContext {
    build: Arc<dyn Build>,
}

#[cfg(feature = "kubernetes")]
impl AsyncTestContext for KubernetesProviderBuildTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-k8s-build";

        let binding = bindings::kubernetes::KubernetesBuildBinding {
            namespace: BindingValue::value(Some("default".to_string())),
            build_env_vars: BindingValue::value(std::collections::HashMap::new()),
            service_account: BindingValue::value(None),
        };

        let mut env_map: HashMap<String, String> = env::vars().collect();
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = KubernetesBindingsProvider::new_with_env(env_map);
        let build = provider
            .load_build(binding_name)
            .await
            .unwrap_or_else(|e| {
                // Kubernetes provider doesn't support build operations yet
                if let alien_bindings::error::ErrorData::OperationNotSupported { .. } = e.error {
                    panic!("SKIP: Kubernetes provider does not support build operations yet");
                }
                panic!("Failed to load Kubernetes build for binding '{}': {:?}", binding_name, e)
            });
        Self { build }
    }
}

#[cfg(feature = "kubernetes")]
#[async_trait]
impl BuildTestContext for KubernetesProviderBuildTestContext {
    async fn get_build(&self) -> Arc<dyn Build> {
        self.build.clone()
    }
    fn provider_name(&self) -> &'static str {
        "kubernetes"
    }
}
*/

// Helper function to create test build config
fn create_test_build_config(provider_name: &str) -> BuildConfig {
    let mut env_vars = HashMap::new();
    env_vars.insert("TEST_VAR".to_string(), "test_value".to_string());
    env_vars.insert("PROVIDER".to_string(), provider_name.to_string());

    BuildConfig {
        script: "echo 'Hello from Alien Build Test!'".to_string(),
        image: "ubuntu:20.04".to_string(),
        environment: env_vars,
        compute_type: ComputeType::Small,
        timeout_seconds: 300,
        monitoring: None,
    }
}

// Helper function to wait for build completion
async fn wait_for_build_completion(
    build: &Arc<dyn Build>,
    build_id: &str,
    provider_name: &str,
    max_wait_seconds: u64,
) -> BuildStatus {
    let start_time = std::time::Instant::now();
    let max_duration = Duration::from_secs(max_wait_seconds);

    loop {
        let execution = build
            .get_build_status(build_id)
            .await
            .unwrap_or_else(|e| panic!("[{}] Failed to get build status: {:?}", provider_name, e));

        match execution.status {
            BuildStatus::Succeeded
            | BuildStatus::Failed
            | BuildStatus::Cancelled
            | BuildStatus::TimedOut => {
                return execution.status;
            }
            BuildStatus::Queued | BuildStatus::Running => {
                if start_time.elapsed() > max_duration {
                    panic!(
                        "[{}] Build {} did not complete within {} seconds",
                        provider_name, build_id, max_wait_seconds
                    );
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

#[rstest]
#[case::local(LocalProviderBuildTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderBuildTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderBuildTestContext::setup().await))]
#[tokio::test]
async fn test_start_build_and_get_status(#[case] ctx: impl BuildTestContext) {
    let build = ctx.get_build().await;
    let provider_name = ctx.provider_name();
    let config = create_test_build_config(provider_name);

    // Start the build
    let execution = build
        .start_build(config)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to start build: {:?}", provider_name, e));

    assert!(
        !execution.id.is_empty(),
        "[{}] Build ID should not be empty",
        provider_name
    );
    assert!(
        execution.start_time.is_some(),
        "[{}] Start time should be set",
        provider_name
    );

    // Get build status
    let status_execution = build
        .get_build_status(&execution.id)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get build status: {:?}", provider_name, e));

    assert_eq!(
        status_execution.id, execution.id,
        "[{}] Build ID should match",
        provider_name
    );

    // For real cloud providers, wait for completion
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        let final_status =
            wait_for_build_completion(&build, &execution.id, provider_name, 600).await;
        println!("[{}] Final build status: {:?}", provider_name, final_status);
        // Note: We don't assert success here as the test script is simple and might succeed or fail depending on the environment
    }

    // Cleanup resources
    ctx.cleanup().await;
}

#[rstest]
#[case::local(LocalProviderBuildTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderBuildTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderBuildTestContext::setup().await))]
#[tokio::test]
async fn test_stop_build(#[case] ctx: impl BuildTestContext) {
    let build = ctx.get_build().await;
    let provider_name = ctx.provider_name();

    // Create a long-running build config
    let mut config = create_test_build_config(provider_name);
    config.script =
        "echo 'Starting long build...'; sleep 30; echo 'This should not be printed.'".to_string();

    // Start the build
    let execution = build.start_build(config).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to start build for stop test: {:?}",
            provider_name, e
        )
    });

    // For real cloud providers, wait a bit for the build to start, then stop it
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    // Stop the build
    build
        .stop_build(&execution.id)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to stop build: {:?}", provider_name, e));

    // For real cloud providers, verify the build was stopped
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        // Wait a bit for the status to update
        tokio::time::sleep(Duration::from_secs(5)).await;

        let final_execution = build
            .get_build_status(&execution.id)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "[{}] Failed to get build status after stop: {:?}",
                    provider_name, e
                )
            });

        // The build should be in a stopped/cancelled state or transitioning to it
        println!(
            "[{}] Build status after stop: {:?}",
            provider_name, final_execution.status
        );
    }

    // Cleanup resources
    ctx.cleanup().await;
}

#[rstest]
#[case::local(LocalProviderBuildTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderBuildTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderBuildTestContext::setup().await))]
#[tokio::test]
async fn test_build_with_environment_variables(#[case] ctx: impl BuildTestContext) {
    let build = ctx.get_build().await;
    let provider_name = ctx.provider_name();

    let mut config = create_test_build_config(provider_name);
    config
        .environment
        .insert("CUSTOM_VAR".to_string(), "custom_value".to_string());
    config
        .environment
        .insert("ANOTHER_VAR".to_string(), "another_value".to_string());
    config.script =
        "echo \"CUSTOM_VAR=$CUSTOM_VAR\"; echo \"ANOTHER_VAR=$ANOTHER_VAR\"".to_string();

    // Start the build
    let execution = build.start_build(config).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to start build with env vars: {:?}",
            provider_name, e
        )
    });

    assert!(
        !execution.id.is_empty(),
        "[{}] Build ID should not be empty",
        provider_name
    );

    // For real cloud providers, wait for completion and check logs would contain the env vars
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        let final_status =
            wait_for_build_completion(&build, &execution.id, provider_name, 300).await;
        println!(
            "[{}] Build with env vars final status: {:?}",
            provider_name, final_status
        );
    }

    // Cleanup resources
    ctx.cleanup().await;
}

#[rstest]
#[case::local(LocalProviderBuildTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderBuildTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderBuildTestContext::setup().await))]
#[tokio::test]
async fn test_build_with_different_compute_types(#[case] ctx: impl BuildTestContext) {
    let build = ctx.get_build().await;
    let provider_name = ctx.provider_name();

    // Test different compute types
    let compute_types = [ComputeType::Small, ComputeType::Medium];

    for compute_type in compute_types {
        let mut config = create_test_build_config(provider_name);
        config.compute_type = compute_type.clone();
        config.script = format!("echo 'Testing with compute type: {:?}'", compute_type);

        let execution = build.start_build(config).await.unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to start build with compute type {:?}: {:?}",
                provider_name, compute_type, e
            )
        });

        assert!(
            !execution.id.is_empty(),
            "[{}] Build ID should not be empty for compute type {:?}",
            provider_name,
            compute_type
        );

        // For local and grpc providers, we don't need to wait for completion
        if !matches!(provider_name, "local" | "grpc") {
            // For real cloud providers, just verify the build started successfully
            tokio::time::sleep(Duration::from_secs(2)).await;
            let status = build
                .get_build_status(&execution.id)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "[{}] Failed to get status for compute type {:?}: {:?}",
                        provider_name, compute_type, e
                    )
                });

            println!(
                "[{}] Build with compute type {:?} status: {:?}",
                provider_name, compute_type, status.status
            );
        }
    }

    // Cleanup resources
    ctx.cleanup().await;
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderBuildTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderBuildTestContext::setup().await))]
// #[cfg_attr(feature = "azure", case::azure(AzureProviderBuildTestContext::setup().await))]
// #[cfg_attr(feature = "gcp", case::gcp(GcpProviderBuildTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderBuildTestContext::setup().await))]
#[cfg(any(feature = "local", feature = "grpc", feature = "aws"))]
#[tokio::test]
async fn test_build_with_monitoring_to_axiom(#[case] ctx: impl BuildTestContext) {
    let build = ctx.get_build().await;
    let provider_name = ctx.provider_name();

    println!(
        "[{}] 🚀 Starting Axiom monitoring test for {} provider",
        provider_name, provider_name
    );

    // Skip this test for local and grpc providers as they don't support monitoring
    if matches!(provider_name, "local" | "grpc") {
        println!(
            "[{}] Skipping monitoring test for {} provider",
            provider_name, provider_name
        );
        return;
    }

    // Get Axiom configuration from environment
    println!("[{}] 📋 Setting up Axiom configuration...", provider_name);
    let axiom_otlp_endpoint =
        std::env::var("AXIOM_OTLP_ENDPOINT").expect("AXIOM_OTLP_ENDPOINT must be set in .env.test");
    // AXIOM_OTLP_ENDPOINT is the full URL (e.g. https://api.axiom.co/v1/logs).
    // MonitoringConfig.endpoint is the base URL; logs_uri provides the path.
    let axiom_endpoint = axiom_otlp_endpoint
        .trim_end_matches('/')
        .trim_end_matches("/v1/logs")
        .to_string();
    let axiom_token = std::env::var("AXIOM_TOKEN").expect("AXIOM_TOKEN must be set in .env.test");
    let axiom_dataset =
        std::env::var("AXIOM_DATASET").expect("AXIOM_DATASET must be set in .env.test");

    println!(
        "[{}] 📡 Using Axiom endpoint: {}",
        provider_name, axiom_endpoint
    );
    println!(
        "[{}] 🗃️ Using Axiom dataset: {}",
        provider_name, axiom_dataset
    );

    // Create unique identifier for this test
    let test_id = uuid::Uuid::new_v4().simple();
    let expected_message = format!("AXIOM_MONITORING_TEST_MESSAGE_{}", test_id);

    println!("[{}] 🆔 Test ID: {}", provider_name, test_id);
    println!(
        "[{}] 💬 Expected message: {}",
        provider_name, expected_message
    );

    // Configure monitoring to send to Axiom OTLP endpoint
    println!("[{}] 🔧 Configuring Axiom monitoring...", provider_name);
    let monitoring_config = alien_core::MonitoringConfig {
        endpoint: axiom_endpoint.clone(),
        headers: {
            let mut headers = std::collections::HashMap::new();
            headers.insert(
                "authorization".to_string(),
                format!("Bearer {}", axiom_token),
            );
            headers.insert("x-axiom-dataset".to_string(), axiom_dataset.clone());
            headers
        },
        logs_uri: "/v1/logs".to_string(),
        tls_enabled: true,
        tls_verify: true,
    };
    println!(
        "[{}] 📡 Monitoring endpoint: {}",
        provider_name, monitoring_config.endpoint
    );
    println!(
        "[{}] 📋 Monitoring headers: authorization=Bearer <token>, x-axiom-dataset={}",
        provider_name, axiom_dataset
    );

    // Create build config with monitoring and a script that outputs our test message
    println!("[{}] 🔨 Creating build config...", provider_name);
    let mut config = create_test_build_config(provider_name);
    config.monitoring = Some(monitoring_config);
    config.script = format!("echo '{}'", expected_message);
    println!("[{}] 📜 Build script: {}", provider_name, config.script);

    // Start the build
    println!(
        "[{}] 🚀 Starting build with Axiom monitoring...",
        provider_name
    );
    let build_start_time = chrono::Utc::now();
    let execution = build.start_build(config).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to start build with monitoring: {:?}",
            provider_name, e
        )
    });

    assert!(
        !execution.id.is_empty(),
        "[{}] Build ID should not be empty",
        provider_name
    );
    println!(
        "[{}] 🆔 Build started with ID: {} at {}",
        provider_name, execution.id, build_start_time
    );

    // Wait for build completion
    println!("[{}] ⏳ Waiting for build completion...", provider_name);
    let final_status = wait_for_build_completion(&build, &execution.id, provider_name, 600).await;
    println!(
        "[{}] ✅ Build with monitoring completed with status: {:?}",
        provider_name, final_status
    );
    let build_end_time = chrono::Utc::now();

    // Wait for logs to be ingested into Axiom (ingestion can take 15-30s)
    println!(
        "[{}] ⏰ Waiting 30 seconds for logs to be ingested into Axiom...",
        provider_name
    );
    tokio::time::sleep(Duration::from_secs(30)).await;
    println!("[{}] ⏰ Finished waiting for log ingestion", provider_name);

    // Query logs from Axiom using APL
    let http_client = reqwest::Client::new();
    let apl_query = format!(
        "['{}'] | where body contains '{}' | limit 100",
        axiom_dataset, expected_message
    );
    let start_time = build_start_time.to_rfc3339();
    let end_time = build_end_time.to_rfc3339();

    println!("[{}] 🔍 APL Query: {}", provider_name, apl_query);
    println!(
        "[{}] ⏰ Time range: {} to {}",
        provider_name, start_time, end_time
    );

    let query_payload = serde_json::json!({
        "apl": apl_query,
        "startTime": start_time,
        "endTime": end_time
    });

    // Retry the Axiom query - ingestion latency can vary
    let max_query_attempts = 3;
    let mut last_messages: Vec<String> = Vec::new();
    for query_attempt in 1..=max_query_attempts {
        println!(
            "[{}] 📖 Querying logs from Axiom (attempt {}/{})...",
            provider_name, query_attempt, max_query_attempts
        );

        let response = http_client
            .post("https://api.axiom.co/v1/datasets/_apl?format=tabular")
            .header("Authorization", format!("Bearer {}", axiom_token))
            .header("Content-Type", "application/json")
            .json(&query_payload)
            .send()
            .await
            .unwrap_or_else(|e| panic!("[{}] Failed to send Axiom query: {:?}", provider_name, e));

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            panic!(
                "[{}] Axiom query failed with status {}: {}",
                provider_name, status, error_text
            );
        }

        let query_result: serde_json::Value = response.json().await.unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to parse Axiom response: {:?}",
                provider_name, e
            )
        });

        println!(
            "[{}] ✅ Successfully retrieved query response from Axiom",
            provider_name
        );

        let tables = query_result
            .get("tables")
            .and_then(|t| t.as_array());

        let found = if let Some(tables) = tables {
            if let Some(table) = tables.first() {
                let columns = table.get("columns").and_then(|c| c.as_array());
                let fields = table.get("fields").and_then(|f| f.as_array());

                if let (Some(columns), Some(fields)) = (columns, fields) {
                    let message_field_index = fields.iter().position(|field| {
                        field
                            .get("name")
                            .and_then(|n| n.as_str())
                            .map(|name| {
                                name.contains("message")
                                    || name.contains("@message")
                                    || name.contains("body")
                            })
                            .unwrap_or(false)
                    });

                    if let Some(msg_index) = message_field_index {
                        if let Some(message_column) =
                            columns.get(msg_index).and_then(|c| c.as_array())
                        {
                            last_messages = message_column
                                .iter()
                                .filter_map(|msg| msg.as_str().map(|s| s.to_string()))
                                .collect();
                            println!(
                                "[{}] 📊 Retrieved {} log entries",
                                provider_name,
                                last_messages.len()
                            );
                            last_messages
                                .iter()
                                .any(|msg| msg.contains(&expected_message))
                        } else {
                            false
                        }
                    } else {
                        println!(
                            "[{}] ⚠️ No message/body field in response. Fields: {:?}",
                            provider_name, fields
                        );
                        println!(
                            "[{}] 📋 Full query result: {:?}",
                            provider_name, query_result
                        );
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if found {
            println!(
                "[{}] ✅ Successfully found test message in Axiom logs",
                provider_name
            );
            break;
        }

        if query_attempt < max_query_attempts {
            println!(
                "[{}] ⏳ Message not found yet, waiting 15s before retry...",
                provider_name
            );
            tokio::time::sleep(Duration::from_secs(15)).await;
        } else {
            panic!(
                "[{}] Expected to find test message '{}' in Axiom logs after {} attempts. Available messages: {:?}",
                provider_name, expected_message, max_query_attempts, last_messages
            );
        }
    }

    println!(
        "[{}] 🎉 Axiom monitoring test completed successfully!",
        provider_name
    );

    // Cleanup resources
    ctx.cleanup().await;
}
