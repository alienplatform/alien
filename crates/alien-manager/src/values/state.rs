//! Keeps held values out of the state handed to a control plane.
//!
//! Resource configs in a deployment's state carry the environment the
//! workloads run with. Values this manager holds, whether they are variables
//! or stack inputs mapped to variables, are replaced there by a marker when
//! state is saved, and put back when it is loaded to run a step.

use alien_core::{EnvironmentVariable, EnvironmentVariableType, Stack, StackState};

use super::{StoredValue, ValueKind, ValueScope, ValueStore};
use crate::config::ManagerConfig;
use crate::error::{ErrorData, Result};
use crate::traits::deployment_store::DeploymentRecord;
use crate::traits::ReleaseStore;

/// The environment variables a deployment gets from values this manager holds.
#[derive(Debug, Default)]
pub struct HeldValues {
    /// `None` when the manager holds no values: nothing is redacted or restored.
    scope: Option<String>,
    /// Name and value of each variable that comes from a held value.
    variables: Vec<(String, String)>,
    /// Name and planned digest of each `env:` slot the sender planned.
    planned: Vec<(String, String)>,
}

impl HeldValues {
    /// What a step that loaded `held` (the deployment with its values filled
    /// in) runs with. The step keeps these through its checkpoints, so a value
    /// changed on the manager while the step runs is still kept out of the
    /// state the step saves.
    pub fn of_step(
        values: Option<&ValueStore>,
        config: &ManagerConfig,
        deployment: &DeploymentRecord,
        held: &DeploymentRecord,
        stack: Option<&Stack>,
    ) -> Result<Self> {
        if values.is_none() {
            return Ok(Self::default());
        }
        Ok(Self {
            scope: Some(scope_of(deployment)?),
            variables: held_variables(config, deployment, held, stack)?,
            planned: planned_env_digests(deployment),
        })
    }

    /// What this manager holds for `deployment` now, for state that a runner
    /// or an operator reports after running a step this manager did not watch.
    /// A variable whose value changed since the step loaded it is still found
    /// by the digest the sender planned it with.
    pub async fn current(
        values: Option<&ValueStore>,
        config: &ManagerConfig,
        release_store: &dyn ReleaseStore,
        deployment: &DeploymentRecord,
    ) -> Result<Self> {
        let Some(values) = values else {
            return Ok(Self::default());
        };
        let scope = ValueScope::deployment(&deployment.id)?;
        let stored = values.list(&scope).await?;
        let planned = planned_env_digests(deployment);
        if stored.is_empty() && planned.is_empty() {
            return Ok(Self::default());
        }
        let held = with_stored_values(deployment, &stored);
        let stack = if stored.iter().any(|value| value.slot.input_id().is_some()) {
            let release_id = deployment
                .desired_release_id
                .as_deref()
                .or(deployment.current_release_id.as_deref());
            match release_id {
                Some(release_id) => crate::loops::deployment::target_release_stack(
                    release_store,
                    deployment,
                    release_id,
                )
                .await
                .map_err(|e| {
                    ErrorData::internal(format!(
                        "Could not load the stack of the release: {}",
                        e.message
                    ))
                })?,
                None => None,
            }
        } else {
            None
        };
        Ok(Self {
            scope: Some(scope.as_str().to_string()),
            variables: held_variables(config, deployment, &held, stack.as_ref())?,
            planned,
        })
    }

    /// Replaces the held values in `stack_state` before it is saved.
    pub fn redact(&self, stack_state: Option<&mut StackState>) -> Result<()> {
        let (Some(scope), Some(stack_state)) = (&self.scope, stack_state) else {
            return Ok(());
        };
        alien_deployment::redact_planned_values(stack_state, scope, &self.planned).map_err(
            |e| ErrorData::internal(format!("Could not redact held values: {}", e.message)),
        )?;
        alien_deployment::redact_held_values(stack_state, scope, &self.variables).map_err(|e| {
            ErrorData::internal(format!("Could not redact held values: {}", e.message))
        })
    }

    /// Puts the held values back into a saved `stack_state`.
    pub fn restore(&self, stack_state: Option<&mut StackState>) -> Result<()> {
        let (Some(scope), Some(stack_state)) = (&self.scope, stack_state) else {
            return Ok(());
        };
        alien_deployment::restore_held_values(stack_state, scope, &self.variables).map_err(|e| {
            ErrorData::internal(format!("Could not restore held values: {}", e.message))
        })
    }
}

