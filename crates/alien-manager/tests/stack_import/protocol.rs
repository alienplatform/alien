use super::*;
use alien_manager::traits::DeploymentFilter;

async fn protocol_record(fixture: &Fixture, name: &str, version: u32) -> String {
    let mut body = gcp_remote_management_import_request(
        name,
        "us-central1",
        "management",
        "test-project",
        Some("123456789012"),
    );
    let id = reserve_gcp_import_fixture_with_protocol(fixture, &mut body, version).await;
    // Protocol selection needs runnable work, not an external installer reservation.
    fixture
        .deployment_store
        .update_imported_stack_state(
            &alien_manager::auth::Subject::system(),
            &id,
            alien_manager::traits::UpdateImportedDeploymentParams {
                stack_settings: body.stack_settings,
                stack_state: StackState::new(Platform::Gcp),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: fixture.release_id.clone(),
                setup_target: body.setup_target,
                setup_fingerprint: body.setup_fingerprint,
                setup_fingerprint_version: body.setup_fingerprint_version,
                activation_status: Some("provisioning".into()),
                schedule_reconciliation: false,
                input_values: Default::default(),
            },
        )
        .await
        .unwrap();
    id
}

async fn request(
    fixture: &Fixture,
    path: &str,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = alien_manager::routes::sync::router()
        .merge(alien_manager::routes::sync::initialize_router())
        .with_state(fixture.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn old_actor_cannot_acquire_a_newer_deployment_or_write_a_downgrade() {
    let fixture =
        make_fixture_for_platform(Platform::Gcp, Some(source_stack_without_setup_resources()))
            .await;
    let id = protocol_record(&fixture, "new-protocol", 2).await;
    let token = mint_token(
        &fixture.token_store,
        TokenType::Deployment,
        "ax_dep_",
        Some(fixture.deployment_group_id.clone()),
        Some(id.clone()),
    )
    .await;
    let (status, body) = request(
        &fixture,
        "/v1/sync/acquire",
        &token,
        serde_json::json!({
            "session":"old-actor", "deploymentIds":[id], "deploymentModel":"push", "limit":1
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:#}");
    assert!(
        body.to_string().contains("Update the deployment"),
        "{body:#}"
    );
    let record = fixture
        .deployment_store
        .get_deployment(&alien_manager::auth::Subject::system(), &id)
        .await
        .unwrap()
        .unwrap();
    assert!(record.locked_by.is_none());
    assert_eq!(record.deployment_protocol_version, 2);

    let (status, body) = request(
        &fixture,
        "/v1/sync",
        &token,
        serde_json::json!({"deploymentId":id}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:#}");
    let (status, body) = request(
        &fixture,
        "/v1/sync",
        &token,
        serde_json::json!({
            "deploymentId":id,"maxSupportedDeploymentProtocolVersion":2,
            "currentState":{"protocolVersion":1}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:#}");
    assert!(body.to_string().contains("preserve"), "{body:#}");
    let record = fixture
        .deployment_store
        .get_deployment(&alien_manager::auth::Subject::system(), &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.deployment_protocol_version, 2);
    assert!(record.locked_by.is_none());
}

#[tokio::test]
async fn acquisition_filters_unsupported_records_before_its_limit() {
    let fixture =
        make_fixture_for_platform(Platform::Gcp, Some(source_stack_without_setup_resources()))
            .await;
    let newer = protocol_record(&fixture, "unsupported", 2).await;
    let supported = protocol_record(&fixture, "supported", 1).await;
    let acquired = fixture
        .deployment_store
        .acquire(
            &alien_manager::auth::Subject::system(),
            "protocol-reader",
            &DeploymentFilter {
                deployment_ids: Some(vec![newer, supported.clone()]),
                statuses: Some(vec!["provisioning".into()]),
                max_deployment_protocol_version: Some(1),
                limit: Some(1),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].deployment.id, supported);
    assert_eq!(acquired[0].deployment.deployment_protocol_version, 1);
}

#[tokio::test]
async fn kubernetes_bootstrap_retains_gcp_protocol_and_base_environment() {
    let fixture = make_fixture_for_platform(
        Platform::Kubernetes,
        Some(Stack::new("kube".into()).build()),
    )
    .await;
    let (status, body) = request(
        &fixture,
        "/v1/initialize",
        &fixture.dg_token,
        serde_json::json!({
            "name": "gcp-kubernetes-bootstrap", "platform": "kubernetes", "basePlatform": "gcp",
            "setupMethod": "helm", "initialDesiredRelease": "active"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body:#}");
    let id = body["deploymentId"].as_str().unwrap();
    let record = fixture
        .deployment_store
        .get_deployment(&alien_manager::auth::Subject::system(), id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.platform, Platform::Kubernetes);
    assert_eq!(record.base_platform, Some(Platform::Gcp));
    assert_eq!(
        record.deployment_protocol_version,
        alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION
    );
}
