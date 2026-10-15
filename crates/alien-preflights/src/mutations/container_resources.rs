//! Select workload resources before compute pools are planned and materialized.

use crate::{
    error::{ErrorData, Result},
    StackMutation,
};
use alien_core::{
    container_resources::{resolve_container_resources, retain_installed_volume_sizes},
    DeploymentConfig, Stack, StackState,
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
        state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let compute = config.stack_settings.compute.as_ref();
        resolve_container_resources(&mut stack, compute).context(
            ErrorData::StackMutationFailed {
                mutation_name: "ContainerResourcesMutation".to_string(),
                message: "Container resources must match the release's declared choices"
                    .to_string(),
                resource_id: None,
            },
        )?;
        retain_installed_volume_sizes(&mut stack, compute, state);
        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Container, ContainerCode, EnvironmentVariablesSnapshot, ExternalBindings,
        PersistentStorage, Platform, ResourceLifecycle, ResourceSpec, StackResourceState,
        StackSettings, VolumeBackups,
    };

    fn stack_with_volume(size: &str) -> Stack {
        let db = Container::new("db".to_string())
            .code(ContainerCode::Image {
                image: "db:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .stateful(true)
            .persistent_storage(PersistentStorage {
                size: size.to_string(),
                mount_path: "/data".to_string(),
                encryption_key: None,
                backups: VolumeBackups::default(),
            })
            .permissions("db".to_string())
            .build();
        Stack::new("test".to_string())
            .add(db, ResourceLifecycle::Live)
            .build()
    }

    fn volume_size(stack: &Stack) -> String {
        stack.resources["db"]
            .config
            .downcast_ref::<Container>()
            .unwrap()
            .persistent_storage
            .as_ref()
            .unwrap()
            .size
            .clone()
    }

    fn config(compute: serde_json::Value) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings {
                compute: serde_json::from_value(compute).unwrap(),
                ..Default::default()
            })
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    /// The prepared stack the controllers receive keeps an installed volume's size when the
    /// release default drops and the deployment never chose a size; a selection still applies.
    #[tokio::test]
    async fn prepared_volumes_keep_their_installed_size_over_a_lower_release_default() {
        let mut installed = StackState::new(Platform::Aws);
        installed.resources.insert(
            "db".to_string(),
            StackResourceState::new_pending(
                "container".to_string(),
                stack_with_volume("30Gi").resources["db"].config.clone(),
                Some(ResourceLifecycle::Live),
                Vec::new(),
            ),
        );

        let unselected = ContainerResourcesMutation
            .mutate(
                stack_with_volume("20Gi"),
                &installed,
                &config(serde_json::Value::Null),
            )
            .await
            .expect("resolves");
        assert_eq!(volume_size(&unselected), "30Gi");

        let fresh = ContainerResourcesMutation
            .mutate(
                stack_with_volume("20Gi"),
                &StackState::new(Platform::Aws),
                &config(serde_json::Value::Null),
            )
            .await
            .expect("resolves");
        assert_eq!(
            volume_size(&fresh),
            "20Gi",
            "a new deployment uses the default"
        );

        let selected = ContainerResourcesMutation
            .mutate(
                stack_with_volume("20Gi"),
                &installed,
                &config(serde_json::json!({"containers": {"db": {"volumeSizeGib": 40}}})),
            )
            .await
            .expect("resolves");
        assert_eq!(volume_size(&selected), "40Gi");
    }
}
