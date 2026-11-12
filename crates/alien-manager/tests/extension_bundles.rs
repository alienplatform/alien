//! The manager's extension bundle routes and loader: a release uploads each object of a bundle
//! by its digest, a deployment's token downloads them, and the manager loads each digest once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::CommandServer;
use alien_commands::InMemoryCommandRegistry;
use alien_core::{BinaryTarget, ClientConfig, DeploymentState};
use alien_error::AlienError;
use alien_infra::extension_bundle::testing::TestBundle;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::extension_bundles::ExtensionBundles;
use alien_manager::providers::{OssAuthz, StateDirExtensionBundles};
use alien_manager::routes::registry_proxy::{
    CredentialCache, PullValidationCache, RegistryRoutingTable,
};
use alien_manager::routes::AppState;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::{
    AuthValidator, BundleObject, CredentialResolver, DeploymentRecord, ExtensionBundleStore,
};
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use http::header;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

struct RoleValidator(HashMap<String, Subject>);

#[async_trait]
impl AuthValidator for RoleValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        Ok(token.and_then(|token| self.0.get(token).cloned()))
    }
}

struct NoCredentials;

#[async_trait]
impl CredentialResolver for NoCredentials {
    async fn management_external_id(
        &self,
        _: &DeploymentRecord,
    ) -> Result<Option<alien_core::ManagementExternalId>, AlienError> {
        Ok(None)
    }

    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        unreachable!("these routes resolve no credentials")
    }
}

fn subject(scope: Scope, role: Role) -> Subject {
    Subject {
        kind: SubjectKind::ServiceAccount {
            id: "test".to_string(),
        },
        workspace_id: "default".to_string(),
        scope,
        role,
        bearer_token: String::new(),
    }
}

const DEVELOPER: &str = "developer";
const OPERATOR: &str = "operator";
const WORKSPACE_ADMIN: &str = "workspace-admin";

async fn state(store: Arc<dyn ExtensionBundleStore>) -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDatabase::new(&tmp.path().join("manager.db").to_string_lossy())
            .await
            .unwrap(),
    );
    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(tmp.path().join("kv")).await.unwrap());
    let command_storage: Arc<dyn alien_bindings::traits::Storage> = Arc::new(
        LocalStorage::new(tmp.path().join("storage").to_string_lossy().to_string()).unwrap(),
    );
    let subjects = HashMap::from([
        (
            DEVELOPER.to_string(),
            subject(
                Scope::Project {
                    project_id: "default".to_string(),
                },
                Role::ProjectDeveloper,
            ),
        ),
        (
            WORKSPACE_ADMIN.to_string(),
            subject(Scope::Workspace, Role::WorkspaceAdmin),
        ),
        (
            OPERATOR.to_string(),
            subject(
                Scope::Deployment {
                    project_id: "default".to_string(),
                    deployment_id: "dep_1".to_string(),
                },
                Role::DeploymentManager,
            ),
        ),
    ]);
    let state = AppState {
        deployment_store: Arc::new(SqliteDeploymentStore::new(db.clone())),
        release_store: Arc::new(SqliteReleaseStore::new(db.clone())),
        project_store: Arc::new(alien_manager::providers::SingleWorkspaceProjects),
        token_store: Arc::new(SqliteTokenStore::new(db)),
        auth_validator: Arc::new(RoleValidator(subjects)),
        authz: Arc::new(OssAuthz),
        telemetry_backend: Arc::new(alien_manager::providers::NullTelemetryBackend),
        credential_resolver: Arc::new(NoCredentials),
        command_server: Arc::new(CommandServer::new(
            kv.clone(),
            command_storage,
            Arc::new(NullCommandDispatcher),
            Arc::new(InMemoryCommandRegistry::default()),
            "http://localhost:0/v1".to_string(),
            b"test-signing-key".to_vec(),
        )),
        config: Arc::new(ManagerConfig::default()),
        bindings_provider: None,
        target_bindings_providers: HashMap::new(),
        kv,
        http_client: reqwest::Client::new(),
        credential_cache: Arc::new(CredentialCache::new()),
        pull_validation_cache: Arc::new(PullValidationCache::new()),
        registry_routing_table: Arc::new(RegistryRoutingTable::new(vec![]).unwrap()),
        host_registrations: alien_infra::HostRegistrations::NONE,
        tunnels: None,
        charts: None,
        release_channels: None,
        bundle_signing_key: None,
        bundle_sources: None,
        extension_bundles: Some(Arc::new(ExtensionBundles::new(store))),
        values: None,
        value_access: None,
        verify_limiter: Arc::new(alien_manager::values::VerifyLimiter::default()),
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
    };
    (state, tmp)
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

