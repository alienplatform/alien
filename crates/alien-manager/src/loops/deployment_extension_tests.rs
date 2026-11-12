//! Push deployments whose custom resources the manager's deployment loop steps on the server
//! each release's bundle pins: over HTTPS, with a token the manager signs and publishes the key
//! of, and with credentials minted for the extension's identity for each step.

use super::{DeploymentLoop, ProcessOptions};
use crate::auth::Subject;
use crate::extension_controllers::{jwks_router, ExtensionControllers, STEP_TOKEN_JWKS_PATH};
use crate::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore};
use crate::traits::deployment_store::{
    CreateDeploymentParams, DeploymentFilter, DeploymentRecord, SuppliedStacks,
};
use crate::traits::{
    CredentialResolver, DeploymentStore, ExtensionBundleStore, ExtensionIdentity,
    ResolvedCredentials, ServerBindings,
};
use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
use alien_commands::{dispatchers::NullCommandDispatcher, InMemoryCommandRegistry};
use alien_core::{
    Platform, Resource, ResourceEntry, ResourceLifecycle, ServiceAccount, Stack, StackSettings,
};
use alien_error::AlienError;
use alien_infra::extension_bundle::testing::TestBundle;
use alien_infra::step_protocol::protocol::{Credentials, StepRequest};
use alien_infra::step_protocol::test_controller::{ControllerHost, DNS_ZONE};
use alien_infra::step_protocol::token::{self, Expected};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt as _;

/// The URL the manager signs as, and publishes its JWKS under.
const MANAGER_URL: &str = "https://manager.example.com";

/// When the credentials [`minted`] gives expire.
const MINTED_UNTIL: &str = "2099-01-01T00:00:00Z";

/// What the manager mints for the extension's identity: distinct per deployment, and from
/// nothing the deployment's management identity holds.
fn minted(deployment_id: &str) -> Credentials {
    minted_until(deployment_id, MINTED_UNTIL)
}

fn minted_until(deployment_id: &str, expires_at: &str) -> Credentials {
    Credentials::Aws {
        account_id: "123456789012".to_string(),
        region: "us-east-1".to_string(),
        access_key_id: format!("ASIA-EXTENSION-{deployment_id}"),
        secret_access_key: "extension-secret".to_string(),
        session_token: Some("extension-session".to_string()),
        expires_at: Some(expires_at.to_string()),
    }
}

/// A resolver whose management identity is the test platform's, and that mints the
/// extension's credentials, valid until `expires_at`, recording each mint.
struct Minting {
    mints: Mutex<Vec<(String, ExtensionIdentity)>>,
    expires_at: Mutex<String>,
    /// Runs the real resolvers' identity check before minting.
    checks_identity: std::sync::atomic::AtomicBool,
}

impl Default for Minting {
    fn default() -> Self {
        Self {
            mints: Mutex::default(),
            expires_at: Mutex::new(MINTED_UNTIL.to_string()),
            checks_identity: Default::default(),
        }
    }
}

#[async_trait]
impl CredentialResolver for Minting {
    async fn management_external_id(
        &self,
        _: &DeploymentRecord,
    ) -> Result<Option<alien_core::ManagementExternalId>, AlienError> {
        Ok(None)
    }

    async fn resolve(&self, _: &DeploymentRecord) -> Result<alien_core::ClientConfig, AlienError> {
        Ok(alien_core::ClientConfig::Test)
    }

    async fn resolve_with_capability(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<ResolvedCredentials, AlienError> {
        Ok(ResolvedCredentials {
            client_config: self.resolve(deployment).await?,
            has_provision_capability: true,
        })
    }

    async fn resolve_extension_credentials(
        &self,
        deployment: &DeploymentRecord,
        identity: &ExtensionIdentity,
    ) -> Result<Credentials, AlienError> {
        if self
            .checks_identity
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            crate::providers::extension_credentials::check_identity(deployment, identity)?;
        }
        self.mints
            .lock()
            .unwrap()
            .push((deployment.id.clone(), identity.clone()));
        Ok(minted_until(
            &deployment.id,
            &self.expires_at.lock().unwrap(),
        ))
    }
}

