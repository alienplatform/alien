//! Controllers for the resources of a deployment between two releases that list different
//! extensions.

mod support;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, InitialSetupAuthority, Platform, ReleaseInfo, RuntimeMetadata,
    Stack, StackSettings, StackState,
};
use alien_deployment::runner::{DelayStrategy, RunnerPolicy};
use alien_deployment::transport::{DeploymentLoopTransport, StepReconcileResult};
use alien_error::AlienError;
use alien_infra::step_protocol::protocol::{StepRequest, StepResponse};
use alien_infra::step_protocol::test_controller::{ControllerHost, MockCloud, DNS_ZONE};
use alien_infra::step_protocol::{ProxyFactory, Transport, TransportFailure};
use alien_infra::{DefaultPlatformServiceProvider, LoadedExtensions, PlatformServiceProvider};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

struct InProcess {
    host: ControllerHost,
}

impl std::fmt::Debug for InProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InProcess")
    }
}

#[async_trait]
impl Transport for InProcess {
    async fn step(
        &self,
        request: &StepRequest,
        _timeout: Duration,
    ) -> Result<StepResponse, TransportFailure> {
        let wire = serde_json::to_value(request).unwrap();
        let response = self
            .host
            .handle(serde_json::from_value(wire).unwrap())
            .await;
        Ok(serde_json::from_value(serde_json::to_value(response).unwrap()).unwrap())
    }
}

struct LocalTransport;

#[async_trait]
impl DeploymentLoopTransport for LocalTransport {
    async fn reconcile_step(
        &self,
        _deployment_id: &str,
        _state: &DeploymentState,
        _config: &DeploymentConfig,
        _update_heartbeat: bool,
        _suggested_delay_ms: Option<u64>,
        _heartbeats: Vec<alien_core::ResourceHeartbeat>,
        _observed_inventory_batches: Vec<alien_core::ObservedInventoryBatch>,
    ) -> Result<StepReconcileResult, AlienError> {
        Ok(StepReconcileResult {
            state: None,
            config: None,
        })
    }
}

fn stack(lifecycle: &str) -> Stack {
    serde_json::from_value(json!({
        "id": "acme",
        "resources": {
            "zone": {
                "config": { "type": DNS_ZONE, "id": "zone", "domain": "internal.acme.dev" },
                "lifecycle": lifecycle,
                "dependencies": []
            }
        },
        "permissions": { "profiles": {}, "management": "auto" },
        "extensions": [{
            "name": "acme",
            "version": "1.0.0",
            "digest": "sha256:0",
            "resourceTypes": {
                DNS_ZONE: {
                    "lifecycles": ["live", "frozen"],
                    "holdsData": false,
                    "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
                    "controllerPlatforms": ["test"],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": {}, "outputs": {} }
                }
            }
        }]
    }))
    .unwrap()
}

fn empty_stack() -> Stack {
    serde_json::from_value(json!({
        "id": "acme",
        "resources": {},
        "permissions": { "profiles": {}, "management": "auto" }
    }))
    .unwrap()
}

fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: "hash".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build()
}

fn initial_setup(stack: Stack) -> DeploymentState {
    DeploymentState {
        status: DeploymentStatus::InitialSetup,
        platform: Platform::Test,
        current_release: None,
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_1".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            stack: stack.clone(),
        }),
        stack_state: Some(StackState::new(Platform::Test)),
        error: None,
        environment_info: None,
        runtime_metadata: Some(RuntimeMetadata {
            initial_setup_authority: InitialSetupAuthority::DirectSetup,
            prepared_stack: Some(stack),
            pending_prepared_release_id: Some("rel_1".to_string()),
            ..RuntimeMetadata::default()
        }),
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

async fn run_until(
    mut state: DeploymentState,
    provider: &Arc<dyn PlatformServiceProvider>,
    statuses: &[DeploymentStatus],
) -> DeploymentState {
    for _ in 0..60 {
        if statuses.contains(&state.status) {
            return state;
        }
        state = match crate::support::step(
            state.clone(),
            config(),
            ClientConfig::Test,
            Some(provider.clone()),
        )
        .await
        {
            Ok(result) => result.state,
            Err(error) => panic!("step failed at {:?}: {error:#?}", state.status),
        };
    }
    state
}

