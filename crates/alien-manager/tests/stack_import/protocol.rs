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
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn helm_bootstrap_refuses_a_release_helm_would_not_accept() {
    let fixture = make_fixture_for_platform(
        Platform::Kubernetes,
        Some(Stack::new("kube".into()).build()),
    )
    .await;
    let initialize = |name: &str, release: &str| {
        serde_json::json!({
            "name": name, "platform": "kubernetes", "setupMethod": "helm",
            "initialDesiredRelease": "active",
            "helmRelease": { "name": release, "namespace": "alien-orders" }
        })
    };

    let (status, body) = request(
        &fixture,
        "/v1/initialize",
        &fixture.dg_token,
        initialize("helm-orders", "alien-operator"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body:#}");
    let deployment_id = body["deploymentId"].as_str().unwrap().to_string();
    let deployment_token = body["token"].as_str().unwrap().to_string();
    for (release, expected_status) in [
        ("orders; helm list", StatusCode::BAD_REQUEST),
        ("alien-operator", StatusCode::OK),
    ] {
        let (status, body) = request(
            &fixture,
            "/v1/initialize",
            &deployment_token,
            initialize("helm-orders", release),
        )
        .await;
        assert_eq!(status, expected_status, "{body:#}");
        if expected_status == StatusCode::OK {
            assert_eq!(body["deploymentId"], deployment_id);
            assert!(body["token"].is_null());
        } else {
            assert!(body.to_string().contains("helmRelease"), "{body:#}");
        }
    }

    let (status, body) = request(
        &fixture,
        "/v1/initialize",
        &fixture.dg_token,
        initialize("helm-refused", "orders; helm list"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body:#}");
    let deployments = fixture
        .deployment_store
        .list_deployments(
            &alien_manager::auth::Subject::system(),
            &DeploymentFilter {
                name: Some("helm-refused".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(deployments.is_empty(), "{deployments:?}");
}

#[tokio::test]
async fn protocol_three_refuses_previous_reader_before_checkpoint_parsing() {
    let fixture =
        make_fixture_for_platform(Platform::Gcp, Some(source_stack_without_setup_resources()))
            .await;
    let id = protocol_record(&fixture, "controller-checkpoints", 3).await;
    let token = mint_token(
        &fixture.token_store,
        TokenType::Deployment,
        "ax_dep_",
        Some(fixture.deployment_group_id.clone()),
        Some(id.clone()),
    )
    .await;
    for (path, body) in [
        (
            "/v1/sync/acquire",
            serde_json::json!({
                "session": "previous-reader", "deploymentIds": [id], "limit": 1, "deploymentModel": "push",
                "maxSupportedDeploymentProtocolVersion": 2
            }),
        ),
        (
            "/v1/sync",
            serde_json::json!({
                "deploymentId": id, "maxSupportedDeploymentProtocolVersion": 2,
                "currentState": {"protocolVersion": 3, "status": "unknown-to-previous-reader"}
            }),
        ),
    ] {
        let (status, response) = request(&fixture, path, &token, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{response:#}");
        assert!(
            response.to_string().contains("Update the deployment"),
            "{response:#}"
        );
        assert!(!response.to_string().contains("deserialize"));
    }
    let stored = fixture
        .deployment_store
        .get_deployment(&alien_manager::auth::Subject::system(), &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.deployment_protocol_version, 3);
    assert!(stored.locked_by.is_none());
    assert_eq!(stored.status, "provisioning");
}

#[tokio::test]
async fn pull_protocol_upgrade_without_a_claim_is_persisted_and_acknowledged() {
    let fixture = make_fixture(Some(source_stack_without_setup_resources())).await;
    let subject = alien_manager::auth::Subject::system();
    let record = fixture
        .deployment_store
        .create_with_state(
            &subject,
            CreateImportedDeploymentParams {
                name: "pull-protocol".into(),
                deployment_group_id: fixture.deployment_group_id.clone(),
                platform: Platform::Aws,
                deployment_protocol_version: 1,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(Platform::Aws),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                status: "running".into(),
                current_release_id: fixture.release_id.clone(),
                desired_release_id: None,
                import_source: Some(ImportSourceKind::Terraform),
                setup_metadata: None,
                setup_target: "aws".into(),
                setup_fingerprint: "test".into(),
                setup_fingerprint_version: 1,
                deployment_token: None,
                management_config: None,
                input_values: Default::default(),
            },
        )
        .await
        .unwrap();
    let token = mint_token(
        &fixture.token_store,
        TokenType::Deployment,
        "ax_dep_",
        None,
        Some(record.id.clone()),
    )
    .await;
    // A fresh local database must hydrate the installed state before proposing
    // an upgrade; an empty Pending report cannot replace the Running checkpoint.
    let (status, hydrated) = request(
        &fixture,
        "/v1/sync",
        &token,
        serde_json::json!({
            "deploymentId": record.id,
            "session": "pull-upgrade",
            "supportsExecutionClaims": true,
            "maxSupportedDeploymentProtocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "currentState": {"platform":"aws", "status":"pending", "protocolVersion":alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hydrated:#}");
    let mut installed = hydrated["currentState"].clone();
    assert_eq!(installed["status"], "running");
    assert_eq!(installed["protocolVersion"], 1);
    installed["protocolVersion"] =
        serde_json::json!(alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION);
    for _ in 0..2 {
        // The identical second report models a lost acknowledgement. Neither
        // report has an execution claim or changes controller state.
        let (status, response) = request(
            &fixture,
            "/v1/sync",
            &token,
            serde_json::json!({
                "deploymentId": record.id,
                "session": "pull-upgrade",
                "supportsExecutionClaims": true,
                "maxSupportedDeploymentProtocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                "currentState": installed
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{response:#}");
        assert_eq!(
            response["currentState"]["protocolVersion"],
            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        );
        assert_eq!(response["currentState"]["status"], "running");
        let stored = fixture
            .deployment_store
            .get_deployment(&subject, &record.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.deployment_protocol_version,
            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        );
        assert_eq!(stored.status, "running");
        assert_eq!(
            serde_json::to_value(&stored.stack_state).unwrap(),
            serde_json::to_value(&record.stack_state).unwrap()
        );
    }
}