async fn call(
    state: &AppState,
    method: Method,
    uri: String,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body))
        .unwrap();
    let response = alien_manager::routes::extension_bundles::router()
        .with_state(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, body.to_vec())
}

async fn upload(state: &AppState, object: &str, bytes: &[u8]) -> (StatusCode, String) {
    let (status, body) = call(
        state,
        Method::POST,
        format!(
            "/v1/extensions/bundles/{}?extension=acme&object={object}",
            digest(bytes)
        ),
        DEVELOPER,
        bytes.to_vec(),
    )
    .await;
    (status, String::from_utf8_lossy(&body).to_string())
}

fn state_dir_store(dir: &std::path::Path) -> Arc<dyn ExtensionBundleStore> {
    Arc::new(StateDirExtensionBundles::new(dir))
}

#[tokio::test]
async fn a_release_uploads_the_controllers_then_the_core_zip_and_the_operator_downloads_them() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(BinaryTarget::LinuxX64, b"controller")]);
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;

    let missing = call(
        &state,
        Method::HEAD,
        format!("/v1/extensions/bundles/{}", bundle.manifest.digest),
        OPERATOR,
        vec![],
    )
    .await;
    assert_eq!(
        missing.0,
        StatusCode::NOT_FOUND,
        "a missing object is how a release finds what to upload"
    );

    assert_eq!(
        upload(&state, "linux-x64", b"controller").await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        upload(&state, "core", &bundle.zip).await.0,
        StatusCode::CREATED
    );

    let (status, body) = call(
        &state,
        Method::GET,
        format!("/v1/extensions/bundles/{}", bundle.manifest.digest),
        OPERATOR,
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bundle.zip);
    assert!(dir
        .path()
        .join("extensions")
        .join(format!("sha256-{:x}", Sha256::digest(&bundle.zip)))
        .exists());
}

#[tokio::test]
async fn refuses_an_object_whose_bytes_are_not_its_digest() {
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;

    let (status, body) = call(
        &state,
        Method::POST,
        format!(
            "/v1/extensions/bundles/{}?extension=acme&object=linux-x64",
            digest(b"controller")
        ),
        DEVELOPER,
        b"tampered".to_vec(),
    )
    .await;

    let body = String::from_utf8_lossy(&body);
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("Extension 'acme'") && body.contains("linux-x64"),
        "{body}"
    );
    assert!(!dir.path().join("extensions").exists());
}

#[tokio::test]
async fn refuses_a_core_zip_that_names_a_controller_not_uploaded() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(BinaryTarget::LinuxX64, b"controller")]);
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;

    let (status, body) = upload(&state, "core", &bundle.zip).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("Extension 'acme'")
            && body.contains("linux-x64")
            && body.contains("not uploaded"),
        "{body}"
    );
}

#[tokio::test]
async fn refuses_a_core_zip_of_another_extension() {
    let bundle = TestBundle::new("other", "1.0.0", &[]);
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;

    let (status, body) = upload(&state, "core", &bundle.zip).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("Extension 'acme'") && body.contains("bundle of other"),
        "{body}"
    );
}