fn provider(cloud: &std::path::Path) -> Arc<dyn PlatformServiceProvider> {
    let transport = Arc::new(InProcess {
        host: ControllerHost::new(cloud, None),
    });
    let extensions = Arc::new(
        LoadedExtensions::default()
            .with_hooks("acme", alien_deployment::testing::no_hooks("acme"))
            .with_controller("acme", ProxyFactory::new("dep", transport, None)),
    );
    // The process loaded acme for whichever releases list it: as the target release's
    // extension, and as the current release's for an update or deletion away from it.
    let acme = stack("live");
    Arc::new(
        DefaultPlatformServiceProvider::default()
            .with_extensions(&acme, extensions.clone())
            .unwrap()
            .with_current_release(&acme, extensions)
            .unwrap(),
    )
}

/// Deploys `stack` and returns the running deployment.
async fn deployed(stack: Stack, provider: &Arc<dyn PlatformServiceProvider>) -> DeploymentState {
    let state = run_until(
        initial_setup(stack),
        provider,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
            DeploymentStatus::InitialSetupFailed,
        ],
    )
    .await;
    assert_eq!(
        state.status,
        DeploymentStatus::Running,
        "{:#?}",
        state.error
    );
    state
}

/// Updates a running deployment to `stack` and returns it once the update settles.
async fn updated(
    mut state: DeploymentState,
    stack: Stack,
    provider: &Arc<dyn PlatformServiceProvider>,
) -> DeploymentState {
    state.target_release = Some(ReleaseInfo {
        release_id: Some("rel_2".to_string()),
        version: Some("2.0.0".to_string()),
        description: None,
        stack,
    });
    state.status = DeploymentStatus::UpdatePending;
    run_until(
        state,
        provider,
        &[DeploymentStatus::Running, DeploymentStatus::UpdateFailed],
    )
    .await
}

/// An update from a release without the extension to one with it failed after creating the
/// zone: the current release has no extension, the target has it. Deleting must still reach the
/// zone's controller.
#[tokio::test(flavor = "multi_thread")]
async fn delete_after_a_failed_update_that_added_the_extension_reaches_its_controller() {
    let vault = tempfile::TempDir::new().unwrap();
    std::env::set_var("TEST_VAULT_DATA_DIR", vault.path());
    let cloud = tempfile::TempDir::new().unwrap();
    let provider = provider(cloud.path());

    let mut state = deployed(stack("live"), &provider).await;
    let added = state.current_release.clone().unwrap();
    let mut before = added.clone();
    before.release_id = Some("rel_0".to_string());
    before.stack = empty_stack();
    state.current_release = Some(before);
    state.target_release = Some(added);
    state.status = DeploymentStatus::DeletePending;
    let state = run_until(
        state,
        &provider,
        &[
            DeploymentStatus::Deleted,
            DeploymentStatus::DeleteFailed,
            DeploymentStatus::TeardownRequired,
        ],
    )
    .await;
    assert_eq!(
        state.status,
        DeploymentStatus::Deleted,
        "{:#?}",
        state.error
    );
    let zones = MockCloud::at(cloud.path()).state().zones;
    assert_eq!(zones.len(), 1);
    assert!(zones.values().all(|zone| zone.deleted), "{zones:?}");
}

/// The same failed update, then an update to a release without the extension: neither release
/// lists it, so no controller can delete the zone. The update fails, naming the type, rather than
/// dropping the resource without deleting it.
#[tokio::test(flavor = "multi_thread")]
async fn an_update_past_a_failed_one_that_added_the_extension_fails_loudly() {
    let vault = tempfile::TempDir::new().unwrap();
    std::env::set_var("TEST_VAULT_DATA_DIR", vault.path());
    let cloud = tempfile::TempDir::new().unwrap();
    let provider = provider(cloud.path());

    let mut state = deployed(stack("live"), &provider).await;
    let added = state.current_release.clone().unwrap();
    let mut before = added.clone();
    before.release_id = Some("rel_0".to_string());
    before.stack = empty_stack();
    state.current_release = Some(before);
    state.target_release = None;
    let state = updated(state, empty_stack(), &provider).await;
    assert_eq!(state.status, DeploymentStatus::UpdateFailed);
    let zone = &state.stack_state.as_ref().unwrap().resources["zone"];
    let error = zone.error.as_ref().expect("the zone records why");
    assert_eq!(error.code, "CONTROLLER_NOT_AVAILABLE", "{}", error.message);
    assert!(error.message.contains(DNS_ZONE), "{}", error.message);
    let zones = MockCloud::at(cloud.path()).state().zones;
    assert!(zones.values().all(|zone| !zone.deleted), "{zones:?}");
}
