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
                    container.internal_load_balancer.is_some() || container.internet_only_egress
                })
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        for (id, entry) in stack.resources() {
            let Some(container) = entry.config.downcast_ref::<Container>() else {
                continue;
            };
            if let Err(error) = container.validate_network_settings() {
                errors.push(error.to_string());
            }
            if (container.internal_load_balancer.is_some() || container.internet_only_egress)
                && platform != Platform::Aws
            {
                errors.push(format!(
                    "Container '{id}': internalLoadBalancer and internetOnlyEgress require the AWS container runtime; {platform} has no implementation for these fields, refusing to deploy without the requested network isolation"
                ));
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
        let stack: Stack = serde_json::from_value(serde_json::json!({
            "id": "network-test",
            "resources": {"gateway": {
                "config": {"type": "container", "id": "gateway",
                    "code": {"type": "image", "image": "example:latest"},
                    "cpu": {"min": "1", "desired": "1"},
                    "memory": {"min": "128Mi", "desired": "128Mi"},
                    "permissions": "gateway", "links": [], "ports": [{"port": 3128}],
                    "internalLoadBalancer": {"ports": [3128], "subnets": ["subnet-1"],
                        "allowedSources": {"3128": ["10.2.0.0/24"]}}
                }, "lifecycle": "live", "dependencies": []
            }},
            "permissions": {"profiles": {}}
        }))
        .expect("valid stack");
        assert!(
            ContainerNetworkSettingsCheck
                .check(&stack, Platform::Aws)
                .await
                .unwrap()
                .success
        );
        for platform in [
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
            assert!(result
                .errors
                .iter()
                .any(|error| error.contains("refusing to deploy")));
        }
    }
}
