//! Exercise Machines setup ownership without cloud credentials or a Machines host.

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings, Platform, ReleaseInfo,
    ResourceLifecycle, ResourceStatus, RuntimeMetadata, Stack, StackSettings, StackState, Storage,
    StorageBinding,
};
use alien_deployment::{prepare_direct_setup_update, step};
use alien_infra::{StackExecutor, StackResourceStateExt};
use alien_preflights::runner::PreflightRunner;

fn installed_stack() -> Stack {
    Stack::new("demo".to_string()).build()
}

fn target_stack() -> Stack {
    Stack::new("demo".to_string())
        .add(
            Storage::new("archive".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn config(bindings: ExternalBindings) -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings {
            external_bindings: Some(bindings.clone()),
            ..Default::default()
        })
        .external_bindings(bindings)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .allow_frozen_changes(false)
        .build()
}

fn metadata() -> RuntimeMetadata {
    RuntimeMetadata {
        prepared_stack: Some(installed_stack()),
        ..Default::default()
    }
}

#[tokio::test]
async fn new_frozen_machines_storage_requires_setup_even_before_binding_is_supplied() {
    let installed = installed_stack();
    let state = DeploymentState {
        status: DeploymentStatus::UpdatePending,
        platform: Platform::Machines,
        current_release: Some(ReleaseInfo {
            release_id: Some("rel_installed".into()),
            version: None,
            description: None,
            stack: installed,
        }),
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_target".into()),
            version: None,
            description: None,
            stack: target_stack(),
        }),
        stack_state: Some(StackState::new(Platform::Machines)),
        runtime_metadata: Some(metadata()),
        environment_info: None,
        error: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };
    let error = step(
        state,
        config(ExternalBindings::new()),
        ClientConfig::Machines,
        None,
    )
    .await
    .expect_err("runtime must require setup before introducing Frozen storage");
    let cause = error.source.as_ref().expect("preflight cause");
    assert_eq!(cause.code, "DEPLOYMENT_SETUP_REQUIRED", "{error:?}");
    assert!(!cause.retryable);
    assert!(cause.message.contains("archive"));
}

#[tokio::test]
async fn intrinsically_invalid_target_is_not_presented_as_repairable_setup() {
    let target = Stack::new("demo".to_string())
        .add(
            Storage::new("Invalid_ID".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let error = PreflightRunner::new()
        .run_deployment_time_preflights(
            target,
            &StackState::new(Platform::Machines),
            &config(ExternalBindings::new()),
            &ClientConfig::Machines,
            Some(&installed_stack()),
            None,
            None,
        )
        .await
        .expect_err("setup cannot repair an invalid resource declaration");
    assert_eq!(error.code, "VALIDATION_FAILED");
}

#[tokio::test]
async fn direct_machines_setup_still_rejects_missing_external_binding() {
    let error = prepare_direct_setup_update(
        target_stack(),
        &StackState::new(Platform::Machines),
        &config(ExternalBindings::new()),
        &ClientConfig::Machines,
        &metadata(),
    )
    .await
    .expect_err("setup authority does not waive binding prerequisites");
    assert_eq!(
        error.source.as_ref().expect("preflight cause").code,
        "VALIDATION_FAILED"
    );
}

#[tokio::test]
async fn external_machines_storage_setup_and_repeat_never_create_a_storage_controller() {
    let mut bindings = ExternalBindings::new();
    bindings.insert(
        "archive",
        ExternalBinding::Storage(StorageBinding::s3("customer-archive")),
    );
    let config = config(bindings);
    let state = StackState::new(Platform::Machines);
    let prepared = prepare_direct_setup_update(
        target_stack(),
        &state,
        &config,
        &ClientConfig::Machines,
        &metadata(),
    )
    .await
    .expect("valid external storage setup should prepare");
    let mut deployment = DeploymentState {
        status: DeploymentStatus::InitialSetup,
        platform: Platform::Machines,
        current_release: Some(ReleaseInfo {
            release_id: Some("rel_installed".into()),
            version: None,
            description: None,
            stack: installed_stack(),
        }),
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_target".into()),
            version: None,
            description: None,
            stack: target_stack(),
        }),
        stack_state: Some(state),
        runtime_metadata: Some(prepared.clone()),
        environment_info: None,
        error: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };
    for _ in 0..10 {
        if deployment.status == DeploymentStatus::Provisioning {
            break;
        }
        deployment = step(deployment, config.clone(), ClientConfig::Machines, None)
            .await
            .expect("initial setup should adopt external storage")
            .state;
        assert!(matches!(
            deployment.status,
            DeploymentStatus::InitialSetup | DeploymentStatus::Provisioning
        ));
    }
    assert_eq!(deployment.status, DeploymentStatus::Provisioning);
    assert_eq!(
        deployment
            .current_release
            .as_ref()
            .unwrap()
            .release_id
            .as_deref(),
        Some("rel_installed")
    );
    assert_eq!(
        deployment
            .target_release
            .as_ref()
            .unwrap()
            .release_id
            .as_deref(),
        Some("rel_target")
    );
    let installed = deployment.stack_state.unwrap();
    let archive = &installed.resources["archive"];
    assert_eq!(archive.status, ResourceStatus::Running);
    assert!(!archive.has_internal_state());
    assert!(archive.remote_binding_params.is_none());
    assert!(archive.outputs.is_none());
    let repeated = prepare_direct_setup_update(
        target_stack(),
        &installed,
        &config,
        &ClientConfig::Machines,
        &prepared,
    )
    .await
    .expect("repeated setup should remain valid");
    let executor = StackExecutor::builder(
        repeated.prepared_stack.as_ref().unwrap(),
        ClientConfig::Machines,
    )
    .deployment_config(&config)
    .build()
    .unwrap();
    let repeated = executor
        .run_until_synced(installed)
        .await
        .into_result()
        .unwrap();
    assert_eq!(
        repeated.resources["archive"].status,
        ResourceStatus::Running
    );
    assert!(!repeated.resources["archive"].has_internal_state());
    let deletion = StackExecutor::for_deletion(ClientConfig::Machines, &config, None).unwrap();
    let deleted = deletion
        .run_until_synced(repeated)
        .await
        .into_result()
        .expect("deletion only drops the external association");
    assert_eq!(deleted.resources["archive"].status, ResourceStatus::Deleted);
    assert!(!deleted.resources["archive"].has_internal_state());
    assert!(deleted.resources["archive"].remote_binding_params.is_none());
}
