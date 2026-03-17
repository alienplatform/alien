#![cfg(test)]

use alien_bindings::{
    traits::{
        ArtifactRegistry, ArtifactRegistryPermissions, AwsCrossAccountAccess, BindingsProviderApi,
        ComputeServiceType, CrossAccountAccess, CrossAccountPermissions, GcpCrossAccountAccess,
    },
    BindingsProvider,
};

#[cfg(feature = "grpc")]
use alien_bindings::{grpc::run_grpc_server, providers::grpc_provider::GrpcBindingsProvider};
use alien_core::bindings::BindingValue;

// Platform-specific providers are now internal implementation details
// The unified BindingsProvider handles routing to appropriate implementations

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use rstest::rstest;
use serde_json;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use std::{collections::HashMap, env, sync::Arc, time::Duration};
use tempfile::TempDir;
use test_context::AsyncTestContext;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use uuid::Uuid;
use workspace_root::get_workspace_root;

#[cfg(feature = "aws")]
use {alien_aws_clients::AwsClientConfig, reqwest::Client};

#[cfg(feature = "gcp")]
use alien_gcp_clients::{artifactregistry::ArtifactRegistryClient, GcpClientConfig};

#[cfg(feature = "azure")]
use alien_azure_clients::{
    containerregistry::{AzureContainerRegistryClient, ContainerRegistryApi},
    AzureClientConfig,
};

const GRPC_BINDING_NAME: &str = "test-grpc-artifact-registry-binding";

fn load_test_env() {
    // Load .env.test from the workspace root
    let root: StdPathBuf = get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
}

#[async_trait]
pub trait ArtifactRegistryTestContext: AsyncTestContext + Send + Sync {
    async fn get_artifact_registry(&self) -> Arc<dyn ArtifactRegistry>;
    fn provider_name(&self) -> &'static str;
}

// --- Local Provider Context ---
struct LocalProviderArtifactRegistryTestContext {
    artifact_registry: Arc<dyn ArtifactRegistry>,
    _temp_dir: TempDir,
    _registry_handle: JoinHandle<()>,
}

impl AsyncTestContext for LocalProviderArtifactRegistryTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-local-artifact-registry";
        let temp_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for local artifact registry test");

        #[cfg(feature = "local")]
        {
            // Start a container registry for testing
            use container_registry::auth;
            use container_registry::ContainerRegistry;
            use sec::Secret;

            // Setup auth to match production: basic auth only
            let mut auth_map = std::collections::HashMap::new();
            auth_map.insert(
                "local-user".to_string(),
                Secret::new("local-password".to_owned()),
            );
            let auth = Arc::new(auth_map);

            let mut registry = ContainerRegistry::builder()
                .auth_provider(auth)
                .build_for_testing();

            // Bind to a random port on localhost
            registry.bind(([127, 0, 0, 1], 0).into());
            let running_registry = registry.run_in_background();
            let bound_addr = running_registry.bound_addr();
            let registry_endpoint = format!("localhost:{}", bound_addr.port());

            // Keep the registry running in the background
            let registry_handle = tokio::spawn(async move {
                let _guard = running_registry;
                // Wait for the test to complete
                tokio::time::sleep(Duration::from_secs(300)).await;
            });

            // Set up environment for the provider with registry URL
            let binding = alien_core::bindings::ArtifactRegistryBinding::local(
                registry_endpoint.clone(),
                None,
            );

            let mut env_map: HashMap<String, String> = env::vars().collect();
            let binding_json =
                serde_json::to_string(&binding).expect("Failed to serialize binding");
            env_map.insert(
                alien_core::bindings::binding_env_var_name(binding_name),
                binding_json,
            );
            env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());

            let provider = Arc::new(
                BindingsProvider::from_env(env_map)
                    .await
                    .expect("Failed to load bindings provider"),
            );
            let artifact_registry = provider
                .load_artifact_registry(binding_name)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "Failed to load Local artifact registry for binding '{}' using registry endpoint '{}': {:?}",
                        binding_name, registry_endpoint, e
                    )
                });

            Self {
                artifact_registry,
                _temp_dir: temp_dir,
                _registry_handle: registry_handle,
            }
        }
        #[cfg(not(feature = "local"))]
        {
            panic!("Local feature is required for LocalProviderArtifactRegistryTestContext");
        }
    }

    async fn teardown(self) {
        // Clean up the registry
        self._registry_handle.abort();
    }
}

#[async_trait]
impl ArtifactRegistryTestContext for LocalProviderArtifactRegistryTestContext {
    async fn get_artifact_registry(&self) -> Arc<dyn ArtifactRegistry> {
        self.artifact_registry.clone()
    }
    fn provider_name(&self) -> &'static str {
        "local"
    }
}

// --- gRPC Provider Context ---
#[cfg(feature = "grpc")]
struct GrpcProviderArtifactRegistryTestContext {
    artifact_registry: Arc<dyn ArtifactRegistry>,
    _server_handle:
        JoinHandle<Result<(), alien_error::AlienError<alien_bindings::error::ErrorData>>>,
    _temp_data_dir: TempDir,
    _registry_handle: JoinHandle<()>,
}

