//! Keeps held values out of the state handed to a control plane.
//!
//! Resource configs in a deployment's state carry the environment the
//! workloads run with. Values this manager holds are replaced there by a
//! marker when state is saved, and put back when it is loaded to run a step.

use alien_core::StackState;

use super::{ValueScope, ValueStore};
use crate::error::{ErrorData, Result};

/// Environment variables (name, value) the store holds for the deployment.
async fn held_variables(store: &ValueStore, deployment_id: &str) -> Result<Vec<(String, String)>> {
    let scope = ValueScope::deployment(deployment_id)?;
    Ok(store
        .list(&scope)
        .await?
        .into_iter()
        .filter_map(|stored| {
            let name = stored.slot.env_name()?.to_string();
            Some((name, stored.value))
        })
        .collect())
}

/// Replaces the deployment's held values in `stack_state` before it is saved.
pub async fn redact_held_values(
    store: Option<&ValueStore>,
    deployment_id: &str,
    stack_state: Option<&mut StackState>,
) -> Result<()> {
    let (Some(store), Some(stack_state)) = (store, stack_state) else {
        return Ok(());
    };
    let held = held_variables(store, deployment_id).await?;
    let scope = ValueScope::deployment(deployment_id)?;
    alien_deployment::redact_held_values(stack_state, scope.as_str(), &held)
        .map_err(|e| ErrorData::internal(format!("Could not redact held values: {}", e.message)))
}

/// Puts the deployment's held values back into a saved `stack_state`.
pub async fn restore_held_values(
    store: Option<&ValueStore>,
    deployment_id: &str,
    stack_state: Option<&mut StackState>,
) -> Result<()> {
    let (Some(store), Some(stack_state)) = (store, stack_state) else {
        return Ok(());
    };
    let held = held_variables(store, deployment_id).await?;
    let scope = ValueScope::deployment(deployment_id)?;
    alien_deployment::restore_held_values(stack_state, scope.as_str(), &held)
        .map_err(|e| ErrorData::internal(format!("Could not restore held values: {}", e.message)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_bindings::providers::kv::local::LocalKv;
    use alien_core::{
        Platform, Resource, ResourceLifecycle, ResourceStatus, StackResourceState, Worker,
        WorkerCode,
    };

    use super::*;
    use crate::values::cipher::fixtures::sealed_backend;
    use crate::values::ValueKind;

    const CANARY: &str = "canary-9d41c2";

    async fn store_holding(deployment_id: &str) -> (tempfile::TempDir, ValueStore) {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let dir = tempfile::tempdir().unwrap();
        let kv = LocalKv::single_process(dir.path().join("kv"))
            .await
            .unwrap();
        let store = ValueStore::new(Arc::new(sealed_backend(Arc::new(kv)).1));
        store
            .put(
                &ValueScope::deployment(deployment_id).unwrap(),
                &"env:API_URL".parse().unwrap(),
                ValueKind::Plain,
                CANARY.to_string(),
            )
            .await
            .unwrap();
        (dir, store)
    }

    fn state_running_with_canary() -> StackState {
        let mut worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        worker.environment = [
            ("API_URL".to_string(), CANARY.to_string()),
            ("KEPT".to_string(), "kept".to_string()),
        ]
        .into();
        let resource = Resource::new(worker);
        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(
            "api".to_string(),
            StackResourceState {
                progress: None,
                display_name: None,
                description: None,
                resource_type: resource.resource_type().as_ref().to_string(),
                internal_state: None,
                status: ResourceStatus::Running,
                outputs: None,
                previous_config: Some(resource.clone()),
                config: resource,
                retry_attempt: 0,
                error: None,
                lifecycle: Some(ResourceLifecycle::Live),
                controller_platform: None,
                dependencies: Vec::new(),
                last_failed_state: None,
                remote_binding_params: None,
                external_binding: false,
            },
        );
        state
    }

    #[tokio::test]
    async fn saved_state_has_no_held_value_and_a_loaded_state_gets_it_back() {
        let (_dir, store) = store_holding("dep_1").await;
        let mut state = state_running_with_canary();
        let original = serde_json::to_value(&state).unwrap();

        redact_held_values(Some(&store), "dep_1", Some(&mut state))
            .await
            .unwrap();
        let saved = serde_json::to_string(&state).unwrap();
        assert!(!saved.contains(CANARY), "{saved}");
        assert!(saved.contains("alien-held:"), "{saved}");
        assert!(
            saved.contains("kept"),
            "values the manager does not hold stay"
        );

        restore_held_values(Some(&store), "dep_1", Some(&mut state))
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), original);
    }

    #[tokio::test]
    async fn another_deployments_values_are_not_redacted() {
        let (_dir, store) = store_holding("dep_1").await;
        let mut state = state_running_with_canary();

        redact_held_values(Some(&store), "dep_2", Some(&mut state))
            .await
            .unwrap();

        assert!(serde_json::to_string(&state).unwrap().contains(CANARY));
    }

    #[tokio::test]
    async fn without_a_store_state_is_left_alone() {
        let mut state = state_running_with_canary();
        redact_held_values(None, "dep_1", Some(&mut state))
            .await
            .unwrap();
        assert!(serde_json::to_string(&state).unwrap().contains(CANARY));
    }
}
