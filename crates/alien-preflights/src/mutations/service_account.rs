//! ServiceAccount mutation that creates ServiceAccount resources from permission profiles.

use crate::error::{ErrorData, Result};
use crate::StackMutation;
use alien_core::{
    permissions::{PermissionProfile, PermissionSet, PermissionSetReference},
    Build, DeploymentConfig, Platform, ResourceEntry, ResourceLifecycle, ServiceAccount, Stack,
    StackState,
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
        stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        if stack_state.platform == Platform::Machines {
            return false;
        }

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

            let permission_profile =
                with_runtime_baseline_permissions(&stack, profile_name, permission_profile);

            // Create ServiceAccount from permission profile
            let service_account_id = format!("{}-sa", profile_name);
            let service_account = ServiceAccount::from_permission_profile(
                service_account_id.clone(),
                &permission_profile,
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
                enabled_when: None,
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

fn with_runtime_baseline_permissions(
    stack: &Stack,
    profile_name: &str,
    profile: &PermissionProfile,
) -> PermissionProfile {
    let mut profile = profile.clone();

    for entry in stack.resources.values() {
        if let Some(worker) = entry.config.downcast_ref::<alien_core::Worker>() {
            if worker.permissions == profile_name {
                add_global_permission(&mut profile, "worker/execute");
            }
        }

        if let Some(build) = entry.config.downcast_ref::<Build>() {
            if build.permissions == profile_name {
                add_global_permission(&mut profile, "build/execute");
            }
        }
    }

    profile
}

fn add_global_permission(profile: &mut PermissionProfile, permission_set_id: &'static str) {
    let permissions = profile.0.entry("*".to_string()).or_default();
    if !permissions
        .iter()
        .any(|permission| permission.id() == permission_set_id)
    {
        permissions.push(PermissionSetReference::from_name(permission_set_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceLifecycle, Worker, WorkerCode};

    #[test]
    fn worker_profiles_get_execute_baseline() {
        let worker = Worker::new("api".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/api:latest".to_string(),
            })
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .build();

        let profile =
            with_runtime_baseline_permissions(&stack, "execution", &PermissionProfile::new());

        let global = profile.0.get("*").expect("global permissions");
        assert!(global
            .iter()
            .any(|permission| permission.id() == "worker/execute"));
        assert!(!global
            .iter()
            .any(|permission| permission.id() == "aws/tag-tamper-protection"));
    }

    #[test]
    fn build_profiles_get_execute_baseline() {
        let build = Build::new("builder".to_string())
            .permissions("execution".to_string())
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add(build, ResourceLifecycle::Frozen)
            .build();

        let profile =
            with_runtime_baseline_permissions(&stack, "execution", &PermissionProfile::new());

        let global = profile.0.get("*").expect("global permissions");
        assert!(global
            .iter()
            .any(|permission| permission.id() == "build/execute"));
    }

    #[test]
    fn skips_machines_platform() {
        let stack = Stack::new("test-stack".to_string())
            .permissions(
                alien_core::PermissionsConfig::default()
                    .with_profile("execution", PermissionProfile::new()),
            )
            .build();
        let stack_state = StackState::new(Platform::Machines);
        let config = DeploymentConfig::builder()
            .stack_settings(Default::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "2026-07-06T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(Default::default())
            .build();

        assert!(!ServiceAccountMutation.should_run(&stack, &stack_state, &config));
    }
}