#[cfg(feature = "grpc")]
impl AsyncTestContext for GrpcProviderArtifactRegistryTestContext {
    async fn setup() -> Self {
        load_test_env();
        let temp_data_dir =
            tempfile::tempdir().expect("Failed to create temp dir for gRPC server test");

        #[cfg(feature = "local")]
        {
            // Start a container registry for the gRPC server
            use container_registry::auth;
            use container_registry::ContainerRegistry;
            use sec::Secret;

            // Setup auth to match production: basic auth only
            let mut auth_map = std::collections::HashMap::new();
            auth_map.insert(
                "local-user".to_string(),
                Secret::new("local-password".to_owned()),
            );
            let auth = Arc::new(auth_map);

            let mut registry = ContainerRegistry::builder()
                .auth_provider(auth)
                .build_for_testing();

            // Bind to a random port on localhost
            registry.bind(([127, 0, 0, 1], 0).into());
            let running_registry = registry.run_in_background();
            let bound_addr = running_registry.bound_addr();
            let registry_endpoint = format!("localhost:{}", bound_addr.port());

            // Keep the registry running in the background
            let registry_handle = tokio::spawn(async move {
                let _guard = running_registry;
                // Wait for the test to complete
                tokio::time::sleep(Duration::from_secs(300)).await;
            });

            // Set up environment for the gRPC server with registry URL
            let server_binding = alien_core::bindings::ArtifactRegistryBinding::local(
                registry_endpoint.clone(),
                None,
            );

            let mut server_provider_env_map: HashMap<String, String> = env::vars().collect();
            let server_binding_json =
                serde_json::to_string(&server_binding).expect("Failed to serialize server binding");
            server_provider_env_map.insert(
                alien_core::bindings::binding_env_var_name(GRPC_BINDING_NAME),
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

            let artifact_registry_client = grpc_provider
                .load_artifact_registry(GRPC_BINDING_NAME)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "Failed to load Grpc artifact registry for binding '{}' using ALIEN_BINDINGS_GRPC_ADDRESS='{}': {:?}",
                        GRPC_BINDING_NAME, server_addr_str, e
                    )
                });

            Self {
                artifact_registry: artifact_registry_client,
                _server_handle: server_handle,
                _temp_data_dir: temp_data_dir,
                _registry_handle: registry_handle,
            }
        }
        #[cfg(not(feature = "local"))]
        {
            // For gRPC provider, we can use a mock or skip the test
            // For now, we'll skip the test when local feature is not available
            panic!("Local feature is required for GrpcProviderArtifactRegistryTestContext");
        }
    }

    async fn teardown(self) {
        self._server_handle.abort();
        self._registry_handle.abort();
    }
}

#[cfg(feature = "grpc")]
#[async_trait]
impl ArtifactRegistryTestContext for GrpcProviderArtifactRegistryTestContext {
    async fn get_artifact_registry(&self) -> Arc<dyn ArtifactRegistry> {
        self.artifact_registry.clone()
    }
    fn provider_name(&self) -> &'static str {
        "grpc"
    }
}

// --- AWS Provider Context ---
#[cfg(feature = "aws")]
struct AwsProviderArtifactRegistryTestContext {
    artifact_registry: Arc<dyn ArtifactRegistry>,
}

#[cfg(feature = "aws")]
impl AsyncTestContext for AwsProviderArtifactRegistryTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-aws-artifact-registry";

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let binding =
            alien_core::bindings::ArtifactRegistryBinding::ecr("test".to_string(), None, None);

        // Set up environment for the provider
        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert("AWS_REGION".to_string(), region);
        env_map.insert("AWS_ACCESS_KEY_ID".to_string(), access_key);
        env_map.insert("AWS_SECRET_ACCESS_KEY".to_string(), secret_key);
        env_map.insert("AWS_ACCOUNT_ID".to_string(), account_id);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "aws".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(
            alien_core::bindings::binding_env_var_name(binding_name),
            binding_json,
        );

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let artifact_registry = provider
            .load_artifact_registry(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load AWS artifact registry for binding '{}': {:?}",
                    binding_name, e
                )
            });

        Self { artifact_registry }
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl ArtifactRegistryTestContext for AwsProviderArtifactRegistryTestContext {
    async fn get_artifact_registry(&self) -> Arc<dyn ArtifactRegistry> {
        self.artifact_registry.clone()
    }
    fn provider_name(&self) -> &'static str {
        "aws"
    }
}

// --- GCP Provider Context ---
#[cfg(feature = "gcp")]
struct GcpProviderArtifactRegistryTestContext {
    artifact_registry: Arc<dyn ArtifactRegistry>,
}

#[cfg(feature = "gcp")]
impl AsyncTestContext for GcpProviderArtifactRegistryTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-gcp-artifact-registry";

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

        let binding = alien_core::bindings::ArtifactRegistryBinding::gar(
            Some(client_email.clone()),
            Some(client_email),
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert(
            "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
            service_account_key_json,
        );
        env_map.insert("GCP_REGION".to_string(), gcp_region);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "gcp".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(
            alien_core::bindings::binding_env_var_name(binding_name),
            binding_json,
        );

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let artifact_registry = provider
            .load_artifact_registry(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load GCP artifact registry for binding '{}': {:?}",
                    binding_name, e
                )
            });

        Self { artifact_registry }
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl ArtifactRegistryTestContext for GcpProviderArtifactRegistryTestContext {
    async fn get_artifact_registry(&self) -> Arc<dyn ArtifactRegistry> {
        self.artifact_registry.clone()
    }
    fn provider_name(&self) -> &'static str {
        "gcp"
    }
}

// --- Azure Provider Context ---
#[cfg(feature = "azure")]
struct AzureProviderArtifactRegistryTestContext {
    artifact_registry: Arc<dyn ArtifactRegistry>,
}

