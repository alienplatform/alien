//! One extension at two digests, v1 in the current release and v2 in the target release,
//! through `step`.

mod support;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, InitialSetupAuthority, Platform, ReleaseInfo, RuntimeMetadata,
    Stack, StackSettings, StackState,
};
use alien_infra::step_protocol::protocol::{StepRequest, StepResponse};
use alien_infra::step_protocol::test_controller::{ControllerHost, MockCloud, DNS_ZONE};
use alien_infra::step_protocol::{ProxyFactory, Transport, TransportFailure};
use alien_infra::{DefaultPlatformServiceProvider, LoadedExtensions, PlatformServiceProvider};
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct InProcess {
    host: ControllerHost,
    steps: AtomicUsize,
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
        self.steps.fetch_add(1, Ordering::SeqCst);
        let wire = serde_json::to_value(request).unwrap();
        let response = self
            .host
            .handle(serde_json::from_value(wire).unwrap())
            .await;
        Ok(serde_json::from_value(serde_json::to_value(response).unwrap()).unwrap())
    }
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

fn release_stack(digest: &str, with_zone: bool) -> Stack {
    let types = if with_zone {
        json!({
            DNS_ZONE: {
                "lifecycles": ["live", "frozen"],
                "holdsData": false,
                "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
                "controllerPlatforms": ["test"],
                "permissionSets": [],
                "link": { "permissionSets": [] },
                "schemas": { "config": {}, "outputs": {} }
            }
        })
    } else {
        json!({})
    };
    let resources = if with_zone {
        json!({
            "zone": {
                "config": { "type": DNS_ZONE, "id": "zone", "domain": "internal.acme.dev" },
                "lifecycle": "live",
                "dependencies": []
            }
        })
    } else {
        json!({})
    };
    serde_json::from_value(json!({
        "id": "acme",
        "resources": resources,
        "permissions": { "profiles": {}, "management": "auto" },
        "extensions": [{
            "name": "acme",
            "version": "1.0.0",
            "digest": digest,
            "resourceTypes": types
        }]
    }))
    .unwrap()
}

fn transport(cloud: &std::path::Path) -> Arc<InProcess> {
    Arc::new(InProcess {
        host: ControllerHost::new(cloud, None),
        steps: AtomicUsize::new(0),
    })
}

fn loaded(transport: &Arc<InProcess>) -> Arc<LoadedExtensions> {
    Arc::new(
        LoadedExtensions::default()
            .with_hooks("acme", alien_deployment::testing::no_hooks("acme"))
            .with_controller("acme", ProxyFactory::new("dep", transport.clone(), None)),
    )
}

/// Deploys v1 with only v1 loaded, then returns the provider for a move to v2: v2 as the target
/// release, v1 as the current one, each with its own controller endpoint.
async fn deployed_v1(
    cloud: &std::path::Path,
    v2_has_zone: bool,
) -> (
    DeploymentState,
    Arc<dyn PlatformServiceProvider>,
    Arc<InProcess>,
    Arc<InProcess>,
) {
    let (t1, t2) = (transport(cloud), transport(cloud));
    let v1 = release_stack("sha256:v1", true);
    let v2 = release_stack("sha256:v2", v2_has_zone);
    let only_v1: Arc<dyn PlatformServiceProvider> = Arc::new(
        DefaultPlatformServiceProvider::default()
            .with_extensions(&v1, loaded(&t1))
            .unwrap(),
    );
    let state = run_until(
        initial_setup(v1.clone()),
        &only_v1,
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
    let moving: Arc<dyn PlatformServiceProvider> = Arc::new(
        DefaultPlatformServiceProvider::default()
            .with_extensions(&v2, loaded(&t2))
            .unwrap()
            .with_current_release(&v1, loaded(&t1))
            .unwrap(),
    );
    t1.steps.store(0, Ordering::SeqCst);
    (state, moving, t1, t2)
}

/// v2 no longer defines the zone's type: the update deletes the zone through v1's controller.
#[tokio::test(flavor = "multi_thread")]
async fn a_type_only_the_current_digest_defines_is_deleted_by_its_controller() {
    let vault = tempfile::TempDir::new().unwrap();
    std::env::set_var("TEST_VAULT_DATA_DIR", vault.path());
    let cloud = tempfile::TempDir::new().unwrap();
    let (mut state, provider, t1, t2) = deployed_v1(cloud.path(), false).await;

    state.target_release = Some(ReleaseInfo {
        release_id: Some("rel_2".to_string()),
        version: Some("2.0.0".to_string()),
        description: None,
        stack: release_stack("sha256:v2", false),
    });
    state.status = DeploymentStatus::UpdatePending;
    let state = run_until(
        state,
        &provider,
        &[DeploymentStatus::Running, DeploymentStatus::UpdateFailed],
    )
    .await;
    assert_eq!(
        state.status,
        DeploymentStatus::Running,
        "{:#?}",
        state.error
    );
    let zones = MockCloud::at(cloud.path()).state().zones;
    assert!(zones.values().all(|zone| zone.deleted), "{zones:?}");
    assert!(
        t1.steps.load(Ordering::SeqCst) > 0,
        "v1's controller deletes the zone"
    );
    assert_eq!(
        t2.steps.load(Ordering::SeqCst),
        0,
        "v2's controller is never asked"
    );
}

/// Both digests define the zone's type: deleting during the unfinished update steps the zone
/// with the target digest's controller.
#[tokio::test(flavor = "multi_thread")]
async fn a_type_both_digests_define_is_deleted_by_the_target_digests_controller() {
    let vault = tempfile::TempDir::new().unwrap();
    std::env::set_var("TEST_VAULT_DATA_DIR", vault.path());
    let cloud = tempfile::TempDir::new().unwrap();
    let (mut state, provider, t1, t2) = deployed_v1(cloud.path(), true).await;

    state.target_release = Some(ReleaseInfo {
        release_id: Some("rel_2".to_string()),
        version: Some("2.0.0".to_string()),
        description: None,
        stack: release_stack("sha256:v2", true),
    });
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
    assert!(zones.values().all(|zone| zone.deleted), "{zones:?}");
    assert!(
        t2.steps.load(Ordering::SeqCst) > 0,
        "v2's controller deletes the zone"
    );
    assert_eq!(
        t1.steps.load(Ordering::SeqCst),
        0,
        "v1's controller is never asked"
    );
}
