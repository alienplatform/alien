//! Setup teardown of a deployment whose release lists an extension, given a provider that
//! loaded none, is refused before it deletes anything, as the step, the runner and the
//! executor are.

use alien_aws_clients::AwsClientConfigExt as _;
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, Platform, ReleaseInfo, StackSettings, StackState,
};
use alien_deployment::loop_contract::LoopOperation;
use alien_deployment::runner::{DelayStrategy, RunnerPolicy};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::AlienError;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordingTransport {
    statuses: Mutex<Vec<DeploymentStatus>>,
}

#[async_trait::async_trait]
impl DeploymentLoopTransport for RecordingTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        state: &DeploymentState,
        _config: &DeploymentConfig,
        _update_heartbeat: bool,
        _suggested_delay_ms: Option<u64>,
        _heartbeats: Vec<alien_core::ResourceHeartbeat>,
        _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
    ) -> std::result::Result<StepReconcileResult, AlienError> {
        self.statuses.lock().unwrap().push(state.status);
        Ok(StepReconcileResult {
            state: None,
            config: None,
        })
    }
}

#[tokio::test]
async fn setup_teardown_refuses_a_provider_that_did_not_load_the_release_extensions() {
    let stack: alien_core::Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {},
        "extensions": [{
            "name": "acme", "version": "1.0.0", "digest": "sha256:acme", "resourceTypes": {}
        }]
    }))
    .unwrap();
    let release = ReleaseInfo {
        release_id: Some("rel_1".to_string()),
        version: None,
        description: None,
        stack,
    };
    let mut state = DeploymentState {
        status: DeploymentStatus::TeardownRequired,
        platform: Platform::Aws,
        current_release: Some(release.clone()),
        target_release: Some(release),
        stack_state: Some(StackState::with_resource_prefix(
            Platform::Aws,
            "test".to_string(),
        )),
        error: None,
        environment_info: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
        runtime_metadata: None,
    };
    let mut config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let transport = RecordingTransport::default();

    let result = alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
        &mut state,
        &mut config,
        &ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock())),
        "dep_1",
        &RunnerPolicy {
            operation: LoopOperation::Delete,
            delay_strategy: DelayStrategy::Inline,
            ..Default::default()
        },
        &transport,
        Some(Arc::new(
            alien_infra::DefaultPlatformServiceProvider::default(),
        )),
    )
    .await;

    let statuses = transport.statuses.into_inner().unwrap();
    let error = result
        .err()
        .expect("setup teardown with a provider that loaded none of the release's extensions");
    assert!(error.to_string().contains("acme"), "{error}");
    assert!(
        !statuses.contains(&DeploymentStatus::Deleted) && state.status != DeploymentStatus::Deleted,
        "checkpoints {statuses:?}, final status {:?}",
        state.status
    );
}
