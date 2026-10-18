//! Install the shared gateway before compute sizing and permission derivation.

use std::collections::BTreeSet;

use alien_core::{
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, Build, Container, ContainerCode, ContainerEgress,
    ContainerSecurity, ContainerSecurityProfile, Daemon, DeploymentConfig, HealthCheck, Kv,
    Platform, Resource, ResourceEntry, ResourceLifecycle, ResourceSpec, Sandbox, SandboxEgress,
    Stack, StackState, Worker,
};
use alien_error::AlienError;
use async_trait::async_trait;

use crate::{
    error::{ErrorData, Result},
    StackMutation,
};

const POLICY_STORE_ID: &str = "egress-state";
const GATEWAY_IMAGE: &str = "ghcr.io/alienplatform/alien-egress-gateway@sha256:7a3a66b0894c05fca445cbf5fd71e351cd8a9a98c40744acad4496b59574bdbd";

/// Managed AWS deployments use the same compute and service-account machinery
/// as application containers. Unsupported deployments are refused before execution.
pub struct SandboxGatewayMutation;

#[async_trait]
impl StackMutation for SandboxGatewayMutation {
    fn description(&self) -> &'static str {
        "Add the sandbox egress gateway and its policy store"
    }

    fn should_run(&self, stack: &Stack, state: &StackState, config: &DeploymentConfig) -> bool {
        state.platform == Platform::Aws
            && config.compute_backend.is_some()
            && stack.resources().any(|(_, entry)| {
                entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .is_some_and(|sandbox| matches!(sandbox.egress, SandboxEgress::Internet))
            })
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let invalid = |message: String| {
            AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: "SandboxGatewayMutation".into(),
                message,
                resource_id: Some(SANDBOX_EGRESS_GATEWAY_ID.into()),
            })
        };
        let sandboxes: BTreeSet<String> = stack
            .resources()
            .filter_map(|(id, entry)| {
                entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .filter(|sandbox| matches!(sandbox.egress, SandboxEgress::Internet))
                    .map(|_| id.clone())
            })
            .collect();
        for (id, entry) in stack.resources() {
            if sandboxes.contains(id) && entry.remote_access {
                return Err(invalid(format!("Sandbox '{id}': internet egress cannot be used with remoteAccess because policy administration is private to this deployment")));
            }
            let links = entry
                .config
                .downcast_ref::<Container>()
                .map(|r| &r.links)
                .or_else(|| entry.config.downcast_ref::<Worker>().map(|r| &r.links))
                .or_else(|| entry.config.downcast_ref::<Daemon>().map(|r| &r.links))
                .or_else(|| entry.config.downcast_ref::<Build>().map(|r| &r.links));
            if links.is_some_and(|links| links.iter().any(|link| sandboxes.contains(link.id()))) {
                if entry.config.downcast_ref::<Build>().is_some() {
                    return Err(invalid(format!("Build '{id}' cannot manage an internet Sandbox: private gateway administration supports Worker, Container and Daemon consumers")));
                }
            }
        }
        let kv = Kv::new(POLICY_STORE_ID.into()).build();
        if let Some(existing) = stack.resources.get(POLICY_STORE_ID) {
            if existing.config.downcast_ref::<Kv>().is_none() || existing.enabled_when.is_some() {
                return Err(invalid(format!("Reserved gateway policy store '{POLICY_STORE_ID}' must be an unconditional KV resource")));
            }
        } else {
            stack.resources.insert(
                POLICY_STORE_ID.into(),
                ResourceEntry {
                    config: Resource::new(kv.clone()),
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies: Vec::new(),
                    enabled_when: None,
                    remote_access: false,
                },
            );
        }
        let gateway = Container::new(SANDBOX_EGRESS_GATEWAY_ID.into())
            .code(ContainerCode::Image {
                image: GATEWAY_IMAGE.into(),
            })
            .cpu(ResourceSpec {
                min: "0.25".into(),
                desired: "0.25".into(),
            })
            .memory(ResourceSpec {
                min: "256Mi".into(),
                desired: "256Mi".into(),
            })
            .replicas(2)
            .permissions(SANDBOX_EGRESS_GATEWAY_ID.into())
            .security(ContainerSecurity {
                profile: ContainerSecurityProfile::Restricted,
                run_as_user: 1000,
                run_as_group: 1000,
                read_only_root_filesystem: true,
            })
            .egress(ContainerEgress::Internet)
            .port(3128)
            .port(3129)
            .health_check(HealthCheck {
                path: "/health".into(),
                port: Some(3129),
                method: "GET".into(),
                timeout_seconds: 3,
                failure_threshold: 3,
            })
            .commands_enabled(false)
            .link(&kv)
            .build();
        gateway
            .validate_network_settings()
            .map_err(|error| invalid(error.to_string()))?;
        if let Some(existing) = stack.resources.get(SANDBOX_EGRESS_GATEWAY_ID) {
            let Some(container) = existing.config.downcast_ref::<Container>() else {
                return Err(invalid(
                    "The reserved sandbox gateway id is already in use".into(),
                ));
            };
            // Re-running preparation preserves computed placement and dependency links.
            if container.code != gateway.code
                || container.security != gateway.security
                || container.egress != gateway.egress
                || container.replicas != gateway.replicas
                || container.permissions != gateway.permissions
                || container.ports != gateway.ports
                || existing.enabled_when.is_some()
            {
                return Err(invalid("The reserved sandbox gateway resource conflicts with its managed configuration".into()));
            }
        } else {
            stack.resources.insert(
                SANDBOX_EGRESS_GATEWAY_ID.into(),
                ResourceEntry {
                    config: Resource::new(gateway),
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: Vec::new(),
                    enabled_when: None,
                    remote_access: false,
                },
            );
        }
        Ok(stack)
    }
}
