//! Fast, deterministic deployment tests using Platform::Test
//!
//! These tests exercise the full alien_deployment::step() lifecycle with no cloud I/O.

use alien_bindings::Vault as _;
use alien_core::{
    ClientConfig, ComputeCluster, ComputeClusterOutputs, DeploymentConfig, DeploymentState,
    DeploymentStatus, EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot,
    Platform, ReleaseInfo, ResourceEntry, ResourceLifecycle, RuntimeMetadata,
    SetupUpdateAuthorization, Stack, StackSettings, StackState, Storage, Worker, WorkerCode,
};
use alien_infra::{register_registry_extension, LocalComputeClusterController};
use chrono::Utc;
use indexmap::IndexMap;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::OnceLock};
use tempfile::TempDir;
use tokio::sync::MutexGuard;

const MAX_STEPS: usize = 100;

static TEST_VAULT_ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

struct TestVaultEnv {
    _guard: MutexGuard<'static, ()>,
    _temp_dir: TempDir,
}

impl TestVaultEnv {
    /// The customer's side of the test vault: what they write with their
    /// cloud's CLI, outside Alien.
    fn customer_vault(&self) -> alien_bindings::providers::vault::LocalVault {
        alien_bindings::providers::vault::LocalVault::new(
            "secrets".to_string(),
            self._temp_dir.path().to_path_buf(),
        )
    }
}

impl Drop for TestVaultEnv {
    fn drop(&mut self) {
        std::env::remove_var("TEST_VAULT_DATA_DIR");
    }
}

async fn test_vault_env() -> TestVaultEnv {
    let guard = TEST_VAULT_ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    std::env::set_var("TEST_VAULT_DATA_DIR", temp_dir.path().to_str().unwrap());

    TestVaultEnv {
        _guard: guard,
        _temp_dir: temp_dir,
    }
}

/// Helper to run deployment steps until a terminal status or max steps
async fn run_until_status(
    mut state: DeploymentState,
    config: DeploymentConfig,
    target_statuses: &[DeploymentStatus],
) -> DeploymentState {
    for step in 0..MAX_STEPS {
        // Check if we've reached one of the target statuses
        if target_statuses.contains(&state.status) {
            return state;
        }

        // Execute one step
        let result =
            alien_deployment::step(state.clone(), config.clone(), ClientConfig::Test, None)
                .await
                .expect("Step should not fail");

        state = result.state;

        // Progress indicator
        println!(
            "Step {}: status={:?}, suggested_delay={:?}",
            step, state.status, result.suggested_delay_ms
        );
    }

    panic!(
        "Did not reach target status after {} steps. Final status: {:?}",
        MAX_STEPS, state.status
    );
}

/// Helper to run until any of the terminal/synced statuses
async fn run_to_completion(state: DeploymentState, config: DeploymentConfig) -> DeploymentState {
    run_until_status(
        state,
        config,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::InitialSetupFailed,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::UpdateFailed,
            DeploymentStatus::DeleteFailed,
            DeploymentStatus::RefreshFailed,
            DeploymentStatus::Deleted,
        ],
    )
    .await
}

/// Helper to request retry on a failed deployment
fn request_retry(state: &mut DeploymentState) {
    state.retry_requested = true;
}

/// Helper to start an update
fn start_update(state: &mut DeploymentState, new_release: ReleaseInfo) {
    state.status = DeploymentStatus::UpdatePending;
    state.target_release = Some(new_release);
}

/// Helper to start a delete
fn start_delete(state: &mut DeploymentState) {
    state.status = DeploymentStatus::DeletePending;
    // Keep target_release when starting delete - it's needed for preflight/mutation steps
    if state.target_release.is_none() && state.current_release.is_some() {
        state.target_release = state.current_release.clone();
    }
}