/// One step a controller server received, with its token's check.
struct Received {
    request: StepRequest,
    token: Result<token::Claims, token::TokenRefusal>,
}

/// A controller server over HTTPS on loopback that checks each token against the keys the
/// manager publishes, for its own URL.
struct Server {
    url: String,
    root: reqwest::Certificate,
    received: Arc<Mutex<Vec<Received>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn start(published: &Value, cloud: &std::path::Path) -> Self {
        let certificate =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der())
                .into(),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let keys = token::keys_from_jwks(published);
        let host = Arc::new(ControllerHost::new(cloud, None));
        let app = {
            let (url, received) = (url.clone(), received.clone());
            axum::Router::new().route(
                "/v1/step",
                axum::routing::post(
                    move |headers: axum::http::HeaderMap, raw: axum::body::Bytes| {
                        let (url, received, keys, host) =
                            (url.clone(), received.clone(), keys.clone(), host.clone());
                        async move {
                            use axum::response::IntoResponse;
                            let request: StepRequest =
                                serde_json::from_slice(&raw).expect("a step request");
                            let bearer = headers
                                .get(axum::http::header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .and_then(|value| value.strip_prefix("Bearer "))
                                .unwrap_or_default()
                                .to_string();
                            let checked = token::verify(
                                &bearer,
                                &keys,
                                Expected {
                                    issuer: MANAGER_URL,
                                    audience: &url,
                                    projects: &["default"],
                                    deployment_id: &request.deployment_id,
                                    extension: "acme",
                                    resource_id: &request.resource_id,
                                    step_id: &request.step_id,
                                    body: &raw,
                                },
                                chrono::Utc::now().timestamp(),
                            );
                            let refused = checked.is_err();
                            received.lock().unwrap().push(Received {
                                request: request.clone(),
                                token: checked,
                            });
                            if refused {
                                return axum::http::StatusCode::UNAUTHORIZED.into_response();
                            }
                            // The test controller's zone makes no cloud call, and takes no
                            // credentials on the test platform.
                            let request = StepRequest {
                                credentials: None,
                                ..request
                            };
                            axum::Json(host.handle(request).await).into_response()
                        }
                    },
                ),
            )
        };
        let tls = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(tls));
        let task = tokio::spawn(async move {
            axum_server::from_tcp_rustls(listener, tls)
                .unwrap()
                .serve(app.into_make_service())
                .await
                .unwrap();
        });
        Self {
            url,
            root: reqwest::Certificate::from_pem(certificate.cert.pem().as_bytes()).unwrap(),
            received,
            task,
        }
    }

    fn received(&self) -> std::sync::MutexGuard<'_, Vec<Received>> {
        self.received.lock().unwrap()
    }
}

/// A stack with the acme extension's zone, whose controller runs on `bundle`'s server, and the
/// identity setup creates for the extension.
fn stack(bundle: &TestBundle) -> Stack {
    stack_whose_identity_is_for(bundle, Some("acme"))
}

/// [`stack`], with an `acme-ext` service account created for `extension`.
fn stack_whose_identity_is_for(bundle: &TestBundle, extension: Option<&str>) -> Stack {
    let mut stack: Stack = serde_json::from_value(json!({
        "id": "acme",
        "resources": {
            "zone": {
                "config": { "type": DNS_ZONE, "id": "zone", "domain": "internal.acme.dev" },
                "lifecycle": "live",
                "dependencies": []
            }
        },
        "permissions": { "profiles": {}, "management": "auto" },
        "extensions": [{
            "name": "acme",
            "version": bundle.manifest.version,
            "digest": bundle.manifest.digest,
            "resourceTypes": {
                DNS_ZONE: {
                    "lifecycles": ["live"],
                    "holdsData": false,
                    "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
                    "controllerPlatforms": ["test"],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": {}, "outputs": {} }
                }
            }
        }]
    }))
    .unwrap();
    let identity = ServiceAccount::new("acme-ext".to_string())
        .maybe_extension(extension.map(str::to_string))
        .build();
    stack.resources.insert(
        "acme-ext".to_string(),
        ResourceEntry {
            enabled_when: None,
            config: Resource::new(identity),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            display_name: None,
            description: None,
            remote_access: false,
        },
    );
    stack
}

