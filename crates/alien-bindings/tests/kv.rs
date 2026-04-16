#![cfg(test)]

use alien_bindings::{
    traits::{BindingsProviderApi, Kv, PutOptions},
    BindingsProvider,
};

#[cfg(feature = "grpc")]
use alien_bindings::{grpc::run_grpc_server, providers::grpc_provider::GrpcBindingsProvider};
use alien_core::bindings::{self, KvBinding};

#[cfg(feature = "gcp")]
use alien_gcp_clients::firestore::{
    ConcurrencyMode, Database, DatabaseType, FirestoreApi, FirestoreClient,
};
#[cfg(feature = "gcp")]
use alien_gcp_clients::longrunning::OperationResult;
#[cfg(feature = "gcp")]
use alien_gcp_clients::{GcpClientConfig, GcpCredentials};
#[cfg(any(feature = "gcp", feature = "aws"))]
use reqwest::Client;

#[cfg(feature = "aws")]
use alien_aws_clients::dynamodb::{
    AttributeDefinition, CreateTableRequest, DeleteTableRequest, DescribeTableRequest, DynamoDbApi,
    DynamoDbClient, KeySchemaElement,
};
#[cfg(feature = "aws")]
use alien_aws_clients::{AwsClientConfig, AwsCredentialProvider, AwsCredentials};
#[cfg(feature = "aws")]
use alien_client_core::{Error as CloudError, ErrorData as CloudErrorData};

#[cfg(feature = "azure")]
use alien_azure_clients::tables::{AzureTableManagementClient, TableManagementApi};
#[cfg(feature = "azure")]
use alien_azure_clients::{AzureClientConfig, AzureCredentials, AzureTokenCache};

use async_trait::async_trait;
use rstest::rstest;
use std::path::PathBuf as StdPathBuf;
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tempfile::TempDir;
use test_context::AsyncTestContext;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root::get_workspace_root;

const GRPC_BINDING_NAME: &str = "test-grpc-kv-binding";

fn load_test_env() {
    // Load .env.test from the workspace root
    let root: StdPathBuf = get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
}

#[async_trait]
pub trait KvTestContext: AsyncTestContext + Send + Sync {
    async fn get_kv(&self) -> Arc<dyn Kv>;
    fn provider_name(&self) -> &'static str;
    fn track_key(&self, key: &str);
}

// --- Local Provider Context ---
struct LocalProviderTestContext {
    kv: Arc<dyn Kv>,
    _temp_dir: TempDir,
    created_keys: std::sync::Mutex<HashSet<String>>,
}

impl AsyncTestContext for LocalProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-local-kv";
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir for local kv test");
        let temp_dir_path = temp_dir.path().to_str().unwrap().to_string();

        let binding = KvBinding::local(temp_dir_path.clone());

        let mut env_map: HashMap<String, String> = env::vars().collect();
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load bindings provider"),
        );
        let kv = provider.load_kv(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load Local KV for binding '{}' using temp dir '{}': {:?}",
                binding_name, temp_dir_path, e
            )
        });

        Self {
            kv,
            _temp_dir: temp_dir,
            created_keys: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        // Clean up created keys
        let keys_to_cleanup = {
            let keys = self.created_keys.lock().unwrap();
            keys.clone()
        };

        for key in keys_to_cleanup {
            self.cleanup_key(&key).await;
        }
    }
}

#[async_trait]
impl KvTestContext for LocalProviderTestContext {
    async fn get_kv(&self) -> Arc<dyn Kv> {
        self.kv.clone()
    }
    fn provider_name(&self) -> &'static str {
        "local"
    }
    fn track_key(&self, key: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.insert(key.to_string());
    }
}

impl LocalProviderTestContext {
    async fn cleanup_key(&self, key: &str) {
        match self.kv.delete(key).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - key might already be deleted
            }
        }
    }
}

// --- gRPC Provider Context ---
#[cfg(feature = "grpc")]
struct GrpcProviderTestContext {
    kv: Arc<dyn Kv>,
    _server_handle:
        JoinHandle<Result<(), alien_error::AlienError<alien_bindings::error::ErrorData>>>,
    _temp_data_dir: TempDir,
    created_keys: std::sync::Mutex<HashSet<String>>,
}

