use crate::error::Result;
use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};

/// Validates endpoint source identities and refuses unenforced network settings.
pub struct ContainerNetworkSettingsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for ContainerNetworkSettingsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED")
    }

    fn description(&self) -> &'static str {
        "Container endpoints and egress require valid sources and backend enforcement"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<Container>()
                .is_some_and(|container| {
                    !container.internal_endpoints.is_empty() || container.egress.is_some()
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
                errors.push(format!("Container '{id}': {error}"));
            }
            for endpoint in &container.internal_endpoints {
                for source in &endpoint.from {
                    if !stack
                        .resources
                        .get(source.id())
                        .is_some_and(|entry| entry.config.resource_type() == source.resource_type)
                    {
                        errors.push(format!("Container '{id}' internal endpoint '{}' references missing or wrong-type source '{}:{}'", endpoint.name, source.resource_type, source.id()));
                    }
                }
            }
            // Backend implementations must remove their refusal only when source
            // filtering and the outbound address floor are both enforced.
            if (!container.internal_endpoints.is_empty() || container.egress.is_some()) && platform != Platform::Aws {
                errors.push(format!("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED: {platform} does not yet enforce Container '{id}' internalEndpoints/egress"));
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
    async fn validates_source_identity_and_refuses_unenforced_settings() {
        let mut stack: Stack = serde_json::from_value(serde_json::json!({"id":"test", "resources": {
            "api": {"lifecycle":"live", "dependencies":[], "config": {"type":"container", "id":"api",
                "code":{"type":"image","image":"example:latest"}, "permissions":"app", "links":[], "ports":[], "environment":{}, "stateful":false,
                "cpu":{"min":"1","desired":"1"}, "memory":{"min":"512Mi","desired":"512Mi"},
                "internalEndpoints":[{"name":"admin","port":3129,"from":[{"type":"worker","id":"missing"}]}]}}
        }})).unwrap();
        let check = ContainerNetworkSettingsCheck;
        let invalid = check.check(&stack, Platform::Local).await.unwrap();
        assert!(invalid
            .errors
            .iter()
            .any(|error| error.contains("missing or wrong-type")));
        let container = stack
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap();
        container.internal_endpoints[0].from[0] =
            alien_core::ResourceRef::new(Container::RESOURCE_TYPE, "api");
        let valid = check.check(&stack, Platform::Local).await.unwrap();
        assert_eq!(valid.errors.len(), 1);
        assert!(valid.errors[0].contains("CONTAINER_NETWORK_SETTINGS_UNSUPPORTED"));
        stack
            .resources
            .get_mut("api")
            .unwrap()
            .config
            .downcast_mut::<Container>()
            .unwrap()
            .internal_endpoints
            .clear();
        assert!(check.check(&stack, Platform::Local).await.unwrap().success);
    }
}
