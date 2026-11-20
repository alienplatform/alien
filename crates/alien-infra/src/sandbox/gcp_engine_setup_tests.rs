//! A direct Google Cloud setup of a sandbox places the preview signer and the engine grants the
//! package would; an install a package made has them already, so the controller touches nothing.

use std::sync::{Arc, Mutex};

use alien_core::permissions::PermissionProfile;
use alien_core::{
    InitialSetupAuthority, Platform, ResourceLifecycle, ResourceStatus, Sandbox, SandboxCode,
    ServiceAccount,
};
use alien_error::AlienError;
use alien_gcp_clients::agent_platform::MockAgentPlatformApi;
use alien_gcp_clients::iam::{Binding, IamPolicy, MockIamApi};

use crate::core::controller_test::SingleControllerExecutor;
use crate::core::MockPlatformServiceProvider;
use crate::core::StackResourceStateExt as _;
use crate::remote_stack_management::GcpRemoteStackManagementController;
use crate::sandbox::gcp_engine_setup::{preview_signer_account_id, SIGN_JWT_PERMISSION};
use crate::sandbox::{GcpAgentPlatformEngineController, GcpAgentPlatformTemplateController};
use crate::service_account::GcpServiceAccountController;
use alien_core::GcpAgentPlatformEngine;
use alien_core::RemoteStackManagement;
use alien_gcp_clients::agent_platform::SandboxEnvironmentTemplate;
use alien_gcp_clients::longrunning::{Operation, OperationResult};

const PROJECT: &str = "test-project-123";
const MANAGER: &str = "manager@test-project-123.iam.gserviceaccount.com";
const CALLER: &str = "caller@test-project-123.iam.gserviceaccount.com";
const WORKLOAD: &str = "workload@test-project-123.iam.gserviceaccount.com";
const ADDED: &str = "added@test-project-123.iam.gserviceaccount.com";

fn op(done: bool) -> Operation {
    Operation {
        name: Some("projects/p/locations/l/operations/op1".to_string()),
        metadata: None,
        done: Some(done),
        result: done.then(|| OperationResult::Response {
            response: serde_json::json!({
                "name": "projects/p/locations/l/reasoningEngines/eng/sandboxEnvironmentTemplates/tpl1",
                "state": "CREATING"
            }),
        }),
    }
}

fn active() -> SandboxEnvironmentTemplate {
    SandboxEnvironmentTemplate {
        name: Some(
            "projects/p/locations/l/reasoningEngines/eng/sandboxEnvironmentTemplates/tpl1".into(),
        ),
        display_name: None,
        custom_container_environment: None,
        egress_control_config: None,
        state: Some("ACTIVE".to_string()),
        extra: Default::default(),
    }
}

#[derive(Default)]
struct Calls {
    created_accounts: Vec<String>,
    deleted_accounts: Vec<String>,
    created_roles: Vec<String>,
    sign_role_permissions: Vec<String>,
    role_permissions: std::collections::BTreeMap<String, Vec<String>>,
    deleted_roles: Vec<String>,
    signer_policy: Option<IamPolicy>,
    engine_policy: Option<IamPolicy>,
}

fn sandbox(ports: Vec<u16>, egress: alien_core::SandboxEgress) -> Sandbox {
    Sandbox::new("box".to_string())
        .code(SandboxCode::Image {
            image: "ubuntu".to_string(),
            bundled_from: None,
        })
        .egress(egress)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
            ..Default::default()
        })
        .preview_ports(ports)
        .build()
}

fn not_found(name: String) -> alien_client_core::Error {
    AlienError::new(alien_client_core::ErrorData::RemoteResourceNotFound {
        resource_type: "x".to_string(),
        resource_name: name,
    })
}

fn clients(calls: &Arc<Mutex<Calls>>) -> (MockIamApi, MockAgentPlatformApi) {
    clients_with_signer(calls, None)
}