#[cfg(feature = "grpc")]
impl AsyncTestContext for GrpcProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let temp_data_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for ALIEN_DATA_DIR (gRPC server)");
        let temp_data_dir_path = temp_data_dir.path().to_str().unwrap().to_string();

        // Env map for the BindingsProvider used by the gRPC server
        let server_binding = KvBinding::local(temp_data_dir_path.clone());

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

        let kv = grpc_provider
            .load_kv(GRPC_BINDING_NAME)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Grpc KV for binding '{}' using ALIEN_BINDINGS_GRPC_ADDRESS='{}': {:?}",
                    GRPC_BINDING_NAME, server_addr_str, e
                )
            });

        Self {
            kv,
            _server_handle: server_handle,
            _temp_data_dir: temp_data_dir,
            created_keys: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        // Clean up created keys
        let keys_to_cleanup = {
            let keys = self.created_keys.lock().unwrap();
            keys.clone()
        };

        for key in keys_to_cleanup {
            self.cleanup_key(&key).await;
        }

        self._server_handle.abort();
    }
}

#[cfg(feature = "grpc")]
#[async_trait]
impl KvTestContext for GrpcProviderTestContext {
    async fn get_kv(&self) -> Arc<dyn Kv> {
        self.kv.clone()
    }
    fn provider_name(&self) -> &'static str {
        "grpc"
    }
    fn track_key(&self, key: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.insert(key.to_string());
    }
}

#[cfg(feature = "grpc")]
impl GrpcProviderTestContext {
    async fn cleanup_key(&self, key: &str) {
        match self.kv.delete(key).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - key might already be deleted
            }
        }
    }
}

// --- AWS Provider Context ---
#[cfg(feature = "aws")]
struct AwsProviderTestContext {
    kv: Arc<dyn Kv>,
    dynamodb_client: DynamoDbClient,
    table_name: String,
    created_keys: std::sync::Mutex<HashSet<String>>,
}

#[cfg(feature = "aws")]
impl AsyncTestContext for AwsProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-aws-kv";

        let region = env::var("AWS_MANAGEMENT_REGION").expect("AWS_MANAGEMENT_REGION must be set");
        let access_key = env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set");
        let secret_key = env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set");
        let account_id =
            env::var("AWS_MANAGEMENT_ACCOUNT_ID").expect("AWS_MANAGEMENT_ACCOUNT_ID must be set");

        // Generate unique table name
        let table_name = format!(
            "alien-test-kv-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
        );

        // Create DynamoDB client for table management
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

        let dynamodb_client = DynamoDbClient::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        info!("🚀 Creating DynamoDB table for KV test: {}", table_name);

        // Create the table
        Self::create_kv_table(&dynamodb_client, &table_name)
            .await
            .expect("Failed to create test table for AWS KV test");

        info!("✅ Table {} created successfully", table_name);

        // Create KV binding with the new table
        let binding = KvBinding::dynamodb(table_name.clone(), region.clone());

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
        let kv = provider.load_kv(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load AWS KV for binding '{}' using table '{}': {:?}",
                binding_name, table_name, e
            )
        });

        Self {
            kv,
            dynamodb_client,
            table_name,
            created_keys: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting AWS KV test cleanup...");

        // Clean up created keys first
        let keys_to_cleanup = {
            let keys = self.created_keys.lock().unwrap();
            keys.clone()
        };

        for key in keys_to_cleanup {
            self.cleanup_key(&key).await;
        }

        // Clean up the table
        self.cleanup_table().await;

        info!("✅ AWS KV test cleanup completed");
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl KvTestContext for AwsProviderTestContext {
    async fn get_kv(&self) -> Arc<dyn Kv> {
        self.kv.clone()
    }
    fn provider_name(&self) -> &'static str {
        "aws"
    }
    fn track_key(&self, key: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.insert(key.to_string());
    }
}

#[cfg(feature = "aws")]
impl AwsProviderTestContext {
    async fn create_kv_table(client: &DynamoDbClient, table_name: &str) -> Result<(), CloudError> {
        info!("🏗️ Creating KV table: {}", table_name);

        let create_request = CreateTableRequest::builder()
            .table_name(table_name.to_string())
            .billing_mode("PAY_PER_REQUEST".to_string())
            .key_schema(vec![
                KeySchemaElement::builder()
                    .attribute_name("pk".to_string())
                    .key_type("HASH".to_string())
                    .build(),
                KeySchemaElement::builder()
                    .attribute_name("sk".to_string())
                    .key_type("RANGE".to_string())
                    .build(),
            ])
            .attribute_definitions(vec![
                AttributeDefinition::builder()
                    .attribute_name("pk".to_string())
                    .attribute_type("S".to_string())
                    .build(),
                AttributeDefinition::builder()
                    .attribute_name("sk".to_string())
                    .attribute_type("S".to_string())
                    .build(),
            ])
            .build();

        match client.create_table(create_request).await {
            Ok(_) => {
                info!("✅ Table {} created successfully", table_name);

                // Wait for table to become active
                Self::wait_for_table_active(client, table_name).await?;
                Ok(())
            }
            Err(e) => {
                warn!("Failed to create table {}: {:?}", table_name, e);
                Err(e)
            }
        }
    }

