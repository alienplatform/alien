//! Make the gateway address available to workloads through a normal Container binding.

use std::collections::BTreeSet;

use alien_core::{
    sandbox_egress::{has_sandbox_gateway, SANDBOX_EGRESS_GATEWAY_ID},
    Build, Container, Daemon, DeploymentConfig, ResourceRef, Sandbox, SandboxEgress, Stack,
    StackState, Vault, Worker,
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
        state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let sandbox_ids: BTreeSet<String> = stack
            .resources()
            .filter_map(|(id, entry)| {
                entry
                    .config
                    .downcast_ref::<Sandbox>()
                    .filter(|sandbox| matches!(sandbox.egress, SandboxEgress::Internet))
                    .map(|_| id.clone())
            })
            .collect();
        let gateway = ResourceRef::new(Container::RESOURCE_TYPE, SANDBOX_EGRESS_GATEWAY_ID);
        let vault = ResourceRef::new(
            Vault::RESOURCE_TYPE,
            alien_core::sandbox_egress::sandbox_egress_vault_id(state.platform),
        );
        let owner = alien_core::sandbox_egress::sandbox_egress_key_owner(&stack).map(str::to_owned);
        for id in &sandbox_ids {
            let entry = stack
                .resources
                .get_mut(id)
                .expect("sandbox collected above");
            let sandbox = entry
                .config
                .downcast_mut::<Sandbox>()
                .expect("sandbox type");
            if !sandbox.links.contains(&gateway) {
                sandbox.links.push(gateway.clone());
            }
            if !entry.dependencies.contains(&vault) {
                entry.dependencies.push(vault.clone());
            }
            if let Some(owner) = owner.as_ref().filter(|owner| *owner != id) {
                let dependency = ResourceRef::new(Sandbox::RESOURCE_TYPE, owner);
                if !entry.dependencies.contains(&dependency) {
                    entry.dependencies.push(dependency);
                }
            }
        }
        let gateway_entry = stack
            .resources
            .get_mut(SANDBOX_EGRESS_GATEWAY_ID)
            .expect("gateway exists");
        for id in &sandbox_ids {
            let dependency = ResourceRef::new(Sandbox::RESOURCE_TYPE, id);
            if !gateway_entry.dependencies.contains(&dependency) {
                gateway_entry.dependencies.push(dependency);
            }
        }

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
            if links.iter().any(|link| sandbox_ids.contains(link.id())) {
                if !links.contains(&gateway) {
                    links.push(gateway.clone());
                }
                // Loading the signing key needs the vault descriptor. Secret access remains
                // governed by the separately derived exact-entry permission profile.
                if !links.contains(&vault) {
                    links.push(vault.clone());
                }
            }
        }
        Ok(stack)
    }
}