/// `existing` is the description of a signer account that is already there, so the create is a
/// conflict and the read returns it.
fn clients_with_signer(
    calls: &Arc<Mutex<Calls>>,
    existing: Option<String>,
) -> (MockIamApi, MockAgentPlatformApi) {
    let mut iam = MockIamApi::new();
    let c = calls.clone();
    let conflict = existing.is_some();
    iam.expect_create_service_account().returning(move |id, _| {
        if conflict {
            return Err(AlienError::new(
                alien_client_core::ErrorData::RemoteResourceConflict {
                    message: "exists".to_string(),
                    resource_type: "ServiceAccount".to_string(),
                    resource_name: id,
                },
            ));
        }
        c.lock().unwrap().created_accounts.push(id.clone());
        Ok(alien_gcp_clients::iam::ServiceAccount::builder()
            .email(format!("{id}@{PROJECT}.iam.gserviceaccount.com"))
            .build())
    });
    iam.expect_get_service_account().returning(move |_| {
        Ok(alien_gcp_clients::iam::ServiceAccount::builder()
            .description(existing.clone().unwrap_or_default())
            .build())
    });
    let c = calls.clone();
    iam.expect_delete_service_account().returning(move |name| {
        c.lock().unwrap().deleted_accounts.push(name);
        Ok(())
    });
    iam.expect_get_role().returning(|name| Err(not_found(name)));
    let c = calls.clone();
    iam.expect_create_role().returning(move |id, request| {
        let mut calls = c.lock().unwrap();
        if id.ends_with("sandbox_preview_sign") {
            calls.sign_role_permissions = request.role.included_permissions.clone();
        }
        calls
            .role_permissions
            .insert(id.clone(), request.role.included_permissions.clone());
        calls.created_roles.push(id);
        drop(calls);
        Ok(alien_gcp_clients::iam::Role::builder().build())
    });
    let c = calls.clone();
    iam.expect_delete_role().returning(move |name| {
        c.lock().unwrap().deleted_roles.push(name);
        Ok(alien_gcp_clients::iam::Role::builder().build())
    });
    iam.expect_get_service_account_iam_policy()
        .returning(|_| Ok(IamPolicy::default()));
    let c = calls.clone();
    iam.expect_set_service_account_iam_policy()
        .returning(move |_, policy| {
            c.lock().unwrap().signer_policy = Some(policy.clone());
            Ok(policy)
        });

    let mut platform = MockAgentPlatformApi::new();
    platform
        .expect_create_template()
        .returning(|_, _| Ok(op(false)));
    platform.expect_get_operation().returning(|_| Ok(op(true)));
    platform
        .expect_get_template()
        .returning(|_, _| Ok(active()));
    platform
        .expect_list_templates()
        .returning(|_| Ok(vec![active()]));
    platform.expect_delete_template().returning(|_, _| Ok(()));
    // The engine keeps what was written, as the cloud does, so a revoke has something to remove.
    let c = calls.clone();
    platform
        .expect_get_engine_iam_policy()
        .returning(move |_| Ok(c.lock().unwrap().engine_policy.clone().unwrap_or_default()));
    let c = calls.clone();
    platform
        .expect_set_engine_iam_policy()
        .returning(move |_, policy| {
            let mut calls = c.lock().unwrap();
            calls.engine_policy = Some(policy.clone());
            Ok(policy)
        });
    (iam, platform)
}

async fn executor(
    iam: MockIamApi,
    platform: MockAgentPlatformApi,
    setup_run: bool,
) -> SingleControllerExecutor {
    executor_with(iam, platform, setup_run, false).await
}

async fn executor_with(
    iam: MockIamApi,
    platform: MockAgentPlatformApi,
    setup_run: bool,
    management: bool,
) -> SingleControllerExecutor {
    executor_full(iam, platform, setup_run, management, false).await
}

async fn executor_full(
    iam: MockIamApi,
    platform: MockAgentPlatformApi,
    setup_run: bool,
    management: bool,
    remote: bool,
) -> SingleControllerExecutor {
    executor_seeded(
        iam,
        platform,
        setup_run,
        management,
        remote,
        GcpAgentPlatformTemplateController::default(),
    )
    .await
}

async fn executor_seeded(
    iam: MockIamApi,
    platform: MockAgentPlatformApi,
    setup_run: bool,
    management: bool,
    remote: bool,
    controller: GcpAgentPlatformTemplateController,
) -> SingleControllerExecutor {
    executor_seeded_as(
        InitialSetupAuthority::DirectSetup,
        iam,
        platform,
        setup_run,
        management,
        remote,
        controller,
    )
    .await
}