    async fn wait_for_table_active(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> Result<(), CloudError> {
        info!("⏳ Waiting for table {} to become active...", table_name);
        let mut attempts = 0;
        let max_attempts = 30; // 5 minutes max wait

        loop {
            attempts += 1;

            let describe_request = DescribeTableRequest::builder()
                .table_name(table_name.to_string())
                .build();

            match client.describe_table(describe_request).await {
                Ok(response) => {
                    if response.table.table_status.as_deref() == Some("ACTIVE") {
                        info!("✅ Table {} is now active!", table_name);
                        return Ok(());
                    }

                    if attempts >= max_attempts {
                        return Err(CloudError::new(CloudErrorData::Timeout {
                            message: format!(
                                "Table {} didn't become active within 5 minutes",
                                table_name
                            ),
                        }));
                    }

                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
                Err(e) => {
                    warn!("Failed to describe table status: {:?}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }
        }
    }

    async fn cleanup_table(&self) {
        info!("🧹 Cleaning up table: {}", self.table_name);

        let delete_request = DeleteTableRequest::builder()
            .table_name(self.table_name.clone())
            .build();

        match self.dynamodb_client.delete_table(delete_request).await {
            Ok(_) => {
                info!("✅ Table {} deletion completed", self.table_name);
            }
            Err(e) => {
                match &e.error {
                    Some(CloudErrorData::RemoteResourceNotFound { .. }) => {
                        info!(
                            "Table {} already doesn't exist (skipping cleanup)",
                            self.table_name
                        );
                    }
                    _ => {
                        warn!(
                            "Failed to delete table {} during cleanup: {:?}",
                            self.table_name, e
                        );
                        // Still continue cleanup to avoid retry loops
                    }
                }
            }
        }
    }

    async fn cleanup_key(&self, key: &str) {
        match self.kv.delete(key).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - key might already be deleted
            }
        }
    }
}

// --- GCP Provider Context ---
#[cfg(feature = "gcp")]
struct GcpProviderTestContext {
    kv: Arc<dyn Kv>,
    firestore_client: FirestoreClient,
    project_id: String,
    database_id: String,
    collection_name: String,
    created_keys: std::sync::Mutex<HashSet<String>>,
}

#[cfg(feature = "gcp")]
impl AsyncTestContext for GcpProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-gcp-kv";

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

        // Create Firestore client for database management
        let firestore_config = GcpClientConfig {
            project_id: project_id.clone(),
            region: gcp_region.clone(),
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json.clone(),
            },
            service_overrides: None,
            project_number: None,
        };

        let firestore_client = FirestoreClient::new(Client::new(), firestore_config);

        // Generate unique database and collection names
        let database_id = format!(
            "alien-test-kv-db-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
        );
        let collection_name = format!(
            "alien-test-kv-col-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..8].to_lowercase()
        );

        info!(
            "🚀 Creating Firestore database for KV test: {}",
            database_id
        );

        // Create the database
        let database = Database::builder()
            .location_id(gcp_region.clone())
            .r#type(DatabaseType::FirestoreNative)
            .concurrency_mode(ConcurrencyMode::Optimistic)
            .build();

        let create_operation = firestore_client
            .create_database(database_id.clone(), database)
            .await
            .expect("Failed to create test database for KV test");

        info!("✅ Database creation initiated, waiting for completion...");

        // Wait for database creation to complete
        if let Some(operation_name) = create_operation.name {
            let result = Self::wait_for_operation(&firestore_client, &operation_name, 180).await;
            if let Err(e) = result {
                panic!("Database creation failed: {}", e);
            }
        }

        info!("✅ Database {} created successfully", database_id);

        // Create KV binding with the new database
        let binding = KvBinding::firestore(
            project_id.clone(),
            database_id.clone(),
            collection_name.clone(),
        );

        let mut env_map: HashMap<String, String> = HashMap::new();
        env_map.insert(
            "GOOGLE_SERVICE_ACCOUNT_KEY".to_string(),
            gcp_credentials_json,
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
        let kv = provider
            .load_kv(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load GCP KV for binding '{}' using project '{}', database '{}', collection '{}': {:?}",
                    binding_name, project_id, database_id, collection_name, e
                )
            });

