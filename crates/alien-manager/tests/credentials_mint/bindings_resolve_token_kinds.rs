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
async fn a_resolver_token_without_a_kind_resolves_storage_but_not_a_sandbox_the_release_gained() {
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

    current_release_adds_a_remote_sandbox(&fixture).await;
    let (status, _, json) =
        post_resolve_binding(&claimless, "unused", resolve_body(&fixture, "box")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body = {json:#}");
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

#[tokio::test]
async fn a_sandbox_published_in_stack_state_blocks_data_resolves() {
    let (fixture, calls) = fixture().await;
    state_keeps_a_sandbox(&fixture, serde_json::json!({ "service": "sandbox" })).await;
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