async fn executor_seeded_as(
    authority: InitialSetupAuthority,
    iam: MockIamApi,
    platform: MockAgentPlatformApi,
    setup_run: bool,
    management: bool,
    remote: bool,
    controller: GcpAgentPlatformTemplateController,
) -> SingleControllerExecutor {
    let iam = Arc::new(iam);
    let platform = Arc::new(platform);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_gcp_iam_client()
        .returning(move |_| Ok(iam.clone()));
    provider
        .expect_get_gcp_agent_platform_client()
        .returning(move |_| Ok(platform.clone()));
    let mut builder = SingleControllerExecutor::builder()
        // A remote caller is delivered only a sandbox with egress allow and no preview ports.
        .resource(if remote {
            sandbox(vec![], alien_core::SandboxEgress::Allow)
        } else {
            sandbox(vec![8080], alien_core::SandboxEgress::None)
        })
        .resource_prefix("test")
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(controller)
        .platform(Platform::Gcp)
        .initial_setup_authority(authority)
        .setup_run(setup_run)
        .with_dependency(
            GcpAgentPlatformEngine::new(GcpAgentPlatformEngine::id_for_sandbox("box")).build(),
            GcpAgentPlatformEngineController::mock_ready("eng"),
        )
        .with_dependency(
            ServiceAccount::new("workload-sa".to_string()).build(),
            GcpServiceAccountController {
                service_account_email: Some(WORKLOAD.to_string()),
                ..Default::default()
            },
        )
        // An identity for a profile the stack gains later; it holds nothing until the profile exists.
        .with_dependency(
            ServiceAccount::new("added-sa".to_string()).build(),
            GcpServiceAccountController {
                service_account_email: Some(ADDED.to_string()),
                ..Default::default()
            },
        )
        .permission_profile(
            "workload",
            PermissionProfile::new().global(["sandbox/execute"]),
        )
        .service_provider(Arc::new(provider));
    if management {
        builder = builder
            .management_permissions(alien_core::permissions::ManagementPermissions::extend(
                PermissionProfile::new().resource("box", ["sandbox/templates"]),
            ))
            .with_dependency(
                RemoteStackManagement::new("management".to_string()).build(),
                GcpRemoteStackManagementController {
                    service_account_email: Some(MANAGER.to_string()),
                    ..Default::default()
                },
            );
    }
    if remote {
        builder = builder.remote_access(true).with_dependency(
            alien_core::RemoteBindings::new("access".to_string()).build(),
            crate::remote_bindings::GcpRemoteBindingsController {
                service_account_email: Some(CALLER.to_string()),
                ..Default::default()
            },
        );
    }
    builder.build().await.unwrap()
}

fn members(policy: &IamPolicy, role_contains: &str) -> Vec<String> {
    policy
        .bindings
        .iter()
        .filter(|b: &&Binding| b.role.contains(role_contains))
        .flat_map(|b| b.members.clone())
        .collect()
}

#[tokio::test]
async fn a_direct_setup_places_the_preview_signer_and_its_grants_then_removes_them() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (iam, platform) = clients(&calls);
    let mut executor = executor(iam, platform, true).await;
    executor.run_until_terminal().await.expect("create runs");
    assert_eq!(executor.status(), ResourceStatus::Running);

    let signer_id = preview_signer_account_id("test", "box");
    let signer = format!("serviceAccount:{signer_id}@{PROJECT}.iam.gserviceaccount.com");
    {
        let calls = calls.lock().unwrap();
        assert_eq!(calls.created_accounts, vec![signer_id.clone()]);
        assert_eq!(
            calls.sign_role_permissions,
            vec![SIGN_JWT_PERMISSION.to_string()],
            "the role signs and nothing else"
        );
        let sign_role = calls
            .created_roles
            .iter()
            .find(|id| id.ends_with("sandbox_preview_sign"))
            .expect("the signJwt role is created");
        assert!(sign_role.starts_with("role_"), "{sign_role}");
        // The workload may sign as the signer, and only through the signJwt role.
        let on_signer = calls.signer_policy.as_ref().expect("signer policy set");
        assert_eq!(
            members(on_signer, "sandbox_preview_sign"),
            vec![format!("serviceAccount:{WORKLOAD}")]
        );
        // The signer holds the preview-edge verb on the engine and nothing else is granted to it.
        let on_engine = calls.engine_policy.as_ref().expect("engine policy set");
        let held: Vec<_> = on_engine
            .bindings
            .iter()
            .filter(|b| b.members.contains(&signer))
            .collect();
        assert_eq!(held.len(), 1, "{on_engine:?}");
        assert!(held[0].role.contains("preview"), "{}", held[0].role);
    }
    let controller = executor
        .internal_state::<GcpAgentPlatformTemplateController>()
        .unwrap();
    assert_eq!(
        controller.preview_service_account.as_deref(),
        Some(format!("{signer_id}@{PROJECT}.iam.gserviceaccount.com").as_str()),
        "the binding hands the signer to the workload"
    );

    executor.delete().expect("delete is accepted");
    executor.run_until_terminal().await.expect("delete runs");
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    let calls = calls.lock().unwrap();
    assert_eq!(calls.deleted_accounts.len(), 1, "the signer is removed");
    assert!(
        calls.deleted_roles.is_empty(),
        "the shared signing role outlives one sandbox"
    );
    let on_engine = calls.engine_policy.as_ref().unwrap();
    assert!(
        !on_engine
            .bindings
            .iter()
            .any(|b| b.members.contains(&signer)),
        "the engine grant is revoked: {on_engine:?}"
    );
}

