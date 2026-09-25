use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};

/// Prevents platform-specific Secret mounts from being silently ignored.
pub struct KubernetesSecretMountsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for KubernetesSecretMountsCheck {
    fn description(&self) -> &'static str {
        "Existing Kubernetes Secret mounts require the Kubernetes platform"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform != Platform::Kubernetes
            && stack.resources().any(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<Container>()
                    .is_some_and(|container| !container.kubernetes_secret_mounts.is_empty())
            })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let errors = stack
            .resources()
            .filter_map(|(id, entry)| {
                entry
                    .config
                    .downcast_ref::<Container>()
                    .filter(|container| !container.kubernetes_secret_mounts.is_empty())
                    .map(|_| {
                        format!(
                            "Container '{id}' mounts an existing Kubernetes Secret, but the target platform is {platform}"
                        )
                    })
            })
            .collect::<Vec<_>>();

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ContainerCode, KubernetesSecretMount, Resource, ResourceEntry, ResourceLifecycle,
        ResourceSpec,
    };
    use indexmap::IndexMap;

    #[tokio::test]
    async fn rejects_secret_mounts_on_other_platforms() {
        let container = Container::new("agent".to_string())
            .code(ContainerCode::Image {
                image: "agent:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.05".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "128Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .kubernetes_secret_mount(KubernetesSecretMount {
                secret_name: "agent-token".to_string(),
                mount_path: "/var/run/agent".to_string(),
            })
            .permissions("agent".to_string())
            .build();
        let mut resources = IndexMap::new();
        resources.insert(
            "agent".to_string(),
            ResourceEntry {
                config: Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        let stack = Stack {
            id: "test-stack".to_string(),
            resources,
            permissions: Default::default(),
            supported_platforms: None,
            inputs: vec![],
        };

        assert!(!KubernetesSecretMountsCheck.should_run(&stack, Platform::Kubernetes));
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Local,
        ] {
            assert!(KubernetesSecretMountsCheck.should_run(&stack, platform));
            let result = KubernetesSecretMountsCheck
                .check(&stack, platform)
                .await
                .expect("preflight succeeds");
            assert!(!result.success);
            assert!(result.errors[0].contains("agent"));
        }
    }
}
