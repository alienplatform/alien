use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Daemon, Platform, Sandbox, Stack, Worker};

/// Refuses unenforced container egress and invalid internal sandbox links.
pub struct ContainerNetworkSettingsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ContainerNetworkSettingsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED")
    }

    fn description(&self) -> &'static str {
        "Container internet egress requires backend enforcement and network links must resolve"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<Container>()
                .is_some_and(|container| {
                    container.egress.is_some() || !stack.container_linkers(&container.id).is_empty()
                })
                || entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .is_some_and(|sandbox| !sandbox.links.is_empty())
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        for (id, entry) in stack.resources() {
            if platform == Platform::Aws {
                if let Some(worker) = entry.config.downcast_ref::<Worker>() {
                    let targets = worker
                        .links
                        .iter()
                        .filter(|link| link.resource_type == Container::RESOURCE_TYPE)
                        .filter(|link| {
                            stack
                                .resources
                                .get(link.id())
                                .and_then(|entry| entry.config.downcast_ref::<Container>())
                                .is_some_and(|target| !target.ports.is_empty())
                        })
                        .map(|link| link.id())
                        .collect::<std::collections::BTreeSet<_>>();
                    if targets.len() > 4 {
                        errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: AWS Worker '{id}' links to more than four Containers with ports; Lambda has five security group slots including its network group"));
                    }
                }
            }
            if let Some(sandbox) = entry.config.downcast_ref::<Sandbox>() {
                for link in &sandbox.links {
                    if link.resource_type != Container::RESOURCE_TYPE
                        || !stack.resources.get(link.id()).is_some_and(|target| {
                            target.config.resource_type() == Container::RESOURCE_TYPE
                        })
                    {
                        errors.push(format!("Sandbox '{id}' runtime network link must reference an existing Container: '{}'", link.id()));
                    }
                }
            }
            let Some(container) = entry.config.downcast_ref::<Container>() else {
                continue;
            };
            if let Err(error) = container.validate_network_settings() {
                errors.push(format!("Container '{id}': {error}"));
            }
            if container.egress.is_some()
                && !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
            {
                errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: {platform} does not yet enforce Container '{id}' egress internet"));
            }
            if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
                || container.ports.is_empty()
            {
                continue;
            }
            let linkers = stack.container_linkers(id);
            for linker in &linkers {
                let source = &stack.resources[linker.id()].config;
                if let Some(other) = source.downcast_ref::<Container>() {
                    if other.cluster != container.cluster {
                        errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: Container '{}' links to '{id}' in a different compute cluster", linker.id()));
                    }
                } else if let Some(other) = source.downcast_ref::<Daemon>() {
                    if other.cluster != container.cluster {
                        errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: Daemon '{}' links to '{id}' in a different compute cluster", linker.id()));
                    }
                } else if platform != Platform::Aws {
                    errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: {platform} has no exact private network source enforcement for {} '{}' linking to Container '{id}'", linker.resource_type, linker.id()));
                } else if let Some(sandbox) = source.downcast_ref::<Sandbox>() {
                    if !matches!(sandbox.egress, alien_core::SandboxEgress::Internet) {
                        errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: Sandbox '{}' requires the dedicated internet connector to link to Container '{id}'", linker.id()));
                    }
                } else if source.downcast_ref::<Worker>().is_none() {
                    errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: AWS {} '{}' has no supported private source identity for Container '{id}'", linker.resource_type, linker.id()));
                }
            }
            if platform == Platform::Aws
                && linkers.iter().any(|link| {
                    link.resource_type == Worker::RESOURCE_TYPE
                        || link.resource_type == Sandbox::RESOURCE_TYPE
                })
            {
                let ports = container
                    .ports
                    .iter()
                    .map(|port| port.port)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len();
                if ports + usize::from(!container.public_endpoints.is_empty()) > 32 {
                    errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: AWS Container '{id}' requires more than 32 load balancer targets"));
                }
            }
        }
        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

