use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};

/// Refuse network guarantees on runtimes that cannot enforce them.
pub struct ContainerNetworkSettingsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ContainerNetworkSettingsCheck {
    fn description(&self) -> &'static str {
        "Container network boundaries must be validated and supported by the runtime"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Container>()
                .is_some_and(|container| {
                    container.internal_load_balancer.is_some() || container.internet_only_egress()
                })
        })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        for (id, entry) in stack.resources() {
            let Some(container) = entry.config.downcast_ref::<Container>() else {
                continue;
            };
            if container.internal_load_balancer.is_some() || container.internet_only_egress() {
                errors.push(format!("Container '{id}': internalLoadBalancer and security.egress are Alien-internal settings; only deployment preflight may assign them to the managed sandbox gateway"));
            }
        }
        Ok(if errors.is_empty() {
            CheckResult::success()
        } else {
            CheckResult::failed(errors)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requested_isolation_is_never_silently_ignored() {
        let mut stack: Stack = serde_json::from_value(serde_json::json!({
            "id": "network-test",
            "resources": {"gateway": {
                "config": {"type": "container", "id": "gateway",
                    "code": {"type": "image", "image": "example:latest"},
                    "cpu": {"min": "1", "desired": "1"},
                    "memory": {"min": "128Mi", "desired": "128Mi"},
                    "permissions": "gateway", "links": [], "ports": [{"port": 3128}],
                    "internalLoadBalancer": {"ports": [3128], "network": "default-network",
                        "subnetGroup":"sandbox-gateway", "allowedSourceGroups": {"3128": "sandbox"}}
                }, "lifecycle": "live", "dependencies": []
            }},
            "permissions": {"profiles": {}}
        }))
        .expect("valid stack");
        // A developer cannot bypass the guard by using the reserved gateway ID.
        let entry = stack.resources.remove("gateway").unwrap();
        stack
            .resources
            .insert("sandbox-egress-gateway".into(), entry);
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Local,
        ] {
            assert!(ContainerNetworkSettingsCheck.should_run(&stack, platform));
            let result = ContainerNetworkSettingsCheck
                .check(&stack, platform)
                .await
                .unwrap();
            assert!(
                !result.success,
                "must refuse unsupported isolation on {platform}"
            );
            assert!(
                result
                    .errors
                    .iter()
                    .any(|error| error.contains("Alien-internal"))
            );
        }
    }
}