/// Not a direct setup run (a package made the install): nothing is created or granted, and
/// the mocks have no expectation for it.
#[tokio::test]
async fn a_run_that_is_not_direct_setup_touches_no_signer_or_grant() {
    let platform = {
        let mut p = MockAgentPlatformApi::new();
        p.expect_create_template().returning(|_, _| Ok(op(false)));
        p.expect_get_operation().returning(|_| Ok(op(true)));
        p.expect_get_template().returning(|_, _| Ok(active()));
        p.expect_list_templates().returning(|_| Ok(vec![active()]));
        p
    };
    let mut executor = executor(MockIamApi::new(), platform, false).await;
    executor.run_until_terminal().await.expect("create runs");
    let controller = executor
        .internal_state::<GcpAgentPlatformTemplateController>()
        .unwrap();
    assert!(controller.preview_service_account.is_none());
    assert_eq!(controller.engine_setup, Default::default());
}

/// The management identity holds the template verb on the engine, as the package grants it, so a
/// remote manager can replace templates of an install made directly.
#[tokio::test]
async fn a_direct_setup_lets_management_replace_templates_on_the_engine() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (iam, platform) = clients(&calls);
    let mut executor = executor_with(iam, platform, true, true).await;
    executor.run_until_terminal().await.expect("create runs");
    let calls = calls.lock().unwrap();
    let on_engine = calls.engine_policy.as_ref().expect("engine policy set");
    let manager = format!("serviceAccount:{MANAGER}");
    let held: Vec<_> = on_engine
        .bindings
        .iter()
        .filter(|b| b.members.contains(&manager))
        .collect();
    assert_eq!(held.len(), 1, "{on_engine:?}");
    assert!(held[0].role.contains("templates"), "{}", held[0].role);
}

/// The stack's remote caller holds the remote-execute verb on this engine, as the package grants
/// it, so a sandbox published for remote use has a caller that works on a direct install.
#[tokio::test]
async fn a_direct_setup_gives_the_remote_caller_its_engine_grant() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (iam, platform) = clients(&calls);
    let mut executor = executor_full(iam, platform, true, false, true).await;
    executor.run_until_terminal().await.expect("create runs");
    let calls = calls.lock().unwrap();
    let on_engine = calls.engine_policy.as_ref().expect("engine policy set");
    let caller = format!("serviceAccount:{CALLER}");
    let held: Vec<_> = on_engine
        .bindings
        .iter()
        .filter(|b| b.members.contains(&caller))
        .collect();
    assert_eq!(held.len(), 1, "{on_engine:?}");
    assert!(
        held[0].role.ends_with("sandbox_lifecycle_and_exec"),
        "{}",
        held[0].role
    );
}

/// An account already there under the signer's name, with nothing in it that this deployment's
/// setup wrote, is somebody else's: setup stops, records nothing, and teardown deletes nothing.
#[tokio::test]
async fn a_signer_account_this_setup_did_not_make_is_refused_and_never_deleted() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (iam, platform) = clients_with_signer(&calls, Some("made by hand".to_string()));
    let mut executor = executor(iam, platform, true).await;
    let _ = executor.run_until_terminal().await;
    assert_ne!(executor.status(), ResourceStatus::Running);
    let controller = executor
        .internal_state::<GcpAgentPlatformTemplateController>()
        .unwrap();
    assert!(controller.engine_setup.signer_created.is_none());
    executor.delete().ok();
    let _ = executor.run_until_terminal().await;
    assert!(
        calls.lock().unwrap().deleted_accounts.is_empty(),
        "an account the setup did not make is not deleted"
    );
}