#[tokio::test]
async fn a_deployment_token_cannot_upload() {
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;

    let (status, _) = call(
        &state,
        Method::POST,
        format!(
            "/v1/extensions/bundles/{}?extension=acme&object=linux-x64",
            digest(b"controller")
        ),
        OPERATOR,
        b"controller".to_vec(),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// A store that answers every download with a URL, as an embedder's object store does, and
/// counts the lookups.
struct UrlStore {
    url: String,
    gets: AtomicUsize,
}

#[async_trait]
impl ExtensionBundleStore for UrlStore {
    async fn put(&self, _: &Subject, _: &str, _: Vec<u8>) -> Result<(), AlienError> {
        unreachable!("these tests upload nothing")
    }

    async fn get(
        &self,
        _: &Subject,
        _: &str,
        sha256: &str,
    ) -> Result<Option<BundleObject>, AlienError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        Ok(Some(BundleObject::Url(format!("{}/{sha256}", self.url))))
    }
}

fn deployment_on(bundle: &TestBundle) -> DeploymentState {
    serde_json::from_value(serde_json::json!({
        "platform": "aws", "status": "running",
        "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        "currentRelease": {"releaseId": "rel", "stack": {"id": "app", "resources": {}, "extensions": [bundle.manifest]}}
    }))
    .unwrap()
}

#[tokio::test]
async fn fetches_each_digest_once_and_loads_each_release_its_own_module() {
    let v1 = TestBundle::new("acme", "1.0.0", &[]);
    let v2 = TestBundle::with_hooks(
        "acme",
        "2.0.0",
        r#"[{"name":"all","kind":"awsInterceptor"}]"#,
        &[],
    );
    let server = httpmock::MockServer::start_async().await;
    let mut objects = Vec::new();
    for bundle in [&v1, &v2] {
        let hex = format!("{:x}", Sha256::digest(&bundle.zip));
        let zip = bundle.zip.clone();
        objects.push(
            server
                .mock_async(move |when, then| {
                    when.method(httpmock::Method::GET).path(format!("/{hex}"));
                    then.status(200).body(zip);
                })
                .await,
        );
    }
    let store = Arc::new(UrlStore {
        url: server.base_url(),
        gets: AtomicUsize::new(0),
    });
    let bundles = ExtensionBundles::new(store.clone());

    let mut loaded = Vec::new();
    for bundle in [&v1, &v2, &v1, &v2] {
        let provider = bundles
            .provider_for(
                alien_infra::DefaultPlatformServiceProvider::default(),
                "default",
                "dep_1",
                &deployment_on(bundle),
                |_, _| None,
            )
            .await
            .unwrap();
        loaded.push(
            alien_infra::PlatformServiceProvider::interceptors(
                &provider,
                alien_client_core::interceptor::Cloud::Aws,
                None,
            )
            .is_empty(),
        );
    }

    assert_eq!(
        loaded,
        [true, false, true, false],
        "v2's module intercepts, v1's does not"
    );
    assert_eq!(
        store.gets.load(Ordering::SeqCst),
        2,
        "one lookup per digest"
    );
    for object in objects {
        object.assert_hits_async(1).await;
    }
}

#[tokio::test]
async fn refuses_a_fetched_core_zip_that_is_not_the_pinned_digest() {
    let pinned = TestBundle::new("acme", "1.0.0", &[]);
    let other = TestBundle::new("acme", "1.0.1", &[]);
    let server = httpmock::MockServer::start_async().await;
    let hex = format!("{:x}", Sha256::digest(&pinned.zip));
    let zip = other.zip.clone();
    server
        .mock_async(move |when, then| {
            when.method(httpmock::Method::GET).path(format!("/{hex}"));
            then.status(200).body(zip);
        })
        .await;
    let bundles = ExtensionBundles::new(Arc::new(UrlStore {
        url: server.base_url(),
        gets: AtomicUsize::new(0),
    }));

    let error = bundles
        .provider_for(
            alien_infra::DefaultPlatformServiceProvider::default(),
            "default",
            "dep_1",
            &deployment_on(&pinned),
            |_, _| None,
        )
        .await
        .expect_err("the fetched zip is refused");

    assert!(
        error.message.contains("Extension 'acme'") && error.message.contains("sha256"),
        "{}",
        error.message
    );
}

/// A download longer than a bundle object may be is refused as it arrives.
#[tokio::test]
async fn refuses_a_download_larger_than_a_bundle_object() {
    let pinned = TestBundle::new("acme", "1.0.0", &[]);
    let server = httpmock::MockServer::start_async().await;
    let hex = format!("{:x}", Sha256::digest(&pinned.zip));
    let oversized = vec![0u8; alien_infra::extension_bundle::MAX_OBJECT_BYTES as usize + 1];
    server
        .mock_async(move |when, then| {
            when.method(httpmock::Method::GET).path(format!("/{hex}"));
            then.status(200).body(oversized);
        })
        .await;
    let bundles = ExtensionBundles::new(Arc::new(UrlStore {
        url: server.base_url(),
        gets: AtomicUsize::new(0),
    }));

    let error = bundles
        .provider_for(
            alien_infra::DefaultPlatformServiceProvider::default(),
            "default",
            "dep_1",
            &deployment_on(&pinned),
            |_, _| None,
        )
        .await
        .expect_err("the download is refused");

    assert!(error.message.contains("larger than"), "{}", error.message);
}

/// A token bound to no project names the project it reads; a token bound to one can't name
/// another.
#[tokio::test]
async fn a_download_names_its_project_unless_the_token_is_bound_to_one() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;
    assert_eq!(
        upload(&state, "core", &bundle.zip).await.0,
        StatusCode::CREATED
    );
    let uri = format!("/v1/extensions/bundles/{}", bundle.manifest.digest);

    let unnamed = call(&state, Method::GET, uri.clone(), WORKSPACE_ADMIN, vec![]).await;
    let named = call(
        &state,
        Method::GET,
        format!("{uri}?project=default"),
        WORKSPACE_ADMIN,
        vec![],
    )
    .await;
    let another = call(
        &state,
        Method::GET,
        format!("{uri}?project=other"),
        OPERATOR,
        vec![],
    )
    .await;

    assert_eq!(unnamed.0, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&unnamed.1).contains("?project="));
    assert_eq!(named.0, StatusCode::OK);
    assert_eq!(another.0, StatusCode::FORBIDDEN);
}