fn scope_of(deployment: &DeploymentRecord) -> Result<String> {
    Ok(ValueScope::deployment(&deployment.id)?.as_str().to_string())
}

fn planned_env_digests(deployment: &DeploymentRecord) -> Vec<(String, String)> {
    deployment
        .deployment_config
        .iter()
        .flat_map(|config| &config.value_slots)
        .filter_map(|slot| {
            let name = slot.slot.strip_prefix("env:")?;
            Some((name.to_string(), slot.digest.clone()))
        })
        .collect()
}

/// `deployment` with every value in `stored` filled in, without checking it
/// against what the sender planned.
fn with_stored_values(deployment: &DeploymentRecord, stored: &[StoredValue]) -> DeploymentRecord {
    let mut held = deployment.clone();
    let variables = held.user_environment_variables.get_or_insert_with(Vec::new);
    for value in stored {
        if let Some(name) = value.slot.env_name() {
            variables.retain(|have| have.name != name);
            variables.push(EnvironmentVariable {
                name: name.to_string(),
                value: value.value.clone(),
                var_type: match value.kind {
                    ValueKind::Plain => EnvironmentVariableType::Plain,
                    ValueKind::Secret => EnvironmentVariableType::Secret,
                },
                target_resources: None,
            });
        } else if let Some(id) = value.slot.input_id() {
            if let Ok(parsed) = serde_json::from_str(&value.value) {
                held.input_values.insert(id.to_string(), parsed);
            }
        }
    }
    held
}

