//! A running resource that an update does not touch keeps reporting while the update runs.
mod support;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo, ResourceLifecycle,
    ResourceRef, Stack, StackInputDefinition, StackInputEnvironmentMapping, StackInputKind,
    StackInputProvider, StackSettings, Storage,
};
use alien_infra::DefaultPlatformServiceProvider;
use std::sync::Arc;
use tempfile::TempDir;

fn storage(id: &str) -> Storage {
    Storage::new(id.to_string()).build()
}

fn base_stack() -> Stack {
    Stack::new("beat".to_string())
        .add(storage("kept"), ResourceLifecycle::Live)
        .build()
}

fn required_secret_stack() -> Stack {
    let mut stack = base_stack();
    stack.inputs = vec![StackInputDefinition {
        id: "databasePassword".to_string(),
        kind: StackInputKind::Secret,
        when: None,
        provided_by: vec![StackInputProvider::Deployer],
        required: true,
        label: "Database password".to_string(),
        description: "Password of the customer's database".to_string(),
        placeholder: None,
        default: None,
        platforms: None,
        validation: None,
        generate: None,
        env: vec![StackInputEnvironmentMapping {
            name: "DATABASE_PASSWORD".to_string(),
            target_resources: None,
            var_type: None,
        }],
    }];
    stack
}

fn release(id: &str, stack: Stack) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some(id.to_string()),
        version: None,
        description: None,
        stack,
        signed: None,
        signer_lists: Vec::new(),
    }
}

struct Deployment {
    _directory: TempDir,
    bindings: Arc<alien_local::LocalBindingsProvider>,
    services: Arc<DefaultPlatformServiceProvider>,
    client: ClientConfig,
    config: DeploymentConfig,
    state: DeploymentState,
}

impl Deployment {
    /// A local deployment of `base_stack`, run to Running.
    async fn running() -> Self {
        let directory = TempDir::new().unwrap();
        let bindings = alien_local::LocalBindingsProvider::new(directory.path()).unwrap();
        let mut deployment = Self {
            services: Arc::new(DefaultPlatformServiceProvider::with_local_bindings(
                bindings.clone(),
            )),
            client: ClientConfig::Local {
                state_directory: directory.path().to_string_lossy().into_owned(),
            },
            config: DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables: vec![],
                    hash: String::new(),
                    created_at: String::new(),
                })
                .external_bindings(ExternalBindings::default())
                .allow_frozen_changes(false)
                .build(),
            state: DeploymentState {
                status: DeploymentStatus::Pending,
                platform: Platform::Local,
                current_release: None,
                target_release: Some(release("one", base_stack())),
                stack_state: None,
                error: None,
                environment_info: None,
                runtime_metadata: None,
                retry_requested: false,
                protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
            },
            bindings,
            _directory: directory,
        };
        while deployment.state.status != DeploymentStatus::Running {
            deployment.step().await;
        }
        deployment
    }

    /// One step. Returns the resources that reported in it.
    async fn step(&mut self) -> Vec<String> {
        let result = crate::support::step(
            self.state.clone(),
            self.config.clone(),
            self.client.clone(),
            Some(self.services.clone()),
        )
        .await
        .unwrap();
        self.state = result.state;
        result
            .heartbeats
            .into_iter()
            .map(|heartbeat| heartbeat.resource_id)
            .collect()
    }

    fn start_update(&mut self, target: Stack) {
        self.state.status = DeploymentStatus::UpdatePending;
        self.state.target_release = Some(release("two", target));
    }
}

/// Steps until `stop` holds, returning what reported in each step along with the status it ended in.
async fn step_until(
    deployment: &mut Deployment,
    stop: impl Fn(DeploymentStatus) -> bool,
) -> Vec<(DeploymentStatus, Vec<String>)> {
    let mut steps = Vec::new();
    for _ in 0..30 {
        let reported = deployment.step().await;
        let status = deployment.state.status;
        steps.push((status, reported));
        if stop(status) {
            return steps;
        }
    }
    panic!("the deployment did not reach the expected status: {steps:?}");
}

fn reports_of(
    steps: &[(DeploymentStatus, Vec<String>)],
    status: DeploymentStatus,
    id: &str,
) -> usize {
    steps
        .iter()
        .filter(|(step_status, reported)| {
            *step_status == status && reported.iter().any(|reported| reported == id)
        })
        .count()
}

#[tokio::test]
async fn an_unchanged_resource_reports_once_while_an_update_is_in_progress() {
    let mut deployment = Deployment::running().await;
    // Three storages in a chain: the update takes several steps in the Updating status.
    deployment.start_update(
        Stack::new("beat".to_string())
            .add(storage("kept"), ResourceLifecycle::Live)
            .add(storage("first"), ResourceLifecycle::Live)
            .add_with_dependencies(
                storage("second"),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Storage::RESOURCE_TYPE, "first")],
            )
            .add_with_dependencies(
                storage("third"),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(Storage::RESOURCE_TYPE, "second")],
            )
            .build(),
    );
    let steps = step_until(&mut deployment, |status| {
        status == DeploymentStatus::Running
    })
    .await;

    let updating = steps
        .iter()
        .filter(|(status, _)| *status == DeploymentStatus::Updating)
        .count();
    assert!(
        updating >= 3,
        "the update must take several steps: {steps:?}"
    );
    // The update never runs "kept", so only the read for untouched resources reports it. It is
    // read once and then left alone until the interval passes, not on every step.
    assert_eq!(
        reports_of(&steps, DeploymentStatus::Updating, "kept"),
        1,
        "{steps:?}"
    );
    deployment.bindings.shutdown().await;
}

#[tokio::test]
async fn a_missing_secret_that_nothing_reads_does_not_hold_the_update() {
    // The secret maps to workloads only and this stack has none, so nothing is held: the update
    // finishes instead of waiting for the customer. Held readers are covered in test_platform.
    let mut deployment = Deployment::running().await;
    deployment.start_update(required_secret_stack());
    let steps = step_until(&mut deployment, |status| {
        status == DeploymentStatus::Running
    })
    .await;
    assert!(
        steps
            .iter()
            .all(|(status, _)| *status != DeploymentStatus::WaitingForSecrets),
        "{steps:?}"
    );
    assert_eq!(
        deployment
            .state
            .current_release
            .as_ref()
            .and_then(|release| release.release_id.as_deref()),
        Some("two")
    );
    deployment.bindings.shutdown().await;
}