/// What a stack import and a served chart run a custom type with: the registries load the
/// extension's hooks module, so its importer runs the module, and a bundle the manager lacks
/// fails naming the extension.
#[tokio::test]
async fn the_importers_a_stack_import_runs_carry_the_extensions_hooks() {
    let bundle = TestBundle::with_hooks(
        "acme",
        "1.0.0",
        r#"[{"name":"import-zone","kind":"importer","resourceType":"acme/zone","platform":"aws"}]"#,
        &[],
    );
    let mut manifest = bundle.manifest.clone();
    manifest.resource_types = serde_json::from_value(serde_json::json!({
        "acme/zone": {
            "lifecycles": ["frozen"], "holdsData": false,
            "setup": {"cloudformation": [], "terraform": [], "helm": [], "importers": ["aws"]},
            "controllerPlatforms": [], "permissionSets": [], "link": {"permissionSets": []},
            "schemas": {"config": {}, "outputs": {}}
        }
    }))
    .unwrap();
    let mut stack = alien_core::Stack::new("app".to_string()).build();
    stack.extensions = vec![manifest];
    let entry: alien_core::ResourceEntry = serde_json::from_value(serde_json::json!({
        "config": {"type": "acme/zone", "id": "zone"}, "lifecycle": "frozen", "dependencies": []
    }))
    .unwrap();
    let settings = alien_core::StackSettings::default();
    let import = |registries: &alien_infra::DeploymentRegistries| {
        registries
            .importers
            .run(
                &alien_core::ResourceType::from("acme/zone"),
                alien_core::Platform::Aws,
                serde_json::json!({}),
                &alien_core::import::ImportContext {
                    resource_id: "zone",
                    platform: alien_core::Platform::Aws,
                    region: "us-east-1",
                    stack_settings: &settings,
                    management_config: None,
                    resource: &entry,
                },
            )
            .unwrap_err()
            .to_string()
    };
    let dir = tempfile::tempdir().unwrap();
    let (state, _tmp) = state(state_dir_store(dir.path())).await;

    let missing = state
        .registries(
            &subject(Scope::Workspace, Role::WorkspaceAdmin),
            "default",
            &stack,
        )
        .await
        .err()
        .unwrap();
    assert_eq!(
        upload(&state, "core", &bundle.zip).await.0,
        StatusCode::CREATED
    );
    let registries = state
        .registries(
            &subject(Scope::Workspace, Role::WorkspaceAdmin),
            "default",
            &stack,
        )
        .await
        .unwrap();

    assert!(
        missing.message.contains("Extension 'acme'"),
        "{}",
        missing.message
    );
    assert_eq!(missing.http_status_code, Some(404), "the store's status");
    // The module answers with no output, which the importer can't read: it ran.
    let ran = import(&registries);
    assert!(!ran.contains("has not loaded"), "{ran}");
}

