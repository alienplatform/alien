use super::*;

async fn fixture() -> (Fixture, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let resolver: Arc<dyn CredentialResolver> = Arc::new(CountingCredentialResolver {
        config: managed_aws_config(),
        calls: calls.clone(),
    });
    let fixture = build(
        Platform::Aws,
        HashMap::new(),
        resolver,
        Arc::new(Mutex::new(None)),
    )
    .await;

    let mut stack_state = StackState::new(Platform::Aws);
    stack_state.resources.insert(
        "files".to_string(),
        StackResourceState::builder()
            .resource_type(Storage::RESOURCE_TYPE.as_ref().to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(Storage {
                id: "files".to_string(),
                public_read: false,
                versioning: false,
                lifecycle_rules: Vec::new(),
            }))
            .maybe_lifecycle(Some(ResourceLifecycle::Frozen))
            .maybe_remote_binding_params(Some(serde_json::json!({
                "service": "s3",
                "bucketName": "remote-files",
            })))
            .dependencies(Vec::new())
            .build(),
    );
    fixture
        .state
        .deployment_store
        .update_imported_stack_state(
            &Subject::system(),
            &fixture.deployment_a,
            stack_state,
            None,
            RuntimeMetadata::default(),
            None,
            "test".to_string(),
            "test".to_string(),
            1,
        )
        .await
        .expect("remote binding fixture should persist stack state");

    (fixture, calls)
}

async fn post_resolve_binding(
    fixture: &Fixture,
    bearer: &str,
    body: serde_json::Value,
) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
    let router = alien_manager::routes::bindings::router().with_state(fixture.state.clone());
    let request = Request::builder()
        .method("POST")
        .uri("/v1/bindings/resolve")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, headers, json)
}

#[tokio::test]
async fn validates_server_state_before_resolving_credentials() {
    let (fixture, calls) = fixture().await;

    let (status, _, _) = post_resolve_binding(
        &fixture,
        &fixture.token_a,
        serde_json::json!({
            "deploymentId": fixture.deployment_a,
            "resourceId": "missing",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let (status, _, _) = post_resolve_binding(
        &fixture,
        &fixture.token_a,
        serde_json::json!({
            "deploymentId": fixture.deployment_a,
            "resourceId": "files",
            "binding": { "service": "local-storage" },
        }),
    )
    .await;
    assert!(status.is_client_error());
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let (status, headers, json) = post_resolve_binding(
        &fixture,
        &fixture.token_a,
        serde_json::json!({
            "deploymentId": fixture.deployment_a,
            "resourceId": "files",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {json:#}");
    assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
    assert_eq!(headers.get(header::PRAGMA).unwrap(), "no-cache");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(json["service"], "s3");
    assert_eq!(json["binding"]["bucketName"], "remote-files");
    assert!(json["clientConfig"]["platform"].is_null());
    chrono::DateTime::parse_from_rfc3339(
        json["expiresAt"]
            .as_str()
            .expect("expiresAt must be present"),
    )
    .expect("expiresAt must be RFC3339");
}

#[tokio::test]
async fn denies_viewer_before_resolving_credentials() {
    let (fixture, calls) = fixture().await;
    let viewer_token = mint_token(
        &fixture.state.token_store,
        TokenType::Deployment,
        "ax_deploy_",
        None,
        None,
    )
    .await;

    let (status, _, _) = post_resolve_binding(
        &fixture,
        &viewer_token,
        serde_json::json!({
            "deploymentId": fixture.deployment_a,
            "resourceId": "files",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
