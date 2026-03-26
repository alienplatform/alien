#![cfg(test)]

use alien_bindings::{
    traits::{BindingsProviderApi, Vault},
    BindingsProvider,
};

#[cfg(feature = "grpc")]
use alien_bindings::{grpc::run_grpc_server, providers::grpc_provider::GrpcBindingsProvider};
use alien_core::bindings::{self, VaultBinding};

// Unified BindingsProvider handles routing to appropriate implementations

use async_trait::async_trait;
use rstest::rstest;
use std::path::PathBuf as StdPathBuf;
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tempfile::TempDir;
use test_context::AsyncTestContext;
use tokio::task::JoinHandle;
use uuid::Uuid;
use workspace_root::get_workspace_root;

#[cfg(feature = "azure")]
use alien_azure_clients::keyvault::{
    AzureKeyVaultManagementClient, AzureKeyVaultSecretsClient, KeyVaultManagementApi,
    KeyVaultSecretsApi,
};
#[cfg(feature = "azure")]
use alien_azure_clients::models::keyvault::{
    AccessPolicyEntry, Permissions, PermissionsSecretsItem, Sku, SkuFamily, SkuName,
    VaultCreateOrUpdateParameters, VaultProperties,
};
#[cfg(feature = "azure")]
use alien_azure_clients::{AzureClientConfig, AzureCredentials, AzureTokenCache};
#[cfg(feature = "azure")]
use alien_error::{AlienError, Context};
#[cfg(feature = "azure")]
use base64::{engine::general_purpose, Engine as _};
#[cfg(feature = "azure")]
use reqwest::Client;
#[cfg(feature = "azure")]
use tracing::{info, warn};

const GRPC_BINDING_NAME: &str = "test-grpc-vault-binding";

fn load_test_env() {
    // Load .env.test from the workspace root
    let root: StdPathBuf = get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
}

#[async_trait]
pub trait VaultTestContext: AsyncTestContext + Send + Sync {
    async fn get_vault(&self) -> Arc<dyn Vault>;
    fn provider_name(&self) -> &'static str;
    fn track_secret(&self, secret_name: &str);
}

// --- Local Provider Context ---
struct LocalProviderTestContext {
    vault: Arc<dyn Vault>,
    _temp_dir: TempDir, // Keep TempDir to ensure it's cleaned up on drop
    created_secrets: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl AsyncTestContext for LocalProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-local-vault";
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for local vault test");
        let temp_dir_path = temp_dir.path().to_str().unwrap().to_string();

        let binding = VaultBinding::local(binding_name, temp_dir_path.clone());

        let mut env_map: HashMap<String, String> = env::vars().collect();
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load bindings provider"),
        );
        let vault = provider.load_vault(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load Local vault for binding '{}' using temp dir '{}': {:?}",
                binding_name, temp_dir_path, e
            )
        });
        Self {
            vault,
            _temp_dir: temp_dir,
            created_secrets: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        // Clean up created secrets
        let secrets_to_cleanup = {
            let secrets = self.created_secrets.lock().unwrap();
            secrets.clone()
        };

        for secret_name in secrets_to_cleanup {
            self.cleanup_secret(&secret_name).await;
        }
    }
}

#[async_trait]
impl VaultTestContext for LocalProviderTestContext {
    async fn get_vault(&self) -> Arc<dyn Vault> {
        self.vault.clone()
    }
    fn provider_name(&self) -> &'static str {
        "local"
    }
    fn track_secret(&self, secret_name: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        secrets.insert(secret_name.to_string());
    }
}

impl LocalProviderTestContext {
    async fn cleanup_secret(&self, secret_name: &str) {
        match self.vault.delete_secret(secret_name).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - resource might already be deleted
            }
        }
    }
}

// --- gRPC Provider Context ---
#[cfg(feature = "grpc")]
struct GrpcProviderTestContext {
    vault: Arc<dyn Vault>,
    _server_handle:
        JoinHandle<Result<(), alien_error::AlienError<alien_bindings::error::ErrorData>>>,
    _temp_data_dir: TempDir, // Manages ALIEN_DATA_DIR for the gRPC server's LocalBindingsProvider
    created_secrets: std::sync::Mutex<HashSet<String>>,
}