/// A store that refuses every download with `status`, or with no status at all.
struct RefusingStore {
    status: Option<u16>,
}

#[async_trait]
impl ExtensionBundleStore for RefusingStore {
    async fn put(&self, _: &Subject, _: &str, _: Vec<u8>) -> Result<(), AlienError> {
        unreachable!("these tests upload nothing")
    }

    async fn get(&self, _: &Subject, _: &str, _: &str) -> Result<Option<BundleObject>, AlienError> {
        let mut error = AlienError::new(alien_error::GenericError {
            message: "the store refused".to_string(),
        });
        error.http_status_code = self.status;
        Err(error)
    }
}

/// A store's refusal reaches the caller with its status, and its error stays the cause; a failure
/// with no status is the manager's own.
#[tokio::test]
async fn the_registries_answer_with_the_stores_status_and_keep_its_error() {
    let mut stack = alien_core::Stack::new("app".to_string()).build();
    stack.extensions = vec![TestBundle::new("acme", "1.0.0", &[]).manifest];

    for (status, answered) in [(Some(403), 403), (None, 500)] {
        let (state, _tmp) = state(Arc::new(RefusingStore { status })).await;
        let error = state
            .registries(
                &subject(Scope::Workspace, Role::WorkspaceAdmin),
                "default",
                &stack,
            )
            .await
            .err()
            .unwrap();

        assert_eq!(
            error.http_status_code,
            Some(answered),
            "a store answering {status:?}"
        );
        assert_eq!(
            error.source.as_ref().map(|cause| cause.message.as_str()),
            Some("the store refused"),
            "a store answering {status:?}"
        );
    }
}

/// What a hosted store's failure looks like: a sentence for the caller, with the request that
/// failed, naming the Platform API's address, beneath it.
const PLATFORM_DETAIL: &str =
    "error sending request for url (http://platform-api.internal:8080/v1/extensions/bundles)";

/// A store whose failure says `message`, carries [`PLATFORM_DETAIL`] as its cause, and has
/// `status` (`None`: no status).
struct FailingStore {
    message: &'static str,
    status: Option<u16>,
}

#[async_trait]
impl ExtensionBundleStore for FailingStore {
    async fn put(&self, _: &Subject, _: &str, _: Vec<u8>) -> Result<(), AlienError> {
        unreachable!("these tests upload nothing")
    }

    async fn get(&self, _: &Subject, _: &str, _: &str) -> Result<Option<BundleObject>, AlienError> {
        let cause = AlienError::new(alien_error::GenericError {
            message: PLATFORM_DETAIL.to_string(),
        });
        let mut error = alien_error::ContextError::context(
            cause,
            alien_error::GenericError {
                message: self.message.to_string(),
            },
        );
        error.http_status_code = self.status;
        Err(error)
    }
}

