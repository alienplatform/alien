//! Permission narrowing reconciles recorded grants without removing unrelated access.
use alien_core::{
    permissions::PermissionProfile, ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, GcpClientConfig, GcpCredentials, InitialSetupAuthority, Platform, Resource,
    ResourceLifecycle, ResourceStatus, ServiceAccount, Stack, StackResourceState, StackSettings,
    StackState,
};
use alien_gcp_clients::{
    iam::{Binding, IamPolicy, MockIamApi},
    resource_manager::MockResourceManagerApi,
};
use alien_infra::{
    deserialize_controller, MockPlatformServiceProvider, StackExecutor, StackResourceStateExt,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn gcp_account_update_removes_old_predefined_grants_and_preserves_unrelated_bindings() {
    let email = "synthetic-sender@synthetic-project.iam.gserviceaccount.com";
    let member = format!("serviceAccount:{email}");
    let other = "serviceAccount:other@synthetic-project.iam.gserviceaccount.com";
    let owned = Binding {
        role: "roles/pubsub.publisher".into(),
        members: vec![member.clone(), other.into()],
        condition: None,
    };
    let unrelated: Binding = serde_json::from_value(json!({
        "role":"roles/pubsub.publisher",
        "members":[member],
        "condition":{"title":"Unrelated", "expression":"resource.name.endsWith('/unrelated')"}
    }))
    .unwrap();
    let metadata = Binding {
        role: "roles/viewer".into(),
        members: vec![member.clone()],
        condition: None,
    };
    let policy = Arc::new(Mutex::new(IamPolicy {
        version: Some(3),
        bindings: vec![owned, unrelated.clone(), metadata.clone()],
        etag: Some("synthetic-etag".into()),
        kind: None,
        resource_id: None,
    }));
    let read = policy.clone();
    let mut manager = MockResourceManagerApi::new();
    manager
        .expect_get_project_iam_policy()
        .times(1)
        .returning(move |_, _| Ok(read.lock().unwrap().clone()));
    let written = policy.clone();
    manager
        .expect_set_project_iam_policy()
        .times(0..=1)
        .returning(move |_, next, _| {
            assert_eq!(next.etag.as_deref(), Some("synthetic-etag"));
            *written.lock().unwrap() = next.clone();
            Ok(next)
        });
    let mut iam = MockIamApi::new();
    iam.expect_set_service_account_iam_policy()
        .times(1)
        .returning(|_, policy| Ok(policy));
    let manager = Arc::new(manager);
    let iam = Arc::new(iam);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_gcp_resource_manager_client()
        .returning(move |_| Ok(manager.clone()));
    services
        .expect_get_gcp_iam_client()
        .returning(move |_| Ok(iam.clone()));

    let profile = PermissionProfile::new().resource("*", ["queue/data-write"]);
    let old = ServiceAccount::from_permission_profile("sender-sa".into(), &profile, |id| {
        alien_permissions::get_permission_set(id).cloned()
    })
    .unwrap();
    let desired = ServiceAccount::new("sender-sa".into()).build();
    let target = Stack::new("synthetic".into())
        .add(desired.clone(), ResourceLifecycle::Frozen)
        .build();
    let mut resource = StackResourceState::new_pending(
        "service-account".into(),
        Resource::new(old),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    resource.status = ResourceStatus::Running;
    resource
        .set_internal_controller(Some(
            deserialize_controller(json!({
                "type":"GcpServiceAccountController", "_controllerStateVersion":1,
                "state":"ready", "serviceAccountEmail":email
            }))
            .unwrap(),
        ))
        .unwrap();
    let mut state = StackState::with_resource_prefix(Platform::Gcp, "synthetic".into());
    state.resources.insert("sender-sa".into(), resource);
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let client = ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: "synthetic-project".into(),
        region: "us-central1".into(),
        project_number: Some("123456789012".into()),
        credentials: GcpCredentials::AccessToken {
            token: "synthetic".into(),
        },
        service_overrides: None,
    }));
    let executor = StackExecutor::builder(&target, client)
        .deployment_config(&config)
        .service_provider(Arc::new(services))
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .lifecycle_filter(vec![ResourceLifecycle::Frozen])
        .step_running_resources(false)
        .build()
        .unwrap();
    for _ in 0..8 {
        state = executor.step(state).await.unwrap().next_state;
        state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        let account = &state.resources["sender-sa"];
        if account.status == ResourceStatus::Running
            && account.config == Resource::new(desired.clone())
        {
            break;
        }
    }
    assert_eq!(state.resources["sender-sa"].status, ResourceStatus::Running);
    assert_eq!(state.resources["sender-sa"].config, Resource::new(desired));
    let bindings = &policy.lock().unwrap().bindings;
    assert!(bindings
        .iter()
        .any(|binding| serde_json::to_value(binding).unwrap()
            == serde_json::to_value(&unrelated).unwrap()));
    assert!(bindings
        .iter()
        .any(|binding| serde_json::to_value(binding).unwrap()
            == serde_json::to_value(&metadata).unwrap()));
    assert!(bindings
        .iter()
        .any(|binding| binding.role == "roles/pubsub.publisher"
            && binding.condition.is_none()
            && binding.members == [other]));
    assert!(!bindings
        .iter()
        .any(|binding| binding.role == "roles/pubsub.publisher"
            && binding.condition.is_none()
            && binding.members.contains(&member)));
}

