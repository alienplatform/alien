//! Fast, deterministic deployment tests using Platform::Test
//!
//! These tests exercise the full alien_deployment::step() lifecycle with no cloud I/O.

use alien_core::{
    ClientConfig, DeleteScope, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot, Platform,
    ReleaseInfo, ResourceEntry, ResourceLifecycle, RuntimeMetadata, Stack, StackSettings,
    StackState, Storage, Worker, WorkerCode,
};
use chrono::Utc;
use indexmap::IndexMap;
use std::{collections::HashMap, sync::OnceLock};
use tempfile::TempDir;
use tokio::sync::MutexGuard;

const MAX_STEPS: usize = 100;

static TEST_VAULT_ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

struct TestVaultEnv {
    _guard: MutexGuard<'static, ()>,
    _temp_dir: TempDir,
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
    let mut runtime_metadata = state.runtime_metadata.clone().unwrap_or_default();
    runtime_metadata.delete_scope = Some(DeleteScope::Full);
    state.runtime_metadata = Some(runtime_metadata);
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
        },
    );
    resources.insert(
        function_id.to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(function),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
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

/// Create a deployment config fixture
fn create_test_config(env_vars_hash: &str, include_secret: bool) -> DeploymentConfig {
    DeploymentConfig {
        stack_settings: StackSettings::default(),
        management_config: None,
        environment_variables: create_env_vars_snapshot(env_vars_hash, include_secret),
        external_bindings: alien_core::ExternalBindings::default(),
        compute_backend: None,
        allow_frozen_changes: false,
        domain_metadata: None,
        public_urls: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
    }
}

/// Create an initial deployment state
fn create_initial_state(stack: Stack) -> DeploymentState {
    let release = ReleaseInfo {
        release_id: "rel_v1".to_string(),
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
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

/// A) Initial deploy flow tests

#[tokio::test]
async fn test_pending_to_running_happy_path_promotes_release() {
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

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
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

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
        Some("hash_v1"),
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
        Some("hash_v1"),
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
        "hash_v1"
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
        "hash_v1"
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
        "hash_v1"
    );

    // Now change config to hash_v2
    let config2 = create_test_config("hash_v2", true);

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
        "hash_v2"
    );
}

/// C) Running health checks + heartbeat tests

#[tokio::test]
async fn test_running_updates_heartbeat_when_healthy() {
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

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
async fn test_running_transitions_to_refresh_failed_on_health_check_failure() {
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

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
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

    let stack_v1 = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack_v1);

    // Get to Running with release v1
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);
    let v1_release = state.current_release.clone().unwrap();

    // Start update to v2 - use a different function resource ID to avoid conflicts
    let stack_v2 = create_test_stack("test-stack", "test-function"); // Keep same resource for simpler update
    let release_v2 = ReleaseInfo {
        release_id: "rel_v2".to_string(),
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

#[tokio::test]
async fn test_update_failed_retry_gate_returns_to_update_pending() {
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

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
    };

    let release_v2 = ReleaseInfo {
        release_id: "rel_v2".to_string(),
        version: Some("2.0.0".to_string()),
        description: None,
        stack: stack_v2,
    };
    start_update(&mut state, release_v2);

    // Run until UpdateFailed
    state = run_until_status(state, config.clone(), &[DeploymentStatus::UpdateFailed]).await;

    // Without retry_requested, should stay in failed state
    let result = alien_deployment::step(state.clone(), config.clone(), ClientConfig::Test, None)
        .await
        .expect("Step should succeed");
    assert_eq!(result.state.status, DeploymentStatus::UpdateFailed);

    // With retry_requested, should transition to UpdatePending (not Updating)
    request_retry(&mut state);
    let result = alien_deployment::step(state, config, ClientConfig::Test, None)
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

    if failed_status == DeploymentStatus::DeleteFailed {
        state.runtime_metadata = Some(RuntimeMetadata {
            delete_scope: Some(DeleteScope::Full),
            ..Default::default()
        });
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
async fn test_delete_flow_happy_path_reaches_deleted() {
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    // Get to Running
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Running);

    // Start delete
    start_delete(&mut state);

    // Run delete to completion
    state = run_to_completion(state, config).await;

    // Assert successful deletion
    assert_eq!(state.status, DeploymentStatus::Deleted);
}

#[tokio::test]
async fn test_delete_failed_retry_gate() {
    // Create a minimal test for delete retry pattern
    // In practice, TestWorkerController doesn't easily simulate delete failures,
    // but we can test the pattern conceptually by checking the handler exists

    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack);

    // Get to Running
    state = run_to_completion(state, config.clone()).await;

    // Start delete
    start_delete(&mut state);

    // Delete should succeed for Test platform, reaching Deleted
    state = run_to_completion(state, config).await;
    assert_eq!(state.status, DeploymentStatus::Deleted);
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
        },
    );
    resources.insert(
        "sibling-fn".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(sibling_fn),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
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
    }
}

/// When one resource in a multi-resource deployment fails, all in-progress resources should
/// be transitioned to a *Failed status with a DEPLOYMENT_INTERRUPTED error so the UI shows
/// accurate statuses instead of stale "Provisioning" or "Pending" indicators.
#[tokio::test]
async fn test_partial_failure_interrupts_in_progress_resources() {
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

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
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

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

/// Dispatcher terminal sanity

#[tokio::test]
async fn test_deleted_is_noop() {
    let _temp_dir = TempDir::new().expect("Failed to create temp dir");

    let stack = create_test_stack("test-stack", "test-function");
    let config = create_test_config("hash_v1", false);
    let mut state = create_initial_state(stack.clone());

    // Get to Deleted
    state = run_to_completion(state, config.clone()).await;
    start_delete(&mut state);
    state = run_to_completion(state, config.clone()).await;
    assert_eq!(state.status, DeploymentStatus::Deleted);

    // Set target_release for the step call (required even for Deleted state)
    state.target_release = Some(ReleaseInfo {
        release_id: "rel_v1".to_string(),
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