/// A Kubernetes release of `default` named `app` whose stack lists an extension, in a state that
/// serves charts and loads bundles from `store`.
async fn serving_a_chart(store: FailingStore) -> (AppState, tempfile::TempDir) {
    let (mut state, tmp) = state(Arc::new(store)).await;
    state.charts = Some(Arc::new(alien_manager::routes::charts::ChartSettings::new(
        None, false,
    )));
    let mut stack = alien_core::Stack::new("app".to_string()).build();
    stack.extensions = vec![TestBundle::new("acme", "1.0.0", &[]).manifest];
    state
        .release_store
        .create_release(
            &subject(Scope::Workspace, Role::WorkspaceAdmin),
            alien_manager::traits::CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(alien_core::Platform::Kubernetes, stack)]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();
    (state, tmp)
}

/// A pull of the chart's first version, and the OCI error it answers with.
async fn pull_chart(state: &AppState) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/v2/charts/app/manifests/1.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {WORKSPACE_ADMIN}"))
        .body(Body::empty())
        .unwrap();
    let response = alien_manager::routes::registry_proxy::router()
        .with_state(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

/// A chart pull the store refuses is a denied pull, saying the refusal and not its causes.
#[tokio::test]
async fn a_chart_pull_the_store_refuses_is_denied() {
    let (state, _tmp) = serving_a_chart(FailingStore {
        message: "Cannot read this project's extension bundles",
        status: Some(403),
    })
    .await;

    let (status, body) = pull_chart(&state).await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["errors"][0]["code"], "DENIED", "{body}");
    let message = body["errors"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("Cannot read this project's extension bundles"),
        "{message}"
    );
    assert!(!message.contains("platform-api.internal"), "{message}");
}

/// A chart pull for a bundle the store lacks answers 404, as a missing chart version does,
/// with the store's message and none of its causes.
#[tokio::test]
async fn a_chart_pull_for_a_missing_bundle_is_manifest_unknown() {
    let (state, _tmp) = serving_a_chart(FailingStore {
        message: "Extension 'acme' has no bundle sha256:00",
        status: Some(404),
    })
    .await;

    let (status, body) = pull_chart(&state).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["errors"][0]["code"], "MANIFEST_UNKNOWN", "{body}");
    let message = body["errors"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("Extension 'acme' has no bundle"),
        "{message}"
    );
    assert!(!message.contains("platform-api.internal"), "{message}");
}

/// A chart pull the store fails with no status is the manager's own failure, which says only
/// the step: nothing of the store's error reaches the caller.
#[tokio::test]
async fn a_chart_pull_the_store_fails_says_only_the_step() {
    let (state, _tmp) = serving_a_chart(FailingStore {
        message:
            "MISSING_CALLER_WORKSPACE_NAME: http://platform-api.internal:8080 needs a workspace",
        status: None,
    })
    .await;

    let (status, body) = pull_chart(&state).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["errors"][0]["code"], "UNKNOWN", "{body}");
    assert_eq!(
        body["errors"][0]["message"], "failed to load the release's extensions",
        "{body}"
    );
}

/// A stack import answers a registries failure with the error's own response, which hides an
/// internal error's causes.
#[tokio::test]
async fn a_stack_import_answers_an_internal_failure_without_its_causes() {
    use axum::response::IntoResponse;
    let mut stack = alien_core::Stack::new("app".to_string()).build();
    stack.extensions = vec![TestBundle::new("acme", "1.0.0", &[]).manifest];
    let (state, _tmp) = state(Arc::new(FailingStore {
        message: "failed to ask for an extension bundle download",
        status: None,
    }))
    .await;
    let error = state
        .registries(
            &subject(Scope::Workspace, Role::WorkspaceAdmin),
            "default",
            &stack,
        )
        .await
        .err()
        .unwrap();

    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(!body.contains("platform-api.internal"), "{body}");
}
