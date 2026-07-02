//! Tests that binding params are synced to the control plane only for `remote_access` resources.

use super::helpers::*;
use crate::error::Result;
use alien_core::{ResourceLifecycle, Stack, Vault};

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
