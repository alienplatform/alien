//! Preserve explicitly accepted keyless installations before deriving key permissions.

use crate::{error::Result, StackMutation};
use alien_core::{
    Container, DeploymentConfig, Postgres, ResourceStatus, Stack, StackState, Storage,
};
use async_trait::async_trait;

pub struct ExistingEncryptionMutation;

#[async_trait]
impl StackMutation for ExistingEncryptionMutation {
    fn description(&self) -> &'static str {
        "Preserve installed keyless resources when the release allows them"
    }

    fn should_run(&self, _stack: &Stack, _state: &StackState, _config: &DeploymentConfig) -> bool {
        true
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        for (id, entry) in &mut stack.resources {
            // Failed provisioning can already own data. Pending and deleted resources
            // have nothing to retain and must use the release's encryption choice.
            let installed = state.resources.get(id).filter(|installed| {
                !matches!(
                    installed.status,
                    ResourceStatus::Pending | ResourceStatus::Deleted
                )
            });
            // Consume the release policy so compatibility checks compare the resolved
            // configuration with installations created before the policy existed.
            if let Some(storage) = entry.config.downcast_mut::<Storage>() {
                if std::mem::take(&mut storage.allow_unencrypted_existing)
                    && installed
                        .and_then(|old| old.config.downcast_ref::<Storage>())
                        .is_some_and(|old| old.encryption_key.is_none())
                {
                    storage.encryption_key = None;
                }
            } else if let Some(postgres) = entry.config.downcast_mut::<Postgres>() {
                if std::mem::take(&mut postgres.allow_unencrypted_existing)
                    && installed
                        .and_then(|old| old.config.downcast_ref::<Postgres>())
                        .is_some_and(|old| old.encryption_key.is_none())
                {
                    postgres.encryption_key = None;
                }
            } else if let Some(container) = entry.config.downcast_mut::<Container>() {
                if let Some(volume) = container.persistent_storage.as_mut() {
                    if std::mem::take(&mut volume.allow_unencrypted_existing)
                        && installed
                            .and_then(|old| old.config.downcast_ref::<Container>())
                            .and_then(|old| old.persistent_storage.as_ref())
                            .is_some_and(|old| old.encryption_key.is_none())
                    {
                        volume.encryption_key = None;
                    }
                }
            }
        }
        Ok(stack)
    }
}
