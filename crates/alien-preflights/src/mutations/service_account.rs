//! ServiceAccount mutation that creates ServiceAccount resources from permission profiles.

use crate::error::{ErrorData, Result};
use crate::StackMutation;
use alien_core::{
    permissions::PermissionSet, DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle,
    ServiceAccount, Stack, StackState,
};
use alien_error::Context;
use async_trait::async_trait;
use tracing::{debug, info};

/// Mutation that creates ServiceAccount resources from permission profiles.
///
/// For each permission profile in the stack (except management profile on cross-account platforms),
/// creates a ServiceAccount resource with resolved permission sets.
pub struct ServiceAccountMutation;

#[async_trait]
impl StackMutation for ServiceAccountMutation {
    fn description(&self) -> &'static str {
        "Create ServiceAccount resources from permission profiles"
    }

    fn should_run(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Run if stack has permission profiles
        !stack.permissions.profiles.is_empty()
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Creating ServiceAccount resources from permission profiles");

        // Permission set resolver closure
        let permission_set_resolver = |permission_set_id: &str| -> Option<PermissionSet> {
            alien_permissions::get_permission_set(permission_set_id).cloned()
        };

        let mut service_accounts_to_add = Vec::new();

        for (profile_name, permission_profile) in &stack.permissions.profiles {
            // Skip management profile on cross-account platforms (AWS/GCP/Azure)
            // as these are handled by RemoteStackManagement
            if profile_name == "management"
                && matches!(
                    stack_state.platform,
                    Platform::Aws | Platform::Gcp | Platform::Azure
                )
            {
                debug!(
                    "Skipping management profile for cross-account platform {:?}",
                    stack_state.platform
                );
                continue;
            }

            debug!(
                "Creating ServiceAccount for permission profile: {}",
                profile_name
            );

            // Create ServiceAccount from permission profile
            let service_account_id = format!("{}-sa", profile_name);
            let service_account = ServiceAccount::from_permission_profile(
                service_account_id.clone(),
                permission_profile,
                permission_set_resolver,
            )
            .context(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: format!(
                    "Failed to create ServiceAccount from permission profile '{}'",
                    profile_name
                ),
                resource_id: Some(service_account_id.clone()),
            })?;

            // Add as a frozen resource (ServiceAccounts are infrastructure)
            let resource_entry = ResourceEntry {
                config: alien_core::Resource::new(service_account),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            };

            service_accounts_to_add.push((service_account_id, resource_entry));
        }

        // Add all service accounts to the stack
        for (service_account_id, resource_entry) in service_accounts_to_add {
            debug!("Adding ServiceAccount '{}' to stack", service_account_id);
            stack.resources.insert(service_account_id, resource_entry);
        }

        info!(
            "Successfully created {} ServiceAccount resources",
            stack
                .resources
                .iter()
                .filter(|(_, entry)| {
                    entry.config.resource_type().to_string() == "service-account"
                })
                .count()
        );

        Ok(stack)
    }
}
