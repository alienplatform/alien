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
    let mut iam = MockIamApi::new();
    let c = calls.clone();
    iam.expect_create_service_account().returning(move |id, _| {
        c.lock().unwrap().created_accounts.push(id.clone());
        Ok(alien_gcp_clients::iam::ServiceAccount::builder()
            .email(format!("{id}@{PROJECT}.iam.gserviceaccount.com"))
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
        .controller(GcpAgentPlatformTemplateController::default())
        .platform(Platform::Gcp)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
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