/// A manager whose deployment loop signs with one key and mints through [`Minting`].
struct Manager {
    _temp: tempfile::TempDir,
    store: Arc<SqliteDeploymentStore>,
    bundles: Arc<crate::providers::StateDirExtensionBundles>,
    resolver: Arc<Minting>,
    deployment_loop: DeploymentLoop,
    /// The JWKS the manager serves at its well-known path.
    jwks: Value,
}

impl Manager {
    /// `local` admits controller servers on loopback, trusting these roots, as only a test does.
    async fn new(local: Option<Vec<reqwest::Certificate>>) -> Self {
        Self::build(local, true).await
    }

    /// A manager with no step-token key.
    async fn unsigned() -> Self {
        Self::build(None, false).await
    }

    async fn build(local: Option<Vec<reqwest::Certificate>>, signs: bool) -> Self {
        // The secrets digest key the manager builder installs.
        alien_deployment::set_ephemeral_secrets_digest_key();
        let temp = tempfile::tempdir().unwrap();
        let db = Arc::new(
            SqliteDatabase::new(temp.path().join("manager.db").to_str().unwrap())
                .await
                .unwrap(),
        );
        let store = Arc::new(SqliteDeploymentStore::new(db.clone()));
        let bindings = Arc::new(ServerBindings {
            local_queues: Arc::new(alien_local::LocalQueueService::default()),
            kv: Arc::new(LocalKv::new(temp.path().join("kv")).await.unwrap()),
            command_storage: Arc::new(
                LocalStorage::new(temp.path().join("storage").to_string_lossy().to_string())
                    .unwrap(),
            ),
            command_dispatcher: Arc::new(NullCommandDispatcher),
            command_registry: Arc::new(InMemoryCommandRegistry::default()),
            artifact_registry: None,
            bindings_provider: None,
            target_bindings_providers: Default::default(),
            registry_routing_table: Arc::new(
                crate::routes::registry_proxy::RegistryRoutingTable::new(vec![]).unwrap(),
            ),
        });
        let bundles = Arc::new(crate::providers::StateDirExtensionBundles::new(
            temp.path().join("bundles"),
        ));
        let resolver = Arc::new(Minting::default());
        let key = Arc::new(ed25519_compact::KeyPair::from_seed(
            ed25519_compact::Seed::new([9; 32]),
        ));
        let response = jwks_router(&key)
            .oneshot(
                axum::http::Request::get(STEP_TOKEN_JWKS_PATH)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let jwks: Value = serde_json::from_slice(
            &http_body_util::BodyExt::collect(response.into_body())
                .await
                .unwrap()
                .to_bytes(),
        )
        .unwrap();
        let config = crate::config::ManagerConfig {
            base_url: Some(MANAGER_URL.to_string()),
            ..Default::default()
        };
        let controllers = ExtensionControllers::new(config.base_url(), key, resolver.clone());
        let controllers = match local {
            Some(roots) => controllers.allowing_local(roots),
            None => controllers,
        };
        let deployment_loop = DeploymentLoop::new(
            Arc::new(config),
            store.clone(),
            Arc::new(SqliteReleaseStore::new(db.clone())),
            resolver.clone(),
            bindings,
            None,
        )
        .with_extension_bundles(Some(Arc::new(
            crate::extension_bundles::ExtensionBundles::new(bundles.clone()),
        )))
        .with_extension_controllers(signs.then(|| Arc::new(controllers)));
        Self {
            _temp: temp,
            store,
            bundles,
            resolver,
            deployment_loop,
            jwks,
        }
    }

    /// A push deployment of `stack`, whose extension's bundle this manager stores.
    async fn deploy(&self, name: &str, bundle: &TestBundle) -> String {
        let hex = bundle.manifest.digest.strip_prefix("sha256:").unwrap();
        self.bundles
            .put(&Subject::system(), hex, bundle.zip.clone())
            .await
            .unwrap();
        self.store
            .create_deployment(
                &Subject::system(),
                CreateDeploymentParams {
                    name: name.to_string(),
                    deployment_group_id: "dg_test".to_string(),
                    platform: Platform::Test,
                    deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                    base_platform: None,
                    stack_settings: StackSettings {
                        deployment_model: alien_core::DeploymentModel::Push,
                        ..Default::default()
                    },
                    stack_state: None,
                    environment_variables: None,
                    public_subdomain: None,
                    input_values: Default::default(),
                    setup_item: None,
                    deployment_token: None,
                },
            )
            .await
            .unwrap()
            .id
    }

    /// Runs the deployment loop on `id`, releasing `stack`, until it runs or fails, or for at
    /// most `ticks` ticks, each once the deployment is due. Returns the stored record.
    async fn run(&self, id: &str, stack: &Stack, ticks: usize) -> DeploymentRecord {
        let caller = Subject::system();
        let filter = DeploymentFilter {
            deployment_ids: Some(vec![id.to_string()]),
            ..Default::default()
        };
        let mut ran = 0;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while ran < ticks && std::time::Instant::now() < deadline {
            // A step that asks for a delay makes the deployment due again only after it.
            let Some(mut item) = self
                .store
                .acquire(&caller, "manager-session", &filter, 1)
                .await
                .unwrap()
                .pop()
            else {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            };
            item.deployment.desired_release_id = Some(format!("rel_{id}"));
            item.deployment.supplied_stacks = Some(SuppliedStacks {
                desired: Some(stack.clone()),
                current: None,
            });
            self.deployment_loop
                .process_deployment(
                    item.deployment,
                    item.execution_claim,
                    "manager-session",
                    ProcessOptions::deployment_tick(),
                )
                .await;
            ran += 1;
            let stored = self
                .store
                .get_deployment(&caller, id)
                .await
                .unwrap()
                .unwrap();
            if stored.status == "running" || stored.status.ends_with("-failed") {
                return stored;
            }
        }
        self.store
            .get_deployment(&caller, id)
            .await
            .unwrap()
            .unwrap()
    }
}

/// The manager steps the zone on the server the release pins until it runs. Every step carries
/// a token that verifies against the JWKS the manager publishes, for that server and this
/// deployment, and credentials minted for the extension's identity for that step.
#[tokio::test]
async fn a_push_deployment_steps_its_custom_resource_on_the_pinned_server() {
    let cloud = tempfile::tempdir().unwrap();
    let probe = Manager::new(None).await;
    let server = Server::start(&probe.jwks, cloud.path()).await;
    let manager = Manager::new(Some(vec![server.root.clone()])).await;
    let bundle = TestBundle::serving("acme", "1.0.0", &server.url);
    let stack = stack(&bundle);
    let id = manager.deploy("extended", &bundle).await;

    let stored = manager.run(&id, &stack, 10).await;

    assert_eq!(stored.status, "running", "{:?}", stored.error);
    let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
    let outputs = serde_json::to_value(zone.outputs.as_ref().unwrap()).unwrap();
    assert_eq!(outputs["domain"], "internal.acme.dev", "{outputs}");
    let received = server.received();
    assert!(!received.is_empty(), "the server got no step");
    for step in received.iter() {
        let claims = step.token.as_ref().unwrap_or_else(|reason| {
            panic!(
                "step {} had a token that failed: {reason}",
                step.request.step_id
            )
        });
        assert_eq!(claims.iss, MANAGER_URL);
        assert_eq!(claims.aud, server.url);
        assert_eq!(claims.deployment_id, id);
        assert_eq!(step.request.deployment_id, id);
        assert_eq!(step.request.credentials, Some(minted(&id)));
    }
    let mints = manager.resolver.mints.lock().unwrap();
    assert_eq!(mints.len(), received.len(), "one mint per step");
    for (deployment, identity) in mints.iter() {
        assert_eq!(deployment, &id);
        assert_eq!(
            identity,
            &ExtensionIdentity {
                extension: "acme".to_string(),
                id: "acme-ext".to_string(),
                identity: "test-sa-acme-ext".to_string(),
            }
        );
    }
}

/// Two deployments of the same extension, released with different `controller-url`s, are each
/// stepped on their own release's server only.
#[tokio::test]
async fn two_deployments_of_one_extension_reach_their_own_servers() {
    let (cloud_a, cloud_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let probe = Manager::new(None).await;
    let server_a = Server::start(&probe.jwks, cloud_a.path()).await;
    let server_b = Server::start(&probe.jwks, cloud_b.path()).await;
    let manager = Manager::new(Some(vec![server_a.root.clone(), server_b.root.clone()])).await;
    let bundle_a = TestBundle::serving("acme", "1.0.0", &server_a.url);
    let bundle_b = TestBundle::serving("acme", "1.0.0", &server_b.url);
    let (stack_a, stack_b) = (stack(&bundle_a), stack(&bundle_b));
    let a = manager.deploy("a", &bundle_a).await;
    let b = manager.deploy("b", &bundle_b).await;

    let (ran_a, ran_b) = tokio::join!(manager.run(&a, &stack_a, 10), manager.run(&b, &stack_b, 10));

    assert_eq!(ran_a.status, "running", "{:?}", ran_a.error);
    assert_eq!(ran_b.status, "running", "{:?}", ran_b.error);
    for (server, id) in [(&server_a, &a), (&server_b, &b)] {
        let received = server.received();
        assert!(!received.is_empty(), "{id}'s server got no step");
        assert!(
            received.iter().all(|step| step.request.deployment_id == *id
                && step.request.credentials == Some(minted(id))
                && step.token.is_ok()),
            "{}'s server got another deployment's step",
            server.url
        );
    }
}

/// A `controller-url` that resolves to a loopback address is refused when the manager calls it:
/// the step fails as unreachable, and nothing reaches the server there.
#[tokio::test]
async fn the_manager_never_calls_a_controller_url_on_a_private_address() {
    let cloud = tempfile::tempdir().unwrap();
    let manager = Manager::new(None).await;
    let server = Server::start(&manager.jwks, cloud.path()).await;
    let port = server.url.rsplit(':').next().unwrap();
    for (name, url) in [
        ("by-address", server.url.clone()),
        ("by-name", format!("https://localhost:{port}")),
    ] {
        let bundle = TestBundle::serving("acme", "1.0.0", &url);
        let stack = stack(&bundle);
        let id = manager.deploy(name, &bundle).await;

        let stored = manager.run(&id, &stack, 3).await;

        let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
        let error = zone.error.as_ref().expect("the zone's step failed");
        assert_eq!(
            error.code, "EXTENSION_CONTROLLER_UNREACHABLE",
            "{url}: {error}"
        );
        assert!(
            error.message.contains("not a public address"),
            "{url}: {}",
            error.message
        );
    }
    assert!(server.received().is_empty(), "a step reached the server");
    assert!(
        manager.resolver.mints.lock().unwrap().is_empty(),
        "credentials were minted for a server that can't be called"
    );
}

/// Credentials that expire before a step's deadline and grace are never sent: the step fails
/// before the server is called.
#[tokio::test]
async fn credentials_that_expire_before_the_step_ends_are_never_sent() {
    let cloud = tempfile::tempdir().unwrap();
    let probe = Manager::new(None).await;
    let server = Server::start(&probe.jwks, cloud.path()).await;
    let manager = Manager::new(Some(vec![server.root.clone()])).await;
    *manager.resolver.expires_at.lock().unwrap() =
        (chrono::Utc::now() + chrono::Duration::seconds(20)).to_rfc3339();
    let bundle = TestBundle::serving("acme", "1.0.0", &server.url);
    let stack = stack(&bundle);
    let id = manager.deploy("short-lived", &bundle).await;

    let stored = manager.run(&id, &stack, 3).await;

    let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
    let error = zone.error.as_ref().expect("the zone's step failed");
    assert_eq!(error.code, "AUTHENTICATION_FAILED", "{error}");
    assert!(
        error.message.contains("expire before the step's deadline"),
        "{}",
        error.message
    );
    assert!(server.received().is_empty(), "a step reached the server");
}

/// The identity a step's credentials are minted for is the service account setup created for
/// this extension: one with no extension, or another's, is refused before any mint or call.
#[tokio::test]
async fn an_identity_not_created_for_the_extension_gets_no_credentials() {
    let cloud = tempfile::tempdir().unwrap();
    let probe = Manager::new(None).await;
    let server = Server::start(&probe.jwks, cloud.path()).await;
    let manager = Manager::new(Some(vec![server.root.clone()])).await;
    let bundle = TestBundle::serving("acme", "1.0.0", &server.url);
    for (name, extension) in [("unowned", None), ("another-extensions", Some("other"))] {
        let stack = stack_whose_identity_is_for(&bundle, extension);
        let id = manager.deploy(name, &bundle).await;

        let stored = manager.run(&id, &stack, 3).await;

        let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
        let error = zone.error.as_ref().expect("the zone's step failed");
        assert_eq!(error.code, "AUTHENTICATION_FAILED", "{name}: {error}");
        assert!(
            error.message.contains("not set up in this deployment"),
            "{name}: {}",
            error.message
        );
    }
    assert!(server.received().is_empty(), "a step reached the server");
    assert!(manager.resolver.mints.lock().unwrap().is_empty());
}

/// A manager with no step-token key says so for a custom resource whose controller runs on a
/// server, and neither mints nor calls anything.
#[tokio::test]
async fn a_manager_without_a_step_token_key_says_so() {
    let cloud = tempfile::tempdir().unwrap();
    let probe = Manager::new(None).await;
    let server = Server::start(&probe.jwks, cloud.path()).await;
    let manager = Manager::unsigned().await;
    let bundle = TestBundle::serving("acme", "1.0.0", &server.url);
    let stack = stack(&bundle);
    let id = manager.deploy("unsigned", &bundle).await;

    let stored = manager.run(&id, &stack, 3).await;

    let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
    let error = zone.error.as_ref().expect("the zone's step failed");
    assert_eq!(
        error.code,
        crate::extension_controllers::STEP_TOKEN_KEY_MISSING,
        "{error}"
    );
    assert!(
        error.message.contains("no step-token signing key"),
        "{}",
        error.message
    );
    assert!(server.received().is_empty());
    assert!(manager.resolver.mints.lock().unwrap().is_empty());
}

/// An identity the resolver's check refuses fails the step as AUTHENTICATION_FAILED, naming the
/// extension, and the server is never called.
#[tokio::test]
async fn a_refused_identity_fails_the_step_as_an_authentication_failure() {
    let cloud = tempfile::tempdir().unwrap();
    let probe = Manager::new(None).await;
    let server = Server::start(&probe.jwks, cloud.path()).await;
    let manager = Manager::new(Some(vec![server.root.clone()])).await;
    manager
        .resolver
        .checks_identity
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let bundle = TestBundle::serving("acme", "1.0.0", &server.url);
    let stack = stack(&bundle);
    let id = manager.deploy("refused", &bundle).await;

    let stored = manager.run(&id, &stack, 3).await;

    let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
    let error = zone.error.as_ref().expect("the zone's step failed");
    assert_eq!(error.code, "AUTHENTICATION_FAILED", "{error}");
    assert!(
        error
            .message
            .contains("cannot mint credentials for the identity of extension 'acme'"),
        "{}",
        error.message
    );
    assert!(server.received().is_empty(), "a step reached the server");
    assert!(manager.resolver.mints.lock().unwrap().is_empty());
}
