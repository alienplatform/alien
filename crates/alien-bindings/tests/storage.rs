#![cfg(test)]

use alien_bindings::{
    traits::{BindingsProviderApi, Storage},
    BindingsProvider,
};

#[cfg(feature = "grpc")]
use alien_bindings::{grpc::run_grpc_server, providers::grpc_provider::GrpcBindingsProvider};
use alien_core::bindings::{self, StorageBinding};

// Now using unified BindingsProvider instead of platform-specific providers

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::TryStreamExt;
use object_store::{
    path::Path, GetOptions, GetRange as OsGetRange, MultipartUpload, ObjectMeta, PutMode,
    PutMultipartOpts, PutOptions,
};
use rstest::rstest;
use std::path::PathBuf as StdPathBuf;
use std::{collections::HashMap, env, sync::Arc};
use tempfile::TempDir;
use test_context::AsyncTestContext;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use workspace_root::get_workspace_root;

const GRPC_BINDING_NAME: &str = "test-grpc-storage-binding";

fn load_test_env() {
    // Load .env.test from the workspace root
    let root: StdPathBuf = get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
}

#[async_trait]
pub trait StorageTestContext: AsyncTestContext + Send + Sync {
    async fn get_storage(&self) -> Arc<dyn Storage>;
    fn provider_name(&self) -> &'static str;
}

// --- Local Provider Context ---
struct LocalProviderTestContext {
    storage: Arc<dyn Storage>,
    _temp_dir: TempDir, // Keep TempDir to ensure it's cleaned up on drop
}

impl AsyncTestContext for LocalProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-local-storage";
        let temp_dir =
            tempfile::tempdir().expect("Failed to create temp dir for local storage test");
        let temp_dir_path = temp_dir.path().to_str().unwrap().to_string();

        let binding = StorageBinding::local(temp_dir_path.clone());

        let mut env_map: HashMap<String, String> = env::vars().collect();
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());

        let provider = Arc::new(
            BindingsProvider::from_env(env_map)
                .await
                .expect("Failed to load bindings provider"),
        );
        let storage = provider
            .load_storage(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Local storage for binding '{}' using ALIEN_DATA_DIR='{}': {:?}",
                    binding_name, temp_dir_path, e
                )
            });
        Self {
            storage,
            _temp_dir: temp_dir,
        }
    }
}

#[async_trait]
impl StorageTestContext for LocalProviderTestContext {
    async fn get_storage(&self) -> Arc<dyn Storage> {
        self.storage.clone()
    }
    fn provider_name(&self) -> &'static str {
        "local"
    }
}

// --- gRPC Provider Context ---
#[cfg(feature = "grpc")]
struct GrpcProviderTestContext {
    storage: Arc<dyn Storage>,
    _server_handle:
        JoinHandle<Result<(), alien_error::AlienError<alien_bindings::error::ErrorData>>>,
    _temp_data_dir: TempDir, // Manages ALIEN_DATA_DIR for the gRPC server's LocalBindingsProvider
}

#[cfg(feature = "grpc")]
impl AsyncTestContext for GrpcProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let temp_data_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for ALIEN_DATA_DIR (gRPC server)");
        let temp_data_dir_path = temp_data_dir.path().to_str().unwrap().to_string();

        // Env map for the BindingsProvider used by the gRPC server
        let server_binding = StorageBinding::local(temp_data_dir_path.clone());

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

        let storage_client = grpc_provider
            .load_storage(GRPC_BINDING_NAME)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Grpc storage for binding '{}' using ALIEN_BINDINGS_GRPC_ADDRESS='{}': {:?}",
                    GRPC_BINDING_NAME, server_addr_str, e
                )
            });

        Self {
            storage: storage_client,
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
impl StorageTestContext for GrpcProviderTestContext {
    async fn get_storage(&self) -> Arc<dyn Storage> {
        self.storage.clone()
    }
    fn provider_name(&self) -> &'static str {
        "grpc"
    }
}

// --- AWS Provider Context ---
#[cfg(feature = "aws")]
struct AwsProviderTestContext {
    storage: Arc<dyn Storage>,
}

#[cfg(feature = "aws")]
impl AsyncTestContext for AwsProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-aws-storage"; // Consistent naming
        let bucket = env::var("ALIEN_TEST_AWS_S3_BUCKET")
            .expect("ALIEN_TEST_AWS_S3_BUCKET must be set in .env.test");

        let binding = StorageBinding::s3(bucket);

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

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let storage = provider
            .load_storage(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load AWS storage for binding '{}': {:?}",
                    binding_name, e
                )
            });
        Self { storage }
    }
}

#[cfg(feature = "aws")]
#[async_trait]
impl StorageTestContext for AwsProviderTestContext {
    async fn get_storage(&self) -> Arc<dyn Storage> {
        self.storage.clone()
    }
    fn provider_name(&self) -> &'static str {
        "aws"
    }
}

// --- GCP Provider Context ---
#[cfg(feature = "gcp")]
struct GcpProviderTestContext {
    storage: Arc<dyn Storage>,
}

#[cfg(feature = "gcp")]
impl AsyncTestContext for GcpProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-gcp-storage";
        let bucket = env::var("ALIEN_TEST_GCP_GCS_BUCKET")
            .expect("ALIEN_TEST_GCP_GCS_BUCKET must be set in .env.test");
        let service_account_key_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .expect("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set in .env.test");
        let gcp_region = env::var("GOOGLE_MANAGEMENT_REGION")
            .expect("GOOGLE_MANAGEMENT_REGION must be set in .env.test");

        let binding = StorageBinding::gcs(bucket);

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
        let storage = provider
            .load_storage(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load GCP storage for binding '{}': {:?}",
                    binding_name, e
                )
            });
        Self { storage }
    }
}

#[cfg(feature = "gcp")]
#[async_trait]
impl StorageTestContext for GcpProviderTestContext {
    async fn get_storage(&self) -> Arc<dyn Storage> {
        self.storage.clone()
    }
    fn provider_name(&self) -> &'static str {
        "gcp"
    }
}

// --- Azure Provider Context ---
#[cfg(feature = "azure")]
struct AzureProviderTestContext {
    storage: Arc<dyn Storage>,
}

#[cfg(feature = "azure")]
impl AsyncTestContext for AzureProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-azure-storage";
        let storage_account = env::var("ALIEN_TEST_AZURE_STORAGE_ACCOUNT")
            .expect("ALIEN_TEST_AZURE_STORAGE_ACCOUNT must be set in .env.test");
        let container = env::var("ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER")
            .expect("ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER must be set in .env.test");

        let binding = StorageBinding::blob(storage_account, container);

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
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "azure".to_string());
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let storage = provider
            .load_storage(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Azure storage for binding '{}': {:?}",
                    binding_name, e
                )
            });
        Self { storage }
    }
}

#[cfg(feature = "azure")]
#[async_trait]
impl StorageTestContext for AzureProviderTestContext {
    async fn get_storage(&self) -> Arc<dyn Storage> {
        self.storage.clone()
    }
    fn provider_name(&self) -> &'static str {
        "azure"
    }
}

// --- Kubernetes Provider Context ---
#[cfg(feature = "kubernetes")]
struct KubernetesProviderTestContext {
    storage: Arc<dyn Storage>,
    _temp_dir: TempDir, // TempDir is kept to ensure cleanup
}