/// The variables in the environment built with `held`'s values and not in the
/// one built without them: whatever those values contribute, whether as
/// variables or through stack inputs mapped to variables.
fn held_variables(
    config: &ManagerConfig,
    deployment: &DeploymentRecord,
    held: &DeploymentRecord,
    stack: Option<&Stack>,
) -> Result<Vec<(String, String)>> {
    let environment = |record: &DeploymentRecord| {
        crate::loops::deployment::deployment_environment_variables(config, record, stack)
            .map(|snapshot| snapshot.variables)
            .map_err(|e| {
                ErrorData::internal(format!(
                    "Could not resolve the environment of '{}': {}",
                    record.id, e.message
                ))
            })
    };
    let without = environment(deployment)?;
    Ok(environment(held)?
        .into_iter()
        .filter(|variable| {
            !without
                .iter()
                .any(|other| other.name == variable.name && other.value == variable.value)
        })
        .map(|variable| (variable.name, variable.value))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_bindings::providers::kv::local::LocalKv;
    use alien_core::{
        DeploymentConfig, EnvironmentVariablesSnapshot, Platform, Resource, ResourceLifecycle,
        ResourceStatus, StackResourceState, ValueSlotRef, Worker, WorkerCode,
    };
    use alien_error::AlienError;
    use async_trait::async_trait;
    use chrono::Utc;

    use super::*;
    use crate::auth::Subject;
    use crate::traits::release_store::{CreateReleaseParams, ReleaseRecord};
    use crate::values::cipher::fixtures::sealed_backend;

    const CANARY: &str = "canary-9d41c2";
    const INPUT_CANARY: &str = "input-canary-51e0";

    /// A release store for tests whose deployments carry their own stacks.
    struct NoReleases;

    #[async_trait]
    impl ReleaseStore for NoReleases {
        async fn create_release(
            &self,
            _: &Subject,
            _: CreateReleaseParams,
        ) -> std::result::Result<ReleaseRecord, AlienError> {
            unreachable!("the tests do not create releases")
        }
        async fn get_release(
            &self,
            _: &Subject,
            _: &str,
        ) -> std::result::Result<Option<ReleaseRecord>, AlienError> {
            Ok(None)
        }
        async fn get_latest_release(
            &self,
            _: &Subject,
        ) -> std::result::Result<Option<ReleaseRecord>, AlienError> {
            Ok(None)
        }
        async fn list_releases(
            &self,
            _: &Subject,
        ) -> std::result::Result<Vec<ReleaseRecord>, AlienError> {
            Ok(Vec::new())
        }
    }

    async fn store() -> (tempfile::TempDir, ValueStore) {
        alien_deployment::set_ephemeral_secrets_digest_key();
        let dir = tempfile::tempdir().unwrap();
        let kv = LocalKv::single_process(dir.path().join("kv"))
            .await
            .unwrap();
        (
            dir,
            ValueStore::new(Arc::new(sealed_backend(Arc::new(kv)).1)),
        )
    }

    async fn put(store: &ValueStore, deployment_id: &str, slot: &str, value: &str) -> String {
        store
            .put(
                &ValueScope::deployment(deployment_id).unwrap(),
                &slot.parse().unwrap(),
                ValueKind::Plain,
                value.to_string(),
            )
            .await
            .unwrap()
            .digest
    }

    /// A deployment of `dep_1` whose stack maps the input `endpoint` to the
    /// variable `ENDPOINT`.
    fn record(planned: Vec<ValueSlotRef>) -> DeploymentRecord {
        let input: alien_core::StackInputDefinition = serde_json::from_value(serde_json::json!({
            "id": "endpoint", "kind": "string", "providedBy": ["deployer"], "required": false,
            "label": "endpoint", "description": "endpoint", "env": [{"name": "ENDPOINT"}]
        }))
        .unwrap();
        let mut stack = alien_core::Stack::new("app".to_string()).build();
        stack.inputs = vec![input];
        let mut config = DeploymentConfig::builder()
            .deployment_name("dep".to_string())
            .stack_settings(Default::default())
            .input_values(Default::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();
        config.value_slots = planned;
        DeploymentRecord {
            supplied_stacks: Some(crate::traits::deployment_store::SuppliedStacks {
                current: Some(stack.clone()),
                desired: Some(stack),
            }),
            id: "dep_1".to_string(),
            workspace_id: "workspace".to_string(),
            project_id: "project".to_string(),
            name: "dep".to_string(),
            deployment_group_id: "group".to_string(),
            platform: Platform::Aws,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: Some(Default::default()),
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: Some("rel_1".to_string()),
            desired_release_id: None,
            configuration_revision: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_config: Some(config),
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn slot_ref(slot: &str, digest: &str) -> ValueSlotRef {
        ValueSlotRef {
            slot: slot.to_string(),
            kind: EnvironmentVariableType::Plain,
            digest: digest.to_string(),
            env_name: None,
            targets: None,
        }
    }

    /// A running worker whose environment carries the held variable, the
    /// held input's variable and one value the manager does not hold.
    fn state_running_with(api_url: &str, endpoint: &str) -> StackState {
        let mut worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("default".to_string())
            .build();
        worker.environment = [
            ("API_URL".to_string(), api_url.to_string()),
            ("ENDPOINT".to_string(), endpoint.to_string()),
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

    fn saved_json(state: &StackState) -> String {
        serde_json::to_string(state).unwrap()
    }

    /// The deployment with the values a step loads, as the loop builds it.
    async fn held_record(store: &ValueStore, planned: &DeploymentRecord) -> DeploymentRecord {
        crate::values::hydrate_held_values(Some(store), planned)
            .await
            .unwrap()
            .into_owned()
    }

    #[tokio::test]
    async fn saved_state_has_no_held_value_and_a_loaded_state_gets_it_back() {
        let (_dir, store) = store().await;
        let url = put(&store, "dep_1", "env:API_URL", CANARY).await;
        let endpoint = put(
            &store,
            "dep_1",
            "input:endpoint",
            &format!("\"{INPUT_CANARY}\""),
        )
        .await;
        let deployment = record(vec![
            slot_ref("env:API_URL", &url),
            slot_ref("input:endpoint", &endpoint),
        ]);
        let held = held_record(&store, &deployment).await;
        let config = ManagerConfig::default();
        let stack = deployment.supplied_stacks.as_ref().unwrap().current.clone();
        let step =
            HeldValues::of_step(Some(&store), &config, &deployment, &held, stack.as_ref()).unwrap();
        let mut state = state_running_with(CANARY, INPUT_CANARY);
        let original = serde_json::to_value(&state).unwrap();

        step.redact(Some(&mut state)).unwrap();
        let saved = saved_json(&state);
        assert!(!saved.contains(CANARY), "{saved}");
        assert!(
            !saved.contains(INPUT_CANARY),
            "a held input mapped to a variable reaches saved state: {saved}"
        );
        assert!(saved.contains("alien-held:"), "{saved}");
        assert!(
            saved.contains("kept"),
            "values the manager does not hold stay"
        );

        step.restore(Some(&mut state)).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), original);
    }

    #[tokio::test]
    async fn state_reported_by_a_runner_is_redacted_from_what_the_manager_holds() {
        let (_dir, store) = store().await;
        let url = put(&store, "dep_1", "env:API_URL", CANARY).await;
        put(
            &store,
            "dep_1",
            "input:endpoint",
            &format!("\"{INPUT_CANARY}\""),
        )
        .await;
        let deployment = record(vec![slot_ref("env:API_URL", &url)]);
        let mut state = state_running_with(CANARY, INPUT_CANARY);
        let original = serde_json::to_value(&state).unwrap();

        let current = HeldValues::current(
            Some(&store),
            &ManagerConfig::default(),
            &NoReleases,
            &deployment,
        )
        .await
        .unwrap();
        current.redact(Some(&mut state)).unwrap();
        let saved = saved_json(&state);
        assert!(!saved.contains(CANARY), "{saved}");
        assert!(!saved.contains(INPUT_CANARY), "{saved}");

        current.restore(Some(&mut state)).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), original);
    }

    #[tokio::test]
    async fn a_value_changed_while_the_step_ran_is_still_kept_out_of_saved_state() {
        let (_dir, store) = store().await;
        let url = put(&store, "dep_1", "env:API_URL", CANARY).await;
        let endpoint = put(
            &store,
            "dep_1",
            "input:endpoint",
            &format!("\"{INPUT_CANARY}\""),
        )
        .await;
        let deployment = record(vec![
            slot_ref("env:API_URL", &url),
            slot_ref("input:endpoint", &endpoint),
        ]);
        let held = held_record(&store, &deployment).await;
        let config = ManagerConfig::default();
        let stack = deployment.supplied_stacks.as_ref().unwrap().current.clone();
        let step =
            HeldValues::of_step(Some(&store), &config, &deployment, &held, stack.as_ref()).unwrap();

        // The values change on the manager after the step loaded them.
        put(&store, "dep_1", "env:API_URL", "replacement").await;
        store
            .delete(
                &ValueScope::deployment("dep_1").unwrap(),
                &"input:endpoint".parse().unwrap(),
            )
            .await
            .unwrap();

        let mut state = state_running_with(CANARY, INPUT_CANARY);
        step.redact(Some(&mut state)).unwrap();
        let saved = saved_json(&state);
        assert!(!saved.contains(CANARY), "{saved}");
        assert!(!saved.contains(INPUT_CANARY), "{saved}");

        // A report this manager did not watch finds the changed variable by
        // the digest it was planned with.
        let mut reported = state_running_with(CANARY, INPUT_CANARY);
        HeldValues::current(Some(&store), &config, &NoReleases, &deployment)
            .await
            .unwrap()
            .redact(Some(&mut reported))
            .unwrap();
        let saved = saved_json(&reported);
        assert!(!saved.contains(CANARY), "{saved}");
    }

    #[tokio::test]
    async fn another_deployments_values_are_not_redacted() {
        let (_dir, store) = store().await;
        put(&store, "dep_2", "env:API_URL", CANARY).await;
        let deployment = record(Vec::new());
        let mut state = state_running_with(CANARY, INPUT_CANARY);

        HeldValues::current(
            Some(&store),
            &ManagerConfig::default(),
            &NoReleases,
            &deployment,
        )
        .await
        .unwrap()
        .redact(Some(&mut state))
        .unwrap();

        assert!(saved_json(&state).contains(CANARY));
    }

    #[tokio::test]
    async fn without_a_store_state_is_left_alone() {
        let deployment = record(Vec::new());
        let mut state = state_running_with(CANARY, INPUT_CANARY);
        HeldValues::current(None, &ManagerConfig::default(), &NoReleases, &deployment)
            .await
            .unwrap()
            .redact(Some(&mut state))
            .unwrap();
        assert!(saved_json(&state).contains(CANARY));
    }
}
