use super::*;

struct RemoteAwsCredentialResolver {
    source: AwsClientConfig,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CredentialResolver for RemoteAwsCredentialResolver {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        Ok(ClientConfig::Aws(Box::new(self.source.clone())))
    }

    async fn resolve_remote_storage_source(
        &self,
        _deployment: &DeploymentRecord,
    ) -> Result<RemoteStorageCredentialSource, AlienError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(RemoteStorageCredentialSource::Direct(ClientConfig::Aws(
            Box::new(self.source.clone()),
        )))
    }
}

async fn persist_remote_storage_state(fixture: &Fixture) {
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
                cors_allowed_origins: Vec::new(),
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
            UpdateImportedDeploymentParams {
                stack_state,
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                schedule_reconciliation: false,
                input_values: Default::default(),
            },
        )
        .await
        .expect("remote binding fixture should persist stack state");
}

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
    persist_remote_storage_state(&fixture).await;

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

    let (status, _, json) = post_resolve_binding(
        &fixture,
        &fixture.token_a,
        serde_json::json!({
            "deploymentId": fixture.deployment_a,
            "resourceId": "files",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(json["service"], "s3");
    assert_eq!(json["binding"]["bucketName"], "remote-files");
}

#[tokio::test]
async fn resolves_remote_storage_with_stack_identity_credentials_and_disables_response_caching() {
    let calls = Arc::new(AtomicUsize::new(0));
    let resolver: Arc<dyn CredentialResolver> = Arc::new(RemoteAwsCredentialResolver {
        source: AwsClientConfig {
            account_id: "111122223333".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::SessionCredentials {
                access_key_id: "ASIAREMOTEACCESS".to_string(),
                secret_access_key: "remote-secret".to_string(),
                session_token: "remote-session-token".to_string(),
                expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            },
            service_overrides: None,
        },
        calls: calls.clone(),
    });
    let fixture = build(
        Platform::Aws,
        HashMap::new(),
        resolver,
        Arc::new(Mutex::new(None)),
    )
    .await;
    persist_remote_storage_state(&fixture).await;

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
    assert_eq!(json["clientConfig"]["accountId"], "111122223333");
    assert_eq!(
        json["clientConfig"]["credentials"]["type"],
        "sessionCredentials"
    );
    assert_eq!(
        json["clientConfig"]["credentials"]["accessKeyId"],
        "ASIAREMOTEACCESS"
    );
    let lease_expires_at = chrono::DateTime::parse_from_rfc3339(
        json["expiresAt"]
            .as_str()
            .expect("response lease expiry should be a string"),
    )
    .expect("response lease expiry should be RFC3339")
    .with_timezone(&chrono::Utc);
    let remaining = lease_expires_at - chrono::Utc::now();
    assert!(remaining > chrono::Duration::minutes(59));
    assert!(remaining <= chrono::Duration::hours(1));
}

#[tokio::test]
async fn denies_unscoped_deployment_token_before_resolving_credentials() {
    let (fixture, calls) = fixture().await;
    let unscoped_token = mint_token(
        &fixture.state.token_store,
        TokenType::Deployment,
        "ax_deploy_",
        None,
        None,
    )
    .await;

    let (status, _, _) = post_resolve_binding(
        &fixture,
        &unscoped_token,
        serde_json::json!({
            "deploymentId": fixture.deployment_a,
            "resourceId": "files",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
