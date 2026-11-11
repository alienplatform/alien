//! Refuses, on a protected install, what would give management a way to run unsigned code,
//! change an identity, or open the install to outside access.
//!
//! A protected install runs only what the vendor signed, so two things are refused where the
//! install is set up and updated:
//!
//! - Automatic setup. Under it management holds every provision set, including the identity,
//!   network, build and email sets. A protected install is set up with a CloudFormation or
//!   Terraform package the customer runs.
//! - A management permission profile the stack replaces (`override`) or extends with a set it
//!   defines itself. An `extend` may name only built-in sets, such as `vault/data-write` for an
//!   OAuth flow; the renderer drops the family actions (`alien_permissions::generators::Family`)
//!   from those, and a test covers every set in the registry. Closing the list to built-in sets
//!   keeps the check finite: a stack-defined set can't carry a wildcard that reaches a family.

use crate::{
    error::{ErrorData, Result},
    mutations::runs_on_platform_or_base,
    StackMutation,
};
use alien_core::{
    DeploymentConfig, ManagementPermissions, PermissionSetReference, Platform, SetupManagement,
    Stack, StackState,
};
use alien_error::AlienError;
use async_trait::async_trait;

pub struct ProtectedManagementRefusalMutation;

fn on_a_cloud(stack_state: &StackState, config: &DeploymentConfig) -> bool {
    [Platform::Aws, Platform::Gcp, Platform::Azure]
        .into_iter()
        .any(|cloud| runs_on_platform_or_base(stack_state, config, cloud))
}

fn refusal(message: String, resource_id: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::StackMutationFailed {
        mutation_name: "Refuse management permissions a protected install does not allow"
            .to_string(),
        message,
        resource_id: Some(resource_id.to_string()),
    })
}

#[async_trait]
impl StackMutation for ProtectedManagementRefusalMutation {
    fn description(&self) -> &'static str {
        "Refuse management permissions a protected install does not allow"
    }

    fn should_run(
        &self,
        _stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        config.stack_settings.signing.is_some() && on_a_cloud(stack_state, config)
    }

    async fn mutate(
        &self,
        stack: Stack,
        _stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        if config.stack_settings.setup_management == SetupManagement::Automatic {
            return Err(refusal(
                "a protected install does not support automatic setup: management would hold the \
                 identity, network and build permissions that decide what runs. Set it up with the \
                 CloudFormation or Terraform package instead"
                    .to_string(),
                "setup",
            ));
        }
        match stack.management() {
            ManagementPermissions::Auto => {}
            ManagementPermissions::Override(_) => {
                return Err(refusal(
                    "a protected install does not support replacing the management permissions \
                     (`permissions.management.override`). Use `extend` for data permissions"
                        .to_string(),
                    "permissions.management",
                ));
            }
            ManagementPermissions::Extend(profile) => {
                for (scope, sets) in &profile.0 {
                    for reference in sets {
                        let PermissionSetReference::Inline(set) = reference else {
                            continue;
                        };
                        return Err(refusal(
                            format!(
                                "the management permissions define the permission set '{}' \
                                 inline. A protected install gives management only built-in \
                                 sets: an action that can run unsigned code, change an identity \
                                 or open the install to outside access is only the verifier's. \
                                 Name a built-in set instead",
                                set.id
                            ),
                            scope,
                        ));
                    }
                }
            }
        }
        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        EnvironmentVariablesSnapshot, ExternalBindings, PermissionProfile, StackSettings,
        StackSigning,
    };

    fn config(signing: bool, management: SetupManagement) -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings {
                signing: signing.then(|| StackSigning {
                    fingerprint: "SHA256:00".to_string(),
                    channel: "production".to_string(),
                }),
                setup_management: management,
                ..StackSettings::default()
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

    fn extending(set: PermissionSetReference) -> Stack {
        Stack::new("shop".to_string())
            .management(ManagementPermissions::Extend(PermissionProfile(
                [("*".to_string(), vec![set])].into_iter().collect(),
            )))
            .build()
    }

    async fn prepared(
        stack: Stack,
        platform: Platform,
        config: DeploymentConfig,
    ) -> std::result::Result<Stack, String> {
        crate::runner::PreflightRunner::new()
            .apply_mutations(stack, &StackState::new(platform), &config)
            .await
            .map_err(|error| error.to_string())
    }

    #[tokio::test]
    async fn automatic_setup_is_refused_on_a_protected_cloud_install() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let stack = Stack::new("shop".to_string()).build();
            let error = prepared(
                stack,
                platform,
                config(true, SetupManagement::Automatic),
            )
            .await
            .expect_err(&format!("{platform:?}: automatic setup is refused"));
            assert!(error.contains("automatic setup"), "{platform:?}: {error}");
        }
    }

    #[tokio::test]
    async fn manual_setup_and_unprotected_installs_are_unaffected() {
        let stack = || Stack::new("shop".to_string()).build();
        prepared(stack(), Platform::Aws, config(true, SetupManagement::Manual))
            .await
            .expect("a protected install with manual setup is accepted");
        prepared(
            stack(),
            Platform::Aws,
            config(false, SetupManagement::Automatic),
        )
        .await
        .expect("an unprotected install keeps automatic setup");
        prepared(
            stack(),
            Platform::Local,
            config(true, SetupManagement::Automatic),
        )
        .await
        .expect("a local install is not a cloud install");
    }

    #[tokio::test]
    async fn a_management_override_is_refused_on_a_protected_install() {
        let stack = Stack::new("shop".to_string())
            .management(ManagementPermissions::Override(PermissionProfile::new()))
            .build();
        let error = prepared(
            stack.clone(),
            Platform::Aws,
            config(true, SetupManagement::Manual),
        )
        .await
        .expect_err("override is refused");
        assert!(error.contains("override"), "{error}");
        prepared(stack, Platform::Aws, config(false, SetupManagement::Manual))
            .await
            .expect("an unprotected install keeps override");
    }

    fn inline(id: &str) -> PermissionSetReference {
        let mut set = alien_permissions::get_permission_set("storage/data-read")
            .expect("built-in set")
            .clone();
        set.id = id.to_string();
        PermissionSetReference::Inline(set)
    }

    #[tokio::test]
    async fn an_extend_may_name_built_in_sets_but_not_define_one() {
        let protected = || config(true, SetupManagement::Manual);
        let error = prepared(extending(inline("orders/extra")), Platform::Aws, protected())
            .await
            .expect_err("an inline set is refused");
        assert!(error.contains("orders/extra"), "{error}");
        assert!(error.contains("inline"), "{error}");

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let named = extending(PermissionSetReference::Name("vault/data-write".to_string()));
            prepared(named, platform, protected())
                .await
                .unwrap_or_else(|error| panic!("{platform:?}: {error}"));
        }
        prepared(
            extending(inline("orders/extra")),
            Platform::Aws,
            config(false, SetupManagement::Manual),
        )
        .await
        .expect("an unprotected install keeps the inline set");
    }
}