#[cfg(feature = "kubernetes")]
impl AsyncTestContext for KubernetesProviderTestContext {
    async fn setup() -> Self {
        load_test_env();
        let binding_name = "test-k8s-storage";

        // Always use a local file backend for this specific test context
        let temp_dir = tempfile::tempdir()
            .expect("Failed to create temp dir for K8s file test (simplified context)");
        let temp_dir_path_str = temp_dir.path().to_string_lossy().to_string();

        let file_url = format!("file://{}", temp_dir_path_str);

        let binding = StorageBinding::local(temp_dir_path_str.clone());

        let mut env_map: HashMap<String, String> = env::vars().collect(); // Start with process env
        let binding_json = serde_json::to_string(&binding).expect("Failed to serialize binding");
        env_map.insert(bindings::binding_env_var_name(binding_name), binding_json);
        env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "kubernetes".to_string());

        let provider = BindingsProvider::from_env(env_map)
            .await
            .expect("Failed to load bindings provider");
        let storage = provider
            .load_storage(binding_name)
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to load Kubernetes storage (file backend) for binding '{}' with URL '{}': {:?}",
                    binding_name, file_url, e
                )
            });
        Self {
            storage,
            _temp_dir: temp_dir,
        }
    }
}

#[cfg(feature = "kubernetes")]
#[async_trait]
impl StorageTestContext for KubernetesProviderTestContext {
    async fn get_storage(&self) -> Arc<dyn Storage> {
        self.storage.clone()
    }
    fn provider_name(&self) -> &'static str {
        // We could make this dynamic based on the URL (e.g. "kubernetes_file", "kubernetes_s3")
        // For now, keeping it simple.
        "kubernetes"
    }
}

