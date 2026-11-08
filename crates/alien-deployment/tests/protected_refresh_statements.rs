//! A protected install's health checks hand controllers the signed release that is running.
//!
//! On a protected install a machines controller's Ready handler talks to the install's verifier
//! (a replacement machine is admitted only against the signed release), so the release
//! statements must reach it on every health tick, not only while provisioning or updating.

use std::sync::Mutex;

use alien_core::{
    ClientConfig, ComputeCluster, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, Platform, ReleaseInfo, ResourceEntry, ResourceLifecycle,
    ResourceStatus, SignedStatement, Stack, StackSettings, StackSigning,
};
use alien_infra::{
    register_controller_deserializer_extension, register_registry_extension,
    ResourceControllerContext,
};
use alien_macros::controller;
use indexmap::IndexMap;

// `#[controller]` names `crate::core::…`, `crate::error::…` and `crate::Result`.
mod core {
    pub use alien_infra::{
        ResourceController, ResourceControllerContext, ResourceControllerStepResult,
    };
}
mod error {
    pub use alien_infra::ErrorData;
}
use alien_infra::Result;

/// The release statement each Ready handler was handed, in order.
static SEEN: Mutex<Vec<Option<SignedStatement>>> = Mutex::new(Vec::new());

/// Stands in for a machines controller: its Ready handler needs the signed release on a
/// protected install, as the cloud machines controllers do to admit a replacement machine.
#[controller]
pub struct ProtectedMachinesProbe {
    pub ready_ticks: u32,
}

#[controller]
impl ProtectedMachinesProbe {
    #[flow_entry(Create)]
    #[handler(
        state = Creating,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn creating(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.ready_ticks += 1;
        let statements = alien_infra::verifier::protected_statements(ctx, "machines")?;
        SEEN.lock()
            .unwrap()
            .push(statements.map(|statements| statements.release.clone()));
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = ProvisionFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<alien_core::ResourceOutputs> {
        None
    }
}

fn stack() -> Stack {
    let mut resources = IndexMap::new();
    resources.insert(
        "machines".to_string(),
        ResourceEntry {
            config: alien_core::Resource::new(ComputeCluster::new("machines".into()).build()),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            display_name: None,
            description: None,
            remote_access: false,
            enabled_when: None,
        },
    );
    Stack {
        id: "machines-stack".to_string(),
        resources,
        permissions: alien_core::PermissionsConfig {
            profiles: IndexMap::new(),
            management: alien_core::ManagementPermissions::Auto,
        },
        supported_platforms: None,
        inputs: Vec::new(),
        dynamic_container_repositories: Vec::new(),
        dynamic_container_image_resources: Vec::new(),
        operations: None,
    }
}

fn config(stack_settings: StackSettings) -> DeploymentConfig {
    DeploymentConfig {
        gcp_federation_scope: None,
        stored_secret_input_ids: None,
        input_values: Default::default(),
        deployment_name: Some("machines".to_string()),
        stack_settings,
        management_config: None,
        environment_variables: EnvironmentVariablesSnapshot {
            hash: "none".to_string(),
            variables: Vec::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
        },
        external_bindings: alien_core::ExternalBindings::default(),
        base_platform: None,
        label_domain: None,
        observe_label_selector: None,
        observe_all_namespaces: false,
        compute_backend: None,
        allow_frozen_changes: false,
        setup_run: false,
        setup_run_id: None,
        domain_metadata: None,
        public_endpoints: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
        volume_restores: Vec::new(),
        deletion_policy: Default::default(),
        release_statements: None,
        signed_config: None,
    }
}

async fn step(state: DeploymentState, config: &DeploymentConfig) -> DeploymentState {
    alien_deployment::step(state, config.clone(), ClientConfig::Test, None)
        .await
        .expect("the step runs")
        .state
}

#[tokio::test]
async fn a_protected_install_s_health_check_hands_controllers_the_running_release() {
    // Controllers outside alien-infra register how their saved state is read back.
    register_controller_deserializer_extension(Box::new(|type_tag, value| match type_tag {
        "ProtectedMachinesProbe" => Ok(Some(Box::new(serde_json::from_value::<
            ProtectedMachinesProbe,
        >(value)?))),
        _ => Ok(None),
    }));
    register_registry_extension(Box::new(|registry| {
        registry
            .register::<ComputeCluster>(ComputeCluster::RESOURCE_TYPE)
            .with_controller::<ProtectedMachinesProbe>(Platform::Test);
    }));
    let release = SignedStatement::new(br#"{"kind":"release/v2"}"#);
    let signer_list = SignedStatement::new(br#"{"kind":"signers/v1"}"#);

    // Arrange: the install is deployed and running its signed release.
    let mut state = DeploymentState {
        status: DeploymentStatus::Pending,
        platform: Platform::Test,
        current_release: None,
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_1".to_string()),
            version: Some("1.0.0".to_string()),
            description: None,
            stack: stack(),
            signed: Some(release.clone()),
            signer_lists: vec![signer_list.clone()],
        }),
        stack_state: None,
        error: None,
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };
    let unprotected = config(StackSettings::default());
    for _ in 0..50 {
        if state.status == DeploymentStatus::Running {
            break;
        }
        state = step(state, &unprotected).await;
    }
    assert_eq!(state.status, DeploymentStatus::Running, "{:?}", state.error);
    let current = state
        .current_release
        .as_ref()
        .expect("the release is current");
    assert_eq!(current.signed.as_ref(), Some(&release));
    assert!(state.target_release.is_none());
    SEEN.lock().unwrap().clear();

    // Act: the same install, protected, takes its health checks.
    let protected = config(StackSettings {
        signing: Some(StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    });
    for _ in 0..2 {
        state = step(state, &protected).await;
        // Assert: each tick stays healthy, and the controller got the running release.
        assert_eq!(state.status, DeploymentStatus::Running, "{:?}", state.error);
    }
    // A health failure keeps observing with the same statements, and recovers.
    state.status = DeploymentStatus::RefreshFailed;
    state = step(state, &protected).await;
    assert_eq!(state.status, DeploymentStatus::Running, "{:?}", state.error);
    let machines = &state.stack_state.as_ref().unwrap().resources["machines"];
    assert_eq!(machines.status, ResourceStatus::Running);
    assert_eq!(
        *SEEN.lock().unwrap(),
        vec![Some(release.clone()), Some(release.clone()), Some(release)]
    );
}
