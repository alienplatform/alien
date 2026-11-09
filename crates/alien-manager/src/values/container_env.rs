//! Environment values of dynamic containers.
//!
//! A dynamic container's variables live in the deployment's scope under
//! `cenv.<containerName>.<NAME>`. Whatever sends a container to its operator
//! leaves them out; the manager adds them from its store on the way out.

use alien_core::sync::TargetDynamicContainer;

use super::{ValueKind, ValueScope, ValueStore};
use crate::error::Result;

/// Adds the stored environment values of each container in `targets`.
///
/// A plain value goes to `env` and a secret one to `secret_env`. A stored
/// value replaces a variable of the same name the target already carries, in
/// either map. Containers marked deleted are left alone.
pub async fn load_container_env(
    store: &ValueStore,
    deployment_id: &str,
    targets: &mut [TargetDynamicContainer],
) -> Result<()> {
    let scope = ValueScope::deployment(deployment_id)?;
    for target in targets.iter_mut().filter(|target| !target.deleted) {
        // A container named `a` has the prefix of one named `a.b`, so the
        // names are compared exactly after the scan.
        let candidates = store
            .list_prefix(&scope, &format!("cenv.{}.", target.name))
            .await?;
        for stored in candidates {
            let Some((container, name)) = stored.slot.container_env_parts() else {
                continue;
            };
            if container != target.name {
                continue;
            }
            let (name, value) = (name.to_string(), stored.value);
            match stored.kind {
                ValueKind::Plain => {
                    target.secret_env.remove(&name);
                    target.env.insert(name, value);
                }
                ValueKind::Secret => {
                    target.env.remove(&name);
                    target.secret_env.insert(name, value);
                }
            }
        }
    }
    Ok(())
}
