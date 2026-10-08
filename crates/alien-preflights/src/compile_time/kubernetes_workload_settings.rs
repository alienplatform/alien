use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};

/// Prevents Kubernetes-only workload settings from being silently ignored.
pub struct KubernetesWorkloadSettingsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for KubernetesWorkloadSettingsCheck {
    fn description(&self) -> &'static str {
        "Kubernetes workload settings require the Kubernetes platform"
    }

    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform != Platform::Kubernetes
            && stack.resources().any(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<Container>()
                    .is_some_and(|container| {
                        !container.kubernetes_secret_mounts.is_empty()
                            || container.kubernetes_liveness_probe.is_some()
                            || container.kubernetes_readiness_probe.is_some()
                            || container.security.is_some()
                    })
            })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let errors = stack
            .resources()
            .filter_map(|(id, entry)| {
                entry
                    .config
                    .downcast_ref::<Container>()
                    .filter(|container| {
                        !container.kubernetes_secret_mounts.is_empty()
                            || container.kubernetes_liveness_probe.is_some()
                            || container.kubernetes_readiness_probe.is_some()
                            || container.security.is_some()
                    })
                    .map(|_| {
                        format!(
                            "Container '{id}' configures security or Kubernetes workload settings that the target platform {platform} does not support"
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
        ContainerCode, ContainerSecurity, ContainerSecurityProfile, KubernetesSecretMount,
        Resource, ResourceEntry, ResourceLifecycle, ResourceSpec,
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
        };

        assert!(!KubernetesWorkloadSettingsCheck.should_run(&stack, Platform::Kubernetes));
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Local,
        ] {
            assert!(KubernetesWorkloadSettingsCheck.should_run(&stack, platform));
            let result = KubernetesWorkloadSettingsCheck
                .check(&stack, platform)
                .await
                .expect("preflight succeeds");
            assert!(!result.success);
            assert!(result.errors[0].contains("agent"));
        }
    }

    #[tokio::test]
    async fn rejects_security_when_the_target_cannot_apply_it() {
        let container = Container::new("worker".to_string())
            .code(ContainerCode::Image {
                image: "worker:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.05".to_string(),
                desired: "0.5".to_string(),
            })
            .memory(ResourceSpec {
                min: "128Mi".to_string(),
                desired: "512Mi".to_string(),
            })
            .security(ContainerSecurity {
                profile: ContainerSecurityProfile::Restricted,
                run_as_user: 65532,
                run_as_group: 65532,
                read_only_root_filesystem: true,
            })
            .permissions("worker".to_string())
            .build();
        let mut resources = IndexMap::new();
        resources.insert(
            "worker".to_string(),
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
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
        };

        assert!(!KubernetesWorkloadSettingsCheck.should_run(&stack, Platform::Kubernetes));
        assert!(KubernetesWorkloadSettingsCheck.should_run(&stack, Platform::Aws));
        let result = KubernetesWorkloadSettingsCheck
            .check(&stack, Platform::Aws)
            .await
            .expect("preflight succeeds");
        assert!(!result.success);
        assert!(result.errors[0].contains("worker"));
        assert!(result.errors[0].contains("security"));
    }
}