// Macro to generate test cases for each provider context
// Due to limitations with rstest's matrix and async contexts,
// we'll define tests per provider type for now.
// We can explore more advanced rstest features or alternatives if this becomes too verbose.

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_put_and_get(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();
    let path = Path::from(format!("test_put_get_{}.txt", provider_name));
    let data = Bytes::from_static(b"Hello, Alien Test!");

    storage
        .put(&path, data.clone().into())
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to put object: {:?}", provider_name, e));

    let get_result = storage
        .get(&path)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get object: {:?}", provider_name, e));
    let retrieved_data = get_result.bytes().await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to read bytes from GetResult: {:?}",
            provider_name, e
        )
    });

    assert_eq!(
        data, retrieved_data,
        "[{}] Retrieved data mismatch",
        provider_name
    );
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_head_operation(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();
    let path = Path::from(format!("test_head_{}.txt", provider_name));
    let data = Bytes::from_static(b"Head test data");
    let data_size = data.len() as u64;

    storage
        .put(&path, data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for head test: {:?}",
                provider_name, e
            )
        });

    let meta = storage.head(&path).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Head failed for existing object: {:?}",
            provider_name, e
        )
    });
    assert_eq!(
        meta.location, path,
        "[{}] Meta location mismatch",
        provider_name
    );
    assert_eq!(
        meta.size, data_size,
        "[{}] Meta size mismatch",
        provider_name
    );

    let non_existent_path = Path::from(format!("non_existent_file_{}.txt", provider_name));
    match storage.head(&non_existent_path).await {
        Err(object_store::Error::NotFound { .. }) => { /* Expected */ }
        Ok(_) => panic!("[{}] Head succeeded for non-existent object", provider_name),
        Err(e) => panic!(
            "[{}] Unexpected error for head on non-existent object: {:?}",
            provider_name, e
        ),
    }
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_delete_operation(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();
    let path = Path::from(format!("test_delete_file_{}.txt", provider_name));
    let data = Bytes::from_static(b"Delete test data");

    // Put data
    storage
        .put(&path, data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for delete test: {:?}",
                provider_name, e
            )
        });

    // Confirm it exists
    storage.head(&path).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Head failed before delete, object should exist: {:?}",
            provider_name, e
        )
    });

    // Delete the object
    storage.delete(&path).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Delete failed for existing object: {:?}",
            provider_name, e
        )
    });

    // Confirm it's gone
    match storage.head(&path).await {
        Err(object_store::Error::NotFound { .. }) => { /* Expected */ }
        Ok(_) => panic!(
            "[{}] Head succeeded after delete, object should be gone",
            provider_name
        ),
        Err(e) => panic!(
            "[{}] Unexpected error for head after delete: {:?}",
            provider_name, e
        ),
    }

    // Delete non-existent object
    // Note: Behavior of delete on non-existent can vary.
    // object_store trait suggests it SHOULD succeed (idempotency).
    // Let's assume success or NotFound is acceptable.
    let non_existent_path = Path::from(format!("another_non_existent_file_{}.txt", provider_name));
    match storage.delete(&non_existent_path).await {
        Ok(()) => { /* Expected by some stores */ }
        Err(object_store::Error::NotFound { .. }) => { /* Also acceptable */ }
        Err(e) => panic!(
            "[{}] Delete on non-existent path returned an unexpected error: {:?}",
            provider_name, e
        ),
    }
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_list_operations(#[case] ctx: impl StorageTestContext) {
    use futures::stream::BoxStream;

    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();

    // 1. Setup: Create a unique base path and a set of files
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let base_path_str = format!("test_list_run_{}_{}", provider_name, unique_id);
    let base_path = Path::from(base_path_str.clone());

    let p_root_file1 = base_path.child("file1.txt");
    let p_dir1_file2 = base_path.child("dir1").child("file2.txt");
    let p_dir1_file3 = base_path.child("dir1").child("file3.txt");
    let p_dir1_subdirA_file4 = base_path.child("dir1").child("subdirA").child("file4.txt");
    let p_dir2_file5 = base_path.child("dir2").child("file5.txt");

    let files_to_create = vec![
        p_root_file1.clone(),
        p_dir1_file2.clone(),
        p_dir1_file3.clone(),
        p_dir1_subdirA_file4.clone(),
        p_dir2_file5.clone(),
    ];

    let data = Bytes::from_static(b"list_test_data");
    for path in &files_to_create {
        storage
            .put(path, data.clone().into())
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "[{}] Failed to put object at {}: {:?}",
                    provider_name, path, e
                )
            });
    }

    let all_created_files_set: std::collections::HashSet<Path> =
        files_to_create.iter().cloned().collect();

    // Helper to collect and filter listed locations
    async fn collect_filtered_locations(
        stream: BoxStream<'static, object_store::Result<ObjectMeta>>,
        expected_files_set: &std::collections::HashSet<Path>,
        provider_name: &str,
        context_msg: &str,
    ) -> std::collections::HashSet<Path> {
        stream
            .map_ok(|meta| meta.location)
            .try_filter(|loc| futures::future::ready(expected_files_set.contains(loc)))
            .try_collect::<std::collections::HashSet<Path>>()
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "[{}] Failed to collect locations for {}: {:?}",
                    provider_name, context_msg, e
                )
            })
    }

    // 2. Test list()
    // 2.1 List all files under base_path (recursively)
    let list_all_stream = storage.list(Some(&base_path));
    let listed_all_files = collect_filtered_locations(
        list_all_stream,
        &all_created_files_set,
        provider_name,
        "list all under base_path",
    )
    .await;
    assert_eq!(
        listed_all_files, all_created_files_set,
        "[{}] list(Some(&base_path)) mismatch. Expected {:?}, got {:?}",
        provider_name, all_created_files_set, listed_all_files
    );

    // 2.2 List files under a specific prefix (e.g., dir1)
    let prefix_dir1 = base_path.child("dir1");
    let expected_files_in_dir1: std::collections::HashSet<Path> = [
        p_dir1_file2.clone(),
        p_dir1_file3.clone(),
        p_dir1_subdirA_file4.clone(),
    ]
    .iter()
    .cloned()
    .collect();

    let list_dir1_stream = storage.list(Some(&prefix_dir1));
    let listed_files_in_dir1 = collect_filtered_locations(
        list_dir1_stream,
        &all_created_files_set, // Filter against all known files
        provider_name,
        "list under prefix_dir1",
    )
    .await;
    assert_eq!(
        listed_files_in_dir1, expected_files_in_dir1,
        "[{}] list(Some(&prefix_dir1)) mismatch. Expected {:?}, got {:?}",
        provider_name, expected_files_in_dir1, listed_files_in_dir1
    );

    // 3. Test list_with_delimiter()
    // 3.1 Delimiter list at base_path
    let expected_objects_at_base: std::collections::HashSet<Path> =
        [p_root_file1.clone()].iter().cloned().collect();
    let expected_common_prefixes_at_base: std::collections::HashSet<Path> =
        [base_path.child("dir1"), base_path.child("dir2")]
            .iter()
            .cloned()
            .collect();

    let delimiter_result_base = storage
        .list_with_delimiter(Some(&base_path))
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] list_with_delimiter(Some(&base_path)) failed: {:?}",
                provider_name, e
            )
        });

    let actual_objects_at_base: std::collections::HashSet<Path> = delimiter_result_base
        .objects
        .into_iter()
        .map(|o| o.location)
        .collect();
    let actual_common_prefixes_at_base: std::collections::HashSet<Path> =
        delimiter_result_base.common_prefixes.into_iter().collect();

    assert_eq!(
        actual_objects_at_base, expected_objects_at_base,
        "[{}] list_with_delimiter(base_path) objects mismatch. Expected {:?}, got {:?}",
        provider_name, expected_objects_at_base, actual_objects_at_base
    );
    assert_eq!(
        actual_common_prefixes_at_base, expected_common_prefixes_at_base,
        "[{}] list_with_delimiter(base_path) common_prefixes mismatch. Expected {:?}, got {:?}",
        provider_name, expected_common_prefixes_at_base, actual_common_prefixes_at_base
    );

    // 3.2 Delimiter list at prefix_dir1
    let expected_objects_in_dir1_delim: std::collections::HashSet<Path> =
        [p_dir1_file2.clone(), p_dir1_file3.clone()]
            .iter()
            .cloned()
            .collect();
    let expected_common_prefixes_in_dir1_delim: std::collections::HashSet<Path> =
        [prefix_dir1.child("subdirA")].iter().cloned().collect();

    let delimiter_result_dir1 = storage
        .list_with_delimiter(Some(&prefix_dir1))
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] list_with_delimiter(Some(&prefix_dir1)) failed: {:?}",
                provider_name, e
            )
        });

    let actual_objects_in_dir1_delim: std::collections::HashSet<Path> = delimiter_result_dir1
        .objects
        .into_iter()
        .map(|o| o.location)
        .collect();
    let actual_common_prefixes_in_dir1_delim: std::collections::HashSet<Path> =
        delimiter_result_dir1.common_prefixes.into_iter().collect();

    assert_eq!(
        actual_objects_in_dir1_delim, expected_objects_in_dir1_delim,
        "[{}] list_with_delimiter(prefix_dir1) objects mismatch. Expected {:?}, got {:?}",
        provider_name, expected_objects_in_dir1_delim, actual_objects_in_dir1_delim
    );
    assert_eq!(
        actual_common_prefixes_in_dir1_delim, expected_common_prefixes_in_dir1_delim,
        "[{}] list_with_delimiter(prefix_dir1) common_prefixes mismatch. Expected {:?}, got {:?}",
        provider_name, expected_common_prefixes_in_dir1_delim, actual_common_prefixes_in_dir1_delim
    );

    // 4. Test list_with_offset()
    // Get all files sorted by path to determine a valid offset
    let mut sorted_actual_files_under_base: Vec<Path> = storage
        .list(Some(&base_path))
        .map_ok(|meta| meta.location)
        .try_filter(|loc| futures::future::ready(all_created_files_set.contains(loc)))
        .try_collect::<Vec<Path>>()
        .await
        .unwrap();
    sorted_actual_files_under_base.sort();

    if sorted_actual_files_under_base.len() > 1 {
        let offset_path = sorted_actual_files_under_base[0].clone();
        let expected_files_after_offset: std::collections::HashSet<Path> =
            sorted_actual_files_under_base
                .iter()
                .skip(1)
                .cloned()
                .collect();

        let list_offset_stream = storage.list_with_offset(Some(&base_path), &offset_path);
        let listed_files_after_offset = collect_filtered_locations(
            list_offset_stream,
            &all_created_files_set,
            provider_name,
            "list_with_offset",
        )
        .await;

        assert_eq!(
            listed_files_after_offset, expected_files_after_offset,
            "[{}] list_with_offset mismatch. Offset: {:?}, Expected: {:?}, Got: {:?}",
            provider_name, offset_path, expected_files_after_offset, listed_files_after_offset
        );
    } else if !sorted_actual_files_under_base.is_empty() {
        // Case with only one file: list_with_offset should return empty
        let offset_path = sorted_actual_files_under_base[0].clone();
        let list_offset_stream = storage.list_with_offset(Some(&base_path), &offset_path);
        let listed_files_after_offset = collect_filtered_locations(
            list_offset_stream,
            &all_created_files_set,
            provider_name,
            "list_with_offset (single file scenario)",
        )
        .await;
        assert!(
            listed_files_after_offset.is_empty(),
            "[{}] list_with_offset with one file (offset={:?}) should be empty, got {:?}",
            provider_name,
            offset_path,
            listed_files_after_offset
        );
    }

    // 5. Cleanup
    for path in &files_to_create {
        storage.delete(path).await.unwrap_or_else(|e| {
            eprintln!(
                "[{}] Failed to delete object {} during cleanup: {:?}",
                provider_name, path, e
            )
        });
    }
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_copy_operation(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();

    // Add unique identifier to prevent collisions between test runs
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let from_path = Path::from(format!("copy_source_{}_{}.txt", provider_name, unique_id));
    let to_path = Path::from(format!(
        "copy_destination_{}_{}.txt",
        provider_name, unique_id
    ));
    let data = Bytes::from_static(b"Copy test data");

    // Put initial object
    storage
        .put(&from_path, data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for copy source: {:?}",
                provider_name, e
            )
        });

    // Copy object
    storage
        .copy(&from_path, &to_path)
        .await
        .unwrap_or_else(|e| panic!("[{}] Copy operation failed: {:?}", provider_name, e));

    // Verify source still exists
    let source_meta = storage
        .head(&from_path)
        .await
        .unwrap_or_else(|e| panic!("[{}] Head failed for copy source: {:?}", provider_name, e));
    assert_eq!(
        source_meta.size,
        data.len() as u64,
        "[{}] Source size mismatch after copy",
        provider_name
    );

    // Verify destination exists and has same content
    let dest_get_result = storage
        .get(&to_path)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get copied object: {:?}", provider_name, e));
    let dest_data = dest_get_result.bytes().await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to read bytes from copied object: {:?}",
            provider_name, e
        )
    });
    assert_eq!(data, dest_data, "[{}] Copied data mismatch", provider_name);

    // Attempt to copy to existing path (should overwrite)
    let new_data_content = b"New copy data for overwrite";
    let new_data = Bytes::from_static(new_data_content);
    storage
        .put(&from_path, new_data.clone().into())
        .await
        .unwrap(); // update source

    storage
        .copy(&from_path, &to_path)
        .await
        .unwrap_or_else(|e| panic!("[{}] Copy to existing path failed: {:?}", provider_name, e));

    let dest_get_result_updated = storage.get(&to_path).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get updated copied object: {:?}",
            provider_name, e
        )
    });
    let dest_data_updated = dest_get_result_updated.bytes().await.unwrap();
    assert_eq!(
        new_data, dest_data_updated,
        "[{}] Overwritten copied data mismatch",
        provider_name
    );

    // Clean up
    storage.delete(&from_path).await.ok();
    storage.delete(&to_path).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
