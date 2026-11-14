//! A failed first Helm install moves to a corrective release by pin + retry
//! only when the release keeps the frozen setup the installed chart provides.

mod support;

use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
    EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings, InitialSetupAuthority,
    Platform, ReleaseInfo, ResourceLifecycle, RuntimeMetadata, ServiceAccount, Stack,
    StackSettings, StackState, Storage, StorageBinding,
};
use alien_deployment::step;

fn stack(frozen: &[&str], live: &[&str]) -> Stack {
    let mut builder = Stack::new("demo".to_string());
    for id in frozen {
        builder = builder.add(
            ServiceAccount::new(id.to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    for id in live {
        builder = builder.add(
            Storage::new(id.to_string()).build(),
            ResourceLifecycle::Live,
        );
    }
    builder.build()
}

fn config() -> DeploymentConfig {
    let mut bindings = ExternalBindings::new();
    for id in ["broken", "fixed"] {
        bindings.insert(id, ExternalBinding::Storage(StorageBinding::s3("bucket")));
    }
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

/// Release 1 failed initial setup; the deployment is pinned to `target`. The
/// installed stack is prepared as the first install prepares it.
async fn failed_first_install(installed: Stack, target: Stack) -> DeploymentState {
    let (installed, _, _) = alien_preflights::runner::PreflightRunner::new()
        .run_deployment_time_preflights(
            installed,
            &StackState::new(Platform::Kubernetes),
            &config(),
            &ClientConfig::Test,
            None,
            None,
            Some(InitialSetupAuthority::DirectSetup),
        )
        .await
        .expect("release 1 prepares");
    DeploymentState {
        status: DeploymentStatus::InitialSetupFailed,
        platform: Platform::Kubernetes,
        current_release: None,
        target_release: Some(ReleaseInfo {
            release_id: Some("rel_2".into()),
            version: None,
            description: None,
            stack: target,
            signed: None,
            signer_lists: Vec::new(),
        }),
        stack_state: Some(StackState::new(Platform::Kubernetes)),
        runtime_metadata: Some(RuntimeMetadata {
            initial_setup_authority: InitialSetupAuthority::ImportedHandoff,
            prepared_stack: Some(installed),
            pending_prepared_release_id: Some("rel_1".into()),
            ..Default::default()
        }),
        environment_info: None,
        error: None,
        retry_requested: true,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

#[tokio::test]
async fn retry_re_prepares_against_the_pinned_release_when_frozen_setup_is_unchanged() {
    // Release 1 failed on a live resource; release 2 replaces it.
    let state = failed_first_install(
        stack(&["vault"], &["broken"]),
        stack(&["vault"], &["fixed"]),
    )
    .await;
    let next = step(state, config(), ClientConfig::Test, None)
        .await
        .expect("a corrective release with the same frozen setup is accepted")
        .state;
    assert_eq!(next.status, DeploymentStatus::InitialSetup);
    let metadata = next.runtime_metadata.expect("metadata");
    let prepared = metadata.prepared_stack.expect("prepared stack");
    assert!(prepared.resources.contains_key("fixed"));
    assert!(!prepared.resources.contains_key("broken"));
    assert_eq!(
        metadata.pending_prepared_release_id.as_deref(),
        Some("rel_2")
    );
    assert_eq!(
        metadata.initial_setup_authority,
        InitialSetupAuthority::ImportedHandoff
    );
}

#[tokio::test]
async fn retry_refuses_a_pinned_release_that_changes_frozen_setup() {
    // Release 1 included a frozen resource that fails; release 2 drops it.
    let state = failed_first_install(stack(&["vault", "key"], &[]), stack(&["vault"], &[])).await;
    let error = step(state, config(), ClientConfig::Test, None)
        .await
        .expect_err("the installed chart's setup cannot be changed by a retry");
    let message = format!("{error:?}");
    assert!(message.contains("DEPLOYMENT_SETUP_REQUIRED"), "{message}");
    assert!(message.contains("helm upgrade"), "{message}");
}

#[tokio::test]
async fn the_step_after_a_retry_continues_the_pinned_release_through_the_imported_path() {
    let state = failed_first_install(stack(&[], &["broken"]), stack(&[], &["fixed"])).await;
    let retried = step(state, config(), ClientConfig::Test, None)
        .await
        .expect("retry is accepted")
        .state;
    assert_eq!(retried.status, DeploymentStatus::InitialSetup);

    // The next step resumes setup with `continue_imported` against the pinned
    // release's prepared stack; the failed release's resource is not referenced.
    let continued = step(retried, config(), ClientConfig::Test, None)
        .await
        .expect("setup continues against the pinned release")
        .state;
    let metadata = continued.runtime_metadata.expect("metadata");
    let prepared = metadata.prepared_stack.expect("prepared stack");
    assert!(prepared.resources.contains_key("fixed"));
    assert!(!prepared.resources.contains_key("broken"));
    assert_ne!(
        continued.status,
        DeploymentStatus::InitialSetupFailed,
        "{:?}",
        continued.error
    );
    let stack_state = continued.stack_state.expect("stack state");
    assert!(!stack_state.resources.contains_key("broken"));
}
