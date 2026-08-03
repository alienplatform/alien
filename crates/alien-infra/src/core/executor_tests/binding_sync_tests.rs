//! Tests that binding params are synced to the control plane only for `remote_access` resources.

use super::helpers::*;
use crate::error::Result;
use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBinding,
    ExternalBindings, ResourceLifecycle, Stack, Storage, StorageBinding, Vault,
};

/// A controller-provisioned binding must land in `StackResourceState.remote_binding_params` — which
/// is serialized into synced control-plane state — ONLY when the resource is `remote_access: true`.
///
/// A non-remote binding can carry an inline secret (e.g. a Local Postgres password); same-stack
/// workers resolve bindings via the controller/manager, not this synced field, so it must never
/// reach the control plane. Both vaults below reach Running and both emit a `VaultBinding`, so the
/// `remote_access` flag is the only thing that distinguishes them.
#[tokio::test]
async fn remote_binding_params_synced_only_for_remote_access_resources() -> Result<()> {
    let vault_local = Vault::new("vault-local".to_owned()).build();
    let vault_remote = Vault::new("vault-remote".to_owned()).build();

    let stack = Stack::new("remote-binding-gate-test".to_owned())
        .add(vault_local, ResourceLifecycle::Frozen)
        .add_with_remote_access(vault_remote, ResourceLifecycle::Frozen)
        .build();

    let executor = new_executor_with_filter(&stack, vec![ResourceLifecycle::Frozen])?;
    let final_state = run_to_synced(&executor, new_test_state()).await?;

    assert_all_running(&final_state, &["vault-local", "vault-remote"]);

    assert_eq!(
        final_state
            .resources
            .get("vault-local")
            .expect("vault-local in state")
            .remote_binding_params,
        None,
        "a remote_access:false resource must NOT sync its binding params to the control plane"
    );
    assert!(
        final_state
            .resources
            .get("vault-remote")
            .expect("vault-remote in state")
            .remote_binding_params
            .is_some(),
        "a remote_access:true resource must sync its binding params"
    );

    Ok(())
}

#[tokio::test]
async fn external_storage_rejects_remote_access_without_a_controller() -> Result<()> {
    let storage = Storage::new("uploads".to_owned()).build();
    let disabled_stack = Stack::new("external-binding-toggle".to_owned())
        .add(storage.clone(), ResourceLifecycle::Frozen)
        .build();
    let enabled_stack = Stack::new("external-binding-toggle".to_owned())
        .add_with_remote_access(storage, ResourceLifecycle::Frozen)
        .build();
    let mut external_bindings = ExternalBindings::new();
    external_bindings.insert(
        "uploads".to_owned(),
        ExternalBinding::Storage(StorageBinding::s3("customer-uploads")),
    );
    let deployment_config = DeploymentConfig::builder()
        .stack_settings(Default::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(external_bindings)
        .allow_frozen_changes(false)
        .build();
    let client_config = ClientConfig::Test;

    let disabled_executor =
        crate::core::StackExecutor::builder(&disabled_stack, client_config.clone())
            .deployment_config(&deployment_config)
            .build()?;
    let disabled_state = run_to_synced(&disabled_executor, new_test_state()).await?;
    assert!(disabled_state.resources["uploads"]
        .remote_binding_params
        .is_none());

    let enabled_executor =
        crate::core::StackExecutor::builder(&enabled_stack, client_config.clone())
            .deployment_config(&deployment_config)
            .build()?;
    let error = run_to_synced(&enabled_executor, disabled_state)
        .await
        .expect_err("external Storage must not be exposed through Remote Bindings");
    assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
    assert!(error.message.contains("cannot use an external binding"));

    Ok(())
}