#[cfg(feature = "grpc")]
impl AsyncTestContext for GrpcProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let temp_data_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for ALIEN_DATA_DIR (gRPC server)");

        // Env map for the BindingsProvider used by the gRPC server
        let server_binding = VaultBinding::local(
            GRPC_BINDING_NAME,
            temp_data_dir.path().to_str().unwrap().to_string(),
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

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
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

        tokio::time::sleep(std::time::Duration::from_millis(500)).await; // Allow server to start, increased sleep

        // Env map for the GrpcBindingsProvider (client-side)
        let mut service_provider_env_map: HashMap<String, String> = env::vars().collect();
        service_provider_env_map.insert(
            "ALIEN_BINDINGS_GRPC_ADDRESS".to_string(),
            server_addr_str.clone(),
        );
        service_provider_env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "grpc".to_string());

        let grpc_provider = GrpcBindingsProvider::new_with_env(service_provider_env_map)
            .expect("Failed to load bindings provider");

        let vault = grpc_provider
            .load_vault(GRPC_BINDING_NAME)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Grpc vault for binding '{}' using ALIEN_BINDINGS_GRPC_ADDRESS='{}': {:?}",
                    GRPC_BINDING_NAME, server_addr_str, e
                )
            });

        Self {
            vault,
            _server_handle: server_handle,
            _temp_data_dir: temp_data_dir,
            created_secrets: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        // Clean up created secrets
        let secrets_to_cleanup = {
            let secrets = self.created_secrets.lock().unwrap();
            secrets.clone()
        };

        for secret_name in secrets_to_cleanup {
            self.cleanup_secret(&secret_name).await;
        }

        // Clean up gRPC server
        self._server_handle.abort();
    }
}

#[cfg(feature = "grpc")]
#[async_trait]
impl VaultTestContext for GrpcProviderTestContext {
    async fn get_vault(&self) -> Arc<dyn Vault> {
        self.vault.clone()
    }
    fn provider_name(&self) -> &'static str {
        "grpc"
    }
    fn track_secret(&self, secret_name: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        secrets.insert(secret_name.to_string());
    }
}

#[cfg(feature = "grpc")]
impl GrpcProviderTestContext {
    async fn cleanup_secret(&self, secret_name: &str) {
        match self.vault.delete_secret(secret_name).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - resource might already be deleted
            }
        }
    }
}

// --- Cloud Provider Contexts ---

#[cfg(feature = "aws")]
struct AwsProviderTestContext {
    vault: Arc<dyn Vault>,
    test_vault_name: String,
}

#[cfg(feature = "aws")]
impl AsyncTestContext for AwsProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-aws-vault";

        // Generate unique vault name to avoid conflicts
        let test_vault_name = format!("alien-test-vault-{}", Uuid::new_v4().simple());

        let binding = VaultBinding::parameter_store(test_vault_name.clone());

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert(
            "AWS_REGION".to_string(),
            env::var("AWS_MANAGEMENT_REGION").unwrap(),
        );
        env_map.insert(
            "AWS_ACCESS_KEY_ID".to_string(),
            env::var("AWS_MANAGEMENT_ACCESS_KEY_ID").unwrap(),
        );
        env_map.insert(
            "AWS_SECRET_ACCESS_KEY".to_string(),
            env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY").unwrap(),
        );
        env_map.insert(
            "AWS_ACCOUNT_ID".to_string(),
            env::var("AWS_MANAGEMENT_ACCOUNT_ID").unwrap(),
        );
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "aws".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load AWS bindings provider"),
        );
        let vault = provider.load_vault(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load AWS vault for binding '{}' using vault name '{}': {:?}",
                binding_name, test_vault_name, e
            )
        });

        Self {
            vault,
            test_vault_name,
        }
    }

    async fn teardown(self) {
        // Clean up test resources
        // Note: In a real implementation, we would clean up the AWS resources here
        // For now, we'll rely on the test framework to handle cleanup
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl VaultTestContext for AwsProviderTestContext {
    async fn get_vault(&self) -> Arc<dyn Vault> {
        self.vault.clone()
    }
    fn provider_name(&self) -> &'static str {
        "aws"
    }
    fn track_secret(&self, _secret_name: &str) {
        // AWS provider handles cleanup through its own mechanisms
        // No additional tracking needed
    }
}

