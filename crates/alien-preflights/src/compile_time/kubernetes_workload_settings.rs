use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};

/// Prevents Kubernetes-only workload settings from being silently ignored.
///
/// `security` is different: every platform accepts it. On Local there is no pod to apply it to,
/// so the check warns that it only takes effect on Kubernetes.
pub struct KubernetesWorkloadSettingsCheck;

fn has_kubernetes_only_settings(container: &Container) -> bool {
    !container.kubernetes_secret_mounts.is_empty()
        || container.kubernetes_liveness_probe.is_some()
        || container.kubernetes_readiness_probe.is_some()
}

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
                        has_kubernetes_only_settings(container)
                            || (platform == Platform::Local && container.security.is_some())
                    })
            })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let containers = || {
            stack
                .resources()
                .filter_map(|(id, entry)| Some((id, entry.config.downcast_ref::<Container>()?)))
        };
        let errors = containers()
            .filter(|(_, container)| has_kubernetes_only_settings(container))
            .map(|(id, _)| {
                format!(
                    "Container '{id}' configures Kubernetes workload settings that the target platform {platform} does not support"
                )
            })
            .collect::<Vec<_>>();
        let warnings = if platform == Platform::Local {
            containers()
                .filter(|(_, container)| container.security.is_some())
                .map(|(id, _)| {
                    format!(
                        "Container '{id}' sets security, which applies on Kubernetes only; it has no effect on Local"
                    )
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        if !errors.is_empty() {
            Ok(CheckResult::failed_with_warnings(errors, warnings))
        } else if !warnings.is_empty() {
            Ok(CheckResult::with_warnings(warnings))
        } else {
            Ok(CheckResult::success())
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
                display_name: None,
                description: None,
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
            operations: None,
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
    async fn security_only_warns_on_local() {
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
                run_as_user: Some(65532),
                run_as_group: Some(65532),
                read_only_root_filesystem: Some(true),
                writable_paths: Vec::new(),
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
                display_name: None,
                description: None,
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
            operations: None,
        };

        assert!(!KubernetesWorkloadSettingsCheck.should_run(&stack, Platform::Kubernetes));
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            assert!(!KubernetesWorkloadSettingsCheck.should_run(&stack, platform));
        }
        assert!(KubernetesWorkloadSettingsCheck.should_run(&stack, Platform::Local));
        let result = KubernetesWorkloadSettingsCheck
            .check(&stack, Platform::Local)
            .await
            .expect("preflight succeeds");
        assert!(result.success, "security must not fail the Local preflight");
        assert!(result.errors.is_empty());
        assert!(result.warnings[0].contains("worker"));
        assert!(result.warnings[0].contains("Kubernetes only"));
    }
}