        Self {
            kv,
            firestore_client,
            project_id,
            database_id,
            collection_name,
            created_keys: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting GCP KV test cleanup...");

        // Clean up created keys first
        let keys_to_cleanup = {
            let keys = self.created_keys.lock().unwrap();
            keys.clone()
        };

        for key in keys_to_cleanup {
            self.cleanup_key(&key).await;
        }

        // Clean up the database
        self.cleanup_database().await;

        info!("✅ GCP KV test cleanup completed");
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl KvTestContext for GcpProviderTestContext {
    async fn get_kv(&self) -> Arc<dyn Kv> {
        self.kv.clone()
    }
    fn provider_name(&self) -> &'static str {
        "gcp"
    }
    fn track_key(&self, key: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.insert(key.to_string());
    }
}

#[cfg(feature = "gcp")]
impl GcpProviderTestContext {
    async fn wait_for_operation(
        firestore_client: &FirestoreClient,
        operation_name: &str,
        timeout_seconds: u64,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(timeout_seconds);
        let mut check_count = 0;
        let max_checks = timeout_seconds / 5; // Maximum number of checks

        loop {
            check_count += 1;

            if start_time.elapsed() > timeout_duration || check_count > max_checks {
                return Err(format!(
                    "Timeout waiting for operation {} to complete after {} checks",
                    operation_name, check_count
                )
                .into());
            }

            match firestore_client
                .get_operation(operation_name.to_string())
                .await
            {
                Ok(operation) => {
                    if operation.done == Some(true) {
                        // Check if operation succeeded
                        match operation.result {
                            Some(OperationResult::Error { error }) => {
                                return Err(format!("Operation failed: {}", error.message).into());
                            }
                            Some(OperationResult::Response { .. }) | None => {
                                info!(
                                    "✅ Operation {} completed successfully after {} checks!",
                                    operation_name, check_count
                                );
                                return Ok(());
                            }
                        }
                    }

                    // Log progress every 10 checks to avoid spam
                    if check_count % 10 == 0 {
                        info!(
                            "⏳ Operation {} still running after {} checks ({}s), waiting...",
                            operation_name,
                            check_count,
                            start_time.elapsed().as_secs()
                        );
                    }

                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    // Check if this is a "operation not found" error, which often means it completed
                    if let Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) =
                        &e.error
                    {
                        info!("✅ Operation {} appears to have completed (operation not found after {} checks)", operation_name, check_count);
                        return Ok(());
                    } else {
                        warn!(
                            "Error checking operation status (check {}): {:?}",
                            check_count, e
                        );

                        // If we can't check the operation status, treat it as potentially completed
                        // after a reasonable number of failed attempts during cleanup
                        if check_count > 20 {
                            warn!("Too many failed attempts to check operation status, assuming operation completed");
                            return Ok(());
                        }

                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        }
    }

    async fn cleanup_database(&self) {
        info!("🧹 Cleaning up database: {}", self.database_id);

        match self
            .firestore_client
            .delete_database(self.database_id.clone(), None)
            .await
        {
            Ok(operation) => {
                info!("✅ Database {} deletion initiated", self.database_id);

                // Wait for deletion to complete, but with a shorter timeout for cleanup
                if let Some(op_name) = operation.name {
                    match Self::wait_for_operation(&self.firestore_client, &op_name, 120).await {
                        Ok(_) => {
                            info!("✅ Database {} deletion completed", self.database_id);
                        }
                        Err(e) => {
                            warn!(
                                "Database {} deletion timed out, but continuing cleanup: {}",
                                self.database_id, e
                            );
                            // Don't fail the cleanup even if deletion times out
                        }
                    }
                }
            }
            Err(e) => {
                match &e.error {
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) => {
                        info!(
                            "Database {} already doesn't exist (skipping cleanup)",
                            self.database_id
                        );
                    }
                    _ => {
                        warn!(
                            "Failed to delete database {} during cleanup: {:?}",
                            self.database_id, e
                        );
                        // Still continue cleanup to avoid retry loops
                    }
                }
            }
        }
    }

    async fn cleanup_key(&self, key: &str) {
        match self.kv.delete(key).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - key might already be deleted
            }
        }
    }
}

// --- Azure Provider Context ---
#[cfg(feature = "azure")]
struct AzureProviderTestContext {
    kv: Arc<dyn Kv>,
    management_client: AzureTableManagementClient,
    resource_group_name: String,
    account_name: String,
    table_name: String,
    created_keys: std::sync::Mutex<HashSet<String>>,
}