/// The signer carries this deployment's marker, so an earlier attempt of this setup made it: the
/// retry adopts it, records it, and teardown removes it.
#[tokio::test]
async fn a_signer_account_an_earlier_attempt_made_is_adopted_by_the_retry() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let marker = crate::sandbox::gcp_engine_setup::signer_marker("test", "box", "dep_test");
    let (iam, platform) = clients_with_signer(&calls, Some(format!("Signs. {marker}")));
    let mut executor = executor(iam, platform, true).await;
    executor.run_until_terminal().await.expect("create runs");
    assert_eq!(executor.status(), ResourceStatus::Running);
    executor.delete().expect("delete is accepted");
    executor.run_until_terminal().await.expect("delete runs");
    assert_eq!(calls.lock().unwrap().deleted_accounts.len(), 1);
}

/// An older manager rewrites the controller state without the setup record. The signer's own
/// description still names this deployment, so teardown removes it; an account without that
/// marker stays.
#[tokio::test]
async fn a_signer_the_state_forgot_is_removed_only_when_it_carries_this_deployments_marker() {
    let marker = crate::sandbox::gcp_engine_setup::signer_marker("test", "box", "dep_test");
    for (description, removed) in [
        (format!("Signs. {marker}"), 1),
        ("made by hand".to_string(), 0),
        (
            crate::sandbox::gcp_engine_setup::signer_marker("other", "box", "dep_test"),
            0,
        ),
        // Same prefix and sandbox, another deployment: never this deployment's to delete.
        (
            crate::sandbox::gcp_engine_setup::signer_marker("test", "box", "dep_other"),
            0,
        ),
    ] {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let (iam, platform) = clients_with_signer(&calls, Some(description));
        let mut executor = executor_seeded(
            iam,
            platform,
            false,
            false,
            false,
            GcpAgentPlatformTemplateController::mock_ready("eng", "tpl"),
        )
        .await;
        executor.delete().expect("delete is accepted");
        executor.run_until_terminal().await.expect("delete runs");
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert_eq!(calls.lock().unwrap().deleted_accounts.len(), removed);
    }
}

/// A package install (or a sandbox with no preview ports) has no signer of ours: teardown makes
/// no IAM call at all, because the management identity cannot read service accounts. The mock
/// has no expectation, so any call fails the test.
#[tokio::test]
async fn teardown_without_a_setup_record_makes_no_iam_call_for_a_package_install() {
    let mut platform = MockAgentPlatformApi::new();
    platform.expect_delete_template().returning(|_, _| Ok(()));
    let mut executor = executor_seeded_as(
        InitialSetupAuthority::ImportedHandoff,
        MockIamApi::new(),
        platform,
        false,
        false,
        false,
        GcpAgentPlatformTemplateController::mock_ready("eng", "tpl"),
    )
    .await;
    executor.delete().expect("delete is accepted");
    executor.run_until_terminal().await.expect("delete runs");
    assert_eq!(executor.status(), ResourceStatus::Deleted);
}

/// A signer delete that fails (not a 404) fails the delete and keeps the record, so a retry
/// removes the signer instead of forgetting it.
#[tokio::test]
async fn a_failed_signer_delete_fails_the_teardown_and_keeps_the_record() {
    let mut iam = MockIamApi::new();
    iam.expect_delete_service_account().returning(|_| {
        Err(AlienError::new(
            alien_client_core::ErrorData::RemoteServiceUnavailable {
                message: "backend error".to_string(),
            },
        ))
    });
    let mut platform = MockAgentPlatformApi::new();
    platform.expect_delete_template().returning(|_, _| Ok(()));
    let mut controller = GcpAgentPlatformTemplateController::mock_ready("eng", "tpl");
    controller.engine_setup.signer_created =
        Some(format!("signer@{PROJECT}.iam.gserviceaccount.com"));
    let mut executor = executor_seeded(iam, platform, false, false, false, controller).await;
    executor.delete().expect("delete is accepted");
    let _ = executor.run_until_terminal().await;
    // A 5xx is retryable, so the resource stays in Deleting (or DeleteFailed); never Deleted.
    assert_ne!(executor.status(), ResourceStatus::Deleted);
    let controller = executor
        .internal_state::<GcpAgentPlatformTemplateController>()
        .unwrap();
    assert!(controller.engine_setup.signer_created.is_some());
}

