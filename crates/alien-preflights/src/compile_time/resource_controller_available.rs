//! Every resource needs a controller, or a path to an external binding, on the
//! platform it is released for.
//!
//! Without this check a release passes and the deployment fails during setup,
//! when the platform has nothing that can create the resource. On Kubernetes
//! Alien provisions only the resources below; the data-layer resources
//! (Storage, Queue, KV, Postgres, Redis, AI, artifact registries) come from
//! external bindings the installer provides, and the rest have no Kubernetes
//! implementation.

use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Platform, Stack};

/// Resource types with a Kubernetes controller (`alien-infra` registry).
/// `alien-infra` has a test that keeps this list equal to the registry.
pub const KUBERNETES_CONTROLLER_TYPES: &[&str] = &[
    "build",
    "compute-cluster",
    "container",
    "daemon",
    "kubernetes-cluster",
    "sandbox",
    "service-account",
    "vault",
    "worker",
];

/// Resource types an installer can bind to existing infrastructure with an
/// [`alien_core::ExternalBinding`].
pub const EXTERNAL_BINDING_TYPES: &[&str] = &[
    "storage",
    "queue",
    "kv",
    "artifact-registry",
    "vault",
    "postgres",
    "redis",
    "ai",
];

pub struct ResourceControllerAvailableCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ResourceControllerAvailableCheck {
    fn description(&self) -> &'static str {
        "Every resource has a controller or an external-binding path on the target platform"
    }

    fn should_run(&self, _stack: &Stack, platform: Platform) -> bool {
        // Kubernetes is the one target whose controller list is complete in
        // this repository, so it is the only one checked for now.
        platform == Platform::Kubernetes
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();
        for (id, entry) in stack.resources() {
            let resource_type = entry.config.resource_type();
            let kind = resource_type.as_ref();
            if KUBERNETES_CONTROLLER_TYPES.contains(&kind) || EXTERNAL_BINDING_TYPES.contains(&kind)
            {
                continue;
            }
            result.add_error(format!(
                "Resource '{id}' is a {kind}, which is not supported on {platform}: nothing \
                 there can create it, and it has no external binding. Remove it from the \
                 stack, or release this stack for a platform that supports {kind}."
            ));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Key, ResourceLifecycle, Storage};

    #[tokio::test]
    async fn a_key_on_kubernetes_is_refused_at_release_time() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                Storage::new("data".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Key::new("secrets-key".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let check = ResourceControllerAvailableCheck;
        assert!(check.should_run(&stack, Platform::Kubernetes));
        let result = check.check(&stack, Platform::Kubernetes).await.unwrap();
        assert!(!result.success, "{result:?}");
        assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
        assert!(
            result.errors[0].contains("secrets-key"),
            "{:?}",
            result.errors
        );
        assert!(!check.should_run(&stack, Platform::Aws));
    }
}
