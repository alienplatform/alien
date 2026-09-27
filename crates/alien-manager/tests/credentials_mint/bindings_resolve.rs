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
        _resource_id: &str,
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
                encryption_key: None,
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
                stack_settings: StackSettings::default(),
                stack_state,
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                activation_status: None,
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

struct FixedSubject(Subject);

#[async_trait]
impl AuthValidator for FixedSubject {
    async fn validate(&self, _headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        Ok(Some(self.0.clone()))
    }
}

fn remote_bindings_subject(
    fixture: &Fixture,
    kind: alien_manager::auth::RemoteBindingGrant,
    resource_id: Option<&str>,
) -> Subject {
    Subject {
        kind: alien_manager::auth::SubjectKind::ServiceAccount {
            id: "remote-bindings".to_string(),
        },
        workspace_id: "default".to_string(),
        scope: alien_manager::auth::Scope::RemoteBindings {
            project_id: "default".to_string(),
            deployment_id: fixture.deployment_a.clone(),
            capability: alien_manager::auth::RemoteBindingCapability {
                kind,
                resource_id: resource_id.map(str::to_string),
            },
        },
        role: alien_manager::auth::Role::RemoteBindingResolver,
        bearer_token: String::new(),
    }
}

fn with_subject(fixture: &Fixture, subject: Subject) -> Fixture {
    let mut state = fixture.state.clone();
    state.auth_validator = Arc::new(FixedSubject(subject));
    Fixture {
        state,
        deployment_a: fixture.deployment_a.clone(),
        token_a: fixture.token_a.clone(),
        token_b: fixture.token_b.clone(),
        admin_token: fixture.admin_token.clone(),
        group_token: fixture.group_token.clone(),
        captured: fixture.captured.clone(),
    }
}

fn sandbox_only_stack() -> Stack {
    let sandbox = alien_core::Sandbox::new("box".to_string())
        .code(alien_core::SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(alien_core::SandboxEgress::Allow)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let mut stack = mint_test_stack(Platform::Aws);
    stack.resources.shift_remove("files");
    stack.resources.insert(
        "box".to_string(),
        alien_core::ResourceEntry {
            enabled_when: None,
            config: Resource::new(sandbox),
            dependencies: Vec::new(),
            lifecycle: ResourceLifecycle::Frozen,
            remote_access: true,
        },
    );
    stack
}

/// Moves deployment A onto a release whose only remote resource is the sandbox
/// `box`, the way a setup re-import writes the current release directly.
async fn current_release_adds_a_remote_sandbox(fixture: &Fixture) {
    move_current_release(fixture, sandbox_only_stack()).await;
}

async fn move_current_release(fixture: &Fixture, stack: Stack) {
    let release = fixture
        .state
        .release_store
        .create_release(
            &Subject::system(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(Platform::Aws, stack)]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .expect("release with a remote sandbox should persist");
    let deployment = fixture
        .state
        .deployment_store
        .get_deployment(&Subject::system(), &fixture.deployment_a)
        .await
        .unwrap()
        .unwrap();
    fixture
        .state
        .deployment_store
        .update_imported_stack_state(
            &Subject::system(),
            &fixture.deployment_a,
            UpdateImportedDeploymentParams {
                stack_settings: StackSettings::default(),
                stack_state: deployment.stack_state.unwrap(),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: Some(release.id),
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                activation_status: None,
                schedule_reconciliation: false,
                input_values: Default::default(),
            },
        )
        .await
        .expect("current release should move");
}

fn resolve_body(fixture: &Fixture, resource_id: &str) -> serde_json::Value {
    serde_json::json!({
        "deploymentId": fixture.deployment_a,
        "resourceId": resource_id,
    })
}

#[tokio::test]
async fn a_data_capability_cannot_resolve_a_sandbox_the_current_release_gained() {
    let (fixture, calls) = fixture().await;
    let data = with_subject(
        &fixture,
        remote_bindings_subject(
            &fixture,
            alien_manager::auth::RemoteBindingGrant::Data,
            None,
        ),
    );

    let (status, _, json) =
        post_resolve_binding(&data, "unused", resolve_body(&fixture, "files")).await;
    assert_eq!(status, StatusCode::OK, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    current_release_adds_a_remote_sandbox(&fixture).await;

    let (status, _, _) = post_resolve_binding(&data, "unused", resolve_body(&fixture, "box")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_sandbox_capability_passes_authorization_only_for_its_named_sandbox() {
    let (fixture, calls) = fixture().await;
    current_release_adds_a_remote_sandbox(&fixture).await;
    let sandbox = with_subject(
        &fixture,
        remote_bindings_subject(
            &fixture,
            alien_manager::auth::RemoteBindingGrant::Sandbox,
            Some("box"),
        ),
    );

    // Authorization passes; the fixture publishes no sandbox parameters, so a
    // later server-state check refuses it without resolving credentials.
    let (status, _, json) =
        post_resolve_binding(&sandbox, "unused", resolve_body(&fixture, "box")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn deployment_writers_cannot_resolve_a_sandbox() {
    let (fixture, calls) = fixture().await;
    current_release_adds_a_remote_sandbox(&fixture).await;

    for bearer in [&fixture.admin_token, &fixture.group_token, &fixture.token_a] {
        let (status, _, _) =
            post_resolve_binding(&fixture, bearer, resolve_body(&fixture, "box")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_sandbox_sharing_the_release_with_another_remote_resource_refuses_every_resolve() {
    let (fixture, calls) = fixture().await;
    let sandbox = alien_core::Sandbox::new("box".to_string())
        .code(alien_core::SandboxCode::Image {
            image: "ubuntu".to_string(),
        })
        .egress(alien_core::SandboxEgress::Allow)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let mut stack = mint_test_stack(Platform::Aws);
    stack.resources.insert(
        "box".to_string(),
        alien_core::ResourceEntry {
            enabled_when: None,
            config: Resource::new(sandbox),
            dependencies: Vec::new(),
            lifecycle: ResourceLifecycle::Frozen,
            remote_access: true,
        },
    );
    move_current_release(&fixture, stack).await;

    let (status, _, _) = post_resolve_binding(
        &fixture,
        &fixture.admin_token,
        resolve_body(&fixture, "files"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_caller_without_a_claim_on_the_deployment_learns_nothing_about_it() {
    let (fixture, calls) = fixture().await;
    let (status, _, json) = post_resolve_binding(
        &fixture,
        &fixture.token_b,
        resolve_body(&fixture, "missing"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_data_resolve_is_refused_while_the_desired_release_publishes_a_sandbox() {
    let (fixture, calls) = fixture().await;
    let release = fixture
        .state
        .release_store
        .create_release(
            &Subject::system(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(Platform::Aws, sandbox_only_stack())]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();
    fixture
        .state
        .deployment_store
        .set_deployment_desired_release(&Subject::system(), &fixture.deployment_a, &release.id)
        .await
        .unwrap();

    let (status, _, json) = post_resolve_binding(
        &fixture,
        &fixture.admin_token,
        resolve_body(&fixture, "files"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "body = {json:#}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_ai_selector_reveals_nothing_to_a_caller_without_a_claim() {
    let (fixture, _) = fixture().await;
    let (status, _, json) = post_resolve_binding(
        &fixture,
        &fixture.token_b,
        serde_json::json!({ "deploymentId": fixture.deployment_a, "kind": "ai" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body = {json:#}");
}

#[path = "bindings_resolve_verifier.rs"]
mod verifier;
