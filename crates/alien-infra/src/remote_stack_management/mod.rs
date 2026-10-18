mod aws;
mod aws_gateway_setup;
pub use aws::*;

mod aws_import;
pub use aws_import::AwsRemoteStackManagementImporter;

mod gcp;
pub use gcp::*;

mod gcp_import;
pub use gcp_import::GcpRemoteStackManagementImporter;

mod azure;
pub use azure::*;

mod azure_import;
pub use azure_import::AzureRemoteStackManagementImporter;

#[cfg(feature = "test")]
mod test;
#[cfg(feature = "test")]
pub use test::*;

use crate::core::{ResourceControllerContext, StackResourceStateExt};
use crate::error::{ErrorData, Result};
use alien_core::{InitialSetupAuthority, Platform, RemoteStackManagement, Stack, StackState};
use alien_error::{AlienError, Context};

/// A revision of the management permissions the stack asks for: a digest of
/// the management profile with every permission set it names resolved to its
/// body. A release that changes a permission set changes the revision.
pub(crate) fn management_permissions_revision(
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<String>> {
    alien_permissions::management_permissions_revision(ctx.desired_stack.management().profile())
        .context(ErrorData::InfrastructureError {
            message: "Failed to resolve the management permissions revision".to_string(),
            operation: Some("management_permissions_revision".to_string()),
            resource_id: None,
        })
}

fn management_grant_revisions(
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<std::collections::BTreeMap<String, Vec<String>>>> {
    alien_permissions::management_grant_revisions(
        ctx.desired_stack.management().profile(),
        ctx.platform,
    )
    .context(ErrorData::InfrastructureError {
        message: "Failed to record installed management grants".to_string(),
        operation: Some("management_grant_revisions".to_string()),
        resource_id: None,
    })
}

/// Check the installed grant receipt before runtime starts or resumes mutations.
/// Cleanup remains available even when a newer release requires setup changes.
pub(crate) fn validate_installed_management_permissions(
    stack: &Stack,
    state: &StackState,
    authority: InitialSetupAuthority,
) -> Result<()> {
    if authority == InitialSetupAuthority::DirectSetup
        || !matches!(
            state.platform,
            Platform::Aws | Platform::Gcp | Platform::Azure
        )
        || !stack.resources().any(|(_, resource)| {
            resource
                .config
                .downcast_ref::<RemoteStackManagement>()
                .is_some()
        })
    {
        return Ok(());
    }
    let expected =
        alien_permissions::management_grant_revisions(stack.management().profile(), state.platform)
            .context(ErrorData::InfrastructureError {
                message: "Failed to resolve required management permissions".to_string(),
                operation: Some("validate_installed_management_permissions".to_string()),
                resource_id: None,
            })?;
    let Some(expected) = expected else {
        return Ok(());
    };
    if expected.is_empty() {
        return Ok(());
    }
    for (id, resource) in stack.resources() {
        if resource
            .config
            .downcast_ref::<RemoteStackManagement>()
            .is_none()
        {
            continue;
        }
        let installed = state.resources.get(id).ok_or_else(|| {
            AlienError::new(ErrorData::SetupRequired {
                message:
                    "Register the installed management permissions before provisioning resources"
                        .to_string(),
                resource_id: Some(id.to_string()),
            })
        })?;
        let applied = match state.platform {
            Platform::Aws => {
                installed
                    .get_internal_controller_typed::<AwsRemoteStackManagementController>()?
                    .installed_management_grants
            }
            Platform::Gcp => {
                installed
                    .get_internal_controller_typed::<GcpRemoteStackManagementController>()?
                    .installed_management_grants
            }
            Platform::Azure => {
                installed
                    .get_internal_controller_typed::<AzureRemoteStackManagementController>()?
                    .installed_management_grants
            }
            _ => continue,
        };
        if !applied.as_ref().is_some_and(|installed| {
            expected.iter().all(|(scope, grants)| {
                grants.iter().all(|grant| {
                    installed
                        .get(scope)
                        .is_some_and(|available| available.contains(grant))
                        || (scope != "*"
                            && installed
                                .get("*")
                                .is_some_and(|available| available.contains(grant)))
                })
            })
        }) {
            return Err(AlienError::new(ErrorData::SetupRequired {
                message: "The registered management grants do not match this release. Regenerate and apply setup, then register its outputs before retrying".to_string(),
                resource_id: Some(id.to_string()),
            }));
        }
    }
    Ok(())
}

/// Whether the management identity's permissions should be refreshed.
///
/// Management permissions are installed by setup and never refreshed by a
/// release update. Only an Alien direct setup (administrator credentials,
/// `alien deploy`) re-applies them when they changed; under runtime
/// credentials this is never true, because the runtime may not change its
/// own role and the update executor includes Frozen resources.
pub(crate) fn management_permissions_need_refresh(
    ctx: &ResourceControllerContext<'_>,
    applied_revision: Option<&str>,
) -> Result<bool> {
    if ctx.initial_setup_authority != InitialSetupAuthority::DirectSetup {
        return Ok(false);
    }
    // Setup installs the infrastructure grant. Runtime may
    // use it, but must never rewrite its own management identity or remove its guard.
    if !ctx.deployment_config.setup_run {
        return Ok(false);
    }
    Ok(management_permissions_revision(ctx)?.as_deref() != applied_revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::ResourceController;
    use crate::core::StackExecutor;
    use crate::{
        AwsRemoteStackManagementController, AzureRemoteStackManagementController,
        GcpRemoteStackManagementController,
    };
    use alien_core::{
        ManagementPermissions, Platform, RemoteStackManagement, Resource, ResourceEntry,
        ResourceLifecycle, ResourceStatus, StackResourceState, Storage,
    };

    async fn executor(
        platform: Platform,
        controller: impl ResourceController + 'static,
        authority: InitialSetupAuthority,
        setup_run: bool,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(RemoteStackManagement::new("management".to_string()).build())
            .controller(controller)
            .platform(platform)
            .initial_setup_authority(authority)
            .setup_run(setup_run)
            .build()
            .await
            .expect("executor builds")
    }

    /// Per cloud: a controller that applied `applied` revision.
    async fn needs_update(
        platform: Platform,
        authority: InitialSetupAuthority,
        applied: Option<&str>,
        setup_run: bool,
    ) -> (bool, String) {
        let applied = applied.map(str::to_string);
        let executor = match platform {
            Platform::Aws => {
                let mut controller = AwsRemoteStackManagementController::mock_ready("test");
                controller.management_permissions_revision = applied;
                executor(platform, controller, authority, setup_run).await
            }
            Platform::Gcp => {
                let mut controller = GcpRemoteStackManagementController::mock_ready("test");
                controller.management_permissions_revision = applied;
                executor(platform, controller, authority, setup_run).await
            }
            Platform::Azure => {
                let mut controller = AzureRemoteStackManagementController::mock_ready("test");
                controller.management_permissions_revision = applied;
                executor(platform, controller, authority, setup_run).await
            }
            other => panic!("no remote stack management controller for {other:?}"),
        };
        let current = executor
            .with_context(management_permissions_revision)
            .expect("revision")
            .expect("the test stack has a management profile");
        (executor.needs_update().expect("needs_update"), current)
    }

    #[tokio::test]
    async fn direct_setup_refreshes_management_permissions_that_changed() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let (stale, current) = needs_update(
                platform,
                InitialSetupAuthority::DirectSetup,
                Some("stale"),
                true,
            )
            .await;
            assert!(stale, "{platform:?}: a changed revision is refreshed");
            let (legacy, _) =
                needs_update(platform, InitialSetupAuthority::DirectSetup, None, true).await;
            assert!(
                legacy,
                "{platform:?}: an install without a recorded revision is refreshed"
            );
            let (unchanged, _) = needs_update(
                platform,
                InitialSetupAuthority::DirectSetup,
                Some(&current),
                true,
            )
            .await;
            assert!(
                !unchanged,
                "{platform:?}: the applied revision is left alone"
            );
        }
    }

    /// An Azure install from before per-scope roles still has the combined role, which is assigned
    /// at every binding scope. A direct setup replaces it even when the permissions are unchanged;
    /// runtime credentials still never touch the management role.
    #[tokio::test]
    async fn direct_setup_replaces_the_legacy_combined_azure_role() {
        for (authority, setup_run, expected) in [
            (InitialSetupAuthority::DirectSetup, true, true),
            (InitialSetupAuthority::ImportedHandoff, true, false),
            (InitialSetupAuthority::DirectSetup, false, false),
            (InitialSetupAuthority::ImportedHandoff, false, false),
        ] {
            // The applied revision is current, so only the legacy role can ask for the update.
            let (_, current) = needs_update(Platform::Azure, authority, None, setup_run).await;
            let mut controller = AzureRemoteStackManagementController::mock_ready("test");
            controller.management_permissions_revision = Some(current);
            controller.role_definition_id = Some(
                "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleDefinitions/legacy"
                    .to_string(),
            );
            let executor = executor(Platform::Azure, controller, authority, setup_run).await;
            assert_eq!(
                executor.needs_update().expect("needs_update"),
                expected,
                "{authority:?} with setup_run={setup_run}"
            );
        }
    }

    #[tokio::test]
    async fn runtime_credentials_never_refresh_management_permissions() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            for applied in [Some("stale"), None] {
                for authority in [
                    InitialSetupAuthority::ImportedHandoff,
                    InitialSetupAuthority::DirectSetup,
                ] {
                    let (refresh, _) = needs_update(platform, authority, applied, false).await;
                    assert!(!refresh, "{platform:?} with {authority:?} and {applied:?}");
                }
            }
        }
    }
    #[tokio::test]
    async fn runtime_planning_and_resumed_updates_require_installed_grants() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let fixture = match platform {
                Platform::Aws => {
                    executor(
                        platform,
                        AwsRemoteStackManagementController::mock_ready("management"),
                        InitialSetupAuthority::ImportedHandoff,
                        false,
                    )
                    .await
                }
                Platform::Gcp => {
                    executor(
                        platform,
                        GcpRemoteStackManagementController::mock_ready("management"),
                        InitialSetupAuthority::ImportedHandoff,
                        false,
                    )
                    .await
                }
                Platform::Azure => {
                    executor(
                        platform,
                        AzureRemoteStackManagementController::mock_ready("management"),
                        InitialSetupAuthority::ImportedHandoff,
                        false,
                    )
                    .await
                }
                _ => unreachable!(),
            };
            let (mut stack, config, client) = fixture.with_context(|ctx| {
                (
                    ctx.desired_stack.clone(),
                    ctx.deployment_config.clone(),
                    ctx.client_config.clone(),
                )
            });
            stack.resources.get_mut("management").unwrap().lifecycle = ResourceLifecycle::Frozen;
            let storage = Resource::new(Storage::new("pending-storage".to_string()).build());
            stack.resources.insert(
                "pending-storage".to_string(),
                ResourceEntry {
                    config: storage.clone(),
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: vec![],
                    remote_access: false,
                    enabled_when: None,
                },
            );
            let revision = alien_permissions::management_grant_revisions(
                stack.management().profile(),
                platform,
            )
            .unwrap()
            .unwrap();
            let runtime = StackExecutor::builder(&stack, client.clone())
                .deployment_config(&config)
                .lifecycle_filter(vec![ResourceLifecycle::Live])
                .step_out_of_scope_resources(false)
                .build()
                .unwrap();
            for installed in [
                None,
                Some(std::collections::BTreeMap::new()),
                Some(revision.clone()),
            ] {
                let mut state = fixture.stack_state().clone();
                let management = state.resources.get_mut("management").unwrap();
                management.lifecycle = Some(ResourceLifecycle::Frozen);
                management.internal_state.as_mut().unwrap()["installedManagementGrants"] =
                    serde_json::json!(installed);
                if installed.as_ref() == Some(&revision) {
                    assert_eq!(runtime.plan(&state).unwrap().creates, ["pending-storage"]);
                    let mut narrowed = stack.clone();
                    let mut profile = stack.management().profile().unwrap().clone();
                    for grants in profile.0.values_mut() {
                        grants.pop();
                    }
                    narrowed.permissions.management = ManagementPermissions::Override(profile);
                    validate_installed_management_permissions(
                        &narrowed,
                        &state,
                        InitialSetupAuthority::ImportedHandoff,
                    )
                    .unwrap();
                    let mut scoped = stack.clone();
                    let mut profile = stack.management().profile().unwrap().clone();
                    if let Some(global) = profile.0.shift_remove("*") {
                        profile.0.insert("pending-storage".to_string(), global);
                    }
                    scoped.permissions.management = ManagementPermissions::Override(profile);
                    validate_installed_management_permissions(
                        &scoped,
                        &state,
                        InitialSetupAuthority::ImportedHandoff,
                    )
                    .unwrap();
                    let mut expanded = stack.clone();
                    let mut profile = stack.management().profile().unwrap().clone();
                    profile.0.insert(
                        "new-resource-scope".to_string(),
                        vec![alien_core::PermissionSetReference::from_name(
                            "worker/provision",
                        )],
                    );
                    expanded.permissions.management = ManagementPermissions::Override(profile);
                    assert_eq!(
                        validate_installed_management_permissions(
                            &expanded,
                            &state,
                            InitialSetupAuthority::ImportedHandoff
                        )
                        .unwrap_err()
                        .code,
                        "DEPLOYMENT_SETUP_REQUIRED"
                    );
                    continue;
                }
                assert_eq!(
                    runtime.plan(&state).unwrap_err().code,
                    "DEPLOYMENT_SETUP_REQUIRED"
                );
                assert_eq!(
                    runtime.step(state.clone()).await.unwrap_err().code,
                    "DEPLOYMENT_SETUP_REQUIRED"
                );
                let mut pending = StackResourceState::new_pending(
                    Storage::RESOURCE_TYPE.to_string(),
                    storage.clone(),
                    Some(ResourceLifecycle::Live),
                    vec![],
                );
                pending.status = ResourceStatus::Updating;
                state
                    .resources
                    .insert("pending-storage".to_string(), pending);
                assert_eq!(
                    runtime.refresh(state.clone()).await.unwrap_err().code,
                    "DEPLOYMENT_SETUP_REQUIRED"
                );
                state.resources.get_mut("pending-storage").unwrap().status =
                    ResourceStatus::Running;
                let mut cleanup_stack = stack.clone();
                cleanup_stack.resources.shift_remove("pending-storage");
                let cleanup = StackExecutor::builder(&cleanup_stack, client.clone())
                    .deployment_config(&config)
                    .lifecycle_filter(vec![ResourceLifecycle::Live])
                    .step_out_of_scope_resources(false)
                    .build()
                    .unwrap();
                assert_eq!(cleanup.plan(&state).unwrap().deletes, ["pending-storage"]);
            }
        }
    }
}
