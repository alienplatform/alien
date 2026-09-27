use super::*;

fn scoped(
    fixture: &Fixture,
    scope: alien_manager::auth::Scope,
    role: alien_manager::auth::Role,
) -> Fixture {
    with_subject(
        fixture,
        Subject {
            kind: alien_manager::auth::SubjectKind::ServiceAccount {
                id: "resolver".to_string(),
            },
            workspace_id: "default".to_string(),
            scope,
            role,
            bearer_token: String::new(),
        },
    )
}

#[tokio::test]
async fn a_resolver_token_without_a_kind_is_authorized_for_a_sandbox_during_the_transition() {
    let (fixture, calls) = fixture().await;
    let claimless = scoped(
        &fixture,
        alien_manager::auth::Scope::Deployment {
            project_id: "default".to_string(),
            deployment_id: fixture.deployment_a.clone(),
        },
        alien_manager::auth::Role::RemoteBindingResolver,
    );

    let (status, _, json) =
        post_resolve_binding(&claimless, "unused", resolve_body(&fixture, "files")).await;
    assert_eq!(status, StatusCode::OK, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Authorization passes; the fixture publishes no sandbox parameters, so a
    // later server-state check refuses it without resolving credentials.
    current_release_adds_a_remote_sandbox(&fixture).await;
    let (status, _, json) =
        post_resolve_binding(&claimless, "unused", resolve_body(&fixture, "box")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_data_capability_scoped_to_another_resource_is_refused() {
    let (fixture, calls) = fixture().await;
    let data = with_subject(
        &fixture,
        remote_bindings_subject(
            &fixture,
            alien_manager::auth::RemoteBindingGrant::Data,
            Some("other"),
        ),
    );
    let (status, _, json) =
        post_resolve_binding(&data, "unused", resolve_body(&fixture, "files")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_commands_capability_for_the_deployment_cannot_resolve_bindings() {
    let (fixture, calls) = fixture().await;
    let commands = scoped(
        &fixture,
        alien_manager::auth::Scope::Commands {
            project_id: "default".to_string(),
            deployment_id: fixture.deployment_a.clone(),
            capability: alien_manager::auth::CommandCapability::Send,
        },
        alien_manager::auth::Role::CommandCapability,
    );
    let (status, _, json) =
        post_resolve_binding(&commands, "unused", resolve_body(&fixture, "files")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_sandbox_capability_for_another_deployment_learns_nothing() {
    let (fixture, calls) = fixture().await;
    current_release_adds_a_remote_sandbox(&fixture).await;
    let other = scoped(
        &fixture,
        alien_manager::auth::Scope::RemoteBindings {
            project_id: "default".to_string(),
            deployment_id: "some-other-deployment".to_string(),
            capability: alien_manager::auth::RemoteBindingCapability {
                kind: alien_manager::auth::RemoteBindingGrant::Sandbox,
                resource_id: Some("box".to_string()),
            },
        },
        alien_manager::auth::Role::RemoteBindingResolver,
    );
    let (status, _, json) =
        post_resolve_binding(&other, "unused", resolve_body(&fixture, "box")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

async fn state_keeps_a_sandbox(fixture: &Fixture, status: ResourceStatus) {
    let deployment = fixture
        .state
        .deployment_store
        .get_deployment(&Subject::system(), &fixture.deployment_a)
        .await
        .unwrap()
        .unwrap();
    let mut stack_state = deployment.stack_state.unwrap();
    stack_state.resources.insert(
        "box".to_string(),
        StackResourceState::builder()
            .resource_type(alien_core::Sandbox::RESOURCE_TYPE.as_ref().to_string())
            .status(status)
            .config(Resource::new(
                alien_core::Sandbox::new("box".to_string())
                    .code(alien_core::SandboxCode::Image {
                        image: "ubuntu".to_string(),
                    })
                    .egress(alien_core::SandboxEgress::Allow)
                    .lifecycle(alien_core::SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
            ))
            .maybe_lifecycle(Some(ResourceLifecycle::Frozen))
            .maybe_remote_binding_params(Some(serde_json::json!({ "service": "sandbox" })))
            .dependencies(Vec::new())
            .build(),
    );
    fixture
        .state
        .deployment_store
        .update_imported_stack_state(
            &Subject::system(),
            &fixture.deployment_a,
            UpdateImportedDeploymentParams {
                stack_settings: StackSettings::default(),
                stack_state,
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: deployment.current_release_id,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                activation_status: None,
                schedule_reconciliation: false,
                input_values: Default::default(),
            },
        )
        .await
        .expect("stack state with a sandbox should persist");
}

#[tokio::test]
async fn a_sandbox_published_in_stack_state_blocks_data_resolves() {
    let (fixture, calls) = fixture().await;
    state_keeps_a_sandbox(&fixture, ResourceStatus::Running).await;
    let (status, _, json) = post_resolve_binding(
        &fixture,
        &fixture.admin_token,
        resolve_body(&fixture, "files"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {json:#}");
    assert!(
        json.to_string().contains("has a remote sandbox"),
        "body = {json:#}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
