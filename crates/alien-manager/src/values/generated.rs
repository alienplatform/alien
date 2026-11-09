//! Secret inputs the manager generates: the `generate` rule on a stack input.
//!
//! The first step that needs a generated input's value creates it, with the
//! operating system's CSPRNG, and keeps it in the deployment's value scope
//! like any other secret. The slot is written only while it is empty, so a
//! value is never replaced, and two steps that start together end up with the
//! same one. Other services learn that the slot exists, never its value.

use std::borrow::Cow;
use std::collections::HashMap;

use alien_core::{
    Stack, StackInputDefinition, StackInputGenerate, STACK_INPUT_GENERATE_MAX_LENGTH,
    STACK_INPUT_GENERATE_MIN_LENGTH,
};
use alien_error::AlienError;
use rand::distr::{Alphanumeric, SampleString};
use rand::rngs::OsRng;
use rand::TryRngCore;

use super::{ValueKind, ValueMeta, ValueScope, ValueSlot, ValueStore};
use crate::error::{ErrorData, Result};
use crate::traits::deployment_store::DeploymentRecord;

/// The deployment's generated secret inputs.
#[derive(Debug, Default, Clone)]
pub struct GeneratedInputs {
    /// Metadata of every generated slot the store holds, for the step report.
    pub slots: Vec<ValueMeta>,
}

fn generate_value(generate: &StackInputGenerate) -> Result<String> {
    let length = generate.length;
    if !(STACK_INPUT_GENERATE_MIN_LENGTH..=STACK_INPUT_GENERATE_MAX_LENGTH).contains(&length) {
        return Err(AlienError::new(ErrorData::InternalError {
            message: format!(
                "Generated input length {length} is outside {STACK_INPUT_GENERATE_MIN_LENGTH}..={STACK_INPUT_GENERATE_MAX_LENGTH}"
            ),
        }));
    }
    Ok(Alphanumeric.sample_string(&mut OsRng.unwrap_err(), length as usize))
}

/// The inputs of `stack` this manager generates for `deployment`: generated,
/// applicable, and without a value already on the record.
fn inputs_to_generate<'a>(
    stack: &'a Stack,
    deployment: &DeploymentRecord,
) -> Vec<&'a StackInputDefinition> {
    let supplied = deployment
        .deployment_config
        .as_ref()
        .map(|config| &config.input_values);
    let record_values = deployment.input_values.clone();
    let platform = deployment.platform;
    stack
        .inputs
        .iter()
        .filter(|input| {
            input.is_generated()
                && input.applies(&stack.inputs, &record_values, platform)
                && !record_values.contains_key(&input.id)
                && !supplied.is_some_and(|values| values.contains_key(&input.id))
        })
        .collect()
}