// TODO (DOC): Unsupported on AWS right now.
// #[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_copy_if_not_exists_operation(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();

    // Add unique identifier to prevent collisions between test runs
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let from_path = Path::from(format!("cif_source_{}_{}.txt", provider_name, unique_id));
    let to_path_new = Path::from(format!("cif_dest_new_{}_{}.txt", provider_name, unique_id));
    let to_path_existing = Path::from(format!(
        "cif_dest_existing_{}_{}.txt",
        provider_name, unique_id
    ));
    let data = Bytes::from_static(b"CIF test data");
    let data_existing = Bytes::from_static(b"CIF existing data");

    // Put initial object
    storage
        .put(&from_path, data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for CIF source: {:?}",
                provider_name, e
            )
        });

    // Put an object at one of the destination paths
    storage
        .put(&to_path_existing, data_existing.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for CIF existing dest: {:?}",
                provider_name, e
            )
        });

    // Copy to a new path (should succeed)
    storage
        .copy_if_not_exists(&from_path, &to_path_new)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] copy_if_not_exists to new path failed: {:?}",
                provider_name, e
            )
        });

    let new_get_result = storage
        .get(&to_path_new)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get CIF new object: {:?}", provider_name, e));
    let new_data_retrieved = new_get_result.bytes().await.unwrap();
    assert_eq!(
        data, new_data_retrieved,
        "[{}] CIF new data mismatch",
        provider_name
    );

    // Attempt to copy to an existing path
    // Behavior: should not overwrite. object_store::Error::AlreadyExists is common for stores that detect this.
    // Some stores might return Ok(()) and not actually copy.
    match storage
        .copy_if_not_exists(&from_path, &to_path_existing)
        .await
    {
        Ok(()) => {
            // This is acceptable if the store guarantees no overwrite.
            // Verify the existing destination was not changed
            let existing_get_result = storage.get(&to_path_existing).await.unwrap_or_else(|e| {
                panic!(
                    "[{}] Failed to get CIF existing object after cif (Ok): {:?}",
                    provider_name, e
                )
            });
            let existing_data_retrieved = existing_get_result.bytes().await.unwrap();
            assert_eq!(
                data_existing, existing_data_retrieved,
                "[{}] Data at existing destination was overwritten by copy_if_not_exists (Ok case)",
                provider_name
            );
        }
        Err(object_store::Error::AlreadyExists { .. }) => { /* Expected by some stores like LocalFileSystem */
        }
        Err(e) => panic!(
            "[{}] Unexpected error for copy_if_not_exists to existing path: {:?}",
            provider_name, e
        ),
    }

    // Verify the existing destination was not changed (again, in case of Ok(()) from above)
    let existing_get_result = storage.get(&to_path_existing).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get CIF existing object after cif (final check): {:?}",
            provider_name, e
        )
    });
    let existing_data_retrieved = existing_get_result.bytes().await.unwrap();
    assert_eq!(
        existing_data_retrieved, data_existing,
        "[{}] Data at existing destination was overwritten by copy_if_not_exists (final check)",
        provider_name
    );

    // Clean up
    storage.delete(&from_path).await.ok();
    storage.delete(&to_path_new).await.ok();
    storage.delete(&to_path_existing).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_rename_operation(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();

    // Add unique identifier to prevent collisions between test runs
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let from_path = Path::from(format!("rename_source_{}_{}.txt", provider_name, unique_id));
    let to_path = Path::from(format!(
        "rename_destination_{}_{}.txt",
        provider_name, unique_id
    ));
    let data: Bytes = Bytes::from_static(b"Rename test data");

    // Put initial object
    storage
        .put(&from_path, data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for rename source: {:?}",
                provider_name, e
            )
        });

    // Rename object
    storage
        .rename(&from_path, &to_path)
        .await
        .unwrap_or_else(|e| panic!("[{}] Rename operation failed: {:?}", provider_name, e));

    // Verify source no longer exists
    match storage.head(&from_path).await {
        Err(object_store::Error::NotFound { .. }) => { /* Expected */ }
        Ok(_) => panic!(
            "[{}] Head succeeded for rename source after rename, should be gone",
            provider_name
        ),
        Err(e) => panic!(
            "[{}] Unexpected error for head on rename source after rename: {:?}",
            provider_name, e
        ),
    }

    // Verify destination exists and has same content
    let dest_get_result = storage
        .get(&to_path)
        .await
        .unwrap_or_else(|e| panic!("[{}] Failed to get renamed object: {:?}", provider_name, e));
    let dest_data = dest_get_result.bytes().await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to read bytes from renamed object: {:?}",
            provider_name, e
        )
    });
    assert_eq!(data, dest_data, "[{}] Renamed data mismatch", provider_name);

    // Clean up
    storage.delete(&to_path).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
