//! Custom resources deployed and deleted through `alien_deployment::step`, their controllers
//! reached over the step protocol.
//!
//! Each deployment's service provider carries what the process loaded for its extensions, and
//! `step` builds that deployment's registries from it. Deployments stepped at the same time
//! reach only their own controllers.

mod support;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, InitialSetupAuthority, Platform, ReleaseInfo, ResourceStatus,
    RuntimeMetadata, Stack, StackSettings, StackState,
};
use alien_infra::step_protocol::protocol::{StepRequest, StepResponse};
use alien_infra::step_protocol::test_controller::{ControllerHost, MockCloud, DNS_ZONE};
use alien_infra::step_protocol::{
    ProxyFactory, Transport, TransportFailure, PROXY_CONTROLLER_TYPE,
};
use alien_infra::{DefaultPlatformServiceProvider, LoadedExtensions, PlatformServiceProvider};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Hands each step to a controller in this process, through the same JSON as the wire.
struct InProcess {
    host: ControllerHost,
    requests: std::sync::Mutex<Vec<Value>>,
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
        self.requests.lock().unwrap().push(wire.clone());
        let response = self
            .host
            .handle(serde_json::from_value(wire).unwrap())
            .await;
        Ok(serde_json::from_value(serde_json::to_value(response).unwrap()).unwrap())
    }
}