#[cfg(feature = "azure")]
impl AsyncTestContext for AzureProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        tracing_subscriber::fmt::try_init().ok();

        let binding_name = "test-azure-kv";

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
        let account_name = env::var("ALIEN_TEST_AZURE_STORAGE_ACCOUNT")
            .expect("ALIEN_TEST_AZURE_STORAGE_ACCOUNT must be set in .env.test");

        // Generate unique table name
        let table_name = format!(
            "alientestkv{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..8].to_lowercase()
        );

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

        let management_client = AzureTableManagementClient::new(
            Client::new(),
            AzureTokenCache::new(client_config.clone()),
        );

        info!("🚀 Creating Azure table for KV test: {}", table_name);

        // Create the table
        management_client
            .create_table(&resource_group_name, &account_name, &table_name)
            .await
            .expect("Failed to create test table for Azure KV test");

        info!("✅ Table {} created successfully", table_name);

        // Create KV binding with the new table
        let binding = KvBinding::table_storage(
            resource_group_name.clone(),
            account_name.clone(),
            table_name.clone(),
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
        let kv = provider.load_kv(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load Azure KV for binding '{}' using account '{}', table '{}': {:?}",
                binding_name, account_name, table_name, e
            )
        });

        Self {
            kv,
            management_client,
            resource_group_name,
            account_name,
            table_name,
            created_keys: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting Azure KV test cleanup...");

        // Clean up created keys first
        let keys_to_cleanup = {
            let keys = self.created_keys.lock().unwrap();
            keys.clone()
        };

        for key in keys_to_cleanup {
            self.cleanup_key(&key).await;
        }

        // Clean up the table
        self.cleanup_table().await;

        info!("✅ Azure KV test cleanup completed");
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl KvTestContext for AzureProviderTestContext {
    async fn get_kv(&self) -> Arc<dyn Kv> {
        self.kv.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
    fn track_key(&self, key: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.insert(key.to_string());
    }
}

#[cfg(feature = "azure")]
impl AzureProviderTestContext {
    async fn cleanup_table(&self) {
        info!("🧹 Cleaning up table: {}", self.table_name);

        match self
            .management_client
            .delete_table(
                &self.resource_group_name,
                &self.account_name,
                &self.table_name,
            )
            .await
        {
            Ok(_) => {
                info!("✅ Table {} deletion completed", self.table_name);
            }
            Err(e) => {
                match &e.error {
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. }) => {
                        info!(
                            "Table {} already doesn't exist (skipping cleanup)",
                            self.table_name
                        );
                    }
                    _ => {
                        warn!(
                            "Failed to delete table {} during cleanup: {:?}",
                            self.table_name, e
                        );
                        // Still continue cleanup to avoid retry loops
                    }
                }
            }
        }
    }

    async fn cleanup_key(&self, key: &str) {
        match self.kv.delete(key).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - key might already be deleted
            }
        }
    }
}

// --- Kubernetes Provider Context ---
#[cfg(feature = "kubernetes")]
struct KubernetesProviderTestContext {
    kv: Arc<dyn Kv>,
    _temp_dir: TempDir,
    created_keys: std::sync::Mutex<HashSet<String>>,
}

#[cfg(feature = "kubernetes")]
impl AsyncTestContext for KubernetesProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-k8s-kv";

        // Use Redis for Kubernetes - in tests we'll use a mock/local implementation
        let temp_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for K8s KV test (local fallback)");

        let binding = KvBinding::local(temp_dir.path().to_string_lossy().to_string());

        let mut env_map: HashMap<String, String> = env::vars().collect();
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);
        env_map.insert(
            "ALIEN_DEPLOYMENT_TYPE".to_string(),
            "kubernetes".to_string(),
        );

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load bindings provider"),
        );
        let kv = provider.load_kv(binding_name).await.unwrap_or_else(|e| {
            panic!(
                "Failed to load Kubernetes KV (local fallback) for binding '{}': {:?}",
                binding_name, e
            )
        });

        Self {
            kv,
            _temp_dir: temp_dir,
            created_keys: std::sync::Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        // Clean up created keys
        let keys_to_cleanup = {
            let keys = self.created_keys.lock().unwrap();
            keys.clone()
        };

        for key in keys_to_cleanup {
            self.cleanup_key(&key).await;
        }
    }
}

#[cfg(feature = "kubernetes")]
#[async_trait]
impl KvTestContext for KubernetesProviderTestContext {
    async fn get_kv(&self) -> Arc<dyn Kv> {
        self.kv.clone()
    }
    fn provider_name(&self) -> &'static str {
        "kubernetes"
    }
    fn track_key(&self, key: &str) {
        let mut keys = self.created_keys.lock().unwrap();
        keys.insert(key.to_string());
    }
}

