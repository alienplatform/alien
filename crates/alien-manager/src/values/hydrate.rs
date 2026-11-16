use std::borrow::Cow;

use alien_core::{EnvironmentVariable, EnvironmentVariableType, ValueSlotRef};
use alien_error::AlienError;

use super::{constant_time_eq, StoredValue, ValueKind, ValueScope, ValueSlot, ValueStore};
use crate::error::{ErrorData, Result};
use crate::traits::deployment_store::DeploymentRecord;

/// The deployment with the values it names in `valueSlots` loaded from `store`.
///
/// A deployment whose config carries no `valueSlots` comes back unchanged.
/// Otherwise each slot's stored value must still match the digest the sender
/// planned with, and its kind must match. A missing or changed value fails the
/// whole deployment: nothing runs with a value the sender did not plan.
/// Loaded variables replace same-named ones the record carries; loaded inputs
/// are added to the record's and the config's `input_values`.
pub async fn hydrate_held_values<'a>(
    store: Option<&ValueStore>,
    deployment: &'a DeploymentRecord,
) -> Result<Cow<'a, DeploymentRecord>> {
    let slots = match &deployment.deployment_config {
        Some(config) if !config.value_slots.is_empty() => config.value_slots.as_slice(),
        _ => return Ok(Cow::Borrowed(deployment)),
    };
    let store = store.ok_or_else(|| {
        AlienError::new(ErrorData::ValuesUnavailable {
            reason: format!(
                "deployment '{}' keeps values on its manager, but this manager has no value store",
                deployment.id
            ),
        })
    })?;
    let scope = ValueScope::deployment(&deployment.id)?;

    let mut variables = Vec::new();
    let mut inputs = Vec::new();
    for held in slots {
        let (slot, stored) = load_held(store, &scope, held).await?;
        if let Some(name) = slot.env_name() {
            variables.push(EnvironmentVariable {
                name: name.to_string(),
                value: stored.value,
                var_type: held.kind,
                target_resources: held.targets.clone(),
            });
        } else if let Some(id) = slot.input_id() {
            let value: serde_json::Value = serde_json::from_str(&stored.value).map_err(|_| {
                ErrorData::bad_request(format!(
                    "The value stored for '{}' is not JSON text",
                    held.slot
                ))
            })?;
            inputs.push((id.to_string(), value));
        }
    }

    let mut hydrated = deployment.clone();
    let existing = hydrated
        .user_environment_variables
        .get_or_insert_with(Vec::new);
    existing.retain(|have| variables.iter().all(|held| held.name != have.name));
    existing.extend(variables);
    for (id, value) in inputs {
        if let Some(config) = hydrated.deployment_config.as_mut() {
            config.input_values.insert(id.clone(), value.clone());
        }
        hydrated.input_values.insert(id, value);
    }
    Ok(Cow::Owned(hydrated))
}

