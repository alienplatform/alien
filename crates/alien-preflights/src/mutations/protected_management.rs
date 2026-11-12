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
    mutations::{
        management_permission_profile::generate_auto_management_profile, runs_on_platform_or_base,
    },
    StackMutation,
};
use alien_core::{
    DeploymentConfig, ManagementPermissions, PermissionSet, PermissionSetReference, Platform, Postgres, Queue, ResourceLifecycle,
    SetupManagement, Stack, StackState,
};
use alien_error::AlienError;
use alien_permissions::generators::{aws_family, aws_passed_to_services, azure_family, gcp_family};
use async_trait::async_trait;

pub struct ProtectedManagementRefusalMutation;

fn on_a_cloud(stack_state: &StackState, config: &DeploymentConfig) -> bool {
    [Platform::Aws, Platform::Gcp, Platform::Azure]
        .into_iter()
        .any(|cloud| runs_on_platform_or_base(stack_state, config, cloud))
}

/// The first action of a built-in set the renderer would drop from a protected install's
/// management identity, as `(action, family)`. The built-in profile is filtered silently; a set
/// the stack adds is refused instead, so the vendor gets all of it or a clear error.
fn first_dropped_action(set: &PermissionSet) -> Option<(String, &'static str)> {
    for entry in set.platforms.aws.iter().flatten() {
        if !entry.effect.is_allow() {
            continue;
        }
        let services = [&entry.binding.stack, &entry.binding.resource]
            .into_iter()
            .flatten()
            .find_map(aws_passed_to_services);
        for action in entry.grant.actions.iter().flatten() {
            if let Some(family) = aws_family(action, services.as_deref()) {
                return Some((action.clone(), family.name()));
            }
        }
    }
    for entry in set.platforms.gcp.iter().flatten() {
        let permissions = entry
            .grant
            .permissions
            .iter()
            .chain(&entry.grant.residual_permissions)
            .flatten();
        for permission in permissions {
            if let Some(family) = gcp_family(permission) {
                return Some((permission.clone(), family.name()));
            }
        }
    }
    for entry in set.platforms.azure.iter().flatten() {
        for action in entry.grant.actions.iter().flatten() {
            if let Some(family) = azure_family(action) {
                return Some((action.clone(), family.name()));
            }
        }
    }
    None
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
        stack_state: &StackState,
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
        // Setup grants management one publisher role on every topic named `<prefix>-*-rq`, which
        // is how it dispatches commands to any worker, including one a later release adds. A
        // queue whose topic ends the same way would fall under that grant.
        if runs_on_platform_or_base(stack_state, config, Platform::Gcp) {
            if let Some((id, _)) = stack.resources().find(|(id, entry)| {
                id.ends_with("-rq") && entry.config.downcast_ref::<Queue>().is_some()
            }) {
                return Err(refusal(
                    format!(
                        "the queue '{id}' ends in '-rq'. On a protected GCP install management \
                         may publish to every topic that ends in '-rq' (the commands topics of \
                         workers), so a queue cannot use that ending. Rename it"
                    ),
                    id,
                ));
            }
        }
        // A live database is created and resized by the controller: its password, network
        // placement and access rules are writes only setup may make on a protected install. A
        // frozen one is created by the setup package, which manages those itself.
        if let Some((id, _)) = stack.resources().find(|(_, entry)| {
            entry.lifecycle == ResourceLifecycle::Live
                && entry.config.downcast_ref::<Postgres>().is_some()
        }) {
            return Err(refusal(
                format!(
                    "the Postgres database '{id}' is live. On a protected install the database \
                     has to be frozen, so setup creates it: change its lifecycle to frozen, or \
                     install without release signing"
                ),
                id,
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
                // A stack that was prepared before carries the sets the built-in profile derived
                // from its resources; those are filtered by the renderer, not refused. Only
                // what the vendor added is checked.
                let derived = generate_auto_management_profile(&stack, stack_state, config)?;
                for (scope, sets) in &profile.0 {
                    for reference in sets {
                        if derived
                            .as_ref()
                            .and_then(|derived| derived.0.get(scope))
                            .is_some_and(|derived| derived.contains(reference))
                        {
                            continue;
                        }
                        let set = match reference {
                            PermissionSetReference::Name(id) => {
                                let Some(set) = alien_permissions::get_permission_set(id) else {
                                    continue;
                                };
                                if let Some((action, family)) = first_dropped_action(set) {
                                    return Err(refusal(
                                        format!(
                                            "the management permissions add the permission set \
                                             '{id}', which grants '{action}' ({family}). A \
                                             protected install gives management no action that \
                                             can run unsigned code, change an identity or open \
                                             the install to outside access; only the verifier \
                                             writes those. Remove the set"
                                        ),
                                        scope,
                                    ));
                                }
                                continue;
                            }
                            PermissionSetReference::Inline(set) => set,
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
            let error = prepared(stack, platform, config(true, SetupManagement::Automatic))
                .await
                .expect_err(&format!("{platform:?}: automatic setup is refused"));
            assert!(error.contains("automatic setup"), "{platform:?}: {error}");
        }
    }

    #[tokio::test]
    async fn manual_setup_and_unprotected_installs_are_unaffected() {
        let stack = || Stack::new("shop".to_string()).build();
        prepared(
            stack(),
            Platform::Aws,
            config(true, SetupManagement::Manual),
        )
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
        let error = prepared(
            extending(inline("orders/extra")),
            Platform::Aws,
            protected(),
        )
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

        // A built-in set that would lose an action to the family filter is refused by name.
        let error = prepared(
            extending(PermissionSetReference::Name(
                "service-account/provision".to_string(),
            )),
            Platform::Aws,
            protected(),
        )
        .await
        .expect_err("a set with family actions is refused");
        assert!(error.contains("'service-account/provision'"), "{error}");
        assert!(error.contains("IDENTITY"), "{error}");
        prepared(
            extending(inline("orders/extra")),
            Platform::Aws,
            config(false, SetupManagement::Manual),
        )
        .await
        .expect("an unprotected install keeps the inline set");
    }
    #[tokio::test]
    async fn a_queue_ending_in_rq_is_refused_on_protected_gcp_only() {
        let stack = || {
            Stack::new("shop".to_string())
                .add(Queue::new("jobs-rq".to_string()).build(), ResourceLifecycle::Live)
                .build()
        };
        let error = prepared(stack(), Platform::Gcp, config(true, SetupManagement::Manual))
            .await
            .expect_err("the queue falls under management's publisher grant");
        assert!(error.contains("jobs-rq") && error.contains("-rq"), "{error}");
        prepared(stack(), Platform::Gcp, config(false, SetupManagement::Manual))
            .await
            .expect("an unprotected install is unaffected");
        prepared(stack(), Platform::Aws, config(true, SetupManagement::Manual))
            .await
            .expect("the grant is GCP's");
    }

    #[tokio::test]
    async fn a_live_postgres_is_refused_on_every_protected_cloud_install() {
        let stack = |lifecycle| {
            Stack::new("shop".to_string())
                .add(Postgres::new("db".to_string()).build(), lifecycle)
                .build()
        };
        let kubernetes_on = |cloud| {
            let mut config = config(true, SetupManagement::Manual);
            config.base_platform = Some(cloud);
            config
        };
        let cases = [
            (Platform::Aws, config(true, SetupManagement::Manual)),
            (Platform::Gcp, config(true, SetupManagement::Manual)),
            (Platform::Azure, config(true, SetupManagement::Manual)),
            (Platform::Kubernetes, kubernetes_on(Platform::Aws)),
            (Platform::Kubernetes, kubernetes_on(Platform::Gcp)),
            (Platform::Kubernetes, kubernetes_on(Platform::Azure)),
        ];
        for (platform, protected) in cases {
            let label = format!("{platform:?}/{:?}", protected.base_platform);
            let error = prepared(stack(ResourceLifecycle::Live), platform, protected.clone())
                .await
                .expect_err(&format!("{label}: a live database is refused"));
            assert!(
                error.contains("'db'") && error.contains("frozen") && error.contains("signing"),
                "{label}: {error}"
            );
            prepared(stack(ResourceLifecycle::Frozen), platform, protected)
                .await
                .unwrap_or_else(|error| panic!("{label}: a frozen database is accepted: {error}"));
        }
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            prepared(
                stack(ResourceLifecycle::Live),
                platform,
                config(false, SetupManagement::Manual),
            )
            .await
            .unwrap_or_else(|error| panic!("{platform:?}: unsigned keeps a live database: {error}"));
        }
    }
}
