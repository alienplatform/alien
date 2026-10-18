use crate::error::Result;
use crate::StackMutation;
use alien_core::{Container, Daemon, DeploymentConfig, ResourceRef, Stack, StackState, Worker};

/// Makes endpoint addresses available to their authorized workload consumers.
pub struct ContainerEndpointLinksMutation;

#[async_trait::async_trait]
impl StackMutation for ContainerEndpointLinksMutation {
    fn description(&self) -> &'static str {
        "Link internal container endpoints to their authorized workloads"
    }

    fn should_run(&self, stack: &Stack, _state: &StackState, _config: &DeploymentConfig) -> bool {
        stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<Container>()
                .is_some_and(|container| !container.internal_endpoints.is_empty())
        })
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let edges: Vec<_> = stack
            .resources
            .values()
            .filter_map(|entry| entry.config.downcast_ref::<Container>())
            .flat_map(|container| {
                container
                    .internal_endpoints
                    .iter()
                    .flat_map(move |endpoint| {
                        endpoint.from.iter().map(move |source| {
                            (
                                source.clone(),
                                ResourceRef::new(Container::RESOURCE_TYPE, container.id.clone()),
                            )
                        })
                    })
            })
            .collect();
        for (source, target) in edges {
            if source == target {
                continue;
            }
            let Some(entry) = stack.resources.get_mut(source.id()) else {
                continue;
            };
            let links = if let Some(worker) = entry.config.downcast_mut::<Worker>() {
                Some(&mut worker.links)
            } else if let Some(container) = entry.config.downcast_mut::<Container>() {
                Some(&mut container.links)
            } else if let Some(daemon) = entry.config.downcast_mut::<Daemon>() {
                Some(&mut daemon.links)
            } else {
                // Sandbox parent resources provision before endpoint containers.
                // Their backend installs the address into each runtime sandbox.
                None
            };
            if let Some(links) = links {
                if !links.contains(&target) {
                    links.push(target);
                }
            }
        }
        Ok(stack)
    }
}