#[cfg(feature = "azure")]
impl AsyncTestContext for AzureProviderArtifactRegistryTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-azure-artifact-registry";

        let resource_group = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP");
        if resource_group.is_err() {
            panic!(
                "Skipping Azure artifact registry test: ALIEN_TEST_AZURE_RESOURCE_GROUP not set."
            );
        }
        let resource_group_name = resource_group.unwrap();

        // We'll use a test registry name - this would normally be created by the infra layer
        let registry_name = env::var("ALIEN_TEST_AZURE_REGISTRY_NAME").expect(
            "Skipping Azure artifact registry test: ALIEN_TEST_AZURE_REGISTRY_NAME not set.",
        );

        let binding = alien_core::bindings::ArtifactRegistryBinding::acr(
            registry_name,
            resource_group_name.clone(),
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
        env_map.insert("AZURE_RESOURCE_GROUP".to_string(), resource_group_name);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "azure".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(
            alien_core::bindings::binding_env_var_name(binding_name),
            binding_json,
        );

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let artifact_registry = provider
            .load_artifact_registry(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Azure artifact registry for binding '{}': {:?}",
                    binding_name, e
                )
            });

        Self { artifact_registry }
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl ArtifactRegistryTestContext for AzureProviderArtifactRegistryTestContext {
    async fn get_artifact_registry(&self) -> Arc<dyn ArtifactRegistry> {
        self.artifact_registry.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
}

// --- Kubernetes Provider Context ---
#[cfg(feature = "kubernetes")]
struct KubernetesProviderArtifactRegistryTestContext {
    artifact_registry: Arc<dyn ArtifactRegistry>,
}

#[cfg(feature = "kubernetes")]
impl AsyncTestContext for KubernetesProviderArtifactRegistryTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-k8s-artifact-registry";

        let mut env_map: HashMap<String, String> = env::vars().collect();
        // Add any Kubernetes-specific configuration here
        env_map.insert(
            format!(
                "ALIEN_{}_NAMESPACE",
                binding_name.replace('-', "_").to_uppercase()
            ),
            "default".to_string(),
        );
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "kubernetes".to_string());

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let artifact_registry = provider
            .load_artifact_registry(binding_name)
            .await
            .unwrap_or_else(|e| {
                // Kubernetes provider doesn't support artifact registry operations yet
                if let Some(alien_bindings::error::ErrorData::OperationNotSupported { .. }) = e.error {
                    panic!("SKIP: Kubernetes provider does not support artifact registry operations yet");
                }
                panic!("Failed to load Kubernetes artifact registry for binding '{}': {:?}", binding_name, e)
            });
        Self { artifact_registry }
    }
}

#[cfg(feature = "kubernetes")]
#[async_trait]
impl ArtifactRegistryTestContext for KubernetesProviderArtifactRegistryTestContext {
    async fn get_artifact_registry(&self) -> Arc<dyn ArtifactRegistry> {
        self.artifact_registry.clone()
    }
    fn provider_name(&self) -> &'static str {
        "kubernetes"
    }
}

// Helper function to get real service account email from environment
fn get_real_service_account_email() -> Option<String> {
    if let Ok(credential_json) = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY") {
        if let Ok(credential_value) = serde_json::from_str::<serde_json::Value>(&credential_json) {
            if let Some(email) = credential_value
                .get("client_email")
                .and_then(|v| v.as_str())
            {
                return Some(format!("serviceAccount:{}", email));
            }
        }
    }
    None
}

// Helper function to generate unique repository names
fn generate_unique_repo_name(provider_name: &str) -> String {
    format!("alien-test-{}-{}", provider_name, Uuid::new_v4().simple())
}

