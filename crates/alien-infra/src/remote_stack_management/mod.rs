mod aws;
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
    let expected = alien_permissions::management_permissions_revision(stack.management().profile())
        .context(ErrorData::InfrastructureError {
            message: "Failed to resolve required management permissions".to_string(),
            operation: Some("validate_installed_management_permissions".to_string()),
            resource_id: None,
        })?;
    let Some(expected) = expected else {
        return Ok(());
    };
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
                    .management_permissions_revision
            }
            Platform::Gcp => {
                installed
                    .get_internal_controller_typed::<GcpRemoteStackManagementController>()?
                    .management_permissions_revision
            }
            Platform::Azure => {
                installed
                    .get_internal_controller_typed::<AzureRemoteStackManagementController>()?
                    .management_permissions_revision
            }
            _ => continue,
        };
        if applied.as_deref() != Some(expected.as_str()) {
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
        Platform, RemoteStackManagement, Resource, ResourceEntry, ResourceLifecycle,
        ResourceStatus, StackResourceState, Worker, WorkerCode,
    };

    async fn executor(
        platform: Platform,
        controller: impl ResourceController + 'static,
        authority: InitialSetupAuthority,
    ) -> SingleControllerExecutor {
        SingleControllerExecutor::builder()
            .resource(RemoteStackManagement::new("management".to_string()).build())
            .controller(controller)
            .platform(platform)
            .initial_setup_authority(authority)
            .build()
            .await
            .expect("executor builds")
    }

    /// Per cloud: a controller that applied `applied` revision.
    async fn needs_update(
        platform: Platform,
        authority: InitialSetupAuthority,
        applied: Option<&str>,
    ) -> (bool, String) {
        let applied = applied.map(str::to_string);
        let executor = match platform {
            Platform::Aws => {
                let mut controller = AwsRemoteStackManagementController::mock_ready("test");
                controller.management_permissions_revision = applied;
                executor(platform, controller, authority).await
            }
            Platform::Gcp => {
                let mut controller = GcpRemoteStackManagementController::mock_ready("test");
                controller.management_permissions_revision = applied;
                executor(platform, controller, authority).await
            }
            Platform::Azure => {
                let mut controller = AzureRemoteStackManagementController::mock_ready("test");
                controller.management_permissions_revision = applied;
                executor(platform, controller, authority).await
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
            let (stale, current) =
                needs_update(platform, InitialSetupAuthority::DirectSetup, Some("stale")).await;
            assert!(stale, "{platform:?}: a changed revision is refreshed");
            let (legacy, _) =
                needs_update(platform, InitialSetupAuthority::DirectSetup, None).await;
            assert!(
                legacy,
                "{platform:?}: an install without a recorded revision is refreshed"
            );
            let (unchanged, _) =
                needs_update(platform, InitialSetupAuthority::DirectSetup, Some(&current)).await;
            assert!(
                !unchanged,
                "{platform:?}: the applied revision is left alone"
            );
        }
    }

    #[tokio::test]
    async fn runtime_credentials_never_refresh_management_permissions() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            for applied in [Some("stale"), None] {
                let (refresh, _) =
                    needs_update(platform, InitialSetupAuthority::ImportedHandoff, applied).await;
                assert!(!refresh, "{platform:?} with {applied:?}");
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
                    )
                    .await
                }
                Platform::Gcp => {
                    executor(
                        platform,
                        GcpRemoteStackManagementController::mock_ready("management"),
                        InitialSetupAuthority::ImportedHandoff,
                    )
                    .await
                }
                Platform::Azure => {
                    executor(
                        platform,
                        AzureRemoteStackManagementController::mock_ready("management"),
                        InitialSetupAuthority::ImportedHandoff,
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
            let worker = Resource::new(
                Worker::new("pending-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "example.invalid/worker:test".to_string(),
                    })
                    .build(),
            );
            stack.resources.insert(
                "pending-worker".to_string(),
                ResourceEntry {
                    config: worker.clone(),
                    lifecycle: ResourceLifecycle::Live,
                    dependencies: vec![],
                    remote_access: false,
                    enabled_when: None,
                },
            );
            let revision =
                alien_permissions::management_permissions_revision(stack.management().profile())
                    .unwrap()
                    .unwrap();
            let runtime = StackExecutor::builder(&stack, client)
                .deployment_config(&config)
                .lifecycle_filter(vec![ResourceLifecycle::Live])
                .step_out_of_scope_resources(false)
                .build()
                .unwrap();
            for installed in [None, Some("older-grants"), Some(revision.as_str())] {
                let mut state = fixture.stack_state().clone();
                let management = state.resources.get_mut("management").unwrap();
                management.lifecycle = Some(ResourceLifecycle::Frozen);
                management.internal_state.as_mut().unwrap()["managementPermissionsRevision"] =
                    serde_json::json!(installed);
                if installed == Some(revision.as_str()) {
                    assert_eq!(runtime.plan(&state).unwrap().creates, ["pending-worker"]);
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
                    "worker".to_string(),
                    worker.clone(),
                    Some(ResourceLifecycle::Live),
                    vec![],
                );
                pending.status = ResourceStatus::Updating;
                state
                    .resources
                    .insert("pending-worker".to_string(), pending);
                assert_eq!(
                    runtime.refresh(state).await.unwrap_err().code,
                    "DEPLOYMENT_SETUP_REQUIRED"
                );
            }
        }
    }
}