/// Create a minimal stack fixture for Platform::Test
fn create_test_stack(stack_id: &str, function_id: &str) -> Stack {
    let function = Worker::new(function_id.to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .build();

    let mut resources = IndexMap::new();
    resources.insert(
        function_id.to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(function),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );

    let mut profiles = IndexMap::new();
    profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

    Stack {
        id: stack_id.to_string(),
        resources,
        permissions: alien_core::PermissionsConfig {
            profiles,
            management: alien_core::ManagementPermissions::Auto,
        },
        supported_platforms: None,
        inputs: Vec::new(),
        dynamic_container_repositories: Vec::new(),
        dynamic_container_image_resources: Vec::new(),
    }
}

/// Create a stack with one setup-owned Frozen resource and one Alien-owned Live resource.
fn create_test_stack_with_storage(stack_id: &str, storage_id: &str, function_id: &str) -> Stack {
    let storage = Storage::new(storage_id.to_string()).build();
    let function = Worker::new(function_id.to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .build();

    let mut resources = IndexMap::new();
    resources.insert(
        storage_id.to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(storage),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );
    resources.insert(
        function_id.to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(function),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );

    let mut profiles = IndexMap::new();
    profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

    Stack {
        id: stack_id.to_string(),
        resources,
        permissions: alien_core::PermissionsConfig {
            profiles,
            management: alien_core::ManagementPermissions::Auto,
        },
        supported_platforms: None,
        inputs: Vec::new(),
        dynamic_container_repositories: Vec::new(),
        dynamic_container_image_resources: Vec::new(),
    }
}

/// Create an environment variables snapshot fixture
fn create_env_vars_snapshot(hash: &str, include_secret: bool) -> EnvironmentVariablesSnapshot {
    let mut variables = vec![EnvironmentVariable {
        name: "PLAIN_VAR".to_string(),
        value: "plain_value".to_string(),
        var_type: EnvironmentVariableType::Plain,
        target_resources: None,
    }];

    if include_secret {
        variables.push(EnvironmentVariable {
            name: "SECRET_VAR".to_string(),
            value: "secret_value".to_string(),
            var_type: EnvironmentVariableType::Secret,
            target_resources: None,
        });
    }

    EnvironmentVariablesSnapshot {
        hash: hash.to_string(),
        variables,
        created_at: Utc::now().to_rfc3339(),
    }
}

fn expected_secrets_sync_hash(secret_value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"\0vault-sync:vault-backed-consumers:v3\0");
    hasher.update(b"SECRET_VAR\0");
    hasher.update(secret_value.as_bytes());
    hasher.update(b"\0");
    format!("{:x}", hasher.finalize())
}

/// Create a deployment config fixture
fn create_test_config(env_vars_hash: &str, include_secret: bool) -> DeploymentConfig {
    DeploymentConfig {
        stored_secret_input_ids: None,
        input_values: Default::default(),
        deployment_name: Some("test deployment".to_string()),
        stack_settings: StackSettings::default(),
        management_config: None,
        environment_variables: create_env_vars_snapshot(env_vars_hash, include_secret),
        external_bindings: alien_core::ExternalBindings::default(),
        base_platform: None,
        label_domain: None,
        observe_label_selector: None,
        observe_all_namespaces: false,
        compute_backend: None,
        allow_frozen_changes: false,
        domain_metadata: None,
        public_endpoints: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
        volume_restores: Vec::new(),
    }
}

/// Create an initial deployment state
fn create_initial_state(stack: Stack) -> DeploymentState {
    let release = ReleaseInfo {
        release_id: Some("rel_v1".to_string()),
        version: Some("1.0.0".to_string()),
        description: None,
        stack,
    };

    DeploymentState {
        status: DeploymentStatus::Pending,
        platform: Platform::Test,
        current_release: None,
        target_release: Some(release),
        stack_state: None,
        error: None,
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

/// A) Initial deploy flow tests

#[tokio::test]
async fn test_pending_to_running_happy_path_promotes_release() {
    let _vault = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    // Track initial target release
    let initial_target = state.target_release.clone().unwrap();

    // Run to completion
    state = run_to_completion(state, config).await;

    // Assert successful deployment
    assert_eq!(state.status, DeploymentStatus::Running);

    // Assert prepared_stack was set during Pending
    assert!(
        state.runtime_metadata.is_some(),
        "runtime_metadata should be set"
    );
    assert!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .prepared_stack
            .is_some(),
        "prepared_stack should be set"
    );

    // Assert release promotion
    assert_eq!(
        state.current_release.as_ref().unwrap().release_id,
        initial_target.release_id,
        "current_release should be promoted from target"
    );
    assert!(
        state.target_release.is_none(),
        "target_release should be cleared"
    );
}

#[tokio::test]
async fn test_initial_setup_creates_only_frozen_resources() {
    let _vault = test_vault_env().await;

    let stack = create_test_stack_with_storage("test-stack", "test-storage", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    state = run_until_status(state, config.clone(), &[DeploymentStatus::Provisioning]).await;

    let stack_state = state
        .stack_state
        .as_ref()
        .expect("stack_state should exist after InitialSetup");
    let storage = stack_state
        .resources
        .get("test-storage")
        .expect("frozen storage should be created during InitialSetup");
    assert_eq!(storage.status, alien_core::ResourceStatus::Running);
    assert!(
        !stack_state.resources.contains_key("test-function"),
        "live function must not be created during InitialSetup"
    );

    state = run_to_completion(state, config).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let function = state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .get("test-function")
        .expect("live function should be created during Provisioning");
    assert_eq!(function.status, alien_core::ResourceStatus::Running);
}

#[tokio::test]
async fn stale_waiting_for_machines_returns_to_provisioning() {
    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let state = run_until_status(
        create_initial_state(stack),
        config.clone(),
        &[DeploymentStatus::Provisioning],
    )
    .await;
    assert!(state.current_release.is_none());
    assert!(state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .is_none());
    let target = state.target_release.clone();
    let stale_state = DeploymentState {
        status: DeploymentStatus::WaitingForMachines,
        ..state
    };

    let result = alien_deployment::step(stale_state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("provisioning step should succeed");

    assert_eq!(result.state.status, DeploymentStatus::Provisioning);
    let completed = run_to_completion(result.state, config).await;
    assert_eq!(completed.status, DeploymentStatus::Running);
    assert_eq!(completed.current_release, target);
    assert!(completed.target_release.is_none());
    assert_eq!(
        completed.stack_state.as_ref().unwrap().resources["test-function"].status,
        alien_core::ResourceStatus::Running
    );
}

#[tokio::test]
async fn initial_update_resumes_prepared_target_after_waiting_for_machines() {
    let config = create_test_config("target-env", false);
    let old_stack = create_test_stack_with_storage("test-stack", "archive", "old-worker");
    let mut state = run_until_status(
        create_initial_state(old_stack),
        config.clone(),
        &[DeploymentStatus::Provisioning],
    )
    .await;
    assert!(state.current_release.is_none());
    let frozen = state.stack_state.as_ref().unwrap().resources["archive"].clone();
    let baseline = state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .prepared_stack
        .clone();
    state
        .runtime_metadata
        .as_mut()
        .unwrap()
        .direct_setup_revision = Some("setup-revision".into());

    let mut target = create_test_stack_with_storage("test-stack", "archive", "new-worker");
    target.resources.get_mut("new-worker").unwrap().config = alien_core::Resource::new(
        Worker::new("new-worker".into())
            .code(WorkerCode::Image {
                image: "demo:target".into(),
            })
            .permissions("default".into())
            .build(),
    );
    let target_release = ReleaseInfo {
        release_id: Some("rel_target".into()),
        version: Some("2.0.0".into()),
        description: None,
        stack: target,
    };
    start_update(&mut state, target_release.clone());
    state = run_until_status(state, config.clone(), &[DeploymentStatus::Updating]).await;
    let pending = state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .clone();
    assert!(pending
        .as_ref()
        .unwrap()
        .resources
        .contains_key("new-worker"));
    assert_eq!(
        state.runtime_metadata.as_ref().unwrap().prepared_stack,
        baseline
    );

    // The setup-owned cluster is already Running; only its persisted inventory changes.
    // Test controllers reconcile the workloads, and no cluster controller action is needed.
    register_registry_extension(Box::new(|registry| {
        registry
            .register::<ComputeCluster>(ComputeCluster::RESOURCE_TYPE)
            .with_controller::<LocalComputeClusterController>(Platform::Test);
    }));
    let cluster = ResourceEntry {
        config: alien_core::Resource::new(ComputeCluster::new("machines".into()).build()),
        lifecycle: ResourceLifecycle::Frozen,
        dependencies: Vec::new(),
        remote_access: false,
        enabled_when: None,
    };
    let metadata = state.runtime_metadata.as_mut().unwrap();
    metadata
        .prepared_stack
        .as_mut()
        .unwrap()
        .resources
        .insert("machines".into(), cluster.clone());
    metadata
        .pending_prepared_stack
        .as_mut()
        .unwrap()
        .resources
        .insert("machines".into(), cluster);
    let pending = metadata.pending_prepared_stack.clone();
    state.platform = Platform::Machines;
    let mut inventory = alien_core::StackResourceState::new_pending(
        ComputeCluster::RESOURCE_TYPE.to_string(),
        alien_core::Resource::new(ComputeCluster::new("machines".into()).build()),
        Some(ResourceLifecycle::Frozen),
        Vec::new(),
    );
    inventory.status = alien_core::ResourceStatus::Running;
    let outputs = ComputeClusterOutputs {
        cluster_id: "demo-cluster".into(),
        horizon_ready: true,
        capacity_group_statuses: vec![],
        total_machines: 0,
    };
    inventory.outputs = Some(alien_core::ResourceOutputs::new(outputs.clone()));
    state
        .stack_state
        .as_mut()
        .unwrap()
        .resources
        .insert("machines".into(), inventory);
    let result = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("updating should wait for inventory");
    state = result.state;
    assert_eq!(state.status, DeploymentStatus::WaitingForMachines);
    assert_eq!(result.suggested_delay_ms, Some(30_000));
    assert!(state.current_release.is_none());
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .pending_prepared_stack,
        pending
    );

    // Round-trip the checkpoint just as a later loop invocation reloads persisted state.
    state = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    state
        .stack_state
        .as_mut()
        .unwrap()
        .resources
        .get_mut("machines")
        .unwrap()
        .outputs = Some(alien_core::ResourceOutputs::new(ComputeClusterOutputs {
        total_machines: 1,
        ..outputs
    }));
    state = run_to_completion(state, config).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert_eq!(state.current_release, Some(target_release));
    assert!(state.target_release.is_none());
    let metadata = state.runtime_metadata.as_ref().unwrap();
    assert_eq!(metadata.prepared_stack, pending);
    assert!(metadata.pending_prepared_stack.is_none());
    assert_eq!(
        metadata.direct_setup_revision.as_deref(),
        Some("setup-revision")
    );
    let resources = &state.stack_state.as_ref().unwrap().resources;
    assert_eq!(
        serde_json::to_value(&resources["archive"]).unwrap(),
        serde_json::to_value(frozen).unwrap()
    );
    assert!(!resources.contains_key("old-worker"));
    assert_eq!(
        resources["new-worker"].status,
        alien_core::ResourceStatus::Running
    );
    let worker = resources["new-worker"]
        .config
        .downcast_ref::<Worker>()
        .unwrap();
    assert_eq!(
        worker.code,
        WorkerCode::Image {
            image: "demo:target".into()
        }
    );
    assert_eq!(
        worker.environment.get("PLAIN_VAR").map(String::as_str),
        Some("plain_value")
    );
}

/// B) Secrets sync behavior tests

#[tokio::test]
async fn test_provisioning_syncs_secrets_before_live_compute() {
    // Verify that secrets are synced before live compute resources are stepped.
    // With Frozen-only InitialSetup, the vault may become Running on the same
    // step that transitions to Provisioning, so Provisioning is the first
    // guaranteed sync point.
    let _vault_env = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", true);
    let mut state = create_initial_state(stack);

    // Run until we reach Provisioning (InitialSetup must complete first)
    state = run_until_status(
        state.clone(),
        config.clone(),
        &[DeploymentStatus::Provisioning],
    )
    .await;

    // Execute one provisioning step. It must sync secrets before stepping
    // the live function.
    let result = alien_deployment::step(state.clone(), config.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    state = result.state;

    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .last_synced_env_vars_hash
            .as_deref(),
        Some(expected_secrets_sync_hash("secret_value").as_str()),
        "Secrets should be synced before live compute is stepped"
    );
}

#[tokio::test]
async fn test_deploy_with_secrets_reaches_running() {
    // End-to-end: a deployment with secret env vars should reach Running.
    // Before the fix, InitialSetup skipped secrets → functions crashed on
    // missing ALIEN_COMMANDS_TOKEN → deployment stuck in InitialSetupFailed.
    let _vault_env = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", true);
    let state = create_initial_state(stack);

    let final_state = run_to_completion(state, config).await;

    assert_eq!(
        final_state.status,
        DeploymentStatus::Running,
        "Deployment with secrets should reach Running (not stuck in InitialSetupFailed)"
    );

    // Secrets should have been synced (hash recorded)
    assert_eq!(
        final_state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .last_synced_env_vars_hash
            .as_deref(),
        Some(expected_secrets_sync_hash("secret_value").as_str()),
    );
}

#[tokio::test]
async fn test_provisioning_syncs_secrets_once_per_hash() {
    let _vault_env = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", true);
    let mut state = create_initial_state(stack);

    // Run until we reach Provisioning
    state = run_until_status(
        state.clone(),
        config.clone(),
        &[DeploymentStatus::Provisioning],
    )
    .await;

    // Execute one provisioning step to trigger secret sync
    let result = alien_deployment::step(state.clone(), config.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    state = result.state;

    // Assert hash was recorded after first sync
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .last_synced_env_vars_hash
            .as_ref()
            .unwrap(),
        &expected_secrets_sync_hash("secret_value")
    );

    // Run another step with same config (should skip sync)
    let result2 = alien_deployment::step(state.clone(), config.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    state = result2.state;

    // Hash should still be hash_v1 (not changed)
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .last_synced_env_vars_hash
            .as_ref()
            .unwrap(),
        &expected_secrets_sync_hash("secret_value")
    );

    // Should continue progressing (no error from skipped sync)
    assert!(
        state.status == DeploymentStatus::Provisioning || state.status == DeploymentStatus::Running
    );
}

#[tokio::test]
async fn test_provisioning_resyncs_when_hash_changes() {
    let _vault_env = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config1 = create_test_config("hash_v1", true);
    let mut state = create_initial_state(stack.clone());

    // Run until Provisioning and sync with hash_v1
    state = run_until_status(
        state.clone(),
        config1.clone(),
        &[DeploymentStatus::Provisioning],
    )
    .await;
    let result = alien_deployment::step(state.clone(), config1.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    state = result.state;

    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .last_synced_env_vars_hash
            .as_ref()
            .unwrap(),
        &expected_secrets_sync_hash("secret_value")
    );

    // Now change the desired secret value. Snapshot-only changes intentionally
    // do not trigger vault writes when the desired vault contents are unchanged.
    let mut config2 = create_test_config("hash_v2", true);
    config2.environment_variables.variables[1].value = "secret_value_v2".to_string();

    // If provisioning already completed (transitioned to Running), set state
    // back to Provisioning to test the resync behavior.
    if state.status != DeploymentStatus::Provisioning {
        state.status = DeploymentStatus::Provisioning;
    }

    // Run another step with new config
    let result2 = alien_deployment::step(state.clone(), config2.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    state = result2.state;

    // Hash should now be hash_v2 (resynced)
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .last_synced_env_vars_hash
            .as_ref()
            .unwrap(),
        &expected_secrets_sync_hash("secret_value_v2")
    );
}

/// C) Running health checks + heartbeat tests

#[tokio::test]
async fn test_running_updates_heartbeat_when_healthy() {
    let _vault = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    // Get to Running state
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);

    // Preserve target_release for the step call (deployment expects it even in Running)
    if state.target_release.is_none() && state.current_release.is_some() {
        state.target_release = state.current_release.clone();
    }

    // Call step() on Running status
    let result = alien_deployment::step(state.clone(), config, ClientConfig::Test, None)
        .await
        .expect("Step should succeed");

    // Assert status remains Running
    assert_eq!(result.state.status, DeploymentStatus::Running);

    // Assert heartbeat flag is set
    assert!(
        result.update_heartbeat,
        "update_heartbeat should be true for healthy Running"
    );
}

#[tokio::test]
async fn health_failure_recovers_through_observation_without_retry_or_reprovisioning() {
    let mut stack = create_test_stack("health-stack", "health-worker");
    let worker = Worker::new("health-worker".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .environment(HashMap::from([
            (
                "SIMULATE_OBSERVED_URL_REFRESH".to_string(),
                "true".to_string(),
            ),
            (
                "SIMULATE_OBSERVED_REFRESH_FAIL_ONCE".to_string(),
                "true".to_string(),
            ),
        ]))
        .build();
    stack.resources.get_mut("health-worker").unwrap().config = alien_core::Resource::new(worker);
    let config = create_test_config("health", false);
    let state = run_to_completion(create_initial_state(stack), config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let release = state.current_release.clone();
    let failed = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .unwrap();
    assert_eq!(failed.state.status, DeploymentStatus::RefreshFailed);
    assert!(!failed.state.retry_requested);
    let recovered = alien_deployment::step(failed.state, config, ClientConfig::Test, None)
        .await
        .unwrap();
    assert_eq!(recovered.state.status, DeploymentStatus::Running);
    assert!(recovered.update_heartbeat);
    assert!(!recovered.state.retry_requested);
    assert_eq!(recovered.state.current_release, release);
    let worker = &recovered.state.stack_state.as_ref().unwrap().resources["health-worker"];
    let outputs = worker
        .outputs
        .as_ref()
        .unwrap()
        .downcast_ref::<alien_core::WorkerOutputs>()
        .unwrap();
    // The second observation proves the existing controller survived; recreating
    // it would reset the observation counter and repeat the first failure.
    assert_eq!(
        outputs.public_endpoints["default"].url,
        "https://observed-2.test"
    );
}

#[tokio::test]
async fn explicit_retry_after_health_failure_preserves_the_running_resource() {
    let mut stack = create_test_stack("retry-health-stack", "health-worker");
    let worker = Worker::new("health-worker".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .environment(HashMap::from([
            (
                "SIMULATE_OBSERVED_URL_REFRESH".to_string(),
                "true".to_string(),
            ),
            (
                "SIMULATE_OBSERVED_REFRESH_FAIL_ONCE".to_string(),
                "true".to_string(),
            ),
        ]))
        .build();
    stack.resources.get_mut("health-worker").unwrap().config = alien_core::Resource::new(worker);
    let config = create_test_config("retry-health", false);
    let state = run_to_completion(create_initial_state(stack), config.clone()).await;
    let failed = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .unwrap();
    assert_eq!(failed.state.status, DeploymentStatus::RefreshFailed);

    let mut retry_state = failed.state;
    request_retry(&mut retry_state);
    let retried = alien_deployment::step(retry_state, config.clone(), ClientConfig::Test, None)
        .await
        .unwrap();
    assert_eq!(retried.state.status, DeploymentStatus::Running);
    assert!(!retried.state.retry_requested);
    let retried_worker = &retried.state.stack_state.as_ref().unwrap().resources["health-worker"];
    assert_eq!(
        retried_worker.status,
        alien_core::ResourceStatus::RefreshFailed
    );
    let retried_outputs = retried_worker
        .outputs
        .as_ref()
        .unwrap()
        .downcast_ref::<alien_core::WorkerOutputs>()
        .unwrap();
    assert_eq!(
        retried_outputs.identifier.as_deref(),
        Some("test:worker:health-worker")
    );

    let observed = alien_deployment::step(retried.state, config, ClientConfig::Test, None)
        .await
        .unwrap();
    assert_eq!(observed.state.status, DeploymentStatus::Running);
    let observed_worker = &observed.state.stack_state.as_ref().unwrap().resources["health-worker"];
    let observed_outputs = observed_worker
        .outputs
        .as_ref()
        .unwrap()
        .downcast_ref::<alien_core::WorkerOutputs>()
        .unwrap();
    assert_eq!(
        observed_outputs.public_endpoints["default"].url,
        "https://observed-2.test"
    );
}

#[tokio::test]
async fn persistent_worker_failure_surfaces_during_provisioning() {
    let _vault = test_vault_env().await;

    // Create a function configured to fail persistently
    let function = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .environment({
            let mut env = HashMap::new();
            env.insert(
                "SIMULATE_PERSISTENT_FAILURE".to_string(),
                "true".to_string(),
            );
            env
        })
        .build();

    let mut resources = IndexMap::new();
    resources.insert(
        "test-function".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(function),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );

    let mut profiles = IndexMap::new();
    profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

    let stack = Stack {
        id: "test-stack".to_string(),
        resources,
        permissions: alien_core::PermissionsConfig {
            profiles,
            management: alien_core::ManagementPermissions::Auto,
        },
        supported_platforms: None,
        inputs: Vec::new(),
        dynamic_container_repositories: Vec::new(),
        dynamic_container_image_resources: Vec::new(),
    };

    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    // This should fail during Provisioning because functions are Live resources.
    state = run_until_status(
        state,
        config.clone(),
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;

    assert_eq!(state.status, DeploymentStatus::ProvisioningFailed);
}

/// D) Update flow tests

#[tokio::test]
async fn test_update_flow_happy_path_promotes_release() {
    let _vault = test_vault_env().await;

    let stack_v1 = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack_v1);

    // Get to Running with release v1
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let v1_release = state.current_release.clone().unwrap();

    // Start an actual config update to v2.
    let mut stack_v2 = create_test_stack("test-stack", "test-function");
    let worker_v2 = Worker::new("test-function".to_string())
        .code(WorkerCode::Image {
            image: "test:v2".to_string(),
        })
        .permissions("default".to_string())
        .build();
    stack_v2.resources.get_mut("test-function").unwrap().config =
        alien_core::Resource::new(worker_v2);
    let release_v2 = ReleaseInfo {
        release_id: Some("rel_v2".to_string()),
        version: Some("2.0.0".to_string()),
        description: None,
        stack: stack_v2,
    };
    start_update(&mut state, release_v2.clone());

    // Run update to completion
    state = run_to_completion(state, config).await;

    // Assert successful update
    assert_eq!(state.status, DeploymentStatus::Running);

    // Assert release promotion
    assert_eq!(
        state.current_release.as_ref().unwrap().release_id,
        release_v2.release_id,
        "current_release should be promoted to v2"
    );
    assert!(
        state.target_release.is_none(),
        "target_release should be cleared"
    );
    assert_ne!(
        state.current_release.as_ref().unwrap().release_id,
        v1_release.release_id,
        "should have updated from v1"
    );
}

fn worker_image_release(image: &str, release_id: &str) -> ReleaseInfo {
    let mut stack = create_test_stack("test-stack", "test-function");
    stack.resources.get_mut("test-function").unwrap().config = alien_core::Resource::new(
        Worker::new("test-function".to_string())
            .code(WorkerCode::Image {
                image: image.to_string(),
            })
            .permissions("default".to_string())
            .build(),
    );
    release_of(release_id, stack)
}

fn deployed_worker_image(state: &DeploymentState, worker_id: &str) -> String {
    match &state.stack_state.as_ref().unwrap().resources[worker_id]
        .config
        .downcast_ref::<Worker>()
        .unwrap()
        .code
    {
        WorkerCode::Image { image } => image.clone(),
        other => panic!("expected an image worker, got {other:?}"),
    }
}

/// A newer release can become the target while an update is still applying the
/// stack prepared for an older one. When that older stack converges, the newer
/// release has not been applied and must not be recorded as deployed.
#[tokio::test]
async fn a_release_that_supersedes_an_in_flight_update_is_applied_before_it_is_recorded() {
    let _vault = test_vault_env().await;
    let config = create_test_config("hash_v1", false);
    let mut state = run_to_completion(
        create_initial_state(create_test_stack("test-stack", "test-function")),
        config.clone(),
    )
    .await;
    assert_eq!(state.status, DeploymentStatus::Running);

    start_update(&mut state, worker_image_release("test:v2", "rel_v2"));
    state = run_until_status(state, config.clone(), &[DeploymentStatus::Updating]).await;
    assert_eq!(
        deployed_worker_image(&state, "test-function"),
        "test:latest",
        "the v2 stack is prepared but not applied yet"
    );

    // The manager rebuilds the target from the deployment record on every
    // pass, so a superseding release replaces it mid-update.
    let release_v3 = worker_image_release("test:v3", "rel_v3");
    state.target_release = Some(release_v3.clone());

    // v2 converges first. It is installed and must be reported as current
    // while v3 is prepared, in case v3 never succeeds.
    state = run_until_status(state, config.clone(), &[DeploymentStatus::UpdatePending]).await;
    assert_eq!(deployed_worker_image(&state, "test-function"), "test:v2");
    assert_eq!(
        state
            .current_release
            .as_ref()
            .unwrap()
            .release_id
            .as_deref(),
        Some("rel_v2")
    );
    assert_eq!(
        state.target_release.as_ref().unwrap().release_id,
        release_v3.release_id
    );

    state = run_to_completion(state, config).await;

    assert_eq!(state.status, DeploymentStatus::Running);
    assert_eq!(
        state.current_release.as_ref().unwrap().release_id,
        release_v3.release_id
    );
    assert_eq!(deployed_worker_image(&state, "test-function"), "test:v3");
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .prepared_stack
            .as_ref()
            .unwrap()
            .resources["test-function"]
            .config
            .downcast_ref::<Worker>()
            .unwrap()
            .code,
        WorkerCode::Image {
            image: "test:v3".to_string()
        },
        "the installed baseline must be the v3 stack"
    );
}

#[tokio::test]
async fn setup_authorized_update_clears_authority_only_on_success() {
    let config = create_test_config("hash_v1", false);
    let mut state = run_to_completion(
        create_initial_state(create_test_stack("test-stack", "test-function")),
        config.clone(),
    )
    .await;

    let release_v2 = ReleaseInfo {
        release_id: Some("rel_v2".to_string()),
        version: Some("2.0.0".to_string()),
        description: None,
        stack: create_test_stack("test-stack", "test-function"),
    };
    state.target_release = Some(release_v2.clone());
    state.status = DeploymentStatus::UpdatePending;
    let frozen_digest = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.prepared_stack.as_ref())
        .unwrap()
        .setup_owned_digest();
    state
        .runtime_metadata
        .as_mut()
        .unwrap()
        .setup_update_authorization = Some(SetupUpdateAuthorization {
        nonce: "successful-setup-revision".to_string(),
        baseline_frozen_digest: frozen_digest.clone(),
        target_frozen_digest: frozen_digest,
        release_id: "rel_v2".to_string(),
        setup_target: "target".to_string(),
        setup_fingerprint: "fingerprint".to_string(),
        setup_fingerprint_version: 1,
    });

    let first_update_step = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("setup-authorized update should accept the desired release");
    assert_eq!(first_update_step.state.status, DeploymentStatus::Updating);

    let completed = run_to_completion(first_update_step.state, config).await;
    assert_eq!(completed.status, DeploymentStatus::Running);
    assert!(
        completed
            .runtime_metadata
            .as_ref()
            .unwrap()
            .setup_update_authorization
            .is_none(),
        "setup authorization must clear in the successful Running transition"
    );
    assert_eq!(
        completed.current_release.as_ref().unwrap().release_id,
        release_v2.release_id
    );
}

#[tokio::test]
async fn consecutive_updates_cannot_delete_an_omitted_frozen_resource() {
    let config = create_test_config("hash_v1", false);
    let mut state = run_to_completion(
        create_initial_state(create_test_stack("test-stack", "test-function")),
        config.clone(),
    )
    .await;
    assert_eq!(state.status, DeploymentStatus::Running);

    // Model setup-owned infrastructure imported alongside a release that does
    // not declare it. This is the ownership shape the runtime must preserve.
    let frozen = Storage::new("setup-storage".to_string()).build();
    state
        .runtime_metadata
        .as_mut()
        .and_then(|metadata| metadata.prepared_stack.as_mut())
        .expect("running deployment has prepared stack")
        .resources
        .insert(
            "setup-storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(frozen.clone()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
    let mut frozen_state = alien_core::StackResourceState::new_pending(
        "storage".to_string(),
        alien_core::Resource::new(frozen),
        Some(ResourceLifecycle::Frozen),
        Vec::new(),
    );
    frozen_state.status = alien_core::ResourceStatus::Running;
    frozen_state.internal_state = Some(serde_json::json!({
        "type": "TestStorageController",
        "_controllerStateVersion": 1,
        "state": "ready",
        "bucketName": "test-setup-storage",
    }));
    state
        .stack_state
        .as_mut()
        .expect("running deployment has stack state")
        .resources
        .insert("setup-storage".to_string(), frozen_state);

    for release_number in [2, 3] {
        let mut target = create_test_stack("test-stack", "test-function");
        target.permissions = state
            .current_release
            .as_ref()
            .expect("running deployment has current release")
            .stack
            .permissions
            .clone();
        start_update(
            &mut state,
            ReleaseInfo {
                release_id: Some(format!("rel_v{release_number}")),
                version: Some(format!("{release_number}.0.0")),
                description: None,
                // Both later releases omit the setup-owned frozen resource.
                stack: target,
            },
        );
        state = run_to_completion(state, config.clone()).await;
        assert_eq!(state.status, DeploymentStatus::Running);
        assert_eq!(
            state
                .stack_state
                .as_ref()
                .and_then(|stack| stack.resources.get("setup-storage"))
                .map(|resource| resource.status),
            Some(alien_core::ResourceStatus::Running),
            "release {release_number} must retain the installed frozen resource"
        );
        assert!(
            state
                .runtime_metadata
                .as_ref()
                .and_then(|metadata| metadata.prepared_stack.as_ref())
                .is_some_and(|stack| stack.resources.contains_key("setup-storage")),
            "release {release_number} must retain frozen ownership in the promoted baseline"
        );
    }
}

#[tokio::test]
async fn update_completes_after_removed_resource_is_deleted() {
    let _vault_env = test_vault_env().await;
    let config = create_test_config("hash_v1", false);
    let mut stack_v1 = create_test_stack("test-stack", "function-a");
    let function_b = Worker::new("function-b".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .build();
    stack_v1.resources.insert(
        "function-b".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(function_b),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );

    let mut state = run_to_completion(create_initial_state(stack_v1), config.clone()).await;
    let stack_v2 = create_test_stack("test-stack", "function-a");
    start_update(
        &mut state,
        ReleaseInfo {
            release_id: Some("rel_v2".to_string()),
            version: Some("2.0.0".to_string()),
            description: None,
            stack: stack_v2,
        },
    );

    let completed = run_to_completion(state, config).await;

    assert_eq!(completed.status, DeploymentStatus::Running);
    assert_eq!(
        completed
            .stack_state
            .as_ref()
            .unwrap()
            .resources
            .get("function-b")
            .unwrap()
            .status,
        alien_core::ResourceStatus::Deleted,
        "the executor's deletion tombstone should be preserved"
    );
    assert_eq!(
        completed.current_release.as_ref().unwrap().release_id,
        Some("rel_v2".to_string())
    );
    assert!(completed.target_release.is_none());
}

#[tokio::test]
async fn stale_waiting_for_machines_returns_to_updating() {
    let stack_v1 = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = run_to_completion(create_initial_state(stack_v1), config.clone()).await;
    let release_v2 = ReleaseInfo {
        release_id: Some("rel_v2".to_string()),
        version: Some("2.0.0".to_string()),
        description: None,
        stack: create_test_stack("test-stack", "test-function-v2"),
    };
    start_update(&mut state, release_v2.clone());
    state = run_until_status(state, config.clone(), &[DeploymentStatus::Updating]).await;
    state.status = DeploymentStatus::WaitingForMachines;

    let result = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("updating step should succeed");

    assert_eq!(result.state.status, DeploymentStatus::Updating);
    let completed = run_to_completion(result.state, config).await;
    assert_eq!(completed.status, DeploymentStatus::Running);
    assert_eq!(completed.current_release, Some(release_v2));
    assert!(completed.target_release.is_none());
    assert!(completed
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .is_none());
    assert_eq!(
        completed.stack_state.as_ref().unwrap().resources["test-function-v2"].status,
        alien_core::ResourceStatus::Running
    );
}

#[tokio::test]
async fn test_update_failed_retry_gate_returns_to_update_pending() {
    let _vault = test_vault_env().await;

    let stack_v1 = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack_v1);

    // Get to Running
    state = run_to_completion(state, config.clone()).await;

    // Start update with a function that will fail
    let function_v2 = Worker::new("test-function-v2".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .environment({
            let mut env = HashMap::new();
            env.insert(
                "SIMULATE_PERSISTENT_FAILURE".to_string(),
                "true".to_string(),
            );
            env
        })
        .build();

    let mut resources = IndexMap::new();
    resources.insert(
        "test-function-v2".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(function_v2),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );

    let mut profiles = IndexMap::new();
    profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

    let stack_v2 = Stack {
        id: "test-stack".to_string(),
        resources,
        permissions: alien_core::PermissionsConfig {
            profiles,
            management: alien_core::ManagementPermissions::Auto,
        },
        supported_platforms: None,
        inputs: Vec::new(),
        dynamic_container_repositories: Vec::new(),
        dynamic_container_image_resources: Vec::new(),
    };

    let release_v2 = ReleaseInfo {
        release_id: Some("rel_v2".to_string()),
        version: Some("2.0.0".to_string()),
        description: None,
        stack: stack_v2,
    };
    start_update(&mut state, release_v2);
    // Setup registration transitions the deployment directly to UpdatePending;
    // it is not an ordinary newer-release reconciliation.
    state.status = DeploymentStatus::UpdatePending;
    let frozen_digest = state
        .runtime_metadata
        .as_ref()
        .and_then(|metadata| metadata.prepared_stack.as_ref())
        .expect("running deployment should retain its prepared stack")
        .setup_owned_digest();
    state
        .runtime_metadata
        .as_mut()
        .unwrap()
        .setup_update_authorization = Some(SetupUpdateAuthorization {
        nonce: "setup-revision".to_string(),
        baseline_frozen_digest: frozen_digest.clone(),
        target_frozen_digest: frozen_digest,
        release_id: "rel_v2".to_string(),
        setup_target: "target".to_string(),
        setup_fingerprint: "fingerprint".to_string(),
        setup_fingerprint_version: 1,
    });

    // Run until UpdateFailed
    state = run_until_status(state, config.clone(), &[DeploymentStatus::UpdateFailed]).await;
    assert!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .setup_update_authorization
            .is_some(),
        "failed update must retain setup authorization before persistence"
    );

    // Persist and restore the failed deployment, matching a manager restart.
    state = serde_json::from_str(
        &serde_json::to_string(&state).expect("failed deployment should serialize"),
    )
    .expect("failed deployment should deserialize");

    let failed_resource = state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .values()
        .find(|resource| {
            matches!(
                resource.status,
                alien_core::ResourceStatus::ProvisionFailed
                    | alien_core::ResourceStatus::UpdateFailed
                    | alien_core::ResourceStatus::DeleteFailed
                    | alien_core::ResourceStatus::RefreshFailed
            )
        })
        .expect("update should contain a failed resource");
    assert!(failed_resource.retry_attempt > 0);
    assert!(failed_resource.last_failed_state.is_some());
    let exhausted_retry_attempt = failed_resource.retry_attempt;
    assert!(
        state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .setup_update_authorization
            .is_some(),
        "a failed, serialized update must retain setup authorization for retry"
    );

    // Without retry_requested, should stay in failed state
    let result = alien_deployment::step(state.clone(), config.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    assert_eq!(result.state.status, DeploymentStatus::UpdateFailed);

    // Keep the persisted failed state to reproduce a corrective release being
    // selected directly by the release controller.
    let mut corrective_state = state.clone();

    // With retry_requested, should transition to UpdatePending (not Updating)
    request_retry(&mut state);
    let result = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    assert_eq!(
        result.state.status,
        DeploymentStatus::UpdatePending,
        "UpdateFailed retry should go to UpdatePending"
    );
    assert!(
        !result.state.retry_requested,
        "retry flag should be cleared"
    );

    let retried_resource = result
        .state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .values()
        .find(|resource| resource.config.id() == "test-function-v2")
        .expect("retried resource should remain in stack state");
    assert!(
        matches!(
            retried_resource.status,
            alien_core::ResourceStatus::ProvisionFailed
                | alien_core::ResourceStatus::UpdateFailed
                | alien_core::ResourceStatus::RefreshFailed
        ),
        "UpdatePending must retain the failed checkpoint until the final desired stack is prepared"
    );
    assert!(retried_resource.last_failed_state.is_some());
    assert!(
        result
            .state
            .runtime_metadata
            .as_ref()
            .unwrap()
            .setup_update_authorization
            .is_some(),
        "retry preparation must not consume setup authorization"
    );

    // Preflight first prepares the exact desired stack. Only the following
    // Updating step may decide that the failed resource is unchanged and
    // resume its saved controller with a fresh retry budget.
    let result = alien_deployment::step(result.state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("retry preflight should succeed");
    assert_eq!(result.state.status, DeploymentStatus::Updating);

    let result = alien_deployment::step(result.state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("unchanged failed resource should resume");
    let resumed_resource = result
        .state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .values()
        .find(|resource| resource.config.id() == "test-function-v2")
        .expect("resumed resource should remain in stack state");
    assert!(
        resumed_resource.retry_attempt < exhausted_retry_attempt,
        "resuming the unchanged failure should grant it a fresh retry budget"
    );

    // A newly selected corrective release enters UpdatePending directly rather
    // than passing through the manual UpdateFailed retry handler. The executor
    // must compare the fully prepared desired config with the failed config and
    // restart planning instead of resuming the superseded polling checkpoint.
    corrective_state.status = DeploymentStatus::UpdatePending;
    corrective_state.retry_requested = false;
    corrective_state.target_release = Some(ReleaseInfo {
        release_id: Some("rel_v3".to_string()),
        version: Some("3.0.0".to_string()),
        description: None,
        stack: create_test_stack("test-stack", "test-function-v2"),
    });
    corrective_state
        .runtime_metadata
        .as_mut()
        .unwrap()
        .setup_update_authorization = None;

    let corrected = run_to_completion(corrective_state, config).await;
    assert_eq!(corrected.status, DeploymentStatus::Running);
    assert_eq!(
        corrected
            .current_release
            .as_ref()
            .and_then(|release| release.release_id.as_deref()),
        Some("rel_v3")
    );
    let corrected_resource = corrected
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .get("test-function-v2")
        .expect("corrected resource should remain in stack state");
    assert_eq!(
        corrected_resource.status,
        alien_core::ResourceStatus::Running
    );
    assert_eq!(corrected_resource.retry_attempt, 0);
    assert!(corrected_resource.error.is_none());
    assert!(corrected_resource.last_failed_state.is_none());
}

async fn assert_failed_retry_transition(
    failed_status: DeploymentStatus,
    retried_status: DeploymentStatus,
) {
    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);
    state.status = failed_status;
    state.stack_state = Some(StackState::new(Platform::Test));

    // Setup and delete retries read what setup recorded; a deployment reaches either
    // failure only after Pending wrote runtime metadata.
    if matches!(
        failed_status,
        DeploymentStatus::DeleteFailed | DeploymentStatus::InitialSetupFailed
    ) {
        state.runtime_metadata = Some(RuntimeMetadata::default());
    }

    let result = alien_deployment::step(state.clone(), config.clone(), ClientConfig::Test, None)
        .await
        .expect("failed status without retry should remain terminal");
    assert_eq!(result.state.status, failed_status);
    assert!(
        !result.state.retry_requested,
        "retry flag should remain false when no retry was requested"
    );

    request_retry(&mut state);
    let result = alien_deployment::step(state, config, ClientConfig::Test, None)
        .await
        .expect("failed status with retry should transition");
    assert_eq!(result.state.status, retried_status);
    assert!(
        !result.state.retry_requested,
        "retry flag should be cleared after retry transition"
    );
}

#[tokio::test]
async fn test_initial_setup_failed_retry_gate_returns_to_initial_setup() {
    assert_failed_retry_transition(
        DeploymentStatus::InitialSetupFailed,
        DeploymentStatus::InitialSetup,
    )
    .await;
}

#[tokio::test]
async fn test_provisioning_failed_retry_gate_returns_to_provisioning() {
    assert_failed_retry_transition(
        DeploymentStatus::ProvisioningFailed,
        DeploymentStatus::Provisioning,
    )
    .await;
}

#[tokio::test]
async fn test_delete_failed_retry_gate_returns_to_deleting() {
    assert_failed_retry_transition(DeploymentStatus::DeleteFailed, DeploymentStatus::Deleting)
        .await;
}

/// E) Delete flow tests

#[tokio::test]
async fn test_delete_flow_happy_path_reaches_teardown_required() {
    let _vault = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    // Get to Running
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);

    // Start delete
    start_delete(&mut state);

    // Normal manager/agent deletion removes runtime-owned resources and then
    // stops at setup-owned teardown handoff.
    state = run_until_status(state, config, &[DeploymentStatus::TeardownRequired]).await;

    assert_eq!(state.status, DeploymentStatus::TeardownRequired);
}

#[tokio::test]
async fn test_delete_runtime_cleanup_reaches_teardown_required() {
    // Create a minimal test for delete retry pattern
    // In practice, TestWorkerController doesn't easily simulate delete failures,
    // but we can test the pattern conceptually by checking the handler exists

    let _vault = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    // Get to Running
    state = run_to_completion(state, config.clone()).await;

    // Start delete
    start_delete(&mut state);

    // Runtime delete should succeed and stop at setup-owned teardown handoff.
    state = run_until_status(state, config, &[DeploymentStatus::TeardownRequired]).await;
    assert_eq!(state.status, DeploymentStatus::TeardownRequired);
}

/// F) Interrupt-on-failure behavior

/// Build a two-resource stack where `failing-fn` will exhaust its retries and fail while
/// `sibling-fn` is independent (no dependency). `sibling-fn` starts provisioning first because
/// the executor processes resources in parallel; its provisioning will still be in progress when
/// `failing-fn` transitions to ProvisionFailed.
fn create_two_function_stack_one_fails(stack_id: &str) -> Stack {
    let failing_fn = Worker::new("failing-fn".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .environment({
            let mut env = HashMap::new();
            env.insert(
                "SIMULATE_PERSISTENT_FAILURE".to_string(),
                "true".to_string(),
            );
            env
        })
        .build();

    let sibling_fn = Worker::new("sibling-fn".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .build();

    let mut resources = IndexMap::new();
    resources.insert(
        "failing-fn".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(failing_fn),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );
    resources.insert(
        "sibling-fn".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(sibling_fn),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );

    let mut profiles = IndexMap::new();
    profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

    Stack {
        id: stack_id.to_string(),
        resources,
        permissions: alien_core::PermissionsConfig {
            profiles,
            management: alien_core::ManagementPermissions::Auto,
        },
        supported_platforms: None,
        inputs: Vec::new(),
        dynamic_container_repositories: Vec::new(),
        dynamic_container_image_resources: Vec::new(),
    }
}

/// Build a two-resource stack where `sibling-fn` depends on `failing-fn`.
/// `sibling-fn` will be in Pending when `failing-fn` fails.
fn create_two_function_stack_dependent_one_fails(stack_id: &str) -> Stack {
    let failing_fn = Worker::new("failing-fn".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .environment({
            let mut env = HashMap::new();
            env.insert(
                "SIMULATE_PERSISTENT_FAILURE".to_string(),
                "true".to_string(),
            );
            env
        })
        .build();

    let sibling_fn = Worker::new("sibling-fn".to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .build();

    let mut resources = IndexMap::new();
    resources.insert(
        "failing-fn".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(failing_fn),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );
    resources.insert(
        "sibling-fn".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(sibling_fn),
            lifecycle: ResourceLifecycle::Live,
            dependencies: vec![alien_core::ResourceRef::new(
                alien_core::Worker::RESOURCE_TYPE,
                "failing-fn".to_string(),
            )],
            remote_access: false,
            enabled_when: None,
        },
    );

    let mut profiles = IndexMap::new();
    profiles.insert("default".to_string(), alien_core::PermissionProfile::new());

    Stack {
        id: stack_id.to_string(),
        resources,
        permissions: alien_core::PermissionsConfig {
            profiles,
            management: alien_core::ManagementPermissions::Auto,
        },
        supported_platforms: None,
        inputs: Vec::new(),
        dynamic_container_repositories: Vec::new(),
        dynamic_container_image_resources: Vec::new(),
    }
}

/// When one resource in a multi-resource deployment fails, all in-progress resources should
/// be transitioned to a *Failed status with a DEPLOYMENT_INTERRUPTED error so the UI shows
/// accurate statuses instead of stale "Provisioning" or "Pending" indicators.
#[tokio::test]
async fn test_partial_failure_interrupts_in_progress_resources() {
    let _vault = test_vault_env().await;

    let stack = create_two_function_stack_one_fails("test-stack");
    let config = create_test_config("hash_v1", false);
    let state = create_initial_state(stack);

    // Run until provisioning fails
    let final_state = run_to_completion(state, config).await;

    assert_eq!(final_state.status, DeploymentStatus::ProvisioningFailed);

    let stack_state = final_state
        .stack_state
        .as_ref()
        .expect("stack_state should be set");

    // The resource that actually failed must have ProvisionFailed
    let failing = stack_state
        .resources
        .get("failing-fn")
        .expect("failing-fn should exist");
    assert_eq!(failing.status, alien_core::ResourceStatus::ProvisionFailed);
    // The real failure is NOT DeploymentInterrupted
    if let Some(err) = &failing.error {
        assert_ne!(
            err.code.as_str(),
            "DEPLOYMENT_INTERRUPTED",
            "failing-fn should have its real error, not DEPLOYMENT_INTERRUPTED"
        );
    }

    // The sibling resource must NOT be left in Provisioning or Pending
    let sibling = stack_state
        .resources
        .get("sibling-fn")
        .expect("sibling-fn should exist");
    assert!(
        matches!(
            sibling.status,
            alien_core::ResourceStatus::ProvisionFailed
                | alien_core::ResourceStatus::UpdateFailed
                | alien_core::ResourceStatus::Running
        ),
        "sibling-fn should be in a terminal status, got {:?}",
        sibling.status,
    );

    // If the sibling was interrupted (not already Running), it must have the DEPLOYMENT_INTERRUPTED error
    if sibling.status != alien_core::ResourceStatus::Running {
        let err = sibling
            .error
            .as_ref()
            .expect("interrupted resource should have an error");
        assert_eq!(
            err.code.as_str(),
            "DEPLOYMENT_INTERRUPTED",
            "interrupted resource should carry DEPLOYMENT_INTERRUPTED error code"
        );
    }
}

/// A resource that was in Pending (never started) when a sibling failed should:
/// - End up in ProvisionFailed with DEPLOYMENT_INTERRUPTED
/// - Have last_failed_state = None (it never had a controller)
/// - On retry, reset cleanly to Pending so it starts fresh
#[tokio::test]
async fn test_partial_failure_pending_resource_retries_from_pending() {
    let _vault = test_vault_env().await;

    // sibling-fn depends on failing-fn — it will be stuck in Pending when failing-fn fails
    let stack = create_two_function_stack_dependent_one_fails("test-stack");
    let config = create_test_config("hash_v1", false);
    let state = create_initial_state(stack.clone());

    // Run until live provisioning fails.
    let mut final_state = run_to_completion(state, config.clone()).await;
    assert_eq!(final_state.status, DeploymentStatus::ProvisioningFailed);

    let stack_state = final_state
        .stack_state
        .as_ref()
        .expect("stack_state should be set");

    // sibling-fn was never started — it must be interrupted
    let sibling = stack_state
        .resources
        .get("sibling-fn")
        .expect("sibling-fn should exist");
    assert_eq!(sibling.status, alien_core::ResourceStatus::ProvisionFailed);
    let err = sibling
        .error
        .as_ref()
        .expect("sibling-fn should have an error");
    assert_eq!(
        err.code.as_str(),
        "DEPLOYMENT_INTERRUPTED",
        "sibling-fn should carry DEPLOYMENT_INTERRUPTED"
    );
    // last_failed_state should be None since no controller was ever initialized
    assert!(
        sibling.last_failed_state.is_none(),
        "Pending resource should have no last_failed_state"
    );

    // Request retry — failing-fn should get a new chance too, but since it still has
    // SIMULATE_PERSISTENT_FAILURE it will fail again.  More importantly, sibling-fn
    // must correctly reset to Pending so it can start provisioning.
    request_retry(&mut final_state);
    let after_retry =
        run_until_status(final_state, config, &[DeploymentStatus::ProvisioningFailed]).await;

    assert_eq!(after_retry.status, DeploymentStatus::ProvisioningFailed);

    // sibling-fn should again be ProvisionFailed (either re-interrupted or actually tried),
    // not stuck in a corrupted state
    let stack_state_after = after_retry
        .stack_state
        .as_ref()
        .expect("stack_state should be set");
    let sibling_after = stack_state_after
        .resources
        .get("sibling-fn")
        .expect("sibling-fn should still exist");
    assert!(
        matches!(
            sibling_after.status,
            alien_core::ResourceStatus::ProvisionFailed | alien_core::ResourceStatus::Running
        ),
        "sibling-fn should be in a terminal status after retry, got {:?}",
        sibling_after.status,
    );
}

fn add_live_worker(stack: &mut Stack, worker: Worker) {
    stack.resources.insert(
        worker.id.clone(),
        ResourceEntry {
            config: alien_core::Resource::new(worker),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );
}

fn image_worker(id: &str, image: &str, memory_mb: u32) -> Worker {
    Worker::new(id.to_string())
        .code(WorkerCode::Image {
            image: image.to_string(),
        })
        .memory_mb(memory_mb)
        .permissions("default".to_string())
        .build()
}

/// A sibling interrupted mid-create keeps the IDs its create recorded. When the corrective
/// release also changes that sibling, the update deletes what the interrupted create made,
/// against the config it was created with, before creating it again.
#[tokio::test]
async fn interrupted_sibling_with_changed_config_is_deleted_before_it_is_recreated() {
    let _vault = test_vault_env().await;
    let config = create_test_config("hash_v1", false);
    let mut state = run_to_completion(
        create_initial_state(create_test_stack("test-stack", "base-fn")),
        config.clone(),
    )
    .await;
    assert_eq!(state.status, DeploymentStatus::Running);

    // Release 2 adds a worker that fails at once (the test controller rejects more than
    // 4096 MB, without retries) and a sibling whose create is still in flight then.
    let mut stack_v2 = create_test_stack("test-stack", "base-fn");
    add_live_worker(&mut stack_v2, image_worker("rejected-fn", "test:v2", 5120));
    add_live_worker(
        &mut stack_v2,
        image_worker("interrupted-sibling-fn", "test:v2", 1024),
    );
    start_update(&mut state, release_of("rel_v2", stack_v2));
    let mut state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::UpdateFailed);

    let sibling = &state.stack_state.as_ref().unwrap().resources["interrupted-sibling-fn"];
    assert_eq!(sibling.status, alien_core::ResourceStatus::ProvisionFailed);
    assert_eq!(
        sibling.error.as_ref().map(|error| error.code.as_str()),
        Some("DEPLOYMENT_INTERRUPTED")
    );
    assert!(
        sibling.internal_state.is_some() && sibling.last_failed_state.is_some(),
        "the interrupted create keeps its controller state"
    );
    let sibling_identifier = "test:worker:interrupted-sibling-fn";
    assert!(alien_infra::test_worker_deletes_issued(sibling_identifier).is_empty());

    // Release 3 fixes the failing worker and changes the interrupted sibling.
    let mut stack_v3 = create_test_stack("test-stack", "base-fn");
    add_live_worker(&mut stack_v3, image_worker("rejected-fn", "test:v3", 1024));
    add_live_worker(
        &mut stack_v3,
        image_worker("interrupted-sibling-fn", "test:v3", 1024),
    );
    start_update(&mut state, release_of("rel_v3", stack_v3));
    let state = run_to_completion(state, config).await;
    assert_eq!(state.status, DeploymentStatus::Running);

    let deletes = alien_infra::test_worker_deletes_issued(sibling_identifier);
    assert_eq!(deletes.len(), 1, "the interrupted create is deleted once");
    assert_eq!(
        deletes[0].code,
        WorkerCode::Image {
            image: "test:v2".to_string()
        },
        "the delete runs against the config the sibling was created with"
    );
    assert_eq!(
        deployed_worker_image(&state, "interrupted-sibling-fn"),
        "test:v3"
    );
    assert_eq!(deployed_worker_image(&state, "rejected-fn"), "test:v3");
    // The rejected worker failed before its create recorded anything, so it had nothing to
    // delete remotely.
    assert!(alien_infra::test_worker_deletes_issued("test:worker:rejected-fn").is_empty());
}

/// A runtime retry does not resume a failed create whose config changed since it failed: that
/// would finish the create with a mix of both configs and never delete what the failed one
/// made. The retry leaves it failed, and the executor deletes it against the config it was
/// created with before creating it with the new one.
#[tokio::test]
async fn provisioning_retry_replaces_a_failed_create_whose_config_changed() {
    let _vault = test_vault_env().await;
    let worker_id = "retry-changed-fn";
    let worker_identifier = "test:worker:retry-changed-fn";
    let mut stack = create_test_stack("test-stack", "base-fn");
    add_live_worker(&mut stack, image_worker(worker_id, "test:v1", 1024));

    // The first attempt injects a variable that makes the worker's create fail right after
    // it recorded the worker.
    let mut failing_config = create_test_config("hash_v1", false);
    failing_config
        .environment_variables
        .variables
        .push(EnvironmentVariable {
            name: "SIMULATE_CREATE_WORKER_FAILURE".to_string(),
            value: "true".to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: Some(vec![worker_id.to_string()]),
        });
    let mut state = run_to_completion(create_initial_state(stack), failing_config).await;
    assert_eq!(state.status, DeploymentStatus::ProvisioningFailed);
    let failed = &state.stack_state.as_ref().unwrap().resources[worker_id];
    assert_eq!(failed.status, alien_core::ResourceStatus::ProvisionFailed);
    assert!(failed.internal_state.is_some() && failed.last_failed_state.is_some());
    assert!(alien_infra::test_worker_deletes_issued(worker_identifier).is_empty());

    // The retry runs with the variable gone.
    let fixed_config = create_test_config("hash_v2", false);
    request_retry(&mut state);
    let state = alien_deployment::step(state, fixed_config.clone(), ClientConfig::Test, None)
        .await
        .expect("the retry step should succeed")
        .state;
    assert_eq!(state.status, DeploymentStatus::Provisioning);
    assert_eq!(
        state.stack_state.as_ref().unwrap().resources[worker_id].status,
        alien_core::ResourceStatus::ProvisionFailed,
        "the retry leaves the changed create to the planner"
    );
    let state = run_to_completion(state, fixed_config).await;
    assert_eq!(state.status, DeploymentStatus::Running);

    let deletes = alien_infra::test_worker_deletes_issued(worker_identifier);
    assert_eq!(deletes.len(), 1, "the failed create is deleted once");
    assert_eq!(
        deletes[0]
            .environment
            .get("SIMULATE_CREATE_WORKER_FAILURE")
            .map(String::as_str),
        Some("true"),
        "the delete runs against the config the worker was created with"
    );
    let worker = &state.stack_state.as_ref().unwrap().resources[worker_id];
    assert_eq!(worker.status, alien_core::ResourceStatus::Running);
    let deployed = worker
        .config
        .downcast_ref::<Worker>()
        .expect("worker config");
    assert!(!deployed
        .environment
        .contains_key("SIMULATE_CREATE_WORKER_FAILURE"));
}

/// A Running deployment with this Frozen store failed mid-update at `updateConfig`, its
/// config unchanged, and the deployment in RefreshFailed.
async fn store_failed_mid_update(store_id: &str) -> (DeploymentState, DeploymentConfig) {
    let config = create_test_config("hash_v1", false);
    let mut state = run_to_completion(
        create_initial_state(create_test_stack_with_storage(
            "retry-stack",
            store_id,
            "retry-fn",
        )),
        config.clone(),
    )
    .await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let store = state
        .stack_state
        .as_mut()
        .unwrap()
        .resources
        .get_mut(store_id)
        .unwrap();
    let mut checkpoint = store.internal_state.clone().expect("store controller");
    checkpoint["state"] = serde_json::json!("updateConfig");
    store.status = alien_core::ResourceStatus::UpdateFailed;
    store.internal_state = Some(checkpoint.clone());
    store.last_failed_state = Some(checkpoint);
    state.status = DeploymentStatus::RefreshFailed;
    (state, config)
}

/// A retry resumes a failed setup-owned resource whose config is unchanged at the exact step
/// it failed in, with runtime credentials, as the retry always did. Recovering resources
/// stuck mid-update depends on resuming that step, not restarting the update.
#[tokio::test]
async fn running_retry_resumes_an_unchanged_setup_owned_failure_at_its_saved_step() {
    let _vault = test_vault_env().await;
    let store_id = "retry-frozen-store";
    let (mut state, config) = store_failed_mid_update(store_id).await;

    request_retry(&mut state);
    let retried = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("the retry step should succeed")
        .state;
    assert_eq!(retried.status, DeploymentStatus::Running);
    assert!(retried.error.is_none());
    let store = &retried.stack_state.as_ref().unwrap().resources[store_id];
    assert_eq!(store.status, alien_core::ResourceStatus::Updating);
    assert_eq!(
        store.internal_state.as_ref().unwrap()["state"],
        "updateConfig",
        "the retry resumes the saved step, not UpdateStart"
    );
    assert!(store.last_failed_state.is_none());
}

/// A running deployment only refreshes, so a failure the retry cannot resume would stay failed
/// without a word. The retry is refused instead, naming each resource and what it needs, and
/// nothing is resumed.
#[tokio::test]
async fn running_retry_refuses_failures_whose_config_changed_and_names_them() {
    let _vault = test_vault_env().await;
    let store_id = "retry-changed-store";
    let (mut state, config) = store_failed_mid_update(store_id).await;
    let resources = &mut state.stack_state.as_mut().unwrap().resources;
    resources.get_mut(store_id).unwrap().config =
        alien_core::Resource::new(Storage::new(store_id.to_string()).versioning(true).build());
    let worker = resources.get_mut("retry-fn").unwrap();
    let mut old = worker.config.downcast_ref::<Worker>().unwrap().clone();
    old.code = WorkerCode::Image {
        image: "test:older".to_string(),
    };
    worker.config = alien_core::Resource::new(old);
    let mut checkpoint = worker.internal_state.clone().unwrap();
    checkpoint["state"] = serde_json::json!("updateCodePolling");
    worker.status = alien_core::ResourceStatus::UpdateFailed;
    worker.internal_state = Some(checkpoint.clone());
    worker.last_failed_state = Some(checkpoint);
    let before = state.stack_state.clone();

    request_retry(&mut state);
    let refused = alien_deployment::step(state, config, ClientConfig::Test, None)
        .await
        .expect("the retry step should succeed")
        .state;
    assert_eq!(refused.status, DeploymentStatus::RefreshFailed);
    assert!(!refused.retry_requested);
    let error = refused.error.expect("the refusal is reported");
    assert_eq!(error.code, "RETRY_CANNOT_RESUME");
    assert!(
        error.message.contains(&format!("'{store_id}'")) && error.message.contains("rerun setup"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("'retry-fn'") && error.message.contains("deploy an update"),
        "{}",
        error.message
    );
    assert_eq!(
        serde_json::to_value(&refused.stack_state).unwrap(),
        serde_json::to_value(&before).unwrap(),
        "nothing is resumed"
    );
}

/// An update whose replace delete is denied fails with the denial named and nothing deleted
/// or created. Once the permission is granted, the user's retry completes the replace.
#[tokio::test]
async fn update_retry_completes_a_replace_once_its_delete_is_allowed() {
    let _vault = test_vault_env().await;
    let config = create_test_config("hash_v1", false);
    let worker_id = "denied-replace-fn";
    let identifier = "test:worker:denied-replace-fn";
    let mut state = run_to_completion(
        create_initial_state(create_test_stack("test-stack", "base-fn")),
        config.clone(),
    )
    .await;
    assert_eq!(state.status, DeploymentStatus::Running);

    // Release 2 adds a worker whose create fails after it recorded the worker.
    let mut stack_v2 = create_test_stack("test-stack", "base-fn");
    let mut failing = image_worker(worker_id, "test:v2", 1024);
    failing.environment.insert(
        "SIMULATE_CREATE_WORKER_FAILURE".to_string(),
        "true".to_string(),
    );
    add_live_worker(&mut stack_v2, failing);
    start_update(&mut state, release_of("rel_v2", stack_v2));
    let mut state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::UpdateFailed);

    // Release 3 fixes it, but this role may not delete the half-created worker.
    alien_infra::deny_test_worker_deletes(identifier);
    let mut stack_v3 = create_test_stack("test-stack", "base-fn");
    add_live_worker(&mut stack_v3, image_worker(worker_id, "test:v3", 1024));
    start_update(&mut state, release_of("rel_v3", stack_v3));
    let mut state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::UpdateFailed);
    let worker = &state.stack_state.as_ref().unwrap().resources[worker_id];
    assert_eq!(worker.status, alien_core::ResourceStatus::ProvisionFailed);
    assert_eq!(
        worker.error.as_ref().map(|error| error.code.as_str()),
        Some("REPLACE_DELETE_DENIED")
    );
    assert!(alien_infra::test_worker_deletes_issued(identifier).is_empty());
    assert_eq!(alien_infra::test_worker_deletes_denied(identifier), 1);

    assert_eq!(alien_infra::allow_test_worker_deletes(identifier), 1);
    request_retry(&mut state);
    let state = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("the retry step should succeed")
        .state;
    assert_eq!(state.status, DeploymentStatus::UpdatePending);
    let state = run_to_completion(state, config).await;
    assert_eq!(
        state.status,
        DeploymentStatus::Running,
        "{:?}",
        state.stack_state.as_ref().unwrap().resources[worker_id]
    );
    let deletes = alien_infra::test_worker_deletes_issued(identifier);
    assert_eq!(deletes.len(), 1, "the half-created worker is deleted once");
    assert_eq!(deployed_worker_image(&state, worker_id), "test:v3");
}

/// A setup rerun with a corrected release does not resume the old create checkpoint of a
/// setup-owned store whose create failed after recording its bucket. A store holds data, so
/// it is not deleted to be replaced either: setup creates it again in place with the new
/// config.
#[tokio::test]
async fn setup_retry_creates_a_failed_setup_owned_store_again_with_the_new_config() {
    let _vault = test_vault_env().await;
    let config = create_test_config("hash_v1", false);
    let store_id = "setup-replaced-store";
    let stack_with_store = |storage: Storage| {
        let mut stack = create_test_stack("test-stack", "base-fn");
        stack.resources.insert(
            store_id.to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        stack
    };

    let failing_store = Storage::new(store_id.to_string())
        .cors_allowed_origins(vec![
            alien_infra::SIMULATE_STORAGE_CREATE_FAILURE_ORIGIN.to_string()
        ])
        .build();
    let state = create_initial_state(stack_with_store(failing_store));
    let mut state = run_until_status(
        state,
        config.clone(),
        &[DeploymentStatus::InitialSetupFailed],
    )
    .await;
    let failed = &state.stack_state.as_ref().unwrap().resources[store_id];
    assert_eq!(failed.status, alien_core::ResourceStatus::ProvisionFailed);
    assert!(failed.internal_state.is_some() && failed.last_failed_state.is_some());
    assert!(alien_infra::test_storage_deletes_issued(store_id).is_empty());

    // Rerun setup with the corrected release: prepare it as the setup CLIs do, then retry.
    let fixed_store = Storage::new(store_id.to_string()).versioning(true).build();
    let stack_v2 = stack_with_store(fixed_store.clone());
    state.runtime_metadata = Some(
        alien_deployment::prepare_direct_setup_update(
            stack_v2.clone(),
            state.stack_state.as_ref().unwrap(),
            &config,
            &ClientConfig::Test,
            state.runtime_metadata.as_ref().unwrap(),
        )
        .await
        .expect("setup update should prepare"),
    );
    state.target_release = Some(release_of("rel_v2", stack_v2));
    request_retry(&mut state);
    let state = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("the retry step should succeed")
        .state;
    assert_eq!(state.status, DeploymentStatus::InitialSetup);
    let store = &state.stack_state.as_ref().unwrap().resources[store_id];
    assert_eq!(
        store.status,
        alien_core::ResourceStatus::ProvisionFailed,
        "the retry leaves the changed create to the setup executor"
    );
    let state = run_to_completion(state, config).await;
    assert_eq!(
        state.status,
        DeploymentStatus::Running,
        "{:?}",
        state.stack_state.as_ref().unwrap().resources[store_id]
    );

    assert!(
        alien_infra::test_storage_deletes_issued(store_id).is_empty(),
        "a data-holding store is never deleted to be replaced"
    );
    let store = &state.stack_state.as_ref().unwrap().resources[store_id];
    assert_eq!(store.status, alien_core::ResourceStatus::Running);
    assert_eq!(store.config, alien_core::Resource::new(fixed_store));
}

/// Dispatcher terminal sanity

#[tokio::test]
async fn test_deleted_is_noop() {
    let _vault = test_vault_env().await;

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack.clone());

    // Construct Deleted directly. The normal step loop intentionally parks at
    // TeardownRequired so a setup-authority caller can delete setup-owned
    // resources separately.
    state = run_to_completion(state, config.clone()).await;
    state.status = DeploymentStatus::Deleted;
    assert_eq!(state.status, DeploymentStatus::Deleted);

    // Set target_release for the step call (required even for Deleted state)
    state.target_release = Some(ReleaseInfo {
        release_id: Some("rel_v1".to_string()),
        version: Some("1.0.0".to_string()),
        description: None,
        stack,
    });

    // Call step on Deleted
    let result = alien_deployment::step(state.clone(), config, ClientConfig::Test, None)
        .await
        .expect("Step should succeed");

    // Assert state unchanged
    assert_eq!(result.state.status, DeploymentStatus::Deleted);
    assert!(!result.update_heartbeat, "should not heartbeat on Deleted");
}

/// F) Live gating flow tests

/// A deployer-provided boolean gate input with a declared default of true.
fn boolean_gate_input(
    id: &str,
    label: &str,
    description: &str,
) -> alien_core::StackInputDefinition {
    alien_core::StackInputDefinition::deployer_boolean(id, label, description, Some(true))
}

/// A stack whose live storage is gated on a deployer boolean with a declared
/// default of true.
fn create_live_gated_stack(stack_id: &str, store_id: &str, function_id: &str) -> Stack {
    let mut stack = create_test_stack(stack_id, function_id);
    stack.resources.insert(
        store_id.to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(Storage::new(store_id.to_string()).build()),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: Some("storeEnabled".to_string()),
        },
    );
    stack.inputs = vec![boolean_gate_input(
        "storeEnabled",
        "Enable the store",
        "Whether to run the store.",
    )];
    stack
}

fn config_with_store_enabled(enabled: bool) -> DeploymentConfig {
    let mut config = create_test_config("hash_v1", false);
    config.input_values = HashMap::from([("storeEnabled".to_string(), serde_json::json!(enabled))]);
    config
}

fn release_of(release_id: &str, stack: Stack) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some(release_id.to_string()),
        version: Some("1.0.0".to_string()),
        description: None,
        stack,
    }
}

/// The full live-gate cycle through the real state machine: default creates
/// the store, a false answer on an update deprovisions it AND lets the
/// deployment converge back to Running (the Deleted entry is pruned), and a
/// true answer on a later update recreates it.
#[tokio::test]
async fn live_gate_flip_deprovisions_and_reprovisions_across_updates() {
    let _vault = test_vault_env().await;

    let stack = create_live_gated_stack("gated-stack", "cache", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack.clone());

    // No answer given: the declared default (true) creates the store.
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert!(state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("cache"));

    // The deployer answers false on an update: the store is deprovisioned
    // and its Deleted entry leaves the state, so the stack computes Running.
    start_update(&mut state, release_of("rel_v2", stack.clone()));
    state = run_to_completion(state, config_with_store_enabled(false)).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert!(!state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("cache"));

    // Back to true on the next update: the store is recreated.
    start_update(&mut state, release_of("rel_v3", stack));
    state = run_to_completion(state, config_with_store_enabled(true)).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let store = state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .get("cache")
        .expect("an accepted gate must recreate the store");
    assert_eq!(store.status, alien_core::ResourceStatus::Running);
}

/// The same live-gate flip, but an ungated worker links the gated store — the shape the
/// link scrub allows. The consumer's recorded dependency makes the executor defer the
/// store's deletion for a step, so the update must stay open until the store is actually
/// gone rather than finishing on the step that scrubs the link.
#[tokio::test]
async fn declining_a_linked_gate_deprovisions_the_store_before_the_update_completes() {
    let _vault = test_vault_env().await;

    let stack = create_live_gated_stack_with_linking_worker("linked-gate-stack", "cache", "api");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack.clone());

    // The declared default is true, so both the store and its consumer come up.
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert!(state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("cache"));

    // Declining takes the store away and scrubs the link. The worker survives.
    start_update(&mut state, release_of("rel_v2", stack));
    state = run_to_completion(state, config_with_store_enabled(false)).await;

    assert_eq!(state.status, DeploymentStatus::Running);
    let resources = &state.stack_state.as_ref().unwrap().resources;
    assert!(
        !resources.contains_key("cache"),
        "the declined store must be deprovisioned, got {:?}",
        resources
            .iter()
            .map(|(id, r)| format!("{id}={:?}", r.status))
            .collect::<Vec<_>>()
    );
    let worker = resources
        .get("api")
        .expect("the ungated worker must outlive the store it linked");
    assert_eq!(worker.status, alien_core::ResourceStatus::Running);
    let links = alien_core::links_of(&worker.config);
    assert!(
        !links.iter().any(|link| link.id == "cache"),
        "the link must leave with the resource, got {links:?}"
    );
}

/// A live gated store with an ungated worker linking it.
fn create_live_gated_stack_with_linking_worker(
    stack_id: &str,
    store_id: &str,
    function_id: &str,
) -> Stack {
    let store = Storage::new(store_id.to_string()).build();
    let function = Worker::new(function_id.to_string())
        .code(WorkerCode::Image {
            image: "test:latest".to_string(),
        })
        .permissions("default".to_string())
        .link(&store)
        .build();

    let mut stack = create_test_stack(stack_id, function_id);
    stack.resources.insert(
        function_id.to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(function),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        },
    );
    stack.resources.insert(
        store_id.to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(store),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: Some("storeEnabled".to_string()),
        },
    );
    stack.inputs = vec![boolean_gate_input(
        "storeEnabled",
        "Enable the store",
        "Whether to run the store.",
    )];
    stack
}

/// A frozen gate is answered once: the answer recorded at creation refuses
/// every later conflicting input value.
#[tokio::test]
async fn a_frozen_gate_answer_is_fixed_for_the_deployment_lifetime() {
    let _vault = test_vault_env().await;

    // A frozen store gated with a declared default of true.
    let mut stack = create_test_stack("fixed-stack", "test-function");
    stack.resources.insert(
        "archive".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(Storage::new("archive".to_string()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: Some("archiveEnabled".to_string()),
        },
    );
    stack.inputs = vec![boolean_gate_input(
        "archiveEnabled",
        "Enable the archive",
        "Whether to create the archive store.",
    )];

    fn config_with_archive_enabled(enabled: bool) -> DeploymentConfig {
        let mut config = create_test_config("hash_v1", false);
        config.input_values =
            HashMap::from([("archiveEnabled".to_string(), serde_json::json!(enabled))]);
        config
    }

    let mut state = create_initial_state(stack.clone());
    state = run_to_completion(state, config_with_archive_enabled(true)).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert_eq!(
        state
            .runtime_metadata
            .as_ref()
            .and_then(|metadata| metadata.persisted_gate_answers.get("archiveEnabled")),
        Some(&true),
        "the answer is recorded at creation"
    );

    // A conflicting answer on an update is refused, not applied.
    start_update(&mut state, release_of("rel_v2", stack.clone()));
    let error = alien_deployment::step(
        state.clone(),
        config_with_archive_enabled(false),
        ClientConfig::Test,
        None,
    )
    .await
    .expect_err("a flipped frozen answer must refuse the update");
    assert_eq!(error.code, "FROZEN_GATE_ANSWER_CHANGED");

    // The same answer passes and the update completes.
    let mut state_matching = state.clone();
    state_matching = run_to_completion(state_matching, config_with_archive_enabled(true)).await;
    assert_eq!(state_matching.status, DeploymentStatus::Running);
}

/// The split-strip ordering guarantee on compute: declining a live worker on
/// an update removes the workload but keeps its derived baseline — the
/// profile-derived service account stays in the prepared stack, so the
/// frozen-compatibility check never fires and a later acceptance recreates
/// the worker.
#[tokio::test]
async fn a_declined_live_worker_keeps_its_derived_baseline_across_updates() {
    let _vault = test_vault_env().await;

    let mut stack = create_test_stack("gated-stack", "proxy");
    stack
        .resources
        .get_mut("proxy")
        .expect("worker entry")
        .enabled_when = Some("proxyEnabled".to_string());
    stack.inputs = vec![boolean_gate_input(
        "proxyEnabled",
        "Enable the proxy",
        "Whether to run the proxy worker.",
    )];

    fn config_with_proxy_enabled(enabled: bool) -> DeploymentConfig {
        let mut config = create_test_config("hash_v1", false);
        config.input_values =
            HashMap::from([("proxyEnabled".to_string(), serde_json::json!(enabled))]);
        config
    }

    fn prepared_stack_of(state: &DeploymentState) -> &Stack {
        state
            .runtime_metadata
            .as_ref()
            .and_then(|metadata| metadata.prepared_stack.as_ref())
            .expect("a completed deployment stores its prepared stack")
    }

    let mut state = create_initial_state(stack.clone());

    // Accepted (default true): the worker runs and the mutations derived its
    // service account into the prepared stack.
    state = run_to_completion(state, config_with_proxy_enabled(true)).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert!(state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("proxy"));
    assert!(
        prepared_stack_of(&state)
            .resources
            .contains_key("default-sa"),
        "the profile-derived service account belongs to the prepared stack: {:?}",
        prepared_stack_of(&state)
            .resources
            .keys()
            .collect::<Vec<_>>()
    );

    // Declined on an update: the worker is deprovisioned, the deployment
    // converges, and the derived service account is still in the prepared
    // stack — the mutations saw the declined worker, only the executor's
    // desired set lost it.
    start_update(&mut state, release_of("rel_v2", stack.clone()));
    state = run_to_completion(state, config_with_proxy_enabled(false)).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    assert!(!state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .contains_key("proxy"));
    assert!(
        prepared_stack_of(&state)
            .resources
            .contains_key("default-sa"),
        "declining the worker must not strip its derived baseline: {:?}",
        prepared_stack_of(&state)
            .resources
            .keys()
            .collect::<Vec<_>>()
    );
    assert!(
        !prepared_stack_of(&state).resources.contains_key("proxy"),
        "the declined worker itself leaves the prepared stack"
    );

    // Accepted again: the worker comes back without any compatibility
    // refusal, because nothing derived ever changed.
    start_update(&mut state, release_of("rel_v3", stack));
    state = run_to_completion(state, config_with_proxy_enabled(true)).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let proxy = state
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .get("proxy")
        .expect("an accepted gate must recreate the worker");
    assert_eq!(proxy.status, alien_core::ResourceStatus::Running);
}

/// A setup import that omitted a gated frozen resource: the runner must not
/// create the resource the deployer declined, while the delivered sibling
/// and the live function still deploy.
#[tokio::test]
async fn a_declined_gated_import_is_not_created_by_the_runner() {
    let _vault = test_vault_env().await;

    let mut stack = create_test_stack_with_storage("test-stack", "assets", "test-function");
    stack.resources.insert(
        "analytics".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(Storage::new("analytics".to_string()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: Some("analyticsEnabled".to_string()),
        },
    );
    stack.inputs = vec![boolean_gate_input(
        "analyticsEnabled",
        "Enable analytics",
        "Whether to create the analytics store.",
    )];

    // The import delivered only the ungated frozen storage; the gated one is
    // absent, which IS the deployer's answer.
    let mut imported_assets = alien_core::StackResourceState::new_pending(
        "storage".to_string(),
        alien_core::Resource::new(Storage::new("assets".to_string()).build()),
        Some(ResourceLifecycle::Frozen),
        Vec::new(),
    );
    imported_assets.status = alien_core::ResourceStatus::Running;
    // Importers always serialize the controller into the state. Running without one
    // means adopted from a binding, which this resource is not.
    imported_assets.internal_state = Some(serde_json::json!({
        "type": "TestStorageController",
        "_controllerStateVersion": 1,
        "state": "ready",
        "bucketName": "test-assets",
    }));
    let mut imported_state = StackState::new(Platform::Test);
    imported_state
        .resources
        .insert("assets".to_string(), imported_assets);

    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);
    state.stack_state = Some(imported_state);

    state = run_to_completion(state, config).await;
    assert_eq!(state.status, DeploymentStatus::Running);

    let resources = &state.stack_state.as_ref().unwrap().resources;
    assert!(
        !resources.contains_key("analytics"),
        "the runner must not create the gated resource the import omitted"
    );
    assert!(resources.contains_key("assets"));
    assert!(resources.contains_key("test-function"));
}

/// A release that introduces a gated frozen resource must fail the update
/// loudly, not drop the resource and report success.
///
/// Frozen resources are setup-owned: only a setup run creates them. The gate
/// has therefore never been answered, and the decline strip must leave the
/// resource alone so the frozen-compatibility check can say what the operator
/// needs to hear — rerun setup. Silently stripping it would converge a
/// deployment that is missing the release's new resource, with nothing said.
#[tokio::test]
async fn an_update_introducing_a_gated_frozen_resource_refuses_instead_of_dropping_it() {
    // The settled release has no gated resource at all, so nothing was ever
    // asked and nothing was recorded.
    let settled_stack = create_test_stack("gate-refusal-stack", "test-function");

    let mut target_stack = create_test_stack("gate-refusal-stack", "test-function");
    target_stack.inputs = vec![alien_core::StackInputDefinition::deployer_boolean(
        "archiveEnabled",
        "Enable the archive",
        "Whether to create the archive store.",
        Some(true),
    )];
    target_stack.resources.insert(
        "archive".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(Storage::new("archive".to_string()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: Some("archiveEnabled".to_string()),
        },
    );

    let state = DeploymentState {
        status: DeploymentStatus::UpdatePending,
        platform: Platform::Test,
        current_release: Some(ReleaseInfo {
            release_id: Some("rel_v1".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            stack: settled_stack.clone(),
        }),
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_v2".to_string()),
            version: Some("2.0.0".to_string()),
            description: None,
            stack: target_stack,
        }),
        stack_state: Some(StackState::new(Platform::Test)),
        error: None,
        environment_info: None,
        runtime_metadata: Some(RuntimeMetadata {
            prepared_stack: Some(settled_stack),
            ..Default::default()
        }),
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    let error = alien_deployment::step(
        state,
        create_test_config("hash_v1", false),
        ClientConfig::Test,
        None,
    )
    .await
    .expect_err("adding a frozen resource must refuse until setup runs again");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("archive"),
        "the refusal should name the resource that needs setup: {rendered}"
    );
}

fn database_password_input() -> alien_core::StackInputDefinition {
    alien_core::StackInputDefinition {
        id: "databasePassword".to_string(),
        kind: alien_core::StackInputKind::Secret,
        provided_by: vec![alien_core::StackInputProvider::Deployer],
        required: true,
        label: "Database password".to_string(),
        description: "Password of the customer's database".to_string(),
        placeholder: None,
        default: None,
        platforms: None,
        validation: None,
        generate: None,
        env: vec![alien_core::StackInputEnvironmentMapping {
            name: "DATABASE_PASSWORD".to_string(),
            target_resources: None,
            var_type: None,
        }],
    }
}

fn worker_environment(state: &DeploymentState, worker_id: &str) -> HashMap<String, String> {
    state.stack_state.as_ref().unwrap().resources[worker_id]
        .config
        .downcast_ref::<Worker>()
        .unwrap()
        .environment
        .clone()
}

fn deployer_reports(state: &DeploymentState) -> Vec<alien_core::DeployerSecretReport> {
    state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .deployer_secrets
        .clone()
}

#[tokio::test]
async fn initial_update_resumes_prepared_target_after_waiting_for_secrets() {
    let vault_env = test_vault_env().await;
    let config = create_test_config("target-env", false);
    let old_stack = create_test_stack_with_storage("test-stack", "archive", "old-worker");
    let mut state = run_until_status(
        create_initial_state(old_stack),
        config.clone(),
        &[DeploymentStatus::Provisioning],
    )
    .await;
    assert!(state.current_release.is_none());
    let frozen = state.stack_state.as_ref().unwrap().resources["archive"].clone();
    let baseline = state
        .runtime_metadata
        .as_ref()
        .unwrap()
        .prepared_stack
        .clone();
    let mut target = create_test_stack_with_storage("test-stack", "archive", "new-worker");
    target.inputs = vec![database_password_input()];
    let target_release = ReleaseInfo {
        release_id: Some("rel_target".into()),
        version: Some("2.0.0".into()),
        description: None,
        stack: target,
    };
    start_update(&mut state, target_release.clone());
    let blocked = run_until_status(
        state,
        config.clone(),
        &[
            DeploymentStatus::WaitingForSecrets,
            DeploymentStatus::Running,
        ],
    )
    .await;
    assert_eq!(blocked.status, DeploymentStatus::WaitingForSecrets);
    assert!(blocked.current_release.is_none());
    assert_eq!(
        blocked.runtime_metadata.as_ref().unwrap().prepared_stack,
        baseline
    );
    assert!(blocked
        .runtime_metadata
        .as_ref()
        .unwrap()
        .pending_prepared_stack
        .as_ref()
        .unwrap()
        .resources
        .contains_key("new-worker"));
    assert_eq!(
        deployer_reports(&blocked)[0].status,
        alien_core::DeployerSecretStatus::Missing
    );

    vault_env
        .customer_vault()
        .set_secret("input-database-password", "demo-secret")
        .await
        .unwrap();
    let checkpoint = serde_json::from_value(serde_json::to_value(&blocked).unwrap()).unwrap();
    let running = run_to_completion(checkpoint, config).await;
    assert_eq!(running.status, DeploymentStatus::Running);
    assert_eq!(running.current_release, Some(target_release));
    assert!(running.target_release.is_none());
    let metadata = running.runtime_metadata.as_ref().unwrap();
    assert!(metadata.pending_prepared_stack.is_none());
    assert!(metadata
        .prepared_stack
        .as_ref()
        .unwrap()
        .resources
        .contains_key("new-worker"));
    let resources = &running.stack_state.as_ref().unwrap().resources;
    assert!(!resources.contains_key("old-worker"));
    assert_eq!(
        resources["new-worker"].status,
        alien_core::ResourceStatus::Running
    );
    assert_eq!(
        serde_json::to_value(&resources["archive"]).unwrap(),
        serde_json::to_value(frozen).unwrap()
    );
    assert_eq!(
        deployer_reports(&running)[0].status,
        alien_core::DeployerSecretStatus::Present
    );
    assert!(!serde_json::to_string(&running)
        .unwrap()
        .contains("demo-secret"));
}

#[tokio::test]
async fn test_missing_deployer_secret_blocks_workloads_until_written() {
    use alien_bindings::Vault as _;

    let vault_env = test_vault_env().await;
    let mut stack = create_test_stack("test-stack", "test-function");
    stack.inputs = vec![database_password_input()];
    let config = create_test_config("hash_v1", false);
    let state = create_initial_state(stack);

    let blocked = run_until_status(
        state,
        config.clone(),
        &[
            DeploymentStatus::WaitingForSecrets,
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;

    assert_eq!(blocked.status, DeploymentStatus::WaitingForSecrets);
    let error = blocked
        .error
        .as_ref()
        .expect("the wait names what is missing");
    assert!(
        error.message.contains("missing: Database password"),
        "{}",
        error.message
    );
    let reports = deployer_reports(&blocked);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].status, alien_core::DeployerSecretStatus::Missing);
    assert_eq!(reports[0].location.name, "input-database-password");
    assert!(reports[0].location.cli_command.contains("'<VALUE>'"));
    let worker = blocked
        .stack_state
        .as_ref()
        .unwrap()
        .resources
        .get("test-function");
    assert!(
        worker.is_none_or(|worker| worker.status != alien_core::ResourceStatus::Running),
        "no workload starts while a required deployer secret is missing"
    );

    vault_env
        .customer_vault()
        .set_secret("input-database-password", "customer-only-value")
        .await
        .unwrap();

    let running = run_until_status(blocked, config, &[DeploymentStatus::Running]).await;

    assert_eq!(running.status, DeploymentStatus::Running);
    assert_eq!(
        deployer_reports(&running)[0].status,
        alien_core::DeployerSecretStatus::Present
    );
    let alien_secrets: serde_json::Value = serde_json::from_str(
        &worker_environment(&running, "test-function")[alien_core::ENV_ALIEN_SECRETS],
    )
    .unwrap();
    assert_eq!(
        alien_secrets["deployerSecrets"],
        serde_json::json!([{
            "name": "DATABASE_PASSWORD",
            "vaultKey": "input-database-password",
            "secretName": "input-database-password",
            "label": "Database password",
            "required": true,
            "version": deployer_reports(&running)[0].version,
        }])
    );
    assert!(deployer_reports(&running)[0].version.is_some());
    assert!(
        !serde_json::to_string(&running)
            .unwrap()
            .contains("customer-only-value"),
        "the deployment state never carries the deployer's secret"
    );
}

/// Runs an update to Running and returns the final state together with
/// whether the worker went through an update on the way.
async fn run_update_and_watch_worker(
    mut state: DeploymentState,
    config: DeploymentConfig,
    worker_id: &str,
) -> (DeploymentState, bool) {
    let mut worker_updated = false;
    for _ in 0..MAX_STEPS {
        if state.status == DeploymentStatus::Running {
            return (state, worker_updated);
        }
        state = alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
            .await
            .expect("Step should not fail")
            .state;
        worker_updated |= state
            .stack_state
            .as_ref()
            .and_then(|stack_state| stack_state.resources.get(worker_id))
            .is_some_and(|worker| worker.status == alien_core::ResourceStatus::Updating);
    }
    panic!(
        "Update did not reach Running after {MAX_STEPS} steps. Final status: {:?}",
        state.status
    );
}

fn deployer_secret_version(state: &DeploymentState, worker_id: &str) -> serde_json::Value {
    let alien_secrets: serde_json::Value =
        serde_json::from_str(&worker_environment(state, worker_id)[alien_core::ENV_ALIEN_SECRETS])
            .unwrap();
    alien_secrets["deployerSecrets"][0]["version"].clone()
}

#[tokio::test]
async fn test_redeploy_after_deployer_secret_overwrite_restarts_workloads() {
    use alien_bindings::Vault as _;

    let vault_env = test_vault_env().await;
    let mut stack = create_test_stack("test-stack", "test-function");
    stack.inputs = vec![database_password_input()];
    let config = create_test_config("hash_v1", false);
    vault_env
        .customer_vault()
        .set_secret("input-database-password", "customer-value-v1")
        .await
        .unwrap();

    let running = run_until_status(
        create_initial_state(stack),
        config.clone(),
        &[DeploymentStatus::Running],
    )
    .await;
    let v1 = deployer_secret_version(&running, "test-function");
    assert!(
        v1.is_string(),
        "the worker records the version it started with"
    );
    assert_eq!(
        deployer_reports(&running)[0].version.as_ref(),
        v1.as_str().map(str::to_string).as_ref()
    );

    // The deployer overwrites the secret with their cloud's CLI, then
    // redeploys the same release with the same config.
    vault_env
        .customer_vault()
        .set_secret("input-database-password", "customer-value-v2")
        .await
        .unwrap();
    let mut redeploy = running;
    let release = redeploy.current_release.clone().unwrap();
    start_update(&mut redeploy, release.clone());
    let (rotated, worker_updated) =
        run_update_and_watch_worker(redeploy, config.clone(), "test-function").await;

    let v2 = deployer_secret_version(&rotated, "test-function");
    assert!(v2.is_string());
    assert_ne!(v1, v2, "the overwrite reaches the worker's config");
    assert!(
        worker_updated,
        "the worker is updated so it restarts and reads the new value"
    );
    assert!(
        !serde_json::to_string(&rotated)
            .unwrap()
            .contains("customer-value-v2"),
        "the deployment state never carries the deployer's secret"
    );

    // Redeploying again without a new write changes nothing.
    let mut again = rotated.clone();
    start_update(&mut again, release);
    let (unchanged, worker_updated) =
        run_update_and_watch_worker(again, config, "test-function").await;

    assert_eq!(
        worker_environment(&unchanged, "test-function"),
        worker_environment(&rotated, "test-function")
    );
    assert!(!worker_updated, "no new version, no restart");
}

#[tokio::test]
async fn test_stored_dual_secret_presence_starts_without_vault_slots() {
    let mut stack = create_test_stack("demo-stack", "demo-worker");
    let mut input = database_password_input();
    input
        .provided_by
        .push(alien_core::StackInputProvider::Developer);
    stack.inputs = vec![input];
    let mut config = create_test_config("hash_v1", false);
    config.stored_secret_input_ids = Some(vec!["databasePassword".to_string()]);
    assert!(config.input_values.is_empty());
    let running = run_until_status(
        create_initial_state(stack),
        config,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::WaitingForSecrets,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;
    assert_eq!(running.status, DeploymentStatus::Running);
    assert!(deployer_reports(&running).is_empty());
    let environment = worker_environment(&running, "demo-worker");
    assert!(!environment.contains_key("DATABASE_PASSWORD"));
    assert!(!environment.contains_key(alien_core::ENV_ALIEN_DEPLOYER_SECRETS));
    assert!(!environment.contains_key(alien_core::ENV_ALIEN_SECRETS));
}

#[tokio::test]
async fn test_stored_deployer_secret_keeps_working_until_the_slot_is_filled() {
    use alien_bindings::Vault as _;

    let vault_env = test_vault_env().await;
    let mut stack = create_test_stack("test-stack", "test-function");
    stack.inputs = vec![database_password_input()];
    let mut config = create_test_config("hash_v1", false);
    config.input_values = HashMap::from([(
        "databasePassword".to_string(),
        serde_json::json!("stored-before-vault-native"),
    )]);
    let state = create_initial_state(stack);

    let running = run_until_status(
        state,
        config.clone(),
        &[
            DeploymentStatus::WaitingForSecrets,
            DeploymentStatus::Running,
        ],
    )
    .await;

    assert_eq!(running.status, DeploymentStatus::Running);
    assert_eq!(
        deployer_reports(&running)[0].status,
        alien_core::DeployerSecretStatus::Missing
    );
    assert!(
        !worker_environment(&running, "test-function").contains_key(alien_core::ENV_ALIEN_SECRETS),
        "the stored value keeps today's path until the slot is filled"
    );

    vault_env
        .customer_vault()
        .set_secret("input-database-password", "customer-only-value")
        .await
        .unwrap();
    let result = alien_deployment::step(running, config, ClientConfig::Test, None)
        .await
        .unwrap();

    assert_eq!(result.state.status, DeploymentStatus::Running);
    assert_eq!(
        deployer_reports(&result.state)[0].status,
        alien_core::DeployerSecretStatus::Present,
        "a running deployment reports the slot once it is filled"
    );
}