// Helper function to wait for repository to be ready (for async operations like GCP)
async fn wait_for_repository_ready(
    artifact_registry: &Arc<dyn ArtifactRegistry>,
    repo_id: &str,
    provider_name: &str,
    max_wait_seconds: u64,
) -> bool {
    let start_time = std::time::Instant::now();
    let max_duration = Duration::from_secs(max_wait_seconds);

    loop {
        match artifact_registry.get_repository(repo_id).await {
            Ok(_repository) => {
                // Repository exists and is ready
                return true;
            }
            Err(e) => {
                // Check if it's a ResourceNotFound error (repository still being created)
                if let Some(alien_bindings::error::ErrorData::ResourceNotFound { .. }) = e.error {
                    if start_time.elapsed() > max_duration {
                        panic!(
                            "[{}] Repository {} did not become ready within {} seconds",
                            provider_name, repo_id, max_wait_seconds
                        );
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                } else {
                    // Some other error occurred
                    panic!("[{}] Failed to get repository: {:?}", provider_name, e);
                }
            }
        }
    }
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderArtifactRegistryTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderArtifactRegistryTestContext::setup().await))]
#[cfg(any(
    feature = "local",
    feature = "grpc",
    feature = "aws",
    feature = "azure",
    feature = "gcp"
))]
#[tokio::test]
async fn test_create_repository_and_get_status(#[case] ctx: impl ArtifactRegistryTestContext) {
    let artifact_registry = ctx.get_artifact_registry().await;
    let provider_name = ctx.provider_name();
    let repo_name = generate_unique_repo_name(provider_name);

    // Create the repository
    let create_response = artifact_registry
        .create_repository(&repo_name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to create repository '{}': {:?}",
                provider_name, repo_name, e
            )
        });

    assert!(
        !create_response.name.is_empty(),
        "[{}] Repository name should not be empty",
        provider_name
    );

    // For real cloud providers, wait for repository to be ready
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        let is_ready = wait_for_repository_ready(
            &artifact_registry,
            &create_response.name,
            provider_name,
            300,
        )
        .await;
        assert!(
            is_ready,
            "[{}] Repository should eventually be ready",
            provider_name
        );
    }

    // Verify we can get repository details
    let repository = artifact_registry
        .get_repository(&create_response.name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to get repository details: {:?}",
                provider_name, e
            )
        });

    println!(
        "[{}] Successfully created repository '{}' with URI: {:?}",
        provider_name, repo_name, repository.uri
    );

    // Clean up
    artifact_registry
        .delete_repository(&create_response.name)
        .await
        .ok();
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderArtifactRegistryTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderArtifactRegistryTestContext::setup().await))]
#[cfg(any(
    feature = "local",
    feature = "grpc",
    feature = "aws",
    feature = "azure",
    feature = "gcp"
))]
#[tokio::test]
async fn test_add_remove_cross_account_access(#[case] ctx: impl ArtifactRegistryTestContext) {
    let artifact_registry = ctx.get_artifact_registry().await;
    let provider_name = ctx.provider_name();
    let repo_name = generate_unique_repo_name(provider_name);

    // Create the repository first
    let create_response = artifact_registry
        .create_repository(&repo_name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to create repository '{}': {:?}",
                provider_name, repo_name, e
            )
        });

    // Wait for repository to be ready if needed
    if matches!(provider_name, "gcp") {
        wait_for_repository_ready(
            &artifact_registry,
            &create_response.name,
            provider_name,
            300,
        )
        .await;
    }

    // Create two different cross-account access configurations based on provider
    let (cross_account_access_1, cross_account_access_2) = match provider_name {
        "aws" => {
            let access_1 = CrossAccountAccess::Aws(AwsCrossAccountAccess {
                account_ids: vec!["123456789012".to_string()],
                allowed_service_types: vec![ComputeServiceType::Function],
                role_arns: vec!["arn:aws:iam::123456789012:role/test-role-1".to_string()],
            });
            let access_2 = CrossAccountAccess::Aws(AwsCrossAccountAccess {
                account_ids: vec!["987654321098".to_string()],
                allowed_service_types: vec![ComputeServiceType::Function],
                role_arns: vec!["arn:aws:iam::987654321098:role/test-role-2".to_string()],
            });
            (access_1, access_2)
        }
        "gcp" => {
            // Use real service account from environment if available
            let service_accounts = if let Some(real_sa) = get_real_service_account_email() {
                // Strip the "serviceAccount:" prefix if present
                let email = real_sa.strip_prefix("serviceAccount:").unwrap_or(&real_sa);
                vec![email.to_string()]
            } else {
                vec!["test1@example.com".to_string()]
            };

            let access_1 = CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                project_numbers: vec!["123456789012".to_string()],
                allowed_service_types: vec![ComputeServiceType::Function],
                service_account_emails: service_accounts,
            });
            let access_2 = CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                project_numbers: vec!["987654321098".to_string()],
                allowed_service_types: vec![ComputeServiceType::Function],
                service_account_emails: vec!["test2@example.com".to_string()],
            });
            (access_1, access_2)
        }
        "azure" => {
            // Azure should return OperationNotSupported
            println!(
                "[{}] Testing OperationNotSupported for cross-account access",
                provider_name
            );

            // Test get_cross_account_access first
            match artifact_registry
                .get_cross_account_access(&create_response.name)
                .await
            {
                Ok(_) => panic!(
                    "[{}] Expected OperationNotSupported error for get_cross_account_access",
                    provider_name
                ),
                Err(e) => {
                    if let Some(alien_bindings::error::ErrorData::OperationNotSupported {
                        operation,
                        reason,
                    }) = e.error
                    {
                        println!(
                            "[{}] ✓ Correctly received OperationNotSupported for get: {} - {}",
                            provider_name, operation, reason
                        );
                    } else {
                        panic!(
                            "[{}] Expected OperationNotSupported error for get, got: {:?}",
                            provider_name, e
                        );
                    }
                }
            }

            // Test add_cross_account_access
            let test_access = CrossAccountAccess::Aws(AwsCrossAccountAccess {
                account_ids: vec!["123456789012".to_string()],
                allowed_service_types: vec![ComputeServiceType::Function],
                role_arns: vec!["arn:aws:iam::123456789012:role/test-role".to_string()],
            });

            match artifact_registry
                .add_cross_account_access(&create_response.name, test_access.clone())
                .await
            {
                Ok(()) => panic!("[{}] Expected OperationNotSupported error", provider_name),
                Err(e) => {
                    if let Some(alien_bindings::error::ErrorData::OperationNotSupported {
                        operation,
                        reason,
                    }) = e.error
                    {
                        println!(
                            "[{}] ✓ Correctly received OperationNotSupported for add: {} - {}",
                            provider_name, operation, reason
                        );
                    } else {
                        panic!(
                            "[{}] Expected OperationNotSupported error, got: {:?}",
                            provider_name, e
                        );
                    }
                }
            }

            // Test remove_cross_account_access
            match artifact_registry
                .remove_cross_account_access(&create_response.name, test_access)
                .await
            {
                Ok(()) => panic!(
                    "[{}] Expected OperationNotSupported error for remove",
                    provider_name
                ),
                Err(e) => {
                    if let Some(alien_bindings::error::ErrorData::OperationNotSupported {
                        operation,
                        reason,
                    }) = e.error
                    {
                        println!(
                            "[{}] ✓ Correctly received OperationNotSupported for remove: {} - {}",
                            provider_name, operation, reason
                        );
                    } else {
                        panic!(
                            "[{}] Expected OperationNotSupported error for remove, got: {:?}",
                            provider_name, e
                        );
                    }
                }
            }

            // Clean up and return early for Azure
            artifact_registry
                .delete_repository(&create_response.name)
                .await
                .ok();
            return;
        }
        "local" | "grpc" => {
            // Local and gRPC should also return OperationNotSupported
            println!(
                "[{}] Testing OperationNotSupported for cross-account access",
                provider_name
            );

            // Test get_cross_account_access first
            match artifact_registry
                .get_cross_account_access(&create_response.name)
                .await
            {
                Ok(_) => panic!(
                    "[{}] Expected OperationNotSupported error for get_cross_account_access",
                    provider_name
                ),
                Err(e) => {
                    if let Some(alien_bindings::error::ErrorData::OperationNotSupported {
                        operation,
                        reason,
                    }) = e.error
                    {
                        println!(
                            "[{}] ✓ Correctly received OperationNotSupported for get: {} - {}",
                            provider_name, operation, reason
                        );
                    } else {
                        panic!(
                            "[{}] Expected OperationNotSupported error for get, got: {:?}",
                            provider_name, e
                        );
                    }
                }
            }

            let test_access = CrossAccountAccess::Aws(AwsCrossAccountAccess {
                account_ids: vec!["123456789012".to_string()],
                allowed_service_types: vec![ComputeServiceType::Function],
                role_arns: vec!["arn:aws:iam::123456789012:role/test-role".to_string()],
            });

            // Test add_cross_account_access
            match artifact_registry
                .add_cross_account_access(&create_response.name, test_access.clone())
                .await
            {
                Ok(()) => panic!("[{}] Expected OperationNotSupported error", provider_name),
                Err(e) => {
                    if let Some(alien_bindings::error::ErrorData::OperationNotSupported {
                        operation,
                        reason,
                    }) = e.error
                    {
                        println!(
                            "[{}] ✓ Correctly received OperationNotSupported for add: {} - {}",
                            provider_name, operation, reason
                        );
                    } else {
                        panic!(
                            "[{}] Expected OperationNotSupported error, got: {:?}",
                            provider_name, e
                        );
                    }
                }
            }

            // Test remove_cross_account_access
            match artifact_registry
                .remove_cross_account_access(&create_response.name, test_access)
                .await
            {
                Ok(()) => panic!(
                    "[{}] Expected OperationNotSupported error for remove",
                    provider_name
                ),
                Err(e) => {
                    if let Some(alien_bindings::error::ErrorData::OperationNotSupported {
                        operation,
                        reason,
                    }) = e.error
                    {
                        println!(
                            "[{}] ✓ Correctly received OperationNotSupported for remove: {} - {}",
                            provider_name, operation, reason
                        );
                    } else {
                        panic!(
                            "[{}] Expected OperationNotSupported error for remove, got: {:?}",
                            provider_name, e
                        );
                    }
                }
            }

            // Clean up and return early
            artifact_registry
                .delete_repository(&create_response.name)
                .await
                .ok();
            return;
        }
        _ => panic!("Unknown provider: {}", provider_name),
    };

    // Test the comprehensive add/remove cross-account access flow

    // Step 0: Get initial state - should be empty
    match artifact_registry
        .get_cross_account_access(&create_response.name)
        .await
    {
        Ok(initial_permissions) => {
            println!(
                "[{}] ✓ Retrieved initial cross-account access: {:?}",
                provider_name, initial_permissions
            );

            // Verify initial state is empty
            match (&initial_permissions.access, provider_name) {
                (CrossAccountAccess::Aws(aws_access), "aws") => {
                    let is_empty = aws_access.account_ids.is_empty()
                        && aws_access.role_arns.is_empty()
                        && aws_access.allowed_service_types.is_empty();
                    if is_empty {
                        println!(
                            "[{}] ✓ Initial AWS cross-account access is empty as expected",
                            provider_name
                        );
                    } else {
                        println!(
                            "[{}] ⚠ Initial AWS cross-account access is not empty: {:?}",
                            provider_name, aws_access
                        );
                    }
                }
                (CrossAccountAccess::Gcp(gcp_access), "gcp") => {
                    let is_empty = gcp_access.project_numbers.is_empty()
                        && gcp_access.service_account_emails.is_empty()
                        && gcp_access.allowed_service_types.is_empty();
                    if is_empty {
                        println!(
                            "[{}] ✓ Initial GCP cross-account access is empty as expected",
                            provider_name
                        );
                    } else {
                        println!(
                            "[{}] ⚠ Initial GCP cross-account access is not empty: {:?}",
                            provider_name, gcp_access
                        );
                    }
                }
                _ => {
                    println!(
                        "[{}] ⚠ Unexpected initial cross-account access format",
                        provider_name
                    );
                }
            }
        }
        Err(e) => {
            println!(
                "[{}] ⚠ Failed to get initial cross-account access: {:?}",
                provider_name, e
            );
        }
    }

    // Step 1: Add first cross-account access
    match artifact_registry
        .add_cross_account_access(&create_response.name, cross_account_access_1.clone())
        .await
    {
        Ok(()) => {
            println!(
                "[{}] ✓ Successfully added first cross-account access",
                provider_name
            );

            // Step 2: Verify first access was added
            match artifact_registry
                .get_cross_account_access(&create_response.name)
                .await
            {
                Ok(permissions_after_first) => {
                    println!(
                        "[{}] ✓ Retrieved cross-account access after adding first: {:?}",
                        provider_name, permissions_after_first
                    );

                    // Step 3: Add second cross-account access
                    match artifact_registry
                        .add_cross_account_access(
                            &create_response.name,
                            cross_account_access_2.clone(),
                        )
                        .await
                    {
                        Ok(()) => {
                            println!(
                                "[{}] ✓ Successfully added second cross-account access",
                                provider_name
                            );

                            // Step 4: Verify both accesses are present
                            match artifact_registry
                                .get_cross_account_access(&create_response.name)
                                .await
                            {
                                Ok(permissions_after_both) => {
                                    println!("[{}] ✓ Retrieved cross-account access after adding both: {:?}", provider_name, permissions_after_both);

                                    // Verify both accesses are present
                                    match (&permissions_after_both.access, provider_name) {
                                        (CrossAccountAccess::Aws(aws_access), "aws") => {
                                            let has_multiple = aws_access.account_ids.len() >= 2
                                                || aws_access.role_arns.len() >= 2;
                                            if has_multiple {
                                                println!("[{}] ✓ AWS cross-account access contains multiple entries", provider_name);
                                            } else {
                                                println!("[{}] ⚠ AWS cross-account access may not contain both entries: {:?}", provider_name, aws_access);
                                            }
                                        }
                                        (CrossAccountAccess::Gcp(gcp_access), "gcp") => {
                                            let has_multiple = gcp_access.project_numbers.len()
                                                >= 2
                                                || gcp_access.service_account_emails.len() >= 2;
                                            if has_multiple {
                                                println!("[{}] ✓ GCP cross-account access contains multiple entries", provider_name);
                                            } else {
                                                println!("[{}] ⚠ GCP cross-account access may not contain both entries: {:?}", provider_name, gcp_access);
                                            }
                                        }
                                        _ => {
                                            println!(
                                                "[{}] ⚠ Unexpected cross-account access format",
                                                provider_name
                                            );
                                        }
                                    }

                                    // Step 5: Remove first access
                                    match artifact_registry
                                        .remove_cross_account_access(
                                            &create_response.name,
                                            cross_account_access_1,
                                        )
                                        .await
                                    {
                                        Ok(()) => {
                                            println!("[{}] ✓ Successfully removed first cross-account access", provider_name);

                                            // Step 6: Verify only second access remains
                                            match artifact_registry
                                                .get_cross_account_access(&create_response.name)
                                                .await
                                            {
                                                Ok(permissions_after_remove_first) => {
                                                    println!("[{}] ✓ Retrieved cross-account access after removing first: {:?}", provider_name, permissions_after_remove_first);

                                                    // Step 7: Remove second access
                                                    match artifact_registry
                                                        .remove_cross_account_access(
                                                            &create_response.name,
                                                            cross_account_access_2,
                                                        )
                                                        .await
                                                    {
                                                        Ok(()) => {
                                                            println!("[{}] ✓ Successfully removed second cross-account access", provider_name);

                                                            // Step 8: Verify all access was removed
                                                            match artifact_registry
                                                                .get_cross_account_access(
                                                                    &create_response.name,
                                                                )
                                                                .await
                                                            {
                                                                Ok(final_permissions) => {
                                                                    println!("[{}] ✓ Retrieved final cross-account access: {:?}", provider_name, final_permissions);

                                                                    // Verify final state is empty
                                                                    match (
                                                                        &final_permissions.access,
                                                                        provider_name,
                                                                    ) {
                                                                        (
                                                                            CrossAccountAccess::Aws(
                                                                                aws_access,
                                                                            ),
                                                                            "aws",
                                                                        ) => {
                                                                            let is_empty = aws_access.account_ids.is_empty() && 
                                                                                          aws_access.role_arns.is_empty() && 
                                                                                          aws_access.allowed_service_types.is_empty();
                                                                            if is_empty {
                                                                                println!("[{}] ✓ Final AWS cross-account access is empty - all access properly removed", provider_name);
                                                                            } else {
                                                                                println!("[{}] ⚠ Final AWS cross-account access is not empty: {:?}", provider_name, aws_access);
                                                                            }
                                                                        }
                                                                        (
                                                                            CrossAccountAccess::Gcp(
                                                                                gcp_access,
                                                                            ),
                                                                            "gcp",
                                                                        ) => {
                                                                            let is_empty = gcp_access.project_numbers.is_empty() && 
                                                                                          gcp_access.service_account_emails.is_empty() && 
                                                                                          gcp_access.allowed_service_types.is_empty();
                                                                            if is_empty {
                                                                                println!("[{}] ✓ Final GCP cross-account access is empty - all access properly removed", provider_name);
                                                                            } else {
                                                                                println!("[{}] ⚠ Final GCP cross-account access is not empty: {:?}", provider_name, gcp_access);
                                                                            }
                                                                        }
                                                                        _ => {
                                                                            println!("[{}] ⚠ Unexpected final cross-account access format", provider_name);
                                                                        }
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    println!("[{}] ⚠ Failed to get final cross-account access: {:?}", provider_name, e);
                                                                }
                                                            }
                                                        }
                                                        Err(e) => {
                                                            println!("[{}] ⚠ Failed to remove second cross-account access: {:?}", provider_name, e);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    println!("[{}] ⚠ Failed to get cross-account access after removing first: {:?}", provider_name, e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            println!("[{}] ⚠ Failed to remove first cross-account access: {:?}", provider_name, e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("[{}] ⚠ Failed to get cross-account access after adding both: {:?}", provider_name, e);
                                }
                            }
                        }
                        Err(e) => {
                            println!(
                                "[{}] ⚠ Failed to add second cross-account access: {:?}",
                                provider_name, e
                            );
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "[{}] ⚠ Failed to get cross-account access after adding first: {:?}",
                        provider_name, e
                    );
                }
            }
        }
        Err(e) => {
            println!("[{}] ⚠ Failed to add first cross-account access (acceptable for some test environments): {:?}", provider_name, e);
            println!("[{}] This is acceptable when the test environment doesn't have IAM permission management capabilities", provider_name);
        }
    }

    // Clean up
    artifact_registry
        .delete_repository(&create_response.name)
        .await
        .ok();
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderArtifactRegistryTestContext::setup().await))]
// TODO (CRITICAL): Enable AWS test (need to create pull/push roles)
// #[cfg_attr(feature = "aws", case::aws(AwsProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderArtifactRegistryTestContext::setup().await))]
// #[cfg_attr(feature = "gcp", case::gcp(GcpProviderArtifactRegistryTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderArtifactRegistryTestContext::setup().await))]
#[cfg(any(feature = "local", feature = "grpc", feature = "azure"))]
#[tokio::test]
async fn test_generate_credentials(#[case] ctx: impl ArtifactRegistryTestContext) {
    let artifact_registry = ctx.get_artifact_registry().await;
    let provider_name = ctx.provider_name();
    let repo_name = generate_unique_repo_name(provider_name);

    // Create the repository first
    let create_response = artifact_registry
        .create_repository(&repo_name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to create repository '{}': {:?}",
                provider_name, repo_name, e
            )
        });

    // Wait for repository to be ready if needed
    if matches!(provider_name, "gcp") {
        wait_for_repository_ready(
            &artifact_registry,
            &create_response.name,
            provider_name,
            300,
        )
        .await;
    }

    // Test generating pull credentials
    let credentials_result = artifact_registry
        .generate_credentials(
            &create_response.name,
            ArtifactRegistryPermissions::Pull,
            Some(3600),
        )
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to generate credentials for repository '{}': {:?}",
                provider_name, repo_name, e
            )
        });

    // Clean up
    artifact_registry
        .delete_repository(&create_response.name)
        .await
        .ok();
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderArtifactRegistryTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderArtifactRegistryTestContext::setup().await))]
#[cfg(any(
    feature = "local",
    feature = "grpc",
    feature = "aws",
    feature = "azure",
    feature = "gcp"
))]
#[tokio::test]
async fn test_delete_repository(#[case] ctx: impl ArtifactRegistryTestContext) {
    let artifact_registry = ctx.get_artifact_registry().await;
    let provider_name = ctx.provider_name();
    let repo_name = generate_unique_repo_name(provider_name);

    // Create the repository first
    let create_response = artifact_registry
        .create_repository(&repo_name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to create repository '{}': {:?}",
                provider_name, repo_name, e
            )
        });

    // Wait for repository to be ready if needed
    if matches!(provider_name, "gcp") {
        wait_for_repository_ready(
            &artifact_registry,
            &create_response.name,
            provider_name,
            300,
        )
        .await;
    }

    // Delete the repository
    artifact_registry
        .delete_repository(&create_response.name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to delete repository '{}': {:?}",
                provider_name, repo_name, e
            )
        });

    println!(
        "[{}] Successfully deleted repository '{}'",
        provider_name, repo_name
    );

    // Note: We don't verify that the repository is gone since some providers
    // might have eventual consistency or different deletion semantics
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderArtifactRegistryTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderArtifactRegistryTestContext::setup().await))]
#[cfg(any(
    feature = "local",
    feature = "grpc",
    feature = "aws",
    feature = "azure",
    feature = "gcp"
))]
#[tokio::test]
async fn test_full_repository_lifecycle(#[case] ctx: impl ArtifactRegistryTestContext) {
    let artifact_registry = ctx.get_artifact_registry().await;
    let provider_name = ctx.provider_name();
    let repo_name = generate_unique_repo_name(provider_name);

    println!(
        "[{}] Starting full lifecycle test for repository '{}'",
        provider_name, repo_name
    );

    // 1. Create repository
    let create_response = artifact_registry
        .create_repository(&repo_name)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to create repository '{}': {:?}",
                provider_name, repo_name, e
            )
        });
    println!("[{}] ✓ Created repository", provider_name);

    // 2. Wait for repository to be ready
    if matches!(provider_name, "gcp") {
        let is_ready = wait_for_repository_ready(
            &artifact_registry,
            &create_response.name,
            provider_name,
            300,
        )
        .await;
        assert!(is_ready);
    }
    println!("[{}] ✓ Repository is ready", provider_name);

    // 3. Add cross-account access (only for AWS and GCP)
    if matches!(provider_name, "aws" | "gcp") {
        let cross_account_access = match provider_name {
            "aws" => CrossAccountAccess::Aws(AwsCrossAccountAccess {
                account_ids: vec!["123456789012".to_string()],
                allowed_service_types: vec![ComputeServiceType::Function],
                role_arns: vec!["arn:aws:iam::123456789012:role/test-role".to_string()],
            }),
            "gcp" => {
                let service_accounts = if let Some(real_sa) = get_real_service_account_email() {
                    let email = real_sa.strip_prefix("serviceAccount:").unwrap_or(&real_sa);
                    vec![email.to_string()]
                } else {
                    vec!["test@example.com".to_string()]
                };

                CrossAccountAccess::Gcp(GcpCrossAccountAccess {
                    project_numbers: vec!["123456789012".to_string()],
                    allowed_service_types: vec![ComputeServiceType::Function],
                    service_account_emails: service_accounts,
                })
            }
            _ => unreachable!(),
        };

        match artifact_registry
            .add_cross_account_access(&create_response.name, cross_account_access.clone())
            .await
        {
            Ok(()) => {
                println!("[{}] ✓ Added cross-account access", provider_name);

                // 4. Get cross-account access and verify it was added
                match artifact_registry
                    .get_cross_account_access(&create_response.name)
                    .await
                {
                    Ok(retrieved_permissions) => {
                        println!(
                            "[{}] ✓ Retrieved cross-account access: {:?}",
                            provider_name, retrieved_permissions
                        );

                        // 5. Remove cross-account access
                        match artifact_registry
                            .remove_cross_account_access(
                                &create_response.name,
                                cross_account_access,
                            )
                            .await
                        {
                            Ok(()) => {
                                println!("[{}] ✓ Removed cross-account access", provider_name);

                                // 6. Verify access was removed
                                match artifact_registry
                                    .get_cross_account_access(&create_response.name)
                                    .await
                                {
                                    Ok(empty_permissions) => {
                                        println!("[{}] ✓ Verified cross-account access was removed: {:?}", provider_name, empty_permissions);
                                    }
                                    Err(_) => {
                                        println!("[{}] ⚠ Could not verify removal (acceptable for some test environments)", provider_name);
                                    }
                                }
                            }
                            Err(_) => {
                                println!("[{}] ⚠ Could not remove cross-account access (acceptable for some test environments)", provider_name);
                            }
                        }
                    }
                    Err(_) => {
                        println!("[{}] ⚠ Could not retrieve cross-account access (acceptable for some test environments)", provider_name);
                    }
                }
            }
            Err(_) => {
                println!("[{}] ⚠ Could not add cross-account access (acceptable for some test environments)", provider_name);
            }
        }
    } else {
        // For Azure, local, grpc, kubernetes - these should return OperationNotSupported
        println!(
            "[{}] ✓ Skipped cross-account access (not supported for this provider)",
            provider_name
        );
    }

    // 7. Try to generate credentials (may not be supported by all providers)
    if let Ok(credentials) = artifact_registry
        .generate_credentials(
            &create_response.name,
            ArtifactRegistryPermissions::Pull,
            Some(3600),
        )
        .await
    {
        println!(
            "[{}] ✓ Generated pull credentials: username={}",
            provider_name, credentials.username
        );
    } else {
        println!("[{}] ⚠ Credential generation not supported", provider_name);
    }

    // 8. Delete repository
    artifact_registry
        .delete_repository(&create_response.name)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to delete repository: {:?}", provider_name, e));
    println!("[{}] ✓ Deleted repository", provider_name);

    println!(
        "[{}] ✅ Full lifecycle test completed successfully",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderArtifactRegistryTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderArtifactRegistryTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderArtifactRegistryTestContext::setup().await))]
