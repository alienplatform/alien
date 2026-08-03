//! Adds the one setup-owned Remote Bindings identity required by opted-in resources.

use crate::{error::ErrorData, error::Result, StackMutation};
use alien_core::{
    DeploymentConfig, Platform, RemoteBindingGrant, RemoteBindings, ResourceEntry,
    ResourceLifecycle, Stack, StackState,
};
use alien_error::AlienError;
use async_trait::async_trait;

pub const REMOTE_BINDINGS_ID: &str = "access";

pub struct RemoteBindingsMutation;

#[async_trait]
impl StackMutation for RemoteBindingsMutation {
    fn description(&self) -> &'static str {
        "Add the setup-owned Remote Bindings identity"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        let platform = config.base_platform.unwrap_or(stack_state.platform);
        matches!(
            platform,
            Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test
        ) && stack
            .resources
            .values()
            .any(ResourceEntry::has_remote_bindings)
            && !stack
                .resources
                .values()
                .any(|entry| entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        if let Some(existing) = stack.resources.get(REMOTE_BINDINGS_ID) {
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: format!(
                    "resource ID '{REMOTE_BINDINGS_ID}' is reserved for application access, but is already used by resource type '{}'",
                    existing.config.resource_type()
                ),
                resource_id: Some(REMOTE_BINDINGS_ID.to_string()),
            }));
        }

        let mut grants = stack
            .resources
            .iter()
            .filter_map(|(resource_id, entry)| {
                alien_core::remote_bindings::remote_binding_for_entry(entry).map(|definition| {
                    RemoteBindingGrant {
                        resource_id: resource_id.clone(),
                        permission_set: definition.permission_set.to_string(),
                        revision: definition.revision,
                    }
                })
            })
            .collect::<Vec<_>>();
        grants.sort_by(|left, right| left.resource_id.cmp(&right.resource_id));
        stack.resources.insert(
            REMOTE_BINDINGS_ID.to_string(),
            ResourceEntry {
                enabled_when: None,
                config: alien_core::Resource::new(
                    RemoteBindings::new(REMOTE_BINDINGS_ID.to_string())
                        .grants(grants)
                        .build(),
                ),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig, StackSettings, Storage,
    };

    fn config() -> DeploymentConfig {
        DeploymentConfig {
            deployment_name: None,
            stack_settings: StackSettings::default(),
            management_config: Some(ManagementConfig::Kubernetes),
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
            input_values: Default::default(),
            allow_frozen_changes: false,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform: None,
            label_domain: None,
            observe_label_selector: None,
            observe_all_namespaces: false,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
        }
    }

    #[tokio::test]
    async fn one_opted_in_resource_adds_one_shared_identity() {
        let stack = Stack::new("byo-bucket".to_string())
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState::new(Platform::Test);
        let mutation = RemoteBindingsMutation;

        assert!(mutation.should_run(&stack, &state, &config()));
        let mutated = mutation.mutate(stack, &state, &config()).await.unwrap();
        assert!(mutated.resources.contains_key(REMOTE_BINDINGS_ID));
        assert_eq!(
            mutated
                .resources
                .values()
                .filter(|entry| entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                .count(),
            1
        );
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");
        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "exports");
        assert_eq!(
            bindings.grants[0].permission_set,
            "storage/remote-data-write"
        );
    }

    #[tokio::test]
    async fn reserved_access_id_is_never_overwritten() {
        let stack = Stack::new("byo-bucket".to_string())
            .add(
                Storage::new(REMOTE_BINDINGS_ID.to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState::new(Platform::Test);

        let error = RemoteBindingsMutation
            .mutate(stack, &state, &config())
            .await
            .expect_err("reserved resource ID must fail instead of being overwritten");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(error
            .to_string()
            .contains("reserved for application access"));
    }

    #[test]
    fn ordinary_storage_does_not_add_remote_bindings() {
        let stack = Stack::new("app".to_string())
            .add(
                Storage::new("internal".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        assert!(!RemoteBindingsMutation.should_run(
            &stack,
            &StackState::new(Platform::Test),
            &config()
        ));
    }
}