/// The value `held` names, loaded from `store`.
///
/// Fails with `DEPLOYMENT_VALUE_MISSING` when nothing is stored for the slot, and with
/// `DEPLOYMENT_VALUES_CHANGED` when the stored kind or digest differs from what the
/// sender planned with.
pub(super) async fn load_held(
    store: &ValueStore,
    scope: &ValueScope,
    held: &ValueSlotRef,
) -> Result<(ValueSlot, StoredValue)> {
    let slot: ValueSlot = held.slot.parse()?;
    let stored = store.get(scope, &slot).await?.ok_or_else(|| {
        AlienError::new(ErrorData::DeploymentValueMissing {
            deployment_id: scope.id().to_string(),
            slot: held.slot.clone(),
        })
    })?;
    let expected_kind = match held.kind {
        EnvironmentVariableType::Plain => ValueKind::Plain,
        EnvironmentVariableType::Secret => ValueKind::Secret,
    };
    if stored.kind != expected_kind
        || !constant_time_eq(stored.digest(scope)?.as_bytes(), held.digest.as_bytes())
    {
        return Err(AlienError::new(ErrorData::DeploymentValuesChanged {
            deployment_id: scope.id().to_string(),
            slot: held.slot.clone(),
        }));
    }
    Ok((slot, stored))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_bindings::providers::kv::local::LocalKv;
    use alien_core::{DeploymentConfig, EnvironmentVariablesSnapshot, Platform};
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::values::cipher::fixtures::sealed_backend;

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

    fn config(value_slots: Vec<ValueSlotRef>) -> DeploymentConfig {
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
        config.value_slots = value_slots;
        config
    }

    fn record(value_slots: Vec<ValueSlotRef>) -> DeploymentRecord {
        DeploymentRecord {
            supplied_stacks: None,
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
            current_release_id: None,
            desired_release_id: None,
            configuration_revision: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: Some(vec![
                EnvironmentVariable {
                    name: "KEPT".to_string(),
                    value: "kept".to_string(),
                    var_type: EnvironmentVariableType::Plain,
                    target_resources: None,
                },
                EnvironmentVariable {
                    name: "API_TOKEN".to_string(),
                    value: "stale".to_string(),
                    var_type: EnvironmentVariableType::Secret,
                    target_resources: None,
                },
            ]),
            management_config: None,
            deployment_config: Some(config(value_slots)),
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

    fn slot_ref(slot: &str, kind: EnvironmentVariableType, digest: &str) -> ValueSlotRef {
        ValueSlotRef {
            slot: slot.to_string(),
            kind,
            digest: digest.to_string(),
            env_name: None,
            targets: None,
        }
    }

    async fn put(store: &ValueStore, slot: &str, kind: ValueKind, value: &str) -> String {
        let scope = ValueScope::deployment("dep_1").unwrap();
        store
            .put(&scope, &slot.parse().unwrap(), kind, value.to_string())
            .await
            .unwrap()
            .digest
    }

    #[tokio::test]
    async fn a_deployment_without_value_slots_is_returned_untouched() {
        let mut deployment = record(Vec::new());
        deployment.deployment_config = None;
        let held = hydrate_held_values(None, &deployment).await.unwrap();
        assert!(matches!(held, Cow::Borrowed(_)));

        let deployment = record(Vec::new());
        let held = hydrate_held_values(None, &deployment).await.unwrap();
        assert!(matches!(held, Cow::Borrowed(_)));
    }

    #[tokio::test]
    async fn loads_variables_and_inputs_that_match_their_digests() {
        let (_dir, store) = store().await;
        let token = put(&store, "env:API_TOKEN", ValueKind::Secret, "s3cret").await;
        let region = put(&store, "env:REGION", ValueKind::Plain, "eu").await;
        let size = put(&store, "input:size", ValueKind::Plain, "{\"n\":3}").await;
        let mut region_ref = slot_ref("env:REGION", EnvironmentVariableType::Plain, &region);
        region_ref.targets = Some(vec!["api".to_string()]);
        let deployment = record(vec![
            slot_ref("env:API_TOKEN", EnvironmentVariableType::Secret, &token),
            region_ref,
            slot_ref("input:size", EnvironmentVariableType::Plain, &size),
        ]);

        let held = hydrate_held_values(Some(&store), &deployment)
            .await
            .unwrap();

        let variables = held.user_environment_variables.as_ref().unwrap();
        let by_name = |name: &str| variables.iter().find(|v| v.name == name).unwrap();
        assert_eq!(variables.len(), 3, "KEPT stays, API_TOKEN is replaced once");
        assert_eq!(by_name("KEPT").value, "kept");
        assert_eq!(by_name("API_TOKEN").value, "s3cret");
        assert_eq!(
            by_name("API_TOKEN").var_type,
            EnvironmentVariableType::Secret
        );
        assert_eq!(by_name("REGION").value, "eu");
        assert_eq!(
            by_name("REGION").target_resources,
            Some(vec!["api".to_string()])
        );
        assert_eq!(held.input_values["size"], json!({"n": 3}));
        assert_eq!(
            held.deployment_config.as_ref().unwrap().input_values["size"],
            json!({"n": 3})
        );
        // The caller's record is not changed.
        assert_eq!(
            deployment.user_environment_variables.as_ref().unwrap()[1].value,
            "stale"
        );
        assert!(deployment.input_values.is_empty());
    }

    #[tokio::test]
    async fn a_value_changed_after_planning_refuses_the_step() {
        let (_dir, store) = store().await;
        let planned = put(&store, "env:API_TOKEN", ValueKind::Secret, "first-s3cret-A").await;
        put(
            &store,
            "env:API_TOKEN",
            ValueKind::Secret,
            "second-s3cret-B",
        )
        .await;
        let deployment = record(vec![slot_ref(
            "env:API_TOKEN",
            EnvironmentVariableType::Secret,
            &planned,
        )]);

        let error = hydrate_held_values(Some(&store), &deployment)
            .await
            .expect_err("a changed value must not run");

        assert_eq!(error.code, "DEPLOYMENT_VALUES_CHANGED");
        assert_eq!(error.http_status_code, Some(409));
        assert!(error.message.contains("env:API_TOKEN"), "{}", error.message);
        assert!(!error.message.contains("second-s3cret-B"));
        assert!(!error.message.contains("first-s3cret-A"));
    }

    #[tokio::test]
    async fn a_value_that_switched_kind_refuses_the_step() {
        let (_dir, store) = store().await;
        let digest = put(&store, "env:LEVEL", ValueKind::Plain, "debug").await;
        let deployment = record(vec![slot_ref(
            "env:LEVEL",
            EnvironmentVariableType::Secret,
            &digest,
        )]);

        let error = hydrate_held_values(Some(&store), &deployment)
            .await
            .expect_err("a kind change must not run");

        assert_eq!(error.code, "DEPLOYMENT_VALUES_CHANGED");
    }

    #[tokio::test]
    async fn a_secret_relabelled_as_plain_is_refused() {
        let (_dir, store) = store().await;
        let digest = put(&store, "env:X", ValueKind::Secret, "relabel-canary-s3cret").await;
        // The digest does not cover the kind, so only the kind check stops this.
        let deployment = record(vec![slot_ref(
            "env:X",
            EnvironmentVariableType::Plain,
            &digest,
        )]);

        let error = hydrate_held_values(Some(&store), &deployment)
            .await
            .expect_err("a secret claimed as plain must not run");

        assert_eq!(error.code, "DEPLOYMENT_VALUES_CHANGED");
        assert!(!format!("{error:?}").contains("relabel-canary-s3cret"));
        assert!(!error.to_string().contains("relabel-canary-s3cret"));
    }

    #[tokio::test]
    async fn a_missing_value_refuses_the_step() {
        let (_dir, store) = store().await;
        let deployment = record(vec![slot_ref(
            "env:API_TOKEN",
            EnvironmentVariableType::Secret,
            &"0".repeat(64),
        )]);

        let error = hydrate_held_values(Some(&store), &deployment)
            .await
            .expect_err("a missing value must not run");

        assert_eq!(error.code, "DEPLOYMENT_VALUE_MISSING");
        assert_eq!(error.http_status_code, Some(409));
        assert!(error.message.contains("env:API_TOKEN"), "{}", error.message);
    }

    #[tokio::test]
    async fn a_manager_without_a_store_refuses_a_deployment_that_names_slots() {
        let deployment = record(vec![slot_ref(
            "env:API_TOKEN",
            EnvironmentVariableType::Secret,
            &"0".repeat(64),
        )]);

        let error = hydrate_held_values(None, &deployment)
            .await
            .expect_err("no store means no values");

        assert_eq!(error.code, "VALUES_UNAVAILABLE");
    }

    #[tokio::test]
    async fn an_input_that_is_not_json_text_is_rejected() {
        let (_dir, store) = store().await;
        let digest = put(&store, "input:size", ValueKind::Plain, "not json").await;
        let deployment = record(vec![slot_ref(
            "input:size",
            EnvironmentVariableType::Plain,
            &digest,
        )]);

        hydrate_held_values(Some(&store), &deployment)
            .await
            .expect_err("input values are stored as JSON text");
    }
}