/// A rerun of setup removes an engine grant the stack no longer gives, and forgets it.
#[tokio::test]
async fn a_setup_rerun_revokes_an_engine_grant_the_stack_no_longer_gives() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let stale = (
        "projects/p/roles/old_role".to_string(),
        "serviceAccount:gone@test-project-123.iam.gserviceaccount.com".to_string(),
    );
    calls.lock().unwrap().engine_policy = Some(IamPolicy {
        bindings: vec![Binding {
            role: stale.0.clone(),
            members: vec![stale.1.clone()],
            condition: None,
        }],
        ..Default::default()
    });
    let (iam, platform) = clients(&calls);
    let mut controller = GcpAgentPlatformTemplateController::mock_ready("eng", "tpl");
    controller.engine_setup.engine_grants = vec![stale.clone()];
    let mut executor = executor_seeded(iam, platform, true, false, false, controller).await;
    executor
        .update(sandbox(vec![8080], alien_core::SandboxEgress::None))
        .expect("update is accepted");
    executor.run_until_terminal().await.expect("update runs");
    let calls = calls.lock().unwrap();
    let policy = calls.engine_policy.as_ref().expect("engine policy set");
    assert!(
        !policy.bindings.iter().any(|b| b.members.contains(&stale.1)),
        "{policy:?}"
    );
    drop(calls);
    let controller = executor
        .internal_state::<GcpAgentPlatformTemplateController>()
        .unwrap();
    assert!(!controller.engine_setup.engine_grants.contains(&stale));
}

/// A setup run again with nothing changed asks for no update; with a profile added since, it asks
/// for exactly one, which places the grant, and then none again.
#[tokio::test]
async fn a_setup_rerun_updates_only_when_the_profiles_changed() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (iam, platform) = clients(&calls);
    let mut executor = executor(iam, platform, true).await;
    executor.run_until_terminal().await.expect("create runs");
    assert!(
        !executor.needs_update().unwrap(),
        "a steady state asks for no update"
    );

    let signer_members = |calls: &Arc<Mutex<Calls>>| {
        let calls = calls.lock().unwrap();
        calls
            .signer_policy
            .as_ref()
            .map(|policy| members(policy, "sandbox_preview_sign"))
            .unwrap_or_default()
    };
    assert_eq!(
        signer_members(&calls),
        vec![format!("serviceAccount:{WORKLOAD}")]
    );

    executor.add_permission_profile(
        "added",
        PermissionProfile::new().resource("box", ["sandbox/execute"]),
    );
    assert!(
        executor.needs_update().unwrap(),
        "a new profile asks for the update"
    );
    executor
        .update(sandbox(vec![8080], alien_core::SandboxEgress::None))
        .expect("update is accepted");
    executor.run_until_terminal().await.expect("update runs");
    assert!(
        signer_members(&calls).contains(&format!("serviceAccount:{ADDED}")),
        "the new profile's identity may sign: {:?}",
        signer_members(&calls)
    );
    assert!(
        !executor.needs_update().unwrap(),
        "the update settled the check: exactly one"
    );
}

/// An install moved to another manager identity gets the management grant again: the identity is
/// an input of the reconcile, though the stack does not name it.
#[tokio::test]
async fn a_setup_rerun_updates_once_when_the_management_identity_changed() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (iam, platform) = clients(&calls);
    let mut executor = executor_full(iam, platform, true, true, false).await;
    executor.run_until_terminal().await.expect("create runs");
    assert!(
        !executor.needs_update().unwrap(),
        "a steady state asks for no update"
    );

    let mut saved = executor
        .stack_state()
        .resources
        .get("management")
        .expect("the management resource is in the state")
        .clone();
    saved
        .set_internal_controller(Some(Box::new(GcpRemoteStackManagementController {
            service_account_email: Some(
                "other-manager@test-project-123.iam.gserviceaccount.com".to_string(),
            ),
            ..Default::default()
        })))
        .unwrap();
    executor.insert_saved_resource("management", saved);
    assert!(
        executor.needs_update().unwrap(),
        "another management identity asks for the update"
    );
    executor
        .update(sandbox(vec![8080], alien_core::SandboxEgress::None))
        .expect("update is accepted");
    executor.run_until_terminal().await.expect("update runs");
    assert!(
        !executor.needs_update().unwrap(),
        "the update settled the check: exactly one"
    );
}

