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

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::InitialSetupAuthority;
use alien_error::{Context, IntoAlienError};
use alien_permissions::get_permission_set;
use sha2::{Digest, Sha256};

/// A revision of the management permissions the stack asks for: a digest of
/// the management profile with every permission set it names resolved to its
/// body. A release that changes a permission set changes the revision.
pub(crate) fn management_permissions_revision(
    ctx: &ResourceControllerContext<'_>,
) -> Result<Option<String>> {
    let Some(profile) = ctx.desired_stack.management().profile() else {
        return Ok(None);
    };
    let resolved = profile
        .0
        .iter()
        .map(|(scope, references)| {
            let sets = references
                .iter()
                .map(|reference| reference.resolve(|name| get_permission_set(name).cloned()))
                .collect::<Vec<_>>();
            (scope, sets)
        })
        .collect::<Vec<_>>();
    let serialized = serde_json::to_vec(&resolved).into_alien_error().context(
        ErrorData::InfrastructureError {
            message: "Failed to serialize the management permissions".to_string(),
            operation: Some("management_permissions_revision".to_string()),
            resource_id: None,
        },
    )?;
    Ok(Some(format!("{:x}", Sha256::digest(&serialized))))
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
    use crate::{
        AwsRemoteStackManagementController, AzureRemoteStackManagementController,
        GcpRemoteStackManagementController,
    };
    use alien_core::{Platform, RemoteStackManagement};

    async fn executor(
        platform: Platform,
        controller: impl ResourceController + 'static,
        authority: InitialSetupAuthority,
        setup_run: bool,
    ) -> SingleControllerExecutor {
        let builder = SingleControllerExecutor::builder()
            .resource(RemoteStackManagement::new("management".to_string()).build())
            .controller(controller)
            .platform(platform)
            .initial_setup_authority(authority)
            .setup_run(setup_run);
        let builder = if platform == Platform::Gcp {
            builder
                .gcp_federation_scope(alien_core::GcpFederationScope {
                    deployment_id: "dep_mock_fixture".to_string(),
                })
                .management_config(alien_core::ManagementConfig::Gcp(
                    alien_core::GcpManagementConfig {
                        service_account_email: "manager@example.iam.gserviceaccount.com"
                            .to_string(),
                        federation_trust: Some(alien_core::GcpFederationTrust {
                            issuer: "https://accounts.google.com".to_string(),
                            subject: "123456789012345678901".to_string(),
                        }),
                    },
                ))
        } else {
            builder
        };
        builder.build().await.expect("executor builds")
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
                // Hold the runtime roles' revision current, so only the management
                // permissions' revision is under test.
                let probe = executor(
                    platform,
                    AzureRemoteStackManagementController::mock_ready("test"),
                    authority,
                    setup_run,
                )
                .await;
                let runtime = probe
                    .with_context(super::azure::current_runtime_roles_revision)
                    .expect("runtime roles revision");
                let mut controller = AzureRemoteStackManagementController::mock_ready("test");
                controller.management_permissions_revision = applied;
                controller.runtime_roles_revision = Some(runtime);
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
                unchanged == (platform == Platform::Aws),
                "{platform:?}: AWS setup must also install its missing boundary"
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

    /// An Azure direct setup saved before role-assignment writes had their own conditioned role
    /// rebuilds its roles on the next setup run, although its permissions are unchanged. Only
    /// setup credentials do; the runtime never rewrites the management roles.
    #[tokio::test]
    async fn direct_setup_rebuilds_azure_roles_saved_before_the_role_assignment_role() {
        for (authority, setup_run, expected) in [
            (InitialSetupAuthority::DirectSetup, true, true),
            (InitialSetupAuthority::ImportedHandoff, true, false),
            (InitialSetupAuthority::DirectSetup, false, false),
        ] {
            let (_, current) = needs_update(Platform::Azure, authority, None, setup_run).await;
            let mut controller = AzureRemoteStackManagementController::mock_ready("test");
            controller.management_permissions_revision = Some(current);
            // Saved by the previous version: no layout recorded.
            controller.role_layout = 0;
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
}