fn stack() -> Stack {
    serde_json::from_value(json!({
        "id": "acme",
        "resources": {
            "zone": {
                "config": { "type": DNS_ZONE, "id": "zone", "domain": "internal.acme.dev" },
                "lifecycle": "live",
                "dependencies": []
            }
        },
        "permissions": { "profiles": {}, "management": "auto" },
        // A zone holds no data, so deleting the deployment deletes it.
        "extensions": [{
            "name": "acme",
            "version": "1.0.0",
            "digest": "sha256:0",
            "resourceTypes": {
                DNS_ZONE: {
                    "lifecycles": ["live"],
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

async fn run_until(
    mut state: DeploymentState,
    provider: &Arc<dyn PlatformServiceProvider>,
    statuses: &[DeploymentStatus],
) -> DeploymentState {
    for _ in 0..60 {
        if statuses.contains(&state.status) {
            return state;
        }
        state = crate::support::step(state, config(), ClientConfig::Test, Some(provider.clone()))
            .await
            .unwrap_or_else(|e| panic!("step failed: {e:#?}"))
            .state;
    }
    panic!(
        "never reached {statuses:?}; last {:?}: {:#?}",
        state.status, state.error
    );
}

/// Where the Pending phase leaves a deployment. Its preflights are covered with the extension's
/// descriptors elsewhere.
fn initial_setup() -> DeploymentState {
    DeploymentState {
        status: DeploymentStatus::InitialSetup,
        platform: Platform::Test,
        current_release: None,
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_1".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            stack: stack(),
        }),
        stack_state: Some(StackState::new(Platform::Test)),
        error: None,
        environment_info: None,
        runtime_metadata: Some(RuntimeMetadata {
            initial_setup_authority: InitialSetupAuthority::DirectSetup,
            prepared_stack: Some(stack()),
            pending_prepared_release_id: Some("rel_1".to_string()),
            ..RuntimeMetadata::default()
        }),
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

/// A deployment whose provider reaches the acme extension's controller over its own transport.
struct Deployment {
    id: &'static str,
    transport: Arc<InProcess>,
    provider: Arc<dyn PlatformServiceProvider>,
}

impl Deployment {
    fn new(id: &'static str, cloud: &Path) -> Self {
        let transport = Arc::new(InProcess {
            host: ControllerHost::new(cloud, None),
            requests: Default::default(),
        });
        let extensions = LoadedExtensions::default()
            .with_hooks("acme", alien_deployment::testing::no_hooks("acme"))
            .with_controller("acme", ProxyFactory::new(id, transport.clone(), None));
        Self {
            id,
            transport,
            provider: Arc::new(
                DefaultPlatformServiceProvider::default()
                    .with_extensions(&stack(), Arc::new(extensions))
                    .unwrap(),
            ),
        }
    }

    /// Deploys the zone, checks it runs, then deletes the deployment.
    async fn deploy_and_delete(&self) {
        let mut state = run_until(
            initial_setup(),
            &self.provider,
            &[
                DeploymentStatus::Running,
                DeploymentStatus::ProvisioningFailed,
            ],
        )
        .await;
        assert_eq!(
            state.status,
            DeploymentStatus::Running,
            "{}: {:#?}",
            self.id,
            state.error
        );
        let zone = &state.stack_state.as_ref().unwrap().resources["zone"];
        assert_eq!(zone.status, ResourceStatus::Running);
        assert_eq!(
            zone.internal_state.as_ref().unwrap()["type"],
            PROXY_CONTROLLER_TYPE
        );
        let outputs = serde_json::to_value(zone.outputs.as_ref().unwrap()).unwrap();
        assert_eq!(outputs["type"], DNS_ZONE);
        assert_eq!(outputs["domain"], "internal.acme.dev");

        state.status = DeploymentStatus::DeletePending;
        state.target_release = state.current_release.clone();
        let state = run_until(
            state,
            &self.provider,
            &[DeploymentStatus::Deleted, DeploymentStatus::DeleteFailed],
        )
        .await;
        assert_eq!(
            state.status,
            DeploymentStatus::Deleted,
            "{}: {:#?}",
            self.id,
            state.error
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn deployments_stepped_together_reach_only_their_own_controllers() {
    let vault = tempfile::TempDir::new().unwrap();
    std::env::set_var("TEST_VAULT_DATA_DIR", vault.path());
    let (cloud_a, cloud_b) = (
        tempfile::TempDir::new().unwrap(),
        tempfile::TempDir::new().unwrap(),
    );
    let a = Deployment::new("dep_a", cloud_a.path());
    let b = Deployment::new("dep_b", cloud_b.path());

    // A provider that loaded no controller for acme: its deployment finds none, whatever the
    // deployments beside it registered.
    let without: Arc<dyn PlatformServiceProvider> = Arc::new(
        DefaultPlatformServiceProvider::default()
            .with_extensions(
                &stack(),
                Arc::new(
                    LoadedExtensions::default()
                        .with_hooks("acme", alien_deployment::testing::no_hooks("acme")),
                ),
            )
            .unwrap(),
    );
    let unserved = async {
        let mut state = initial_setup();
        for _ in 0..60 {
            match crate::support::step(state, config(), ClientConfig::Test, Some(without.clone()))
                .await
            {
                Ok(result) => state = result.state,
                Err(error) => return error,
            }
        }
        panic!("a deployment with no controller for its custom type kept stepping");
    };
    let (_, _, refusal) = tokio::join!(a.deploy_and_delete(), b.deploy_and_delete(), unserved);
    let refusal = format!("{refusal:?}");
    assert!(
        refusal.contains("EXTENSION_CONTROLLER_NOT_LOADED")
            && refusal.contains(
                "The controller of extension 'acme' for acme/dns-zone on test can't run: this process has not loaded the extension's controller"
            ),
        "{refusal}"
    );

    for (deployment, cloud) in [(&a, cloud_a.path()), (&b, cloud_b.path())] {
        let requests = deployment.transport.requests.lock().unwrap();
        assert!(!requests.is_empty());
        assert!(
            requests.iter().all(|r| r["deploymentId"] == deployment.id),
            "{} got another deployment's step",
            deployment.id
        );
        assert!(requests
            .iter()
            .any(|r| r["transitions"] == json!(["delete-start"])));
        let cloud = MockCloud::at(cloud);
        assert_eq!(cloud.state().zones.len(), 1);
        assert!(cloud.state().zones.values().all(|zone| zone.deleted));
    }
}
