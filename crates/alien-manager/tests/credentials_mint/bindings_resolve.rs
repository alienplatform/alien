use super::*;

const STS_RESPONSE: &str = r#"<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::210987654321:assumed-role/AlienManaged/remote-bindings-test</Arn>
      <AssumedRoleId>AROA:remote-bindings-test</AssumedRoleId>
    </AssumedRoleUser>
    <Credentials>
      <AccessKeyId>ASIAREMOTEACCESS</AccessKeyId>
      <SecretAccessKey>remote-secret</SecretAccessKey>
      <SessionToken>remote-session-token</SessionToken>
      <Expiration>2099-01-01T00:00:00Z</Expiration>
    </Credentials>
  </AssumeRoleResult>
  <ResponseMetadata><RequestId>request-assume-role</RequestId></ResponseMetadata>
</AssumeRoleResponse>"#;

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
        Ok(RemoteStorageCredentialSource::AwsAssumeRole {
            source: Box::new(self.source.clone()),
            role_arn: "arn:aws:iam::210987654321:role/AlienManaged".to_string(),
            role_session_name: "remote-bindings-test".to_string(),
            target_account_id: "210987654321".to_string(),
            target_region: "us-east-1".to_string(),
        })
    }
}

async fn mock_sts_handler(
    axum::extract::State(requests): axum::extract::State<Arc<Mutex<Vec<HashMap<String, String>>>>>,
    body: String,
) -> impl axum::response::IntoResponse {
    let form = form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect::<HashMap<_, _>>();
    requests.lock().expect("mock STS requests lock").push(form);
    ([(header::CONTENT_TYPE, "text/xml")], STS_RESPONSE)
}

async fn spawn_mock_sts() -> (String, Arc<Mutex<Vec<HashMap<String, String>>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = axum::Router::new()
        .route("/", axum::routing::post(mock_sts_handler))
        .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock STS server");
    let address = listener.local_addr().expect("read mock STS address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock STS endpoint");
    });
    (format!("http://{address}"), requests)
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
    // The fixture intentionally returns already-materialized AWS session
    // credentials, which cannot be attenuated to the exact bucket. Reaching
    // this fail-closed error proves all server-owned resource gates ran before
    // the resolver without weakening the production handoff rules.
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(json["code"], "CREDENTIAL_MATERIALIZATION_FAILED");
}

#[tokio::test]
async fn resolves_remote_storage_with_scoped_provider_credentials_and_disables_response_caching() {
    let (sts_endpoint, sts_requests) = spawn_mock_sts().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let resolver: Arc<dyn CredentialResolver> = Arc::new(RemoteAwsCredentialResolver {
        source: AwsClientConfig {
            account_id: "111122223333".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIATESTACCESS".to_string(),
                secret_access_key: "test-secret".to_string(),
                session_token: None,
            },
            service_overrides: Some(AwsServiceOverrides {
                endpoints: HashMap::from([("sts".to_string(), sts_endpoint)]),
            }),
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
    assert_eq!(json["clientConfig"]["accountId"], "210987654321");
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

    let requests = sts_requests.lock().expect("mock STS requests lock");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.get("Action").map(String::as_str),
        Some("AssumeRole")
    );
    assert_eq!(
        request.get("RoleArn").map(String::as_str),
        Some("arn:aws:iam::210987654321:role/AlienManaged")
    );
    assert_eq!(
        request.get("DurationSeconds").map(String::as_str),
        Some("3600")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            request.get("Policy").expect("AssumeRole inline policy")
        )
        .expect("inline policy should be valid JSON"),
        serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Sid": "RemoteStorageBucket",
                    "Effect": "Allow",
                    "Action": ["s3:ListBucket"],
                    "Resource": ["arn:aws:s3:::remote-files"]
                },
                {
                    "Sid": "RemoteStorageObjects",
                    "Effect": "Allow",
                    "Action": [
                        "s3:GetObject",
                        "s3:PutObject",
                        "s3:DeleteObject"
                    ],
                    "Resource": ["arn:aws:s3:::remote-files/*"]
                }
            ]
        })
    );
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