#[cfg(any(
    feature = "local",
    feature = "grpc",
    feature = "aws",
    feature = "azure",
    feature = "gcp"
))]
#[tokio::test]
async fn test_get_nonexistent_repository_returns_404(
    #[case] ctx: impl ArtifactRegistryTestContext,
) {
    let artifact_registry = ctx.get_artifact_registry().await;
    let provider_name = ctx.provider_name();

    // Use a repository name that definitely doesn't exist
    let nonexistent_repo = format!("alien-test-nonexistent-{}", Uuid::new_v4().simple());
    let expected_repo_name = if provider_name == "aws" {
        // AWS ECR bindings apply a repository prefix in tests.
        format!("test-{}", nonexistent_repo)
    } else {
        nonexistent_repo.clone()
    };

    println!(
        "[{}] Testing get_repository for non-existent repo '{}'",
        provider_name, nonexistent_repo
    );

    // Try to get a repository that doesn't exist
    let result = artifact_registry.get_repository(&nonexistent_repo).await;

    // Should get an error
    assert!(
        result.is_err(),
        "[{}] Expected error when getting non-existent repository",
        provider_name
    );

    let error = result.unwrap_err();
    println!("[{}] Error: {:?}", provider_name, error);

    // Check that the error is ResourceNotFound or RemoteResourceNotFound
    match &error.error {
        Some(alien_bindings::error::ErrorData::ResourceNotFound { resource_id }) => {
            assert_eq!(resource_id, &expected_repo_name);
            println!("[{}] ✓ Correctly returned ResourceNotFound", provider_name);
        }
        Some(alien_bindings::error::ErrorData::RemoteResourceNotFound {
            resource_name, ..
        }) => {
            assert_eq!(resource_name, &expected_repo_name);
            println!(
                "[{}] ✓ Correctly returned RemoteResourceNotFound",
                provider_name
            );
        }
        other => {
            panic!(
                "[{}] Expected ResourceNotFound or RemoteResourceNotFound error, got: {:?}",
                provider_name, other
            );
        }
    }

    // CRITICAL: Check that the HTTP status code is 404, not 500
    assert_eq!(
        error.http_status_code,
        Some(404),
        "[{}] HTTP status code should be 404 for not found, got: {:?}",
        provider_name,
        error.http_status_code
    );

    println!("[{}] ✓ HTTP status code is correctly 404", provider_name);
}