// TODO (DOC): Unsupported on AWS right now.
// #[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[tokio::test]
async fn test_rename_if_not_exists_operation(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();

    // Add unique identifier to prevent collisions between test runs
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let from_path1 = Path::from(format!("rifne_source1_{}_{}.txt", provider_name, unique_id));
    let to_path_new = Path::from(format!(
        "rifne_dest_new_{}_{}.txt",
        provider_name, unique_id
    ));
    let data1 = Bytes::from_static(b"RIFNE data 1");

    let from_path2 = Path::from(format!("rifne_source2_{}_{}.txt", provider_name, unique_id));
    let to_path_existing = Path::from(format!(
        "rifne_dest_existing_{}_{}.txt",
        provider_name, unique_id
    ));
    let data2 = Bytes::from_static(b"RIFNE data 2");
    let data_existing = Bytes::from_static(b"RIFNE existing data");

    // Put initial objects
    storage
        .put(&from_path1, data1.clone().into())
        .await
        .unwrap_or_else(|e| panic!("[{}] Put from_path1 failed: {:?}", provider_name, e));
    storage
        .put(&from_path2, data2.clone().into())
        .await
        .unwrap_or_else(|e| panic!("[{}] Put from_path2 failed: {:?}", provider_name, e));
    storage
        .put(&to_path_existing, data_existing.clone().into())
        .await
        .unwrap_or_else(|e| panic!("[{}] Put to_path_existing failed: {:?}", provider_name, e));

    // Rename to a new path (should succeed)
    storage
        .rename_if_not_exists(&from_path1, &to_path_new)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] rename_if_not_exists to new path failed: {:?}",
                provider_name, e
            )
        });

    // Verify source1 is gone
    assert!(
        matches!(
            storage.head(&from_path1).await,
            Err(object_store::Error::NotFound { .. })
        ),
        "[{}] from_path1 should be gone",
        provider_name
    );
    // Verify new destination has content of source1
    let new_get_result = storage
        .get(&to_path_new)
        .await
        .unwrap_or_else(|e| panic!("[{}] Get on to_path_new failed: {:?}", provider_name, e));
    assert_eq!(
        new_get_result.bytes().await.unwrap(),
        data1,
        "[{}] to_path_new data mismatch",
        provider_name
    );

    // Attempt to rename to an existing path
    // Behavior: should not overwrite. LocalFS returns AlreadyExists error. Others might Ok(()) and not move.
    match storage
        .rename_if_not_exists(&from_path2, &to_path_existing)
        .await
    {
        Ok(()) => {
            // Acceptable if source is not removed and dest is not overwritten
            let source2_get_result = storage.get(&from_path2).await.unwrap_or_else(|e| {
                panic!(
                    "[{}] Get on from_path2 after RIFNE (Ok) failed: {:?}",
                    provider_name, e
                )
            });
            assert_eq!(
                source2_get_result.bytes().await.unwrap(),
                data2,
                "[{}] from_path2 data changed after RIFNE (Ok)",
                provider_name
            );

            let existing_get_result = storage.get(&to_path_existing).await.unwrap_or_else(|e| {
                panic!(
                    "[{}] Get on to_path_existing after RIFNE (Ok) failed: {:?}",
                    provider_name, e
                )
            });
            assert_eq!(
                existing_get_result.bytes().await.unwrap(),
                data_existing,
                "[{}] Data at existing destination was overwritten by RIFNE (Ok case)",
                provider_name
            );
        }
        Err(object_store::Error::AlreadyExists { .. }) => {
            /* Expected by some stores like LocalFileSystem */
            // Verify source2 still exists
            let source2_get_result = storage.get(&from_path2).await.unwrap_or_else(|e| {
                panic!(
                    "[{}] Get on from_path2 after RIFNE (AlreadyExists) failed: {:?}",
                    provider_name, e
                )
            });
            assert_eq!(
                source2_get_result.bytes().await.unwrap(),
                data2,
                "[{}] from_path2 data changed after RIFNE (AlreadyExists)",
                provider_name
            );
        }
        Err(e) => panic!(
            "[{}] Unexpected error for rename_if_not_exists to existing path: {:?}",
            provider_name, e
        ),
    }

    // Verify the existing destination was not changed (final check)
    let existing_get_result = storage.get(&to_path_existing).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Get on to_path_existing after RIFNE (final check) failed: {:?}",
            provider_name, e
        )
    });
    let existing_data_retrieved = existing_get_result.bytes().await.unwrap();
    assert_eq!(
        existing_data_retrieved, data_existing,
        "[{}] Data at existing destination was overwritten by RIFNE (final check)",
        provider_name
    );

    // Clean up
    storage.delete(&from_path1).await.ok(); // Might be gone
    storage.delete(&to_path_new).await.ok();
    storage.delete(&from_path2).await.ok();
    storage.delete(&to_path_existing).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_put_multipart(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();

    let path = Path::from(format!("multipart_upload_{}.dat", provider_name));

    // Use appropriate part sizes for each provider based on their requirements
    let (part1_size, part2_size, use_fallback) = match provider_name {
        "aws" | "gcp" => {
            // AWS S3 and GCS both work well with 5MB parts
            (5 * 1024 * 1024, 1024, true) // 5MB + 1KB, with fallback enabled
        }
        _ => {
            // Other providers (local, grpc, kubernetes) can use smaller parts for faster tests
            (1024 * 1024, 512, false) // 1MB + 512B, no fallback needed
        }
    };

    // Helper function to create test data
    let create_test_data = |size: usize, pattern: &[u8]| -> Bytes {
        let pattern_len = pattern.len();
        let mut data_vec = Vec::with_capacity(size);
        for i in 0..size {
            data_vec.push(pattern[i % pattern_len]);
        }
        Bytes::from(data_vec)
    };

    let pattern1 = b"ALIEN_TEST_DATA_PATTERN_0123456789ABCDEF";
    let pattern2 = b"SECOND_PART_PATTERN_ZYXWVUTSRQPONMLKJIHGFEDCBA";

    // Try the upload with the specified part sizes
    let upload_result = async {
        let data_part1 = create_test_data(part1_size, pattern1);
        let data_part2 = create_test_data(part2_size, pattern2);

        // Concatenate parts for verification
        let mut full_data_vec = Vec::new();
        full_data_vec.extend_from_slice(&data_part1);
        full_data_vec.extend_from_slice(&data_part2);
        let full_data = Bytes::from(full_data_vec);

        let mut uploader = storage
            .put_multipart_opts(&path, PutMultipartOpts::default())
            .await?;

        // Upload parts with timeout handling for AWS
        if provider_name == "aws" {
            tracing::info!(
                "[{}] Starting upload of part 1 ({} bytes)",
                provider_name,
                part1_size
            );
            let upload_result = tokio::time::timeout(
                std::time::Duration::from_secs(300), // 5 minute timeout
                uploader.put_part(data_part1.clone().into()),
            )
            .await;

            match upload_result {
                Ok(Ok(())) => {
                    tracing::info!("[{}] Part 1 upload completed successfully", provider_name)
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(object_store::Error::Generic {
                        store: "test_put_multipart",
                        source: format!("Part 1 upload timed out after 5 minutes").into(),
                    })
                }
            }
        } else {
            uploader.put_part(data_part1.clone().into()).await?;
        }

        uploader.put_part(data_part2.clone().into()).await?;
        uploader.complete().await?;

        Ok::<Bytes, object_store::Error>(full_data)
    }
    .await;

    // Handle the result, with fallback for AWS if needed
    let full_data = match upload_result {
        Ok(data) => data,
        Err(e) if use_fallback && provider_name == "aws" => {
            tracing::warn!(
                "[{}] Large part upload failed, trying fallback with smaller parts: {:?}",
                provider_name,
                e
            );

            // Clean up any partial upload
            storage.delete(&path).await.ok();

            // Fallback: use smaller parts that are more likely to succeed
            let fallback_part1_size = 1024 * 1024; // 1MB
            let fallback_part2_size = 512; // 512B

            let data_part1 = create_test_data(fallback_part1_size, pattern1);
            let data_part2 = create_test_data(fallback_part2_size, pattern2);

            let mut full_data_vec = Vec::new();
            full_data_vec.extend_from_slice(&data_part1);
            full_data_vec.extend_from_slice(&data_part2);
            let full_data = Bytes::from(full_data_vec);

            let mut uploader = storage
                .put_multipart_opts(&path, PutMultipartOpts::default())
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "[{}] Failed to start fallback multipart upload: {:?}",
                        provider_name, e
                    )
                });

            tracing::info!(
                "[{}] Fallback: uploading smaller part 1 ({} bytes)",
                provider_name,
                fallback_part1_size
            );
            uploader
                .put_part(data_part1.clone().into())
                .await
                .unwrap_or_else(|e| {
                    panic!("[{}] Fallback part 1 upload failed: {:?}", provider_name, e)
                });

            uploader
                .put_part(data_part2.clone().into())
                .await
                .unwrap_or_else(|e| {
                    panic!("[{}] Fallback part 2 upload failed: {:?}", provider_name, e)
                });

            uploader.complete().await.unwrap_or_else(|e| {
                panic!(
                    "[{}] Failed to complete fallback multipart upload: {:?}",
                    provider_name, e
                )
            });

            full_data
        }
        Err(e) => panic!("[{}] Multipart upload failed: {:?}", provider_name, e),
    };

    // Verify the uploaded object
    let get_result = storage.get(&path).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get multipart uploaded object: {:?}",
            provider_name, e
        )
    });
    let retrieved_data = get_result.bytes().await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to read bytes from GetResult: {:?}",
            provider_name, e
        )
    });
    assert_eq!(
        full_data, retrieved_data,
        "[{}] Multipart uploaded data does not match original",
        provider_name
    );

    // Clean up
    storage.delete(&path).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
