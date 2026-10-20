use super::*;
use alien_manager::traits::DeploymentFilter;

async fn create(
    fixture: &Fixture,
    token: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = alien_manager::routes::deployments::router()
        .with_state(fixture.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/deployments")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn request() -> serde_json::Value {
    serde_json::json!({
        "name":"setup-fixture", "platform":"gcp", "setupHandoff":"await-registration",
        "setupMethod":"terraform", "resourcePrefix":"setup-fixture",
        "inputValues":{"tenantLabel":"example"},
        "environmentVariables":[{"name":"FIXTURE_SETTING", "value":"original-fixture-value", "type":"plain"}]
    })
}

#[tokio::test]
async fn reservation_replay_preserves_inputs_release_and_usable_token_without_scheduling() {
    let fixture =
        make_fixture_for_platform(Platform::Gcp, Some(Stack::new("fixture".into()).build())).await;
    let (status, first) = create(&fixture, &fixture.dg_token, request()).await;
    assert_eq!(status, StatusCode::CREATED, "{first:#}");
    let id = first["deployment"]["id"].as_str().unwrap();
    let token = first["token"].as_str().unwrap();
    let token_record = fixture
        .token_store
        .validate_token(&hex::encode(Sha256::digest(token.as_bytes())))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token_record.deployment_id.as_deref(), Some(id));
    assert_eq!(
        token_record.deployment_group_id.as_deref(),
        Some(fixture.deployment_group_id.as_str())
    );
    assert_eq!(token_record.token_type, TokenType::Deployment);
    let token_count = fixture.token_store.list_tokens().await.unwrap().len();
    // Exercise publication and its rollout, not only insertion of a release row.
    let published = alien_manager::routes::releases::router()
        .with_state(fixture.state.clone())
        .oneshot(Request::builder().method("POST").uri("/v1/releases")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin_token))
            .body(Body::from(serde_json::to_vec(&serde_json::json!({
                "project":"default", "stack":{"gcp":Stack::new("new-release".into()).build()}
            })).unwrap())).unwrap()).await.unwrap();
    assert_eq!(published.status(), StatusCode::CREATED);
    let (status, again) = create(&fixture, &fixture.dg_token, request()).await;
    assert_eq!(status, StatusCode::CREATED, "{again:#}");
    assert_eq!(again["deployment"]["id"], id);
    assert_eq!(again["token"], token);
    assert_eq!(
        fixture.token_store.list_tokens().await.unwrap().len(),
        token_count
    );
    let record = fixture
        .deployment_store
        .get_deployment(&alien_manager::auth::Subject::system(), id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.setup_metadata.as_ref().unwrap()["setupReservation"]["releaseId"],
        fixture.release_id.as_deref().unwrap()
    );
    assert!(record.current_release_id.is_none() && record.desired_release_id.is_none());
    assert_eq!(record.input_values["tenantLabel"], "example");
    assert_eq!(
        record.user_environment_variables.as_ref().unwrap()[0].value,
        "original-fixture-value"
    );
    let acquired = fixture
        .deployment_store
        .acquire(
            &alien_manager::auth::Subject::system(),
            "reservation-check",
            &DeploymentFilter {
                deployment_ids: Some(vec![id.into()]),
                statuses: Some(vec!["pending".into()]),
                max_deployment_protocol_version: Some(
                    alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION,
                ),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert!(
        acquired.is_empty(),
        "an external installer owns the pending setup"
    );
    let (status, _) = create(&fixture, token, request()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn reservation_rejects_changed_inputs_and_retains_the_original_token() {
    let fixture =
        make_fixture_for_platform(Platform::Gcp, Some(Stack::new("fixture".into()).build())).await;
    let (status, first) = create(&fixture, &fixture.dg_token, request()).await;
    assert_eq!(status, StatusCode::CREATED, "{first:#}");
    for changed in [
        serde_json::json!({"inputValues":{"tenantLabel":"changed"}}),
        serde_json::json!({"environmentVariables":[]}),
        serde_json::json!({"resourcePrefix":"different-prefix"}),
    ] {
        let mut body = request();
        body.as_object_mut()
            .unwrap()
            .extend(changed.as_object().unwrap().clone());
        let (status, _) = create(&fixture, &fixture.dg_token, body).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }
    let (_, replay) = create(&fixture, &fixture.dg_token, request()).await;
    assert_eq!(replay["deployment"]["id"], first["deployment"]["id"]);
    assert_eq!(replay["token"], first["token"]);
}

#[tokio::test]
async fn creating_a_gcp_backed_kubernetes_deployment_records_the_gcp_protocol() {
    let fixture = make_fixture_for_platform(
        Platform::Kubernetes,
        Some(Stack::new("fixture".into()).build()),
    )
    .await;
    let (status, created) = create(
        &fixture,
        &fixture.dg_token,
        serde_json::json!({"name":"gke-fixture", "platform":"kubernetes", "basePlatform":"gcp"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created:#}");
    let record = fixture
        .deployment_store
        .get_deployment(
            &alien_manager::auth::Subject::system(),
            created["deployment"]["id"].as_str().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.base_platform, Some(Platform::Gcp));
    assert_eq!(
        record.deployment_protocol_version,
        alien_core::GCP_FEDERATION_DEPLOYMENT_PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn cleanup_of_an_unregistered_reservation_removes_it() {
    let fixture =
        make_fixture_for_platform(Platform::Gcp, Some(Stack::new("fixture".into()).build())).await;
    let (status, created) = create(&fixture, &fixture.dg_token, request()).await;
    assert_eq!(status, StatusCode::CREATED, "{created:#}");
    let id = created["deployment"]["id"].as_str().unwrap().to_string();
    let response = alien_manager::routes::deployments::router()
        .with_state(fixture.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/deployments/{id}/delete"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {}", fixture.admin_token))
                .body(Body::from(r#"{"action":"cleanup"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, StatusCode::ACCEPTED, "{body:#}");
    assert_eq!(body["cleanupRequired"], false);
    // Left as delete-pending, it would wait for a manager that has no release to clean up with.
    assert!(fixture
        .deployment_store
        .get_deployment(&alien_manager::auth::Subject::system(), &id)
        .await
        .unwrap()
        .is_none());
    // The same name can be reserved again.
    let (status, again) = create(&fixture, &fixture.dg_token, request()).await;
    assert_eq!(status, StatusCode::CREATED, "{again:#}");
    assert_ne!(again["deployment"]["id"], id.as_str());
}