async fn management_grant_cleanup(retained: bool) {
    use alien_core::{ManagementPermissions, RemoteStackManagement};
    use alien_infra::{ResourceControllerContext, ResourcePermissionsHelper, ResourceRegistry};
    let email = "synthetic-management@synthetic-project.iam.gserviceaccount.com";
    let member = format!("serviceAccount:{email}");
    let other = "serviceAccount:other@synthetic-project.iam.gserviceaccount.com";
    let binding = Binding {
        role: "roles/pubsub.viewer".into(),
        members: vec![member.clone(), other.into()],
        condition: None,
    };
    let unrelated: Binding = serde_json::from_value(json!({"role":"roles/pubsub.viewer","members":[member],"condition":{"title":"Unrelated","expression":"resource.name.endsWith('/unrelated')"}})).unwrap();
    let policy = Arc::new(Mutex::new(IamPolicy {
        version: Some(3),
        bindings: vec![binding, unrelated.clone()],
        etag: Some("synthetic-etag".into()),
        kind: None,
        resource_id: None,
    }));
    let read = policy.clone();
    let mut manager = MockResourceManagerApi::new();
    manager
        .expect_get_project_iam_policy()
        .returning(move |_, _| Ok(read.lock().unwrap().clone()));
    let write = policy.clone();
    manager
        .expect_set_project_iam_policy()
        .returning(move |_, next, _| {
            assert_eq!(next.etag.as_deref(), Some("synthetic-etag"));
            *write.lock().unwrap() = next.clone();
            Ok(next)
        });
    let manager = Arc::new(manager);
    let mut services = MockPlatformServiceProvider::new();
    services
        .expect_get_gcp_resource_manager_client()
        .returning(move |_| Ok(manager.clone()));
    let services: Arc<dyn alien_infra::PlatformServiceProvider> = Arc::new(services);
    let management = RemoteStackManagement::new("management".into()).build();
    let installed = Stack::new("synthetic".into())
        .management(ManagementPermissions::Extend(
            PermissionProfile::new().resource("*", ["queue/heartbeat"]),
        ))
        .add(management.clone(), ResourceLifecycle::Frozen)
        .add(
            alien_core::Queue::new("jobs".into()).build(),
            ResourceLifecycle::Live,
        )
        .build();
    let mut desired = Stack::new("synthetic".into())
        .management(ManagementPermissions::Extend(if retained {
            PermissionProfile::new().resource("*", ["queue/heartbeat"])
        } else {
            PermissionProfile::new()
        }))
        .add(management.clone(), ResourceLifecycle::Frozen);
    if retained {
        desired = desired.add(
            alien_core::Queue::new("other-jobs".into()).build(),
            ResourceLifecycle::Live,
        );
    }
    let desired = desired.build();
    let mut resource = StackResourceState::new_pending(
        "remote-stack-management".into(),
        Resource::new(management),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    resource.status = ResourceStatus::Running;
    resource.set_internal_controller(Some(deserialize_controller(json!({"type":"GcpRemoteStackManagementController","_controllerStateVersion":1,"state":"ready","serviceAccountEmail":email,"roleBound":true,"impersonationGranted":true})).unwrap())).unwrap();
    let mut state = StackState::with_resource_prefix(Platform::Gcp, "synthetic".into());
    state.resources.insert("management".into(), resource);
    let client = ClientConfig::Gcp(Box::new(GcpClientConfig {
        project_id: "synthetic-project".into(),
        region: "us-central1".into(),
        project_number: Some("123456789012".into()),
        credentials: GcpCredentials::AccessToken {
            token: "synthetic".into(),
        },
        service_overrides: None,
    }));
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let registry = Arc::new(ResourceRegistry::with_built_ins());
    let context = ResourceControllerContext {
        desired_config: &desired.resources["management"].config,
        platform: Platform::Gcp,
        client_config: client,
        state: &state,
        resource_prefix: &state.resource_prefix,
        registry: &registry,
        desired_stack: &desired,
        service_provider: &services,
        deployment_config: &config,
        initial_setup_authority: InitialSetupAuthority::DirectSetup,
        heartbeat_collector: Default::default(),
    };
    ResourcePermissionsHelper::reconcile_removed_setup_permissions(
        &context,
        "management",
        "remote-stack-management",
        &installed,
    )
    .await
    .unwrap();
    let bindings = &policy.lock().unwrap().bindings;
    let expected = if retained {
        vec![member, other.to_string()]
    } else {
        vec![other.to_string()]
    };
    assert!(
        bindings.iter().any(|b| b.role == "roles/pubsub.viewer"
            && b.condition.is_none()
            && b.members == expected),
        "only obsolete management grants may be removed"
    );
    assert!(bindings
        .iter()
        .any(|b| serde_json::to_value(b).unwrap() == serde_json::to_value(&unrelated).unwrap()));
}

#[tokio::test]
async fn setup_removes_recorded_gcp_management_role_after_its_last_resource_is_removed() {
    management_grant_cleanup(false).await;
}

#[tokio::test]
async fn setup_preserves_gcp_management_role_still_needed_by_another_resource() {
    management_grant_cleanup(true).await;
}

#[test]
fn legacy_management_checkpoint_does_not_claim_federation_access() {
    let controller = deserialize_controller(json!({
        "type":"GcpRemoteStackManagementController", "_controllerStateVersion":1,
        "state":"ready", "serviceAccountEmail":"synthetic-management@synthetic-project.iam.gserviceaccount.com",
        "roleBound":true, "impersonationGranted":true
    })).unwrap();
    assert!(
        controller.get_outputs().is_none(),
        "legacy impersonation is not a usable federation grant"
    );
    let mut resource = StackResourceState::new_pending(
        "remote-stack-management".into(),
        Resource::new(alien_core::RemoteStackManagement::new("management".into()).build()),
        Some(ResourceLifecycle::Frozen),
        vec![],
    );
    resource.set_internal_controller(Some(controller)).unwrap();
    let saved = resource.internal_state.unwrap();
    assert_eq!(saved["state"], "ready");
    assert_eq!(saved["federationGranted"], false);
    assert_eq!(
        saved["serviceAccountEmail"],
        "synthetic-management@synthetic-project.iam.gserviceaccount.com"
    );
}