#[cfg(feature = "gcp")]
struct GcpProviderTestContext {
    vault: Arc<dyn Vault>,
    test_vault_name: String,
}

#[cfg(feature = "gcp")]
impl AsyncTestContext for GcpProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-gcp-vault";

        // Generate unique vault name to avoid conflicts
        let test_vault_name = format!("alien-test-vault-{}", Uuid::new_v4().simple());

        let binding = VaultBinding::secret_manager(test_vault_name.clone());

        let service_account_key_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .expect("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set in .env.test");
        // Using global Secret Manager, no specific region needed
        let gcp_region = env::var("GOOGLE_MANAGEMENT_REGION").unwrap_or_else(|_| "".to_string());

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert(
            "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
            service_account_key_json,
        );
        env_map.insert("GCP_REGION".to_string(), gcp_region);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "gcp".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load GCP bindings provider"),
        );
        let vault = provider.load_vault(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load GCP vault for binding '{}' using vault name '{}': {:?}",
                binding_name, test_vault_name, e
            )
        });

        Self {
            vault,
            test_vault_name,
        }
    }

    async fn teardown(self) {
        // Clean up test resources
        // Note: In a real implementation, we would clean up the GCP resources here
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl VaultTestContext for GcpProviderTestContext {
    async fn get_vault(&self) -> Arc<dyn Vault> {
        self.vault.clone()
    }
    fn provider_name(&self) -> &'static str {
        "gcp"
    }
    fn track_secret(&self, _secret_name: &str) {
        // GCP provider handles cleanup through its own mechanisms
        // No additional tracking needed
    }
}

#[cfg(feature = "azure")]
struct AzureProviderTestContext {
    vault: Arc<dyn Vault>,
    test_vault_name: String,
    management_client: alien_azure_clients::keyvault::AzureKeyVaultManagementClient,
    secrets_client: alien_azure_clients::keyvault::AzureKeyVaultSecretsClient,
    subscription_id: String,
    resource_group_name: String,
    created_secrets: std::sync::Mutex<HashSet<String>>,
}