// Azure does not support suffix range requests
// #[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_get_with_options(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();

    let path = Path::from(format!("get_options_{}.txt", provider_name));
    let data = Bytes::from_static(b"0123456789ABCDEF"); // 16 bytes

    storage
        .put(&path, data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for get_options test: {:?}",
                provider_name, e
            )
        });

    // Test GetRange::Bounded
    let range_bounded = OsGetRange::Bounded(4..8); // "4567"
    let get_opts_bounded = GetOptions {
        range: Some(range_bounded),
        ..Default::default()
    };
    let result_bounded = storage
        .get_opts(&path, get_opts_bounded)
        .await
        .unwrap_or_else(|e| panic!("[{}] Get with bounded range failed: {:?}", provider_name, e));
    let range_check_bounded = result_bounded.range.clone();
    let data_bounded = result_bounded.bytes().await.unwrap();
    assert_eq!(
        data_bounded,
        Bytes::from_static(b"4567"),
        "[{}] Bounded range data mismatch for {}",
        provider_name,
        provider_name
    );
    assert_eq!(
        range_check_bounded,
        4..8,
        "[{}] Bounded range mismatch for {}",
        provider_name,
        provider_name
    );

    // Test GetRange::Offset
    let range_offset = OsGetRange::Offset(10); // "ABCDEF" (from index 10 to end)
    let get_opts_offset = GetOptions {
        range: Some(range_offset),
        ..Default::default()
    };
    let result_offset = storage
        .get_opts(&path, get_opts_offset)
        .await
        .unwrap_or_else(|e| panic!("[{}] Get with offset range failed: {:?}", provider_name, e));
    let range_check_offset = result_offset.range.clone();
    let data_offset = result_offset.bytes().await.unwrap();
    assert_eq!(
        data_offset,
        Bytes::from_static(b"ABCDEF"),
        "[{}] Offset range data mismatch for {}",
        provider_name,
        provider_name
    );
    assert_eq!(
        range_check_offset,
        10..16,
        "[{}] Offset range in result mismatch for {}",
        provider_name,
        provider_name
    );

    // Test GetRange::Suffix
    let range_suffix = OsGetRange::Suffix(6); // "ABCDEF" (last 6 bytes)
    let get_opts_suffix = GetOptions {
        range: Some(range_suffix),
        ..Default::default()
    };
    let result_suffix = storage
        .get_opts(&path, get_opts_suffix)
        .await
        .unwrap_or_else(|e| panic!("[{}] Get with suffix range failed: {:?}", provider_name, e));
    let range_check_suffix = result_suffix.range.clone();
    let data_suffix = result_suffix.bytes().await.unwrap();
    assert_eq!(
        data_suffix,
        Bytes::from_static(b"ABCDEF"),
        "[{}] Suffix range data mismatch for {}",
        provider_name,
        provider_name
    );
    assert_eq!(
        range_check_suffix,
        10..16,
        "[{}] Suffix range in result mismatch for {}",
        provider_name,
        provider_name
    );

    // Test if_modified_since (expect data)
    let meta_res = storage.head(&path).await;
    if let Ok(meta) = meta_res {
        // Ensure last_modified is not in the future relative to now, which can happen with some FS mocks or clock issues
        let now = chrono::Utc::now();
        if meta.last_modified > now {
            println!("[{}] Warning: Object last_modified timestamp ({:?}) is in the future compared to now ({:?}). Skipping if_modified_since tests as they may be unreliable.", provider_name, meta.last_modified, now);
        } else {
            let slightly_before_mod = meta.last_modified - chrono::Duration::seconds(5);
            let get_opts_modified_since_pass = GetOptions {
                if_modified_since: Some(slightly_before_mod),
                ..Default::default()
            };

            match storage
                .get_opts(&path, get_opts_modified_since_pass.clone())
                .await
            {
                Ok(result_mod_pass) => {
                    assert_eq!(
                        result_mod_pass.bytes().await.unwrap(),
                        data,
                        "[{}] if_modified_since (pass) data mismatch for {}",
                        provider_name,
                        provider_name
                    );
                }
                Err(e) => {
                    panic!(
                        "[{}] Get with if_modified_since (pass) failed unexpectedly for {}: {:?}",
                        provider_name, provider_name, e
                    );
                }
            }

            let slightly_after_mod = meta.last_modified + chrono::Duration::seconds(5);
            let get_opts_modified_since_fail = GetOptions {
                if_modified_since: Some(slightly_after_mod),
                ..Default::default()
            };
            match storage.get_opts(&path, get_opts_modified_since_fail).await {
                 Ok(res) => {
                    // Some stores (like LocalFileSystem or stores with coarse time granularity) 
                    // might return the object if it exists and if_modified_since is very close to actual modification time.
                    // This is often acceptable. We check if data is returned.
                    let _ = res.bytes().await.unwrap(); // Ensure we can read it
                    println!("[{}] Info: Get with if_modified_since (future) returned data for {}, which is acceptable for some backends.", provider_name, provider_name);
                 }
                 Err(object_store::Error::NotModified { .. }) => { /* Ideal, but not guaranteed */ }
                 Err(e) => panic!("[{}] Unexpected error for get_opts with if_modified_since (future) for {}: {:?}", provider_name, provider_name, e),
            }
        }
    } else {
        println!(
            "[{}] Skipping if_modified_since tests for {} as head failed: {:?}",
            provider_name,
            provider_name,
            meta_res.err()
        );
    }

    // Clean up
    storage.delete(&path).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_put_with_options(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let path_create = Path::from(format!(
        "put_options_create_{}_{}.txt",
        provider_name, unique_id
    ));
    let data = Bytes::from_static(b"Put options data");

    // Test PutMode::Create
    let put_opts_create = PutOptions {
        mode: PutMode::Create,
        ..Default::default()
    };
    storage
        .put_opts(&path_create, data.clone().into(), put_opts_create.clone())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Put with PutMode::Create failed for new file for {}: {:?}",
                provider_name, provider_name, e
            )
        });

    // Try to put again with PutMode::Create (should fail with AlreadyExists)
    match storage
        .put_opts(&path_create, data.clone().into(), put_opts_create)
        .await
    {
        Err(object_store::Error::AlreadyExists { .. }) => { /* Expected */ }
        Ok(_) => panic!(
            "[{}] Put with PutMode::Create succeeded on existing file for {}, should have failed",
            provider_name, provider_name
        ),
        Err(e) => panic!(
            "[{}] Unexpected error for PutMode::Create on existing file for {}: {:?}",
            provider_name, provider_name, e
        ),
    }

    // Test PutOptions with PutMode::Overwrite
    let path_overwrite = Path::from(format!(
        "put_options_overwrite_{}_{}.txt",
        provider_name, unique_id
    ));
    let initial_data = Bytes::from_static(b"Initial Overwrite Data");
    let new_data = Bytes::from_static(b"New Overwritten Data");

    storage
        .put(&path_overwrite, initial_data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put initial data for overwrite test for {}: {:?}",
                provider_name, provider_name, e
            )
        });

    let put_opts_overwrite = PutOptions {
        mode: PutMode::Overwrite,
        ..Default::default()
    };
    storage
        .put_opts(&path_overwrite, new_data.clone().into(), put_opts_overwrite)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Put with PutMode::Overwrite failed for {}: {:?}",
                provider_name, provider_name, e
            )
        });

    let get_result_overwrite = storage.get(&path_overwrite).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to get overwritten file for {}: {:?}",
            provider_name, provider_name, e
        )
    });
    let retrieved_data_overwrite = get_result_overwrite.bytes().await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to read bytes from overwritten file for {}: {:?}",
            provider_name, provider_name, e
        )
    });
    assert_eq!(
        retrieved_data_overwrite, new_data,
        "[{}] Data was not overwritten as expected for {}",
        provider_name, provider_name
    );

    // Clean up
    storage.delete(&path_create).await.ok();
    storage.delete(&path_overwrite).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_delete_stream(#[case] ctx: impl StorageTestContext) {
    use futures::{stream, StreamExt as _};

    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let path1 = Path::from(format!(
        "delete_stream_file1_{}_{}.txt",
        provider_name, unique_id
    ));
    let path2 = Path::from(format!(
        "delete_stream_file2_{}_{}.txt",
        provider_name, unique_id
    ));
    let path3 = Path::from(format!(
        "delete_stream_file3_{}_{}.txt",
        provider_name, unique_id
    ));
    let paths_to_create = vec![path1.clone(), path2.clone(), path3.clone()];
    let data = Bytes::from_static(b"delete stream test data");

    for path in &paths_to_create {
        storage
            .put(path, data.clone().into())
            .await
            .unwrap_or_else(|e| {
                panic!(
                    "[{}] Failed to put object at {} for delete_stream test: {:?}",
                    provider_name, path, e
                )
            });
        // Sanity check: ensure file exists before attempting delete_stream
        storage.head(path).await.unwrap_or_else(|e| {
            panic!(
                "[{}] Head failed for {} before delete_stream, should exist: {:?}",
                provider_name, path, e
            )
        });
    }

    let paths_stream = stream::iter(
        paths_to_create
            .clone()
            .into_iter()
            .map(Ok::<Path, object_store::Error>),
    );

    let deleted_paths_results: Vec<object_store::Result<Path>> = storage
        .delete_stream(paths_stream.boxed())
        .collect::<Vec<_>>()
        .await;

    let mut deleted_paths_set = std::collections::HashSet::new();
    for result in deleted_paths_results {
        match result {
            Ok(p) => {
                deleted_paths_set.insert(p);
            }
            Err(e) => panic!(
                "[{}] delete_stream returned an error for one of the paths: {:?}",
                provider_name, e
            ),
        }
    }

    let original_paths_set: std::collections::HashSet<Path> = paths_to_create.into_iter().collect();
    assert_eq!(
        deleted_paths_set, original_paths_set,
        "[{}] Set of deleted paths does not match original set",
        provider_name
    );

    for path in &original_paths_set {
        match storage.head(path).await {
            Err(object_store::Error::NotFound { .. }) => { /* Expected */ }
            Ok(meta) => panic!(
                "[{}] Head succeeded for {} after delete_stream, should be NotFound. Meta: {:?}",
                provider_name, path, meta
            ),
            Err(e) => panic!(
                "[{}] Unexpected error for head on {} after delete_stream: {:?}",
                provider_name, path, e
            ),
        }
    }

    // Test with a non-existent path in the stream (should ideally still process others and report error for this one or succeed if underlying store is idempotent)
    let path_non_existent = Path::from(format!(
        "delete_stream_non_existent_{}_{}.txt",
        provider_name, unique_id
    ));

    // Re-put one file to ensure one valid deletion in the mixed stream
    storage.put(&path3, data.clone().into()).await.unwrap();
    let paths_for_mixed_stream = vec![path3.clone(), path_non_existent.clone()];
    let mixed_paths_stream = stream::iter(
        paths_for_mixed_stream
            .clone()
            .into_iter()
            .map(Ok::<Path, object_store::Error>),
    );

    let mixed_delete_results: Vec<object_store::Result<Path>> = storage
        .delete_stream(mixed_paths_stream.boxed())
        .collect::<Vec<_>>()
        .await;

    let mut success_count = 0;
    for res in mixed_delete_results {
        match res {
            Ok(p) => {
                if p == path3 {
                    success_count += 1;
                }
            }
            Err(e) => {
                // It's okay if deleting a non-existent path errors out with NotFound,
                // but other errors are unexpected.
                if let object_store::Error::NotFound {
                    path: error_path, ..
                } = &e
                {
                    assert!(
                        error_path.ends_with(&path_non_existent.to_string()),
                        "Expected error path '{}' to end with '{}'",
                        error_path,
                        path_non_existent
                    );
                } else {
                    panic!(
                        "[{}] delete_stream with non-existent path returned unexpected error: {:?}",
                        provider_name, e
                    );
                }
            }
        }
    }
    assert_eq!(
        success_count, 1,
        "[{}] Expected one successful deletion in mixed stream",
        provider_name
    );
    // object_store::delete is idempotent for non-existent objects for many stores, so it might return Ok.
    // We just ensure path3 is deleted.
    assert!(
        matches!(
            storage.head(&path3).await,
            Err(object_store::Error::NotFound { .. })
        ),
        "[{}] path3 should be deleted after mixed stream test",
        provider_name
    );

    // Clean up any paths that might still exist if a panic occurred earlier (though unwrap_or_else should prevent this)
    storage.delete(&path1).await.ok();
    storage.delete(&path2).await.ok();
    storage.delete(&path3).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
#[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_get_ranges(#[case] ctx: impl StorageTestContext) {
    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let path = Path::from(format!(
        "get_ranges_test_{}_{}.dat",
        provider_name, unique_id
    ));
    // 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz
    let data_vec: Vec<u8> = (b'0'..=b'9')
        .chain(b'A'..=b'Z')
        .chain(b'a'..=b'z')
        .collect();
    let data = Bytes::from(data_vec.clone());
    let data_len = data.len();

    storage
        .put(&path, data.clone().into())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to put object for get_ranges test: {:?}",
                provider_name, e
            )
        });

    // Define ranges
    // Range 1: "56789" (indices 5-9)
    // Range 2: "XYZ" (indices 33-35)
    // Range 3: "abc" (indices 36-38)
    // Range 4: Full last part "uvwxyz" (indices 56 to end)
    // Range 5: Empty range
    // Range 6: Overlapping with next "PQR" (indices 25-27)
    // Range 7: "STU" (indices 28-30)
    // Range 8: Beyond end of file (should be empty or error depending on store, but get_ranges coalesces)
    // Range 9: Single byte "0"
    // Range 10: Single byte "z" (last byte)
    let ranges_to_fetch = [
        5u64..10u64,                             // "56789"
        33u64..36u64,                            // "XYZ"
        36u64..39u64,                            // "abc"
        (data_len - 6) as u64..data_len as u64,  // "uvwxyz"
        10u64..10u64,                            // "" (empty)
        25u64..28u64,                            // "PQR"
        28u64..31u64,                            // "STU"
        data_len as u64..(data_len + 10) as u64, // "" (beyond EOF)
        0u64..1u64,                              // "0"
        (data_len - 1) as u64..data_len as u64,  // "z"
    ];

    let expected_data_slices = [
        Bytes::from_static(b"56789"),
        Bytes::from_static(b"XYZ"),
        Bytes::from_static(b"abc"),
        Bytes::from_static(b"uvwxyz"),
        Bytes::from_static(b""),
        Bytes::from_static(b"PQR"),
        Bytes::from_static(b"STU"),
        Bytes::from_static(b""), // ObjectStore `get_range` behavior for out of bounds is typically to truncate.
        Bytes::from_static(b"0"),
        Bytes::from_static(b"z"),
    ];

    let retrieved_ranges_data = storage
        .get_ranges(&path, &ranges_to_fetch)
        .await
        .unwrap_or_else(|e| panic!("[{}] get_ranges failed: {:?}", provider_name, e));

    assert_eq!(
        retrieved_ranges_data.len(),
        expected_data_slices.len(),
        "[{}] Number of retrieved ranges differs from expected",
        provider_name
    );

    for i in 0..expected_data_slices.len() {
        assert_eq!(
            retrieved_ranges_data[i],
            expected_data_slices[i],
            "[{}] Data mismatch for range {:?} (index {}). Expected {:?}, got {:?}",
            provider_name,
            ranges_to_fetch[i],
            i,
            String::from_utf8_lossy(&expected_data_slices[i]),
            String::from_utf8_lossy(&retrieved_ranges_data[i])
        );
    }

    // Test with empty ranges list
    let empty_ranges_vec: Vec<std::ops::Range<u64>> = Vec::new();
    let retrieved_empty_ranges_data = storage
        .get_ranges(&path, &empty_ranges_vec)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] get_ranges with empty range list failed: {:?}",
                provider_name, e
            )
        });
    assert!(
        retrieved_empty_ranges_data.is_empty(),
        "[{}] get_ranges with empty input should return empty Vec",
        provider_name
    );

    // Test with a single range
    let single_range = [10u64..15u64]; // "ABCDE"
    let expected_single_data = [Bytes::from_static(b"ABCDE")];
    let retrieved_single_range_data = storage
        .get_ranges(&path, &single_range)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] get_ranges with single range failed: {:?}",
                provider_name, e
            )
        });
    assert_eq!(retrieved_single_range_data.len(), 1);
    assert_eq!(
        retrieved_single_range_data[0], expected_single_data[0],
        "[{}] Data mismatch for single range test",
        provider_name
    );

    // Clean up
    storage.delete(&path).await.ok();
}

