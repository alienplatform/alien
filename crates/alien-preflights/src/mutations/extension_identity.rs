//! Adds the identity each extension's controllers run as.

use crate::{error::ErrorData, error::Result, StackMutation};
use alien_core::{
    DeploymentConfig, ExtensionManifest, PermissionSet, Platform, RemoteStackManagement, Resource,
    ResourceEntry, ResourceLifecycle, ServiceAccount, Stack, StackState,
};
use alien_error::AlienError;
use async_trait::async_trait;

/// The permission sets Alien looks up by name for a custom type, as it does for its own types.
const IDENTITY_OPERATIONS: [&str; 3] = ["provision", "management", "heartbeat"];

/// Adds one setup-owned identity per extension whose types the stack uses, holding those types'
/// `provision`, `management` and `heartbeat` sets. The manager assumes it the way it assumes the
/// remote bindings identity, and on Kubernetes the Operator's pod does, so the extension's calls
/// show in the customer's audit log apart from Alien's, and the management identity never holds
/// an extension's sets.
pub struct ExtensionIdentityMutation;

#[async_trait]
impl StackMutation for ExtensionIdentityMutation {
    fn description(&self) -> &'static str {
        "Add each extension's identity"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        // The identity is trusted the way the management identity is, by the manager on a cloud
        // and by the Operator on Kubernetes, so it exists only beside one.
        let cloud = matches!(
            stack_state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        ) || (stack_state.platform == Platform::Kubernetes
            && matches!(
                config.base_platform,
                Some(Platform::Aws | Platform::Gcp | Platform::Azure)
            ));
        cloud
            && stack
                .resources
                .values()
                .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
            && stack
                .extensions
                .iter()
                .any(|extension| !identity_permission_sets(stack, extension, config).is_empty())
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let identities = stack
            .extensions
            .iter()
            .filter_map(|extension| {
                let sets = identity_permission_sets(&stack, extension, config);
                (!sets.is_empty()).then(|| (extension.name.clone(), extension.identity_id(), sets))
            })
            .collect::<Vec<_>>();
        for (extension, id, sets) in identities {
            if let Some(existing) = stack.resources.get(&id) {
                let ours = existing
                    .config
                    .downcast_ref::<ServiceAccount>()
                    .is_some_and(|account| account.extension.as_deref() == Some(&extension));
                if !ours {
                    return Err(AlienError::new(ErrorData::StackMutationFailed {
                        mutation_name: self.description().to_string(),
                        message: format!(
                            "resource ID '{id}' is reserved for the identity of the extension '{extension}', but is already used by resource type '{}'",
                            existing.config.resource_type()
                        ),
                        resource_id: Some(id),
                    }));
                }
            }
            let mut account = ServiceAccount::new(id.clone())
                .extension(extension.clone())
                .build();
            account.stack_permission_sets = sets;
            stack.resources.insert(
                id,
                ResourceEntry {
                    enabled_when: None,
                    config: Resource::new(account),
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies: Vec::new(),
                    display_name: Some(alien_core::extension_identity_display_name(&extension)),
                    description: Some(format!(
                        "The identity the {extension} extension's controllers run as."
                    )),
                    remote_access: false,
                },
            );
        }
        Ok(stack)
    }
}

/// The `provision`, `management` and `heartbeat` sets of each of `extension`'s types the stack
/// has a resource of. Heartbeat sets follow the stack's heartbeat setting, as Alien's own do.
fn identity_permission_sets(
    stack: &Stack,
    extension: &ExtensionManifest,
    config: &DeploymentConfig,
) -> Vec<PermissionSet> {
    let heartbeats = config.stack_settings.heartbeats.is_enabled();
    extension
        .resource_types
        .iter()
        .filter(|(resource_type, _)| {
            stack
                .resources
                .values()
                .any(|entry| &entry.config.resource_type() == *resource_type)
        })
        .flat_map(|(resource_type, descriptor)| {
            IDENTITY_OPERATIONS
                .into_iter()
                .filter(move |operation| heartbeats || *operation != "heartbeat")
                .filter_map(move |operation| {
                    let id = format!("{resource_type}/{operation}");
                    descriptor
                        .permission_sets
                        .iter()
                        .find(|set| set.id == id)
                        .cloned()
                })
        })
        .collect()
}