/// The deployment with its generated secret inputs filled in, and their
/// metadata.
///
/// A generated input without a value anywhere is generated now and stored,
/// JSON-encoded as a string like every `input:` slot. One that already has a
/// stored value keeps it. A step that loses the race to write the slot adopts
/// the value the winner stored. Without a value store, or without a stack,
/// the deployment comes back unchanged.
pub async fn with_generated_inputs<'a>(
    store: Option<&ValueStore>,
    stack: Option<&Stack>,
    deployment: Cow<'a, DeploymentRecord>,
) -> Result<(Cow<'a, DeploymentRecord>, GeneratedInputs)> {
    let (Some(store), Some(stack)) = (store, stack) else {
        return Ok((deployment, GeneratedInputs::default()));
    };
    let wanted = inputs_to_generate(stack, &deployment);
    if wanted.is_empty() {
        return Ok((deployment, GeneratedInputs::default()));
    }
    let scope = ValueScope::deployment(&deployment.id)?;

    let mut values = HashMap::new();
    let mut slots = Vec::new();
    for input in wanted {
        let slot: ValueSlot = format!("input:{}", input.id).parse()?;
        let stored = match store.get(&scope, &slot).await? {
            Some(stored) => stored,
            None => {
                let generate = input
                    .generate
                    .as_ref()
                    .expect("a generated input has a generate rule");
                let encoded =
                    serde_json::to_string(&generate_value(generate)?).expect("a string serializes");
                store
                    .put_if_absent(&scope, &slot, ValueKind::Secret, encoded)
                    .await?;
                store.get(&scope, &slot).await?.ok_or_else(|| {
                    AlienError::new(ErrorData::DeploymentValueMissing {
                        deployment_id: deployment.id.clone(),
                        slot: slot.to_string(),
                    })
                })?
            }
        };
        let value: serde_json::Value = serde_json::from_str(&stored.value).map_err(|_| {
            ErrorData::bad_request(format!("The value stored for '{slot}' is not JSON text"))
        })?;
        slots.push(stored.meta(&scope)?);
        values.insert(input.id.clone(), value);
    }

    let mut filled = deployment.into_owned();
    for (id, value) in values {
        if let Some(config) = filled.deployment_config.as_mut() {
            config.input_values.insert(id.clone(), value.clone());
        }
        filled.input_values.insert(id, value);
    }
    Ok((Cow::Owned(filled), GeneratedInputs { slots }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_core::{
        DeploymentConfig, EnvironmentVariableType, EnvironmentVariablesSnapshot, Platform,
        StackInputKind, StackInputProvider, ValueSlotRef,
    };
    use chrono::Utc;

    use super::*;

    fn input(
        id: &str,
        generate: Option<u32>,
        platforms: Option<Vec<Platform>>,
    ) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind: StackInputKind::Secret,
            when: None,
            provided_by: vec![StackInputProvider::Developer],
            required: true,
            label: id.to_string(),
            description: id.to_string(),
            placeholder: None,
            default: None,
            platforms,
            validation: None,
            generate: generate.map(|length| StackInputGenerate { length }),
            env: vec![],
        }
    }

    fn stack(inputs: Vec<StackInputDefinition>) -> Stack {
        let mut stack = Stack::new("app".to_string()).build();
        stack.inputs = inputs;
        stack
    }

    fn record() -> DeploymentRecord {
        let config = DeploymentConfig::builder()
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

    fn scope() -> ValueScope {
        ValueScope::deployment("dep_1").unwrap()
    }

    fn signing_key_slot() -> ValueSlot {
        "input:signingKey".parse().unwrap()
    }

    async fn generate(
        store: &ValueStore,
        stack: &Stack,
        record: &DeploymentRecord,
    ) -> (DeploymentRecord, GeneratedInputs) {
        let (filled, generated) =
            with_generated_inputs(Some(store), Some(stack), Cow::Borrowed(record))
                .await
                .unwrap();
        (filled.into_owned(), generated)
    }

    #[tokio::test]
    async fn the_first_step_stores_a_json_string_secret_of_the_declared_length() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::values::fixtures::kv_store(dir.path()).await;
        let stack = stack(vec![input("signingKey", Some(64), None)]);

        let (filled, generated) = generate(&store, &stack, &record()).await;

        let stored = store
            .get(&scope(), &signing_key_slot())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.kind, ValueKind::Secret);
        let value: String = serde_json::from_str(&stored.value).expect("a JSON string");
        assert_eq!(value.len(), 64);
        assert!(value.bytes().all(|byte| byte.is_ascii_alphanumeric()));
        assert_eq!(filled.input_values["signingKey"], value);
        assert_eq!(
            filled.deployment_config.as_ref().unwrap().input_values["signingKey"],
            value
        );
        assert_eq!(generated.slots, vec![stored.meta(&scope()).unwrap()]);
        assert_eq!(generated.slots[0].kind, ValueKind::Secret);
        let reported = serde_json::to_string(&generated.slots).unwrap();
        assert!(!reported.contains(&value), "{reported}");
    }

    #[tokio::test]
    async fn a_second_step_keeps_the_value_and_reports_the_same_slot() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::values::fixtures::kv_store(dir.path()).await;
        let stack = stack(vec![input("signingKey", Some(64), None)]);

        let (first, first_report) = generate(&store, &stack, &record()).await;
        let (second, second_report) = generate(&store, &stack, &record()).await;

        assert_eq!(
            first.input_values["signingKey"],
            second.input_values["signingKey"]
        );
        assert_eq!(first_report.slots, second_report.slots);
        assert_eq!(
            store.list(&scope()).await.unwrap().len(),
            1,
            "one slot, written once"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_first_steps_end_with_one_value() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(crate::values::fixtures::kv_store(dir.path()).await);
        let stack = Arc::new(stack(vec![input("signingKey", Some(64), None)]));
        let record = Arc::new(record());

        let steps: Vec<_> = (0..16)
            .map(|_| {
                let (store, stack, record) = (store.clone(), stack.clone(), record.clone());
                tokio::spawn(async move {
                    let (filled, generated) =
                        with_generated_inputs(Some(&store), Some(&stack), Cow::Borrowed(&*record))
                            .await
                            .unwrap();
                    (filled.input_values["signingKey"].clone(), generated.slots)
                })
            })
            .collect();
        let mut results = Vec::new();
        for step in steps {
            results.push(step.await.unwrap());
        }

        let stored = store
            .get(&scope(), &signing_key_slot())
            .await
            .unwrap()
            .unwrap();
        let winner: serde_json::Value = serde_json::from_str(&stored.value).unwrap();
        for (value, slots) in &results {
            assert_eq!(value, &winner, "every step runs with the stored value");
            assert_eq!(slots, &vec![stored.meta(&scope()).unwrap()]);
        }
        assert_eq!(store.list(&scope()).await.unwrap().len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn the_sqlite_backend_keeps_the_first_of_concurrent_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            crate::stores::sqlite::SqliteDatabase::new(&dir.path().join("m.db").to_string_lossy())
                .await
                .unwrap(),
        );
        let store = Arc::new(ValueStore::new(Arc::new(
            crate::stores::sqlite::SqliteValueBackend::new(db),
        )));
        alien_deployment::set_ephemeral_secrets_digest_key();

        let writes: Vec<_> = (0..16)
            .map(|n| {
                let store = store.clone();
                tokio::spawn(async move {
                    store
                        .put_if_absent(
                            &scope(),
                            &signing_key_slot(),
                            ValueKind::Secret,
                            format!("\"writer-{n}\""),
                        )
                        .await
                        .unwrap()
                        .is_some()
                })
            })
            .collect();
        let mut winners = 0;
        for write in writes {
            winners += usize::from(write.await.unwrap());
        }

        assert_eq!(winners, 1, "exactly one write lands");
        let stored = store
            .get(&scope(), &signing_key_slot())
            .await
            .unwrap()
            .unwrap();
        assert!(stored.value.starts_with("\"writer-"));
    }

    #[tokio::test]
    async fn an_existing_value_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::values::fixtures::kv_store(dir.path()).await;
        let stack = stack(vec![input("signingKey", Some(64), None)]);
        store
            .put(
                &scope(),
                &signing_key_slot(),
                ValueKind::Secret,
                "\"set-by-the-developer\"".to_string(),
            )
            .await
            .unwrap();

        let (filled, _) = generate(&store, &stack, &record()).await;

        assert_eq!(filled.input_values["signingKey"], "set-by-the-developer");
        let stored = store
            .get(&scope(), &signing_key_slot())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.value, "\"set-by-the-developer\"");
    }

    #[tokio::test]
    async fn only_generated_applicable_inputs_without_a_value_are_generated() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::values::fixtures::kv_store(dir.path()).await;
        let stack = stack(vec![
            input("signingKey", Some(32), None),
            input("providedKey", Some(32), None),
            input("gcpOnly", Some(32), Some(vec![Platform::Gcp])),
            input("apiToken", None, None),
        ]);
        let mut record = record();
        record.input_values.insert(
            "providedKey".to_string(),
            serde_json::json!("from-the-record"),
        );

        let (filled, generated) = generate(&store, &stack, &record).await;

        let slots: Vec<_> = generated.slots.iter().map(|m| m.slot.to_string()).collect();
        assert_eq!(slots, ["input:signingKey"]);
        assert_eq!(filled.input_values["providedKey"], "from-the-record");
        assert!(!filled.input_values.contains_key("gcpOnly"));
        assert!(!filled.input_values.contains_key("apiToken"));
        let stored: Vec<_> = store
            .list(&scope())
            .await
            .unwrap()
            .iter()
            .map(|v| v.slot.to_string())
            .collect();
        assert_eq!(stored, ["input:signingKey"]);
    }

    /// A deployment created before its manager stored values got its generated
    /// secrets from the sender, which keeps delivering them with each update.
    /// When the manager starts storing values, its first step must not generate
    /// a second value for any of them: the copy of the sender's value has to be
    /// the one that lands in the empty slot.
    #[tokio::test]
    async fn secrets_the_sender_still_delivers_are_not_generated_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::values::fixtures::kv_store(dir.path()).await;
        let ids: Vec<String> = (0..16).map(|n| format!("secret{n}")).collect();
        let mut inputs: Vec<_> = ids.iter().map(|id| input(id, Some(48), None)).collect();
        inputs.push(input("added", Some(32), None));
        inputs.push(input("held", Some(32), None));
        let stack = stack(inputs);
        let mut record = record();
        let config = record.deployment_config.as_mut().unwrap();
        // The sender stores the 16 answers and `held`. Only `held` is in the
        // manager, so only it has a slot reference.
        let mut stored = ids.clone();
        stored.push("held".to_string());
        config.stored_secret_input_ids = Some(stored);
        config.value_slots = vec![ValueSlotRef {
            slot: "input:held".to_string(),
            kind: EnvironmentVariableType::Secret,
            digest: String::new(),
            env_name: None,
            targets: None,
        }];

        let (filled, generated) = generate(&store, &stack, &record).await;

        // `added` came with a later release and the sender stores nothing for
        // it; `held` is the manager's own. Both are generated as before.
        let slots: Vec<_> = generated.slots.iter().map(|m| m.slot.to_string()).collect();
        assert_eq!(slots, ["input:added", "input:held"]);
        for id in &ids {
            assert!(!filled.input_values.contains_key(id), "{id} was filled");
        }
        // The migration's copy of each sender value lands in an empty slot.
        let scope = scope();
        for id in &ids {
            let slot: ValueSlot = format!("input:{id}").parse().unwrap();
            let copied = store
                .put_if_absent(&scope, &slot, ValueKind::Secret, format!("\"sender-{id}\""))
                .await
                .unwrap();
            assert!(copied.is_some(), "{id}: the manager already held a value");
        }
        assert_eq!(store.list(&scope).await.unwrap().len(), 18);
    }

    #[tokio::test]
    async fn a_sender_that_lists_no_stored_secrets_gets_them_generated() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::values::fixtures::kv_store(dir.path()).await;
        let stack = stack(vec![input("signingKey", Some(32), None)]);
        let mut record = record();
        record
            .deployment_config
            .as_mut()
            .unwrap()
            .stored_secret_input_ids = Some(Vec::new());

        let (_, generated) = generate(&store, &stack, &record).await;

        let slots: Vec<_> = generated.slots.iter().map(|m| m.slot.to_string()).collect();
        assert_eq!(slots, ["input:signingKey"]);
    }

    #[tokio::test]
    async fn a_length_outside_the_allowed_range_fails_without_storing() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::values::fixtures::kv_store(dir.path()).await;

        for length in [
            STACK_INPUT_GENERATE_MIN_LENGTH - 1,
            STACK_INPUT_GENERATE_MAX_LENGTH + 1,
        ] {
            let stack = stack(vec![input("signingKey", Some(length), None)]);
            let error = with_generated_inputs(Some(&store), Some(&stack), Cow::Owned(record()))
                .await
                .expect_err("the length is refused");
            assert!(
                error.message.contains(&length.to_string()),
                "{}",
                error.message
            );
        }
        assert!(store.list(&scope()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn without_a_store_the_deployment_is_unchanged() {
        let stack = stack(vec![input("signingKey", Some(64), None)]);
        let (filled, generated) = with_generated_inputs(None, Some(&stack), Cow::Owned(record()))
            .await
            .unwrap();
        assert!(filled.input_values.is_empty());
        assert!(generated.slots.is_empty());
    }
}
