//! Environment values of dynamic containers.
//!
//! A dynamic container's variables live in the deployment's scope under
//! `cenv.<containerName>.<NAME>`. Whatever sends a container to its operator
//! leaves them out; the manager adds them from its store on the way out.

use alien_core::sync::TargetDynamicContainer;

use super::{hydrate::load_held, StoredValue, ValueKind, ValueScope, ValueSlot, ValueStore};
use crate::error::{ErrorData, Result};

/// Adds the stored environment values of each container in `targets`.
///
/// A target that names its values in `value_slots` gets exactly those, each checked against
/// the kind and digest the sender planned with; anything else stored for the container is
/// left out. A missing or changed value fails the whole call, so no container starts with a
/// value its sender did not plan. A target without `value_slots` gets every value stored
/// under its prefix, as for senders that predate slot references.
///
/// A plain value goes to `env` and a secret one to `secret_env`. A stored value replaces a
/// variable of the same name the target already carries, in either map. Containers marked
/// deleted are left alone. `value_slots` is removed from every target, so the Operator never
/// receives it.
pub async fn load_container_env(
    store: &ValueStore,
    deployment_id: &str,
    targets: &mut [TargetDynamicContainer],
) -> Result<()> {
    let scope = ValueScope::deployment(deployment_id)?;
    for target in targets.iter_mut() {
        let named = target.value_slots.take();
        if target.deleted {
            continue;
        }
        let values = match named {
            Some(slots) => {
                let mut values = Vec::with_capacity(slots.len());
                for held in &slots {
                    // The owner is checked before the store is read, so a slot of another
                    // container is refused as such, never reported as missing or changed.
                    let slot: ValueSlot = held.slot.parse()?;
                    let name = match slot.container_env_parts() {
                        Some((container, name)) if container == target.name => name.to_string(),
                        _ => {
                            return Err(ErrorData::bad_request(format!(
                                "Value slot '{}' does not belong to container '{}'",
                                held.slot, target.name
                            )));
                        }
                    };
                    let (_, stored) = load_held(store, &scope, held).await?;
                    values.push((name, stored));
                }
                values
            }
            None => stored_under_prefix(store, &scope, &target.name).await?,
        };
        for (name, stored) in values {
            match stored.kind {
                ValueKind::Plain => {
                    target.secret_env.remove(&name);
                    target.env.insert(name, stored.value);
                }
                ValueKind::Secret => {
                    target.env.remove(&name);
                    target.secret_env.insert(name, stored.value);
                }
            }
        }
    }
    Ok(())
}