#[cfg(feature = "kubernetes")]
impl KubernetesProviderTestContext {
    async fn cleanup_key(&self, key: &str) {
        match self.kv.delete(key).await {
            Ok(_) => {
                // Successfully deleted
            }
            Err(_) => {
                // Ignore cleanup errors - key might already be deleted
            }
        }
    }
}

// --- Test implementations ---

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_put_and_get(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let key = format!("test-put-get-{}", Uuid::new_v4().simple());
    let value = b"Hello, Alien KV!".to_vec();

    ctx.track_key(&key);

    // Put the value
    let put_result = kv
        .put(&key, value.clone(), None)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to put key-value pair: {:?}", provider_name, e));

    assert!(
        put_result,
        "[{}] Put operation should return true",
        provider_name
    );

    // Get the value
    let retrieved_value = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get value: {:?}", provider_name, e))
        .unwrap_or_else(|| panic!("[{}] Value should exist after put", provider_name));

    assert_eq!(
        value, retrieved_value,
        "[{}] Retrieved value should match original",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_delete_operation(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let key = format!("test-delete-{}", Uuid::new_v4().simple());
    let value = b"Delete test data".to_vec();

    ctx.track_key(&key);

    // Put a value
    kv.put(&key, value.clone(), None).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to put value for delete test: {:?}",
            provider_name, e
        )
    });

    // Verify it exists
    let exists_before = kv.exists(&key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to check existence before delete: {:?}",
            provider_name, e
        )
    });
    assert!(
        exists_before,
        "[{}] Key should exist before delete",
        provider_name
    );

    // Delete the key
    kv.delete(&key)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to delete key: {:?}", provider_name, e));

    // Verify it's gone
    let exists_after = kv.exists(&key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to check existence after delete: {:?}",
            provider_name, e
        )
    });
    assert!(
        !exists_after,
        "[{}] Key should not exist after delete",
        provider_name
    );

    let get_result = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get deleted key: {:?}", provider_name, e));
    assert!(
        get_result.is_none(),
        "[{}] Get should return None for deleted key",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_exists_operation(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let existing_key = format!("test-exists-{}", Uuid::new_v4().simple());
    let nonexistent_key = format!("test-nonexistent-{}", Uuid::new_v4().simple());
    let value = b"Exists test data".to_vec();

    ctx.track_key(&existing_key);

    // Test non-existent key
    let exists_before = kv.exists(&nonexistent_key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to check existence of non-existent key: {:?}",
            provider_name, e
        )
    });
    assert!(
        !exists_before,
        "[{}] Non-existent key should not exist",
        provider_name
    );

    // Put a value
    kv.put(&existing_key, value.clone(), None)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put value for exists test: {:?}",
                provider_name, e
            )
        });

    // Test existing key
    let exists_after = kv.exists(&existing_key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to check existence of existing key: {:?}",
            provider_name, e
        )
    });
    assert!(
        exists_after,
        "[{}] Existing key should exist",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_put_if_not_exists(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let key = format!("test-if-not-exists-{}", Uuid::new_v4().simple());
    let value1 = b"First value".to_vec();
    let value2 = b"Second value".to_vec();

    ctx.track_key(&key);

    let options = Some(PutOptions {
        ttl: None,
        if_not_exists: true,
    });

    // First put should succeed
    let put_result1 = kv
        .put(&key, value1.clone(), options.clone())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed first put with if_not_exists: {:?}",
                provider_name, e
            )
        });
    assert!(
        put_result1,
        "[{}] First put with if_not_exists should succeed",
        provider_name
    );

    // Verify the value was set
    let retrieved_value = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to get value after first put: {:?}",
                provider_name, e
            )
        })
        .unwrap_or_else(|| panic!("[{}] Value should exist after first put", provider_name));
    assert_eq!(
        value1, retrieved_value,
        "[{}] Retrieved value should match first value",
        provider_name
    );

    // Second put should fail (key already exists)
    let put_result2 = kv
        .put(&key, value2.clone(), options)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed second put with if_not_exists: {:?}",
                provider_name, e
            )
        });
    assert!(
        !put_result2,
        "[{}] Second put with if_not_exists should fail",
        provider_name
    );

    // Verify the value wasn't changed
    let retrieved_value2 = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to get value after second put: {:?}",
                provider_name, e
            )
        })
        .unwrap_or_else(|| {
            panic!(
                "[{}] Value should still exist after second put",
                provider_name
            )
        });
    assert_eq!(
        value1, retrieved_value2,
        "[{}] Value should not change after failed if_not_exists",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_ttl_expiry(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let key = format!("test-ttl-{}", Uuid::new_v4().simple());
    let value = b"TTL test data".to_vec();

    ctx.track_key(&key);

    let options = Some(PutOptions {
        ttl: Some(Duration::from_secs(2)), // 2 second TTL
        if_not_exists: false,
    });

    // Put the value with TTL
    kv.put(&key, value.clone(), options)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to put value with TTL: {:?}", provider_name, e));

    // Immediately check that the value exists
    let exists_immediately = kv.exists(&key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to check existence immediately after TTL put: {:?}",
            provider_name, e
        )
    });
    assert!(
        exists_immediately,
        "[{}] Key should exist immediately after put with TTL",
        provider_name
    );

    let value_immediately = kv.get(&key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get value immediately after TTL put: {:?}",
            provider_name, e
        )
    });
    assert!(
        value_immediately.is_some(),
        "[{}] Value should exist immediately after put with TTL",
        provider_name
    );

    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Check that the value has expired (logical expiry)
    let exists_after_ttl = kv.exists(&key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to check existence after TTL expiry: {:?}",
            provider_name, e
        )
    });
    assert!(
        !exists_after_ttl,
        "[{}] Key should not exist after TTL expiry",
        provider_name
    );

    let value_after_ttl = kv.get(&key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get value after TTL expiry: {:?}",
            provider_name, e
        )
    });
    assert!(
        value_after_ttl.is_none(),
        "[{}] Value should not exist after TTL expiry",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_scan_prefix(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let unique_id = Uuid::new_v4().simple();
    let prefix = format!("test-scan-{}", unique_id);

    // Create test data with the prefix
    let test_keys = vec![
        format!("{}:key1", prefix),
        format!("{}:key2", prefix),
        format!("{}:dir1:key3", prefix),
        format!("{}:dir1:key4", prefix),
        format!("{}:dir2:key5", prefix),
    ];

    let other_key = format!("other-prefix-{}:key6", unique_id);

    // Track all keys for cleanup
    for key in &test_keys {
        ctx.track_key(key);
    }
    ctx.track_key(&other_key);

    // Put test data
    for (i, key) in test_keys.iter().enumerate() {
        let value = format!("value{}", i + 1).into_bytes();
        kv.put(key, value, None).await.unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put test data for key '{}': {:?}",
                provider_name, key, e
            )
        });
    }

    // Put data with different prefix (should not be returned)
    kv.put(&other_key, b"other value".to_vec(), None)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to put other key: {:?}", provider_name, e));

    // Small delay for eventual consistency in cloud providers
    if matches!(provider_name, "aws" | "gcp" | "azure") {
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    // Scan with prefix
    let scan_result = kv
        .scan_prefix(&prefix, Some(10), None)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to scan with prefix '{}': {:?}",
                provider_name, prefix, e
            )
        });

    // Verify results
    assert!(
        !scan_result.items.is_empty(),
        "[{}] Scan should return some items",
        provider_name
    );
    assert!(
        scan_result.items.len() <= test_keys.len(),
        "[{}] Scan should not return more items than we put",
        provider_name
    );

    // Check that all returned keys have the correct prefix
    for (key, _value) in &scan_result.items {
        assert!(
            key.starts_with(&prefix),
            "[{}] All returned keys should start with prefix '{}', but got '{}'",
            provider_name,
            prefix,
            key
        );
    }

    // Check that the other key is not included
    let other_key_found = scan_result.items.iter().any(|(key, _)| key == &other_key);
    assert!(
        !other_key_found,
        "[{}] Other key should not be included in prefix scan",
        provider_name
    );

    // Test pagination if supported
    if scan_result.items.len() > 2 {
        let limited_scan = kv
            .scan_prefix(&prefix, Some(2), None)
            .await
            .unwrap_or_else(|e| panic!("[{}] Failed to scan with limit: {:?}", provider_name, e));

        assert!(
            limited_scan.items.len() <= 2,
            "[{}] Limited scan should respect limit",
            provider_name
        );
    }
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_key_validation(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();

    // Test empty key
    let empty_key_result = kv.put("", b"value".to_vec(), None).await;
    assert!(
        empty_key_result.is_err(),
        "[{}] Empty key should be rejected",
        provider_name
    );

    // Test key too long (> 512 bytes)
    let long_key = "a".repeat(513);
    let long_key_result = kv.put(&long_key, b"value".to_vec(), None).await;
    assert!(
        long_key_result.is_err(),
        "[{}] Key exceeding 512 bytes should be rejected",
        provider_name
    );

    // Test invalid characters
    let invalid_key = "key with spaces and @#$%";
    let invalid_key_result = kv.put(invalid_key, b"value".to_vec(), None).await;
    assert!(
        invalid_key_result.is_err(),
        "[{}] Key with invalid characters should be rejected",
        provider_name
    );

    // Test forward slash specifically (now disallowed globally for consistency)
    let forward_slash_key = "test/with/slashes";
    let slash_result = kv.put(forward_slash_key, b"value".to_vec(), None).await;
    assert!(
        slash_result.is_err(),
        "[{}] Key with forward slashes should be rejected globally",
        provider_name
    );

    // Test valid key (using globally allowed characters)
    let valid_key = "valid-key_123:test.path.ext";
    ctx.track_key(valid_key);
    let valid_key_result = kv.put(valid_key, b"value".to_vec(), None).await;
    assert!(
        valid_key_result.is_ok(),
        "[{}] Valid key should be accepted",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_value_validation(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let key = format!("test-value-validation-{}", Uuid::new_v4().simple());

    ctx.track_key(&key);

    // Test empty value (should be allowed)
    // let empty_value_result = kv.put(&key, vec![], None).await;
    // assert!(empty_value_result.is_ok(), "[{}] Empty value should be allowed", provider_name);

    // Test value too large (> 24 KiB)
    let large_value = vec![0u8; 24_577]; // 24KiB + 1 byte
    let large_value_result = kv.put(&key, large_value, None).await;
    assert!(
        large_value_result.is_err(),
        "[{}] Value exceeding 24KiB should be rejected",
        provider_name
    );

    // Test maximum allowed value size (24 KiB)
    let max_value = vec![42u8; 24_576]; // Exactly 24KiB
    let max_value_result = kv.put(&key, max_value.clone(), None).await;
    assert!(
        max_value_result.is_ok(),
        "[{}] Value of exactly 24KiB should be allowed",
        provider_name
    );

    // Verify the large value was stored correctly
    let retrieved_value = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get large value: {:?}", provider_name, e))
        .unwrap_or_else(|| panic!("[{}] Large value should exist", provider_name));
    assert_eq!(
        max_value, retrieved_value,
        "[{}] Retrieved large value should match original",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_get_nonexistent_key(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let nonexistent_key = format!("nonexistent-{}", Uuid::new_v4().simple());

    // Try to get a non-existent key
    let get_result = kv.get(&nonexistent_key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get non-existent key: {:?}",
            provider_name, e
        )
    });

    assert!(
        get_result.is_none(),
        "[{}] Non-existent key should return None",
        provider_name
    );

    // Check exists for non-existent key
    let exists_result = kv.exists(&nonexistent_key).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to check existence of non-existent key: {:?}",
            provider_name, e
        )
    });

    assert!(
        !exists_result,
        "[{}] Non-existent key should not exist",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_overwrite_value(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let key = format!("test-overwrite-{}", Uuid::new_v4().simple());
    let value1 = b"Original value".to_vec();
    let value2 = b"Updated value".to_vec();

    ctx.track_key(&key);

    // Put initial value
    kv.put(&key, value1.clone(), None)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to put initial value: {:?}", provider_name, e));

    // Verify initial value
    let retrieved_value1 = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get initial value: {:?}", provider_name, e))
        .unwrap_or_else(|| panic!("[{}] Initial value should exist", provider_name));
    assert_eq!(
        value1, retrieved_value1,
        "[{}] Retrieved initial value should match",
        provider_name
    );

    // Overwrite with new value
    kv.put(&key, value2.clone(), None)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to overwrite value: {:?}", provider_name, e));

    // Verify new value
    let retrieved_value2 = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to get overwritten value: {:?}",
                provider_name, e
            )
        })
        .unwrap_or_else(|| panic!("[{}] Overwritten value should exist", provider_name));
    assert_eq!(
        value2, retrieved_value2,
        "[{}] Retrieved overwritten value should match new value",
        provider_name
    );
}

#[rstest]
#[cfg_attr(feature = "local", case::local(LocalProviderTestContext::setup().await))]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_binary_data(#[case] ctx: impl KvTestContext) {
    let kv = ctx.get_kv().await;
    let provider_name = ctx.provider_name();
    let key = format!("test-binary-{}", Uuid::new_v4().simple());

    // Create binary data with various byte values
    let binary_data: Vec<u8> = (0..=255).collect();

    ctx.track_key(&key);

    // Put binary data
    kv.put(&key, binary_data.clone(), None)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to put binary data: {:?}", provider_name, e));

    // Get and verify binary data
    let retrieved_data = kv
        .get(&key)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get binary data: {:?}", provider_name, e))
        .unwrap_or_else(|| panic!("[{}] Binary data should exist", provider_name));

    assert_eq!(
        binary_data, retrieved_data,
        "[{}] Retrieved binary data should match original",
        provider_name
    );
}
