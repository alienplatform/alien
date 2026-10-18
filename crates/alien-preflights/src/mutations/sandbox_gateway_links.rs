//! Make the gateway address available to workloads through a normal Container binding.

use std::collections::HashSet;

use alien_core::{
    sandbox_egress::{has_sandbox_gateway, SANDBOX_EGRESS_GATEWAY_ID},
    Build, Container, Daemon, DeploymentConfig, ResourceRef, Sandbox, SandboxEgress, Stack,
    StackState, Worker,
};
use async_trait::async_trait;

use crate::{error::Result, StackMutation};

/// Workloads wait for the gateway after its sandbox parents publish configuration.
/// The sandbox itself must not depend on the gateway, which would introduce a cycle.
pub struct SandboxGatewayLinksMutation;

#[async_trait]
impl StackMutation for SandboxGatewayLinksMutation {
    fn description(&self) -> &'static str {
        "Link sandbox consumers to their egress gateway"
    }

    fn should_run(&self, stack: &Stack, _state: &StackState, _config: &DeploymentConfig) -> bool {
        has_sandbox_gateway(stack)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let sandbox_ids: HashSet<String> = stack
            .resources()
            .filter_map(|(id, entry)| {
                entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .filter(|sandbox| !matches!(sandbox.egress, SandboxEgress::None))
                    .map(|_| id.clone())
            })
            .collect();
        let gateway = ResourceRef::new(Container::RESOURCE_TYPE, SANDBOX_EGRESS_GATEWAY_ID);
        for (id, entry) in &mut stack.resources {
            if id == SANDBOX_EGRESS_GATEWAY_ID {
                continue;
            }
            // Test the type before taking the mutable reference so borrows do not overlap.
            let links = match entry.config.resource_type().as_ref() {
                "worker" => {
                    &mut entry
                        .config
                        .downcast_mut::<Worker>()
                        .expect("worker type")
                        .links
                }
                "container" => {
                    &mut entry
                        .config
                        .downcast_mut::<Container>()
                        .expect("container type")
                        .links
                }
                "daemon" => {
                    &mut entry
                        .config
                        .downcast_mut::<Daemon>()
                        .expect("daemon type")
                        .links
                }
                "build" => {
                    &mut entry
                        .config
                        .downcast_mut::<Build>()
                        .expect("build type")
                        .links
                }
                _ => continue,
            };
            if !links.contains(&gateway) && links.iter().any(|link| sandbox_ids.contains(link.id()))
            {
                links.push(gateway.clone());
            }
        }
        Ok(stack)
    }
}