/// A setup run again, with a remote caller added since, places its engine grant.
#[tokio::test]
async fn a_setup_rerun_places_the_grants_a_new_caller_needs() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let (iam, platform) = clients(&calls);
    let mut executor = executor_full(iam, platform, true, false, true).await;
    executor.run_until_terminal().await.expect("create runs");
    // The caller's grant disappears (as if never placed), the rerun puts it back.
    calls.lock().unwrap().engine_policy = Some(IamPolicy::default());
    executor
        .update(sandbox(vec![], alien_core::SandboxEgress::Allow))
        .expect("update is accepted");
    executor.run_until_terminal().await.expect("update runs");
    let calls = calls.lock().unwrap();
    let on_engine = calls.engine_policy.as_ref().unwrap();
    assert!(
        on_engine
            .bindings
            .iter()
            .any(|b| b.members.contains(&format!("serviceAccount:{CALLER}"))),
        "{on_engine:?}"
    );
}

/// What each side grants, reduced to who holds which permissions where, so the Terraform package
/// and a direct setup can be compared without sharing a naming scheme. A role is its permission
/// list (or its predefined name); a member is the logical identity it stands for.
mod differential {
    use super::*;
    use alien_core::import::EmitContext;
    use alien_core::permissions::ManagementPermissions;
    use alien_core::{ResourceLifecycle, SandboxEgress, Stack, StackSettings};
    use alien_terraform::emitters::gcp::GcpAgentPlatformSandboxEmitter;
    use alien_terraform::TfEmitter;
    use hcl::structure::{Block, Structure};
    use std::collections::{BTreeMap, BTreeSet};

    type Grants = BTreeSet<(String, Vec<String>)>;

    #[derive(Debug, PartialEq, Eq)]
    struct Placed {
        engine: Grants,
        signer: Grants,
        signer_accounts: usize,
    }

    fn attribute(block: &Block, key: &str) -> Option<String> {
        block.body.iter().find_map(|s| match s {
            Structure::Attribute(a) if a.key.as_str() == key => Some(a.expr.to_string()),
            _ => None,
        })
    }

