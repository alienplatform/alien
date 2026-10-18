use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Sandbox, Stack};

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
                .is_some_and(|container| container.egress.is_some())
                || entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .is_some_and(|sandbox| !sandbox.links.is_empty())
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut errors = Vec::new();
        for (id, entry) in stack.resources() {
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
            // Each backend removes its refusal once the external address floor is enforced.
            if container.egress.is_some()
                && !matches!(platform, Platform::Aws | Platform::Kubernetes)
            {
                errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: {platform} does not yet enforce Container '{id}' egress internet"));
            }
        }
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

    #[tokio::test]
    async fn internet_egress_is_refused_until_the_backend_enforces_it() {
        let mut stack: Stack = serde_json::from_value(serde_json::json!({"id":"test", "resources": {
            "api": {"lifecycle":"live", "dependencies":[], "config": {"type":"container", "id":"api",
                "code":{"type":"image","image":"example:latest"}, "permissions":"app", "links":[], "ports":[], "environment":{}, "stateful":false,
                "cpu":{"min":"1","desired":"1"}, "memory":{"min":"512Mi","desired":"512Mi"}, "egress":"internet"}}
        }})).unwrap();
        let check = ContainerNetworkSettingsCheck;
        let invalid = check.check(&stack, Platform::Local).await.unwrap();
        assert!(invalid
            .errors
            .iter()
            .any(|error| error.contains("restricted")));
        stack
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .security = Some(alien_core::ContainerSecurity {
            profile: alien_core::ContainerSecurityProfile::Restricted,
            run_as_user: 65532,
            run_as_group: 65532,
            read_only_root_filesystem: true,
        });
        assert!(
            check
                .check(&stack, Platform::Kubernetes)
                .await
                .unwrap()
                .success
        );
        assert!(!check.check(&stack, Platform::Local).await.unwrap().success);
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