/// Private cloud listeners keep their identity across ordinary workload updates.
/// A replacement is explicit when their immutable port/cluster placement changes.
#[async_trait::async_trait]
impl crate::StackCompatibilityCheck for ContainerNetworkSettingsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("CONTAINER_PRIVATE_NETWORK_REPLACEMENT_REQUIRED")
    }
    fn description(&self) -> &'static str {
        "Existing linked container listeners keep their ports and compute cluster"
    }
    async fn check(&self, old_stack: &Stack, new_stack: &Stack) -> Result<CheckResult> {
        let mut errors = Vec::new();
        for (id, old) in old_stack.resources() {
            let Some(old) = old.config.downcast_ref::<Container>() else {
                continue;
            };
            if old.ports.is_empty()
                || !old_stack.container_linkers(id).iter().any(|link| {
                    link.resource_type == Worker::RESOURCE_TYPE
                        || link.resource_type == Sandbox::RESOURCE_TYPE
                })
            {
                continue;
            }
            let Some(new) = new_stack
                .resources
                .get(id)
                .and_then(|entry| entry.config.downcast_ref::<Container>())
            else {
                continue;
            };
            let ports = |container: &Container| {
                container
                    .ports
                    .iter()
                    .map(|port| port.port)
                    .collect::<std::collections::BTreeSet<_>>()
            };
            if ports(old) != ports(new) || old.cluster != new.cluster {
                errors.push(format!("CONTAINER_PRIVATE_NETWORK_REPLACEMENT_REQUIRED: Container '{id}' has private cloud listeners; changing its ports or compute cluster requires replacing the container resource"));
            }
        }
        Ok(if errors.is_empty() {
            CheckResult::success()
        } else {
            CheckResult::failed(errors)
        })
    }
    async fn check_with_config(
        &self,
        old_stack: &Stack,
        new_stack: &Stack,
        config: &alien_core::DeploymentConfig,
    ) -> Result<CheckResult> {
        if matches!(
            config.compute_backend,
            Some(alien_core::ComputeBackend::Horizon(_))
        ) {
            crate::StackCompatibilityCheck::check(self, old_stack, new_stack).await
        } else {
            Ok(CheckResult::success())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn network_capabilities_accept_enforced_egress_and_refuse_unsupported_linkers() {
        let mut stack: Stack = serde_json::from_value(serde_json::json!({"id":"test", "resources": {
            "api": {"lifecycle":"live", "dependencies":[], "config": {"type":"container", "id":"api",
                "code":{"type":"image","image":"example:latest"}, "permissions":"app", "links":[], "ports":[], "environment":{}, "stateful":false,
                "cpu":{"min":"1","desired":"1"}, "memory":{"min":"512Mi","desired":"512Mi"}, "egress":"internet"}}
        }})).unwrap();
        let check = ContainerNetworkSettingsCheck;
        let invalid = check.check(&stack, Platform::Local).await.unwrap();
        assert!(
            invalid
                .errors
                .iter()
                .any(|error| error.contains("restricted"))
        );
        stack
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .security = Some(alien_core::ContainerSecurity {
            profile: alien_core::ContainerSecurityProfile::Restricted,
            run_as_user: 1000,
            run_as_group: 1000,
            read_only_root_filesystem: false,
        });
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            assert!(
                check.check(&stack, platform).await.unwrap().success,
                "{platform}"
            );
        }
        for platform in [Platform::Local, Platform::Kubernetes] {
            assert!(
                !check.check(&stack, platform).await.unwrap().success,
                "{platform}"
            );
        }
        let container = stack.resources["api"]
            .config
            .downcast_ref::<Container>()
            .unwrap()
            .clone();
        let caller = Worker::new("caller".into())
            .permissions("app".into())
            .code(alien_core::WorkerCode::Image {
                image: "fixture:latest".into(),
            })
            .link(&container)
            .build();
        stack
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .ports
            .push(alien_core::ContainerPort { port: 8080 });
        stack.resources.insert(
            "caller".into(),
            alien_core::ResourceEntry {
                config: alien_core::Resource::new(caller),
                lifecycle: alien_core::ResourceLifecycle::Live,
                dependencies: vec![],
                remote_access: false,
                enabled_when: None,
            },
        );
        let previous = stack.clone();
        let mut changed = stack.clone();
        changed
            .resources
            .get_mut("caller")
            .unwrap()
            .config
            .downcast_mut::<Worker>()
            .unwrap()
            .links
            .clear();
        assert!(
            crate::StackCompatibilityCheck::check(&check, &previous, &changed)
                .await
                .unwrap()
                .success
        );
        changed
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .ports
            .push(alien_core::ContainerPort { port: 9090 });
        assert!(
            !crate::StackCompatibilityCheck::check(&check, &previous, &changed)
                .await
                .unwrap()
                .success
        );
        assert!(check.check(&stack, Platform::Aws).await.unwrap().success);
        for platform in [Platform::Gcp, Platform::Azure] {
            let result = check.check(&stack, platform).await.unwrap();
            assert!(
                !result.success && result.errors.iter().any(|error| error.contains("caller")),
                "{platform}: {result:?}"
            );
        }
        stack
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .egress = None;
        assert!(check.check(&stack, Platform::Local).await.unwrap().success);
    }
}