    fn perms_of_custom_role(block: &Block) -> Vec<String> {
        let body = attribute(block, "permissions").expect("a custom role lists permissions");
        let mut perms: Vec<String> = body
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|p| p.trim().trim_matches('"').to_string())
            .filter(|p| !p.is_empty())
            .collect();
        perms.sort();
        perms
    }

    /// The grants the package renders for the stack: engine-scoped members and the signer's
    /// account-scoped members, read from the generated blocks.
    fn rendered(remote: bool) -> Placed {
        let sandbox = sandbox(
            if remote { vec![] } else { vec![8080] },
            if remote {
                SandboxEgress::Allow
            } else {
                SandboxEgress::None
            },
        );
        let builder = Stack::new("acme".to_string())
            .add(
                GcpAgentPlatformEngine::new(GcpAgentPlatformEngine::id_for_sandbox("box")).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteStackManagement::new("management".to_string()).build(),
                ResourceLifecycle::Frozen,
            );
        let builder = if remote {
            builder
                .add(
                    alien_core::RemoteBindings::new("access".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
        } else {
            builder.add(sandbox, ResourceLifecycle::Frozen)
        };
        let stack = builder
            .management(ManagementPermissions::extend(
                PermissionProfile::new().resource("box", ["sandbox/templates"]),
            ))
            .permission(
                "workload",
                PermissionProfile::new().global(["sandbox/execute"]),
            )
            .build();
        let names: indexmap::IndexMap<String, String> = [
            ("box", "box"),
            (
                &*GcpAgentPlatformEngine::id_for_sandbox("box"),
                "box_engine",
            ),
            ("management", "management"),
            ("access", "access"),
            ("workload-sa", "workload_sa"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let settings = StackSettings::default();
        let ctx = EmitContext {
            stack: &stack,
            resource: stack.resources.get("box").unwrap(),
            resource_id: "box",
            platform: Platform::Gcp,
            targets_kubernetes: false,
            stack_settings: &settings,
            names: &names,
            pins: &alien_core::SetupPins::default(),
        };
        let fragment = GcpAgentPlatformSandboxEmitter.emit(&ctx).expect("renders");

        let custom_roles: BTreeMap<String, Vec<String>> = fragment
            .resource_blocks
            .iter()
            .filter(|b| {
                b.labels.first().map(|l| l.as_str()) == Some("google_project_iam_custom_role")
            })
            .map(|b| (b.labels[1].as_str().to_string(), perms_of_custom_role(b)))
            .collect();
        let identity = |member: &str| -> String {
            for (label, who) in [
                ("google_service_account.workload_sa.", "workload"),
                ("google_service_account.management.", "management"),
                ("google_service_account.access.", "caller"),
                ("google_service_account.box_preview.", "preview-signer"),
            ] {
                if member.contains(label) {
                    return who.to_string();
                }
            }
            panic!("the package grants to an identity this test does not know: {member}");
        };
        let role = |expr: &str| -> Vec<String> {
            let hits: Vec<_> = custom_roles
                .iter()
                .filter(|(label, _)| {
                    expr.contains(&format!(".{label}[")) || expr.contains(&format!(".{label}."))
                })
                .collect();
            match hits.as_slice() {
                [(_, perms)] => (*perms).clone(),
                [] => vec![expr.to_string()],
                _ => panic!("a role expression names several custom roles: {expr}"),
            }
        };
        let collect = |kind: &str| -> Grants {
            fragment
                .resource_blocks
                .iter()
                .filter(|b| b.labels.first().map(|l| l.as_str()) == Some(kind))
                .map(|b| {
                    (
                        identity(&attribute(b, "member").expect("a member")),
                        role(&attribute(b, "role").expect("a role")),
                    )
                })
                .collect()
        };
        Placed {
            engine: collect("google_vertex_ai_reasoning_engine_iam_member"),
            signer: collect("google_service_account_iam_member"),
            signer_accounts: fragment
                .resource_blocks
                .iter()
                .filter(|b| b.labels.first().map(|l| l.as_str()) == Some("google_service_account"))
                .count(),
        }
    }

    fn placed_directly(calls: &Calls) -> Placed {
        let signer_email = format!(
            "{}@{PROJECT}.iam.gserviceaccount.com",
            preview_signer_account_id("test", "box")
        );
        let identity = |member: &str| -> String {
            let email = member.trim_start_matches("serviceAccount:");
            match email {
                e if e == WORKLOAD => "workload",
                e if e == MANAGER => "management",
                e if e == CALLER => "caller",
                e if e == signer_email => "preview-signer",
                other => panic!("direct setup granted to an unknown identity: {other}"),
            }
            .to_string()
        };
        let role = |name: &str| -> Vec<String> {
            let hits: Vec<_> = calls
                .role_permissions
                .iter()
                .filter(|(id, _)| name.ends_with(id.as_str()))
                .collect();
            match hits.as_slice() {
                [(_, perms)] => {
                    let mut perms = (*perms).clone();
                    perms.sort();
                    perms
                }
                [] => vec![name.to_string()],
                _ => panic!("a role name matches several created roles: {name}"),
            }
        };
        let collect = |policy: Option<&IamPolicy>| -> Grants {
            policy
                .map(|p| {
                    p.bindings
                        .iter()
                        .flat_map(|b| {
                            b.members
                                .iter()
                                .map(|m| (identity(m), role(&b.role)))
                                .collect::<Vec<_>>()
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Placed {
            engine: collect(calls.engine_policy.as_ref()),
            signer: collect(calls.signer_policy.as_ref()),
            signer_accounts: calls.created_accounts.len(),
        }
    }

    async fn direct(remote: bool) -> Placed {
        let calls = Arc::new(Mutex::new(Calls::default()));
        let (iam, platform) = clients(&calls);
        let mut executor = executor_full(iam, platform, true, true, remote).await;
        executor.run_until_terminal().await.expect("create runs");
        let calls = calls.lock().unwrap();
        placed_directly(&calls)
    }

    /// The package is the reference: whatever it grants on the engine and on the signer, direct
    /// setup grants the same identities the same permissions, and nothing else.
    #[tokio::test]
    async fn direct_setup_grants_what_the_package_renders() {
        for remote in [false, true] {
            let package = rendered(remote);
            assert!(
                !package.engine.is_empty(),
                "the comparison is empty (remote={remote}), so it proves nothing"
            );
            let direct = direct(remote).await;
            assert_eq!(
                direct, package,
                "direct setup and the package differ (remote={remote})"
            );
        }
    }
}
