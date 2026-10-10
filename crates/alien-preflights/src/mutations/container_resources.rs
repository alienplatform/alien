//! Select workload resources before compute pools are planned and materialized.

use crate::{
    error::{ErrorData, Result},
    StackMutation,
};
use alien_core::{
    container_resources::resolve_container_resources, DeploymentConfig, Stack, StackState,
};
use alien_error::Context;
use async_trait::async_trait;

pub struct ContainerResourcesMutation;

#[async_trait]
impl StackMutation for ContainerResourcesMutation {
    fn description(&self) -> &'static str {
        "Resolve deployment-time container resource selections"
    }

    fn should_run(&self, _stack: &Stack, _state: &StackState, _config: &DeploymentConfig) -> bool {
        true
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        resolve_container_resources(&mut stack, config.stack_settings.compute.as_ref()).context(
            ErrorData::StackMutationFailed {
                mutation_name: "ContainerResourcesMutation".to_string(),
                message: "Container resources must match the release's declared choices"
                    .to_string(),
                resource_id: None,
            },
        )?;
        Ok(stack)
    }
}