#[rstest]
#[case::local(LocalProviderTestContext::setup().await)]
#[cfg_attr(feature = "grpc", case::grpc(GrpcProviderTestContext::setup().await))]
#[cfg_attr(feature = "aws", case::aws(AwsProviderTestContext::setup().await))]
#[cfg_attr(feature = "gcp", case::gcp(GcpProviderTestContext::setup().await))]
#[cfg_attr(feature = "azure", case::azure(AzureProviderTestContext::setup().await))]
// #[cfg_attr(feature = "kubernetes", case::kubernetes(KubernetesProviderTestContext::setup().await))]
#[tokio::test]
async fn test_presigned_requests(#[case] ctx: impl StorageTestContext) {
    use std::time::Duration;

    let storage = ctx.get_storage().await;
    let provider_name = ctx.provider_name();
    let unique_id = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();

    let path = Path::from(format!(
        "presigned_test_{}_{}.txt",
        provider_name, unique_id
    ));
    let test_data = Bytes::from_static(b"Presigned request test data content");
    let expires_in = Duration::from_secs(3600); // 1 hour

    // Test 1: Generate a presigned PUT request and use it to upload data
    let put_request = storage
        .presigned_put(&path, expires_in)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to generate presigned PUT request: {:?}",
                provider_name, e
            )
        });

    // Verify the request is properly constructed
    assert_eq!(put_request.method(), "PUT");
    assert_eq!(put_request.path, path.to_string());
    assert!(!put_request.is_expired());

    // Execute the presigned PUT request
    let put_response = put_request
        .execute(Some(test_data.clone()))
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to execute presigned PUT request: {:?}",
                provider_name, e
            )
        });

    assert!(
        put_response.status_code >= 200 && put_response.status_code < 300,
        "[{}] PUT with presigned request failed with status: {}",
        provider_name,
        put_response.status_code
    );

    // Verify the object was uploaded by checking with the storage directly
    let head_result = storage.head(&path).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Head operation failed after presigned PUT: {:?}",
            provider_name, e
        )
    });
    assert_eq!(
        head_result.size,
        test_data.len() as u64,
        "[{}] Object size mismatch after presigned PUT",
        provider_name
    );

    // Test 2: Generate a presigned GET request and use it to download data
    let get_request = storage
        .presigned_get(&path, expires_in)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to generate presigned GET request: {:?}",
                provider_name, e
            )
        });

    // Verify the request is properly constructed
    assert_eq!(get_request.method(), "GET");
    assert_eq!(get_request.path, path.to_string());
    assert!(!get_request.is_expired());

    // Execute the presigned GET request
    let get_response = get_request.execute(None).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to execute presigned GET request: {:?}",
            provider_name, e
        )
    });

    assert!(
        get_response.status_code >= 200 && get_response.status_code < 300,
        "[{}] GET with presigned request failed with status: {}",
        provider_name,
        get_response.status_code
    );

    let retrieved_data = get_response
        .body
        .unwrap_or_else(|| panic!("[{}] GET response body was empty", provider_name));

    assert_eq!(
        retrieved_data, test_data,
        "[{}] Data retrieved via presigned GET does not match original",
        provider_name
    );

    // Test 3: Generate a presigned DELETE request and use it to delete the object
    let delete_request = storage
        .presigned_delete(&path, expires_in)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to generate presigned DELETE request: {:?}",
                provider_name, e
            )
        });

    // Verify the request is properly constructed
    assert_eq!(delete_request.method(), "DELETE");
    assert_eq!(delete_request.path, path.to_string());
    assert!(!delete_request.is_expired());

    // Execute the presigned DELETE request
    let delete_response = delete_request.execute(None).await.unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to execute presigned DELETE request: {:?}",
            provider_name, e
        )
    });

    assert!(
        delete_response.status_code >= 200 && delete_response.status_code < 300,
        "[{}] DELETE with presigned request failed with status: {}",
        provider_name,
        delete_response.status_code
    );

    // Verify the object was deleted
    match storage.head(&path).await {
        Err(object_store::Error::NotFound { .. }) => { /* Expected */ }
        Ok(_) => panic!(
            "[{}] Head succeeded after presigned DELETE, object should be gone",
            provider_name
        ),
        Err(e) => panic!(
            "[{}] Unexpected error for head after presigned DELETE: {:?}",
            provider_name, e
        ),
    }

    // Test 4: Test URL generation for different backends
    let url_put_request = storage
        .presigned_put(&Path::from("test_url.txt"), expires_in)
        .await
        .unwrap();

    let url = url_put_request.url();
    match provider_name {
        "local" | "kubernetes" => {
            assert!(
                url.starts_with("local://"),
                "[{}] Expected local:// URL, got: {}",
                provider_name,
                url
            );
        }
        "grpc" => {
            // gRPC now correctly proxies to the backend, so it returns the backend's URL type
            // In tests, gRPC is backed by local storage, so expect local:// URLs
            assert!(
                url.starts_with("local://"),
                "[{}] Expected local:// URL (gRPC proxied to local backend), got: {}",
                provider_name,
                url
            );
        }
        "aws" => {
            assert!(
                url.contains("amazonaws.com") || url.starts_with("https://"),
                "[{}] Expected AWS S3 URL, got: {}",
                provider_name,
                url
            );
        }
        "gcp" => {
            assert!(
                url.contains("googleapis.com") || url.starts_with("https://"),
                "[{}] Expected GCS URL, got: {}",
                provider_name,
                url
            );
        }
        "azure" => {
            assert!(
                url.contains("blob.core.windows.net") || url.starts_with("https://"),
                "[{}] Expected Azure Blob URL, got: {}",
                provider_name,
                url
            );
        }
        _ => {
            // For unknown providers, just verify it's a valid-looking URL
            assert!(
                !url.is_empty(),
                "[{}] URL should not be empty",
                provider_name
            );
        }
    }

    // Test 5: Test serialization/deserialization of presigned requests
    let original_request = storage
        .presigned_put(&Path::from("serialization_test.txt"), expires_in)
        .await
        .unwrap();

    let serialized = serde_json::to_string(&original_request).unwrap_or_else(|e| {
        panic!(
            "[{}] Failed to serialize presigned request: {:?}",
            provider_name, e
        )
    });

    let deserialized: alien_bindings::presigned::PresignedRequest =
        serde_json::from_str(&serialized).unwrap_or_else(|e| {
            panic!(
                "[{}] Failed to deserialize presigned request: {:?}",
                provider_name, e
            )
        });

    assert_eq!(original_request.method(), deserialized.method());
    assert_eq!(original_request.path, deserialized.path);
    assert_eq!(original_request.operation, deserialized.operation);
    assert_eq!(original_request.url(), deserialized.url());

    println!(
        "[{}] Presigned request test completed successfully",
        provider_name
    );
}