/// Every value stored for `container`, by variable name.
async fn stored_under_prefix(
    store: &ValueStore,
    scope: &ValueScope,
    container: &str,
) -> Result<Vec<(String, StoredValue)>> {
    // A container named `a` has the prefix of one named `a.b`, so the names are compared
    // exactly after the scan.
    let candidates = store
        .list_prefix(scope, &format!("cenv.{container}."))
        .await?;
    Ok(candidates
        .into_iter()
        .filter_map(|stored| {
            let name = match stored.slot.container_env_parts() {
                Some((owner, name)) if owner == container => name.to_string(),
                _ => return None,
            };
            Some((name, stored))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use alien_core::{EnvironmentVariableType, ValueSlotRef};

    use super::*;
    use crate::values::fixtures::kv_store;

    const DEPLOYMENT: &str = "dep_test";

    fn target(name: &str, value_slots: Option<Vec<ValueSlotRef>>) -> TargetDynamicContainer {
        TargetDynamicContainer {
            name: name.to_string(),
            generation: 1,
            image: "registry.example.com/app@sha256:abc".to_string(),
            cpu: "0.5".to_string(),
            memory: "512Mi".to_string(),
            replicas: 1,
            ports: vec![8080],
            deleted: false,
            env: BTreeMap::new(),
            secret_env: BTreeMap::new(),
            value_slots,
            health_check: None,
            suspended_reason: None,
        }
    }

    /// Stores a value and returns the reference a sender would plan with.
    async fn put(
        store: &ValueStore,
        container: &str,
        name: &str,
        kind: ValueKind,
        value: &str,
    ) -> ValueSlotRef {
        let scope = ValueScope::deployment(DEPLOYMENT).unwrap();
        let slot = ValueSlot::container_env(container, name).unwrap();
        let meta = store
            .put(&scope, &slot, kind, value.to_string())
            .await
            .unwrap();
        ValueSlotRef {
            slot: slot.as_str().to_string(),
            kind: match kind {
                ValueKind::Plain => EnvironmentVariableType::Plain,
                ValueKind::Secret => EnvironmentVariableType::Secret,
            },
            digest: meta.digest,
            env_name: Some(name.to_string()),
            targets: None,
        }
    }

    #[tokio::test]
    async fn named_slots_load_exactly_the_planned_values() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        let token = put(&store, "api", "TOKEN", ValueKind::Secret, "planned").await;
        // Left in the store with nothing naming it: a removal that failed, or a write the
        // control plane refused. It must not reach the container.
        put(&store, "api", "STALE", ValueKind::Secret, "leftover").await;
        let mut targets = vec![target("api", Some(vec![token]))];
        targets[0]
            .secret_env
            .insert("PLATFORM_HELD".to_string(), "from-sender".to_string());

        load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .unwrap();

        assert_eq!(
            targets[0].secret_env,
            BTreeMap::from([
                ("PLATFORM_HELD".to_string(), "from-sender".to_string()),
                ("TOKEN".to_string(), "planned".to_string()),
            ])
        );
        assert!(targets[0].env.is_empty());
        assert_eq!(targets[0].value_slots, None, "never sent to the Operator");
    }

    #[tokio::test]
    async fn an_empty_slot_list_loads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        put(&store, "api", "STALE", ValueKind::Secret, "leftover").await;
        let mut targets = vec![target("api", Some(Vec::new()))];

        load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .unwrap();

        assert!(targets[0].secret_env.is_empty());
        assert!(targets[0].env.is_empty());
    }

    #[tokio::test]
    async fn without_slots_every_stored_value_loads_as_before() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        put(&store, "api", "TOKEN", ValueKind::Secret, "token").await;
        put(&store, "api", "LEVEL", ValueKind::Plain, "debug").await;
        put(&store, "api.worker", "TOKEN", ValueKind::Secret, "worker").await;
        let mut targets = vec![target("api", None)];

        load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .unwrap();

        assert_eq!(
            targets[0].secret_env,
            BTreeMap::from([("TOKEN".to_string(), "token".to_string())])
        );
        assert_eq!(
            targets[0].env,
            BTreeMap::from([("LEVEL".to_string(), "debug".to_string())])
        );
    }

    #[tokio::test]
    async fn a_changed_value_fails_instead_of_starting_the_container() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        let planned = put(&store, "api", "TOKEN", ValueKind::Secret, "planned").await;
        put(&store, "api", "TOKEN", ValueKind::Secret, "replaced").await;
        let mut targets = vec![target("api", Some(vec![planned]))];

        let error = load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .expect_err("a value that changed since it was planned");

        assert_eq!(error.code, "DEPLOYMENT_VALUES_CHANGED");
        assert!(targets[0].secret_env.is_empty());
    }

    #[tokio::test]
    async fn a_changed_kind_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        let mut planned = put(&store, "api", "LEVEL", ValueKind::Plain, "debug").await;
        planned.kind = EnvironmentVariableType::Secret;
        let mut targets = vec![target("api", Some(vec![planned]))];

        let error = load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .expect_err("a plain value planned as a secret");

        assert_eq!(error.code, "DEPLOYMENT_VALUES_CHANGED");
    }

    #[tokio::test]
    async fn a_missing_value_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        let planned = put(&store, "api", "TOKEN", ValueKind::Secret, "planned").await;
        let scope = ValueScope::deployment(DEPLOYMENT).unwrap();
        store
            .delete(&scope, &planned.slot.parse().unwrap())
            .await
            .unwrap();
        let mut targets = vec![target("api", Some(vec![planned]))];

        let error = load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .expect_err("a planned value that is gone");

        assert_eq!(error.code, "DEPLOYMENT_VALUE_MISSING");
    }

    #[tokio::test]
    async fn a_slot_of_another_container_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        let neighbour = put(&store, "api.worker", "TOKEN", ValueKind::Secret, "worker").await;
        let mut targets = vec![target("api", Some(vec![neighbour]))];

        let error = load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .expect_err("a slot under another container's name");

        assert_eq!(error.http_status_code, Some(400));
        assert!(targets[0].secret_env.is_empty());
    }

    #[tokio::test]
    async fn a_missing_slot_of_another_container_is_refused_as_foreign() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        let neighbour = put(&store, "api.worker", "TOKEN", ValueKind::Secret, "worker").await;
        let scope = ValueScope::deployment(DEPLOYMENT).unwrap();
        store
            .delete(&scope, &neighbour.slot.parse().unwrap())
            .await
            .unwrap();
        let mut targets = vec![target("api", Some(vec![neighbour]))];

        let error = load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .expect_err("a slot under another container's name");

        assert_eq!(error.http_status_code, Some(400));
        assert_ne!(error.code, "DEPLOYMENT_VALUE_MISSING");
    }

    #[tokio::test]
    async fn a_deleted_container_gets_nothing_and_loses_its_slots() {
        let dir = tempfile::tempdir().unwrap();
        let store = kv_store(dir.path()).await;
        let planned = put(&store, "api", "TOKEN", ValueKind::Secret, "planned").await;
        let mut gone = target("api", Some(vec![planned]));
        gone.deleted = true;
        let mut targets = vec![gone];

        load_container_env(&store, DEPLOYMENT, &mut targets)
            .await
            .unwrap();

        assert!(targets[0].secret_env.is_empty());
        assert_eq!(targets[0].value_slots, None);
    }

    #[test]
    fn a_target_without_the_field_reads_as_none_and_an_empty_list_as_some() {
        let mut json = serde_json::to_value(target("api", None)).unwrap();
        assert!(json.get("valueSlots").is_none());
        let parsed: TargetDynamicContainer = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(parsed.value_slots, None);

        json["valueSlots"] = serde_json::json!([]);
        let parsed: TargetDynamicContainer = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.value_slots, Some(Vec::new()));
    }
}