#[cfg(feature = "azure")]
impl AsyncTestContext for AzureProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-azure-vault";

        // Get Azure configuration from environment
        let subscription_id = env::var("AZURE_MANAGEMENT_SUBSCRIPTION_ID")
            .expect("AZURE_MANAGEMENT_SUBSCRIPTION_ID not set");
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let client_id =
            env::var("AZURE_MANAGEMENT_CLIENT_ID").expect("AZURE_MANAGEMENT_CLIENT_ID not set");
        let client_secret = env::var("AZURE_MANAGEMENT_CLIENT_SECRET")
            .expect("AZURE_MANAGEMENT_CLIENT_SECRET not set");
        let resource_group_name = env::var("ALIEN_TEST_AZURE_RESOURCE_GROUP")
            .expect("ALIEN_TEST_AZURE_RESOURCE_GROUP not set");

        // Generate unique vault name to avoid conflicts (must be 3-24 alphanumeric characters)
        let test_vault_name = format!(
            "alientest{}",
            Uuid::new_v4().simple().to_string()[..8].to_lowercase()
        );

        let config = AzureClientConfig {
            subscription_id: subscription_id.clone(),
            tenant_id: tenant_id.clone(),
            region: Some("eastus".to_string()),
            credentials: AzureCredentials::ServicePrincipal {
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
            },
            service_overrides: None,
        };

        info!(
            "🔧 Using subscription: {} and resource group: {} for Azure Key Vault testing",
            subscription_id, resource_group_name
        );

        let management_client = AzureKeyVaultManagementClient::new(Client::new(), AzureTokenCache::new(config.clone()));
        let secrets_client = AzureKeyVaultSecretsClient::new(Client::new(), AzureTokenCache::new(config.clone()));

        // Create the actual Azure Key Vault
        Self::create_azure_key_vault(&management_client, &resource_group_name, &test_vault_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to create Azure Key Vault '{}': {:?}",
                    test_vault_name, e
                )
            });

        // Wait for vault to be ready
        info!("⏳ Waiting for Azure Key Vault to be ready...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        info!("✅ Azure Key Vault should be ready for operations");

        // Create the bindings provider vault instance
        let binding = VaultBinding::key_vault(test_vault_name.clone());

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert("AZURE_TENANT_ID".to_string(), tenant_id.clone());
        env_map.insert("AZURE_CLIENT_ID".to_string(), client_id.clone());
        env_map.insert("AZURE_CLIENT_SECRET".to_string(), client_secret.clone());
        env_map.insert("AZURE_SUBSCRIPTION_ID".to_string(), subscription_id.clone());
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "azure".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load Azure bindings provider"),
        );
        let vault = provider.load_vault(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load Azure vault for binding '{}' using vault name '{}': {:?}",
                binding_name, test_vault_name, e
            )
        });

        Self {
            vault,
            test_vault_name,
            management_client,
            secrets_client,
            subscription_id,
            resource_group_name,
            created_secrets: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Azure Key Vault test cleanup...");

        // Clean up created secrets first
        let secrets_to_cleanup = {
            let secrets = self.created_secrets.lock().unwrap();
            secrets.clone()
        };

        for secret_name in secrets_to_cleanup {
            self.cleanup_secret(&secret_name).await;
        }

        // Clean up the vault
        self.cleanup_vault().await;

        info!("✅ Azure Key Vault test cleanup completed");
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl VaultTestContext for AzureProviderTestContext {
    async fn get_vault(&self) -> Arc<dyn Vault> {
        self.vault.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
    fn track_secret(&self, secret_name: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        secrets.insert(secret_name.to_string());
        info!(
            "📝 Tracking secret for cleanup: {}/{}",
            self.test_vault_name, secret_name
        );
    }
}

#[cfg(feature = "azure")]
impl AzureProviderTestContext {
    async fn create_azure_key_vault(
        management_client: &AzureKeyVaultManagementClient,
        resource_group_name: &str,
        vault_name: &str,
    ) -> Result<(), alien_client_core::Error> {
        let tenant_id =
            env::var("AZURE_MANAGEMENT_TENANT_ID").expect("AZURE_MANAGEMENT_TENANT_ID not set");
        let tenant_uuid = Uuid::parse_str(&tenant_id).expect("Invalid tenant ID format");

        // Get the service principal object ID for access policies
        let management_principal_id =
            Self::resolve_service_principal_object_id(management_client).await?;

        // Create access policy for the service principal with secret permissions
        let access_policy = AccessPolicyEntry {
            object_id: management_principal_id,
            tenant_id: tenant_uuid,
            application_id: None,
            permissions: Permissions {
                secrets: vec![
                    PermissionsSecretsItem::Get,
                    PermissionsSecretsItem::Set,
                    PermissionsSecretsItem::List,
                    PermissionsSecretsItem::Delete,
                ],
                keys: vec![],         // Empty keys permissions
                certificates: vec![], // Empty certificates permissions
                storage: vec![],      // Empty storage permissions
            },
        };

        let vault_properties = VaultProperties {
            tenant_id: tenant_uuid,
            sku: Sku {
                name: SkuName::Standard,
                family: SkuFamily::A,
            },
            access_policies: vec![access_policy], // Use access policies for Key Vault
            enable_rbac_authorization: false,     // Disable RBAC to use access policies
            enable_soft_delete: true,
            enabled_for_deployment: false,
            enabled_for_disk_encryption: false,
            enabled_for_template_deployment: false,
            private_endpoint_connections: vec![],
            public_network_access: "Enabled".to_string(),
            soft_delete_retention_in_days: 7,
            vault_uri: None,
            enable_purge_protection: None,
            network_acls: None,
            create_mode: None,
            provisioning_state: None,
            hsm_pool_resource_id: None,
        };

        let vault_params = VaultCreateOrUpdateParameters {
            location: "East US".to_string(),
            properties: vault_properties,
            tags: {
                let mut tags = HashMap::new();
                tags.insert("Environment".to_string(), "Test".to_string());
                tags.insert("Application".to_string(), "alien-test".to_string());
                tags
            },
        };

        info!("🔧 Creating Azure Key Vault: {}", vault_name);
        management_client
            .create_or_update_vault(
                resource_group_name.to_string(),
                vault_name.to_string(),
                vault_params,
            )
            .await?;

        info!("✅ Azure Key Vault created successfully: {}", vault_name);
        Ok(())
    }

    /// Automatically resolve the service principal's object ID by decoding the JWT token
    async fn resolve_service_principal_object_id(
        management_client: &AzureKeyVaultManagementClient,
    ) -> Result<String, alien_client_core::Error> {
        info!("🔍 Auto-resolving object ID from JWT token...");

        // Get a bearer token for Azure Resource Manager (this will contain the oid claim)
        let bearer_token = management_client
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await
            .context(alien_client_core::ErrorData::HttpRequestFailed {
                message: "Failed to get bearer token".to_string(),
            })?;

        // Parse the JWT token to extract the payload (claims)
        let parts: Vec<&str> = bearer_token.split('.').collect();
        if parts.len() != 3 {
            return Err(AlienError::new(
                alien_client_core::ErrorData::InvalidClientConfig {
                    message: "Invalid JWT token format - expected 3 parts".to_string(),
                    errors: None,
                },
            ));
        }

        // Decode the payload (claims) part
        let claims_b64 = parts[1];
        let claims_bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(claims_b64)
            .map_err(|e| {
                AlienError::new(alien_client_core::ErrorData::DataLoadError {
                    message: format!("Failed to decode JWT payload: {}", e),
                })
            })?;

        // Parse the claims as JSON
        let claims_json: serde_json::Value =
            serde_json::from_slice(&claims_bytes).map_err(|e| {
                AlienError::new(alien_client_core::ErrorData::DataLoadError {
                    message: format!("Failed to parse JWT claims JSON: {}", e),
                })
            })?;

        // Extract the oid (object ID) claim from the token
        let object_id = claims_json
            .get("oid")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AlienError::new(alien_client_core::ErrorData::InvalidClientConfig {
                    message: "JWT token does not contain 'oid' claim (object ID)".to_string(),
                    errors: Some(format!("Available claims: {}", claims_json)),
                })
            })?;

        info!("✅ Auto-resolved object ID from JWT: {}", object_id);
        Ok(object_id.to_string())
    }

    async fn cleanup_vault(&self) {
        info!("🧹 Cleaning up Azure Key Vault: {}", self.test_vault_name);

        match self
            .management_client
            .delete_vault(
                self.resource_group_name.clone(),
                self.test_vault_name.clone(),
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Azure Key Vault {} deleted successfully",
                    self.test_vault_name
                );
            }
            Err(e) => {
                if !matches!(
                    e,
                    alien_client_core::Error {
                        error: Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete Azure Key Vault {} during cleanup: {:?}",
                        self.test_vault_name, e
                    );
                }
            }
        }
    }

    async fn cleanup_secret(&self, secret_name: &str) {
        info!(
            "🧹 Cleaning up secret: {}/{}",
            self.test_vault_name, secret_name
        );

        match self
            .secrets_client
            .delete_secret(
                format!("{}.vault.azure.net", self.test_vault_name),
                secret_name.to_string(),
            )
            .await
        {
            Ok(_) => {
                info!(
                    "✅ Secret {}/{} deleted successfully",
                    self.test_vault_name, secret_name
                );
            }
            Err(e) => {
                if !matches!(
                    e,
                    alien_client_core::Error {
                        error: Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }),
                        ..
                    }
                ) {
                    warn!(
                        "Failed to delete secret {}/{} during cleanup: {:?}",
                        self.test_vault_name, secret_name, e
                    );
                }
            }
        }
    }
}

