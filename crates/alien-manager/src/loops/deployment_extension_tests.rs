//! Push deployments whose custom resources the manager's deployment loop steps in the
//! controller module each release's bundle carries: in the manager's process, with no
//! credentials of its own, its cloud calls signed with credentials minted for the extension's
//! identity for each step.

use super::{DeploymentLoop, ProcessOptions};
use crate::auth::Subject;
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
use alien_infra::extension_bundle::testing::{controller_guest, TestBundle};
use alien_infra::step_protocol::protocol::Credentials;
use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, Mutex};

/// The custom type of the test's controller module: each step runs the resource's `probe`.
const PROBE: &str = "acme/probe";

/// The EC2 endpoint of the region the minted credentials are for.
const AWS_EC2: &str = "https://ec2.us-east-1.amazonaws.com";

/// The acme extension's bundle, whose controller is the test's controller module.
fn bundle() -> TestBundle {
    TestBundle::with_controller_module("acme", "1.0.0", controller_guest(), &[])
}

/// When the credentials [`minted`] gives expire.
const MINTED_UNTIL: &str = "2099-01-01T00:00:00Z";

/// What the manager mints for the extension's identity: distinct per deployment, and from
/// nothing the deployment's management identity holds.
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

/// A stack with the acme extension's probe, which calls EC2 once in each step, and the
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
                "config": {
                    "type": PROBE, "id": "zone", "probe": "cloud",
                    "arg": {
                        "cloud": "aws", "service": "ec2", "method": "POST", "path": "/",
                        "body": "Action=DescribeRegions&Version=2016-11-15"
                    }
                },
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
                PROBE: {
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

/// A manager whose deployment loop mints through [`Minting`]. The calls its controller modules
/// make to EC2 go to a server on this machine.
struct Manager {
    _temp: tempfile::TempDir,
    store: Arc<SqliteDeploymentStore>,
    bundles: Arc<crate::providers::StateDirExtensionBundles>,
    resolver: Arc<Minting>,
    deployment_loop: DeploymentLoop,
}

impl Manager {
    async fn new(ec2: &str) -> Self {
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
        let host = alien_infra::step_protocol::wasm::WasmHost::new()
            .unwrap()
            .routing(AWS_EC2, ec2);
        let deployment_loop = DeploymentLoop::new(
            Arc::new(crate::config::ManagerConfig::default()),
            store.clone(),
            Arc::new(SqliteReleaseStore::new(db.clone())),
            resolver.clone(),
            bindings,
            None,
        )
        .with_extension_bundles(Some(Arc::new(
            crate::extension_bundles::ExtensionBundles::new(bundles.clone())
                .with_module_host(Arc::new(host)),
        )));
        Self {
            _temp: temp,
            store,
            bundles,
            resolver,
            deployment_loop,
        }
    }

    /// A push deployment of `stack`, whose extension's bundle this manager stores.
    async fn deploy(&self, name: &str, bundle: &TestBundle) -> String {
        let hex = bundle.manifest.digest.strip_prefix("sha256:").unwrap();
        self.bundles
            .put(&Subject::system(), "default", hex, bundle.zip.clone())
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

/// The access key id each of `ids`' steps is signed with.
fn keys_of(ids: &[&str]) -> Vec<String> {
    ids.iter()
        .map(|id| format!("ASIA-EXTENSION-{id}"))
        .collect()
}

/// The manager steps the probe in the bundle's controller module until it runs. Every call the
/// module makes reaches the cloud signed with credentials minted for the extension's identity
/// in this deployment, one mint per step.
#[tokio::test(flavor = "multi_thread")]
async fn a_push_deployment_steps_its_custom_resource_in_its_controller_module() {
    let (ec2, signers) = describe_regions_ec2().await;
    let manager = Manager::new(&ec2).await;
    let bundle = bundle();
    let stack = stack(&bundle);
    let id = manager.deploy("extended", &bundle).await;

    let stored = manager.run(&id, &stack, 10).await;

    assert_eq!(stored.status, "running", "{:?}", stored.error);
    let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
    let progress = zone.internal_state.as_ref().unwrap()["remote"]["progress"].to_string();
    assert!(progress.contains("DescribeRegionsResponse"), "{progress}");
    assert!(!progress.contains("extension-secret"), "{progress}");
    let signers = signers.lock().unwrap();
    assert!(
        !signers.is_empty(),
        "no call of the module reached the cloud"
    );
    assert!(
        signers.iter().all(|key| *key == keys_of(&[&id])[0]),
        "{signers:?}"
    );
    let mints = manager.resolver.mints.lock().unwrap();
    assert_eq!(mints.len(), signers.len(), "one mint per step");
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

/// Two deployments of the same extension share its compiled module, and each one's calls are
/// signed with the credentials minted for it.
#[tokio::test(flavor = "multi_thread")]
async fn two_deployments_of_one_extension_call_the_cloud_as_their_own_identities() {
    let (ec2, signers) = describe_regions_ec2().await;
    let manager = Manager::new(&ec2).await;
    let bundle = bundle();
    let stack = stack(&bundle);
    let a = manager.deploy("a", &bundle).await;
    let b = manager.deploy("b", &bundle).await;

    let (ran_a, ran_b) = tokio::join!(manager.run(&a, &stack, 10), manager.run(&b, &stack, 10));

    assert_eq!(ran_a.status, "running", "{:?}", ran_a.error);
    assert_eq!(ran_b.status, "running", "{:?}", ran_b.error);
    let mut signers = signers.lock().unwrap().clone();
    signers.sort();
    signers.dedup();
    let mut expected = keys_of(&[&a, &b]);
    expected.sort();
    assert_eq!(signers, expected);
    let mints = manager.resolver.mints.lock().unwrap();
    for id in [&a, &b] {
        assert!(mints.iter().any(|(deployment, _)| deployment == id));
    }
}

/// An extension the deployment has no identity for, as on Linux servers or when its types hold
/// no permission sets, still runs its controller module: the step carries no credentials, so
/// the module's cloud call is refused before anything is minted or sent.
#[tokio::test(flavor = "multi_thread")]
async fn an_extension_without_an_identity_steps_with_no_credentials() {
    let (ec2, signers) = describe_regions_ec2().await;
    let manager = Manager::new(&ec2).await;
    let bundle = bundle();
    let mut stack = stack(&bundle);
    stack.resources.shift_remove("acme-ext");
    let id = manager.deploy("no-identity", &bundle).await;

    let stored = manager.run(&id, &stack, 10).await;

    assert_eq!(stored.status, "running", "{:?}", stored.error);
    let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
    let progress = zone.internal_state.as_ref().unwrap()["remote"]["progress"].to_string();
    assert!(progress.contains("refused: "), "{progress}");
    assert!(
        signers.lock().unwrap().is_empty(),
        "the module called the cloud"
    );
    assert!(manager.resolver.mints.lock().unwrap().is_empty());
}

/// Credentials that expire before a step's deadline and grace are never used: the step fails
/// before the module runs.
#[tokio::test(flavor = "multi_thread")]
async fn credentials_that_expire_before_the_step_ends_are_never_used() {
    let (ec2, signers) = describe_regions_ec2().await;
    let manager = Manager::new(&ec2).await;
    *manager.resolver.expires_at.lock().unwrap() =
        (chrono::Utc::now() + chrono::Duration::seconds(20)).to_rfc3339();
    let bundle = bundle();
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
    assert!(
        signers.lock().unwrap().is_empty(),
        "the module called the cloud"
    );
}

/// Credentials whose expiry can't be read are never used, and the step's error says so rather
/// than call them short-lived.
#[tokio::test(flavor = "multi_thread")]
async fn credentials_with_no_readable_expiry_are_never_used() {
    let (ec2, signers) = describe_regions_ec2().await;
    let manager = Manager::new(&ec2).await;
    *manager.resolver.expires_at.lock().unwrap() = "next tuesday".to_string();
    let bundle = bundle();
    let stack = stack(&bundle);
    let id = manager.deploy("unreadable-expiry", &bundle).await;

    let stored = manager.run(&id, &stack, 3).await;

    let zone = &stored.stack_state.as_ref().unwrap().resources["zone"];
    let error = zone.error.as_ref().expect("the zone's step failed");
    assert_eq!(error.code, "AUTHENTICATION_FAILED", "{error}");
    assert!(
        error.message.contains("have no readable expiry"),
        "{}",
        error.message
    );
    assert!(
        signers.lock().unwrap().is_empty(),
        "the module called the cloud"
    );
}

/// The identity a step's credentials are minted for is the service account setup created for
/// this extension: one with no extension, or another's, is refused before any mint or call.
#[tokio::test(flavor = "multi_thread")]
async fn an_identity_not_created_for_the_extension_gets_no_credentials() {
    let (ec2, signers) = describe_regions_ec2().await;
    let manager = Manager::new(&ec2).await;
    let bundle = bundle();
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
    assert!(
        signers.lock().unwrap().is_empty(),
        "the module called the cloud"
    );
    assert!(manager.resolver.mints.lock().unwrap().is_empty());
}

/// An identity the resolver's check refuses fails the step as AUTHENTICATION_FAILED, naming the
/// extension, and the module makes no call.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_identity_fails_the_step_as_an_authentication_failure() {
    let (ec2, signers) = describe_regions_ec2().await;
    let manager = Manager::new(&ec2).await;
    manager
        .resolver
        .checks_identity
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let bundle = bundle();
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
    assert!(
        signers.lock().unwrap().is_empty(),
        "the module called the cloud"
    );
    assert!(manager.resolver.mints.lock().unwrap().is_empty());
}

/// An EC2 on loopback that answers DescribeRegions and records each call's access key id.
async fn describe_regions_ec2() -> (String, Arc<Mutex<Vec<String>>>) {
    let signers: Arc<Mutex<Vec<String>>> = Arc::default();
    let kept = signers.clone();
    let app = axum::Router::new().route(
        "/",
        axum::routing::post(move |headers: axum::http::HeaderMap| {
            let kept = kept.clone();
            async move {
                let signer = headers["authorization"]
                    .to_str()
                    .unwrap()
                    .split("Credential=")
                    .nth(1)
                    .and_then(|rest| rest.split('/').next())
                    .unwrap()
                    .to_string();
                kept.lock().unwrap().push(signer);
                "<DescribeRegionsResponse><regionInfo><item><regionName>us-east-1</regionName></item></regionInfo></DescribeRegionsResponse>"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, signers)
}