// Test implementations

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
// TODO(CRITICAL): Enable gRPC after local is stateful
// #[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[tokio::test]
async fn test_set_and_get_secret(#[case] ctx: impl VaultTestContext) {
    let vault = ctx.get_vault().await;
    let provider_name = ctx.provider_name();
    let secret_name = format!("test-secret-{}", Uuid::new_v4().simple());
    let secret_value = "test-secret-value";

    // Track the secret for cleanup
    ctx.track_secret(&secret_name);

    // Set the secret
    vault
        .set_secret(&secret_name, secret_value)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to set secret: {:?}", provider_name, e));

    // Small delay for cloud providers to ensure secret is fully available
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }

    // Get the secret
    let retrieved_value = vault
        .get_secret(&secret_name)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get secret: {:?}", provider_name, e));

    assert_eq!(
        secret_value, retrieved_value,
        "[{}] Retrieved secret value mismatch",
        provider_name
    );
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
// TODO(CRITICAL): Enable gRPC after local is stateful
// #[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[tokio::test]
async fn test_delete_secret(#[case] ctx: impl VaultTestContext) {
    let vault = ctx.get_vault().await;
    let provider_name = ctx.provider_name();
    let secret_name = format!("test-secret-delete-{}", Uuid::new_v4().simple());
    let secret_value = "test-secret-value";

    // Track the secret for cleanup
    ctx.track_secret(&secret_name);

    // Set the secret
    vault
        .set_secret(&secret_name, secret_value)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to set secret for delete test: {:?}",
                provider_name, e
            )
        });

    // Small delay for cloud providers to ensure secret is fully available
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }

    // Verify it exists
    let retrieved_value = vault.get_secret(&secret_name).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get secret before delete: {:?}",
            provider_name, e
        )
    });

    assert_eq!(
        secret_value, retrieved_value,
        "[{}] Retrieved secret value mismatch before delete",
        provider_name
    );

    // Small delay for cloud providers before delete to ensure operations are fully propagated
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // Delete the secret
    vault
        .delete_secret(&secret_name)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to delete secret: {:?}", provider_name, e));

    // Verify it's gone - this should fail with a not found error
    let get_result = vault.get_secret(&secret_name).await;
    assert!(
        get_result.is_err(),
        "[{}] Expected error when getting deleted secret, but got: {:?}",
        provider_name,
        get_result
    );
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
// TODO(CRITICAL): Enable gRPC after local is stateful
// #[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[tokio::test]
async fn test_get_nonexistent_secret(#[case] ctx: impl VaultTestContext) {
    let vault = ctx.get_vault().await;
    let provider_name = ctx.provider_name();
    let nonexistent_secret_name = format!("nonexistent-secret-{}", Uuid::new_v4().simple());

    // Try to get a non-existent secret
    let get_result = vault.get_secret(&nonexistent_secret_name).await;

    assert!(
        get_result.is_err(),
        "[{}] Expected error when getting nonexistent secret, but got: {:?}",
        provider_name,
        get_result
    );
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
// TODO(CRITICAL): Enable gRPC after local is stateful
// #[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[tokio::test]
async fn test_update_secret(#[case] ctx: impl VaultTestContext) {
    let vault = ctx.get_vault().await;
    let provider_name = ctx.provider_name();
    let secret_name = format!("test-update-secret-{}", Uuid::new_v4().simple());
    let initial_value = "initial-value";
    let updated_value = "updated-value";

    // Track the secret for cleanup
    ctx.track_secret(&secret_name);

    // Set initial value
    vault
        .set_secret(&secret_name, initial_value)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to set initial secret value: {:?}",
                provider_name, e
            )
        });

    // Small delay for cloud providers to ensure secret is fully available
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
    }

    // Verify initial value
    let retrieved_initial = vault.get_secret(&secret_name).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get initial secret value: {:?}",
            provider_name, e
        )
    });

    assert_eq!(
        initial_value, retrieved_initial,
        "[{}] Initial secret value mismatch",
        provider_name
    );

    // Update the secret (set_secret should handle updates)
    vault
        .set_secret(&secret_name, updated_value)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to update secret: {:?}", provider_name, e));

    // GCP Secret Manager updates can be eventually consistent, so poll briefly.
    let mut retrieved_updated = vault.get_secret(&secret_name).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get updated secret value: {:?}",
            provider_name, e
        )
    });
    if provider_name == "gcp" && retrieved_updated != updated_value {
        for _ in 0..5 {
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            retrieved_updated = vault.get_secret(&secret_name).await.unwrap_or_else(|e| {
                panic!(
                    "[{}] Failed to get updated secret value: {:?}",
                    provider_name, e
                )
            });
            if retrieved_updated == updated_value {
                break;
            }
        }
    }

    assert_eq!(
        updated_value, retrieved_updated,
        "[{}] Updated secret value mismatch",
        provider_name
    );
}
