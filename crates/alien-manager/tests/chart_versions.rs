//! Chart versions the manager's registry serves never change content.
//!
//! GitOps tools (Argo CD, Flux, Renovate) treat a chart version as immutable:
//! they cache it, compare it, and only act when a higher one appears. A
//! manager upgrade changes the Operator image every chart installs, so the
//! same release must publish a new, higher version, and a version must never
//! resolve to a different digest.
//!
//! The test runs two managers over one database, one per Operator image, the
//! way a manager upgrade does, and pulls through the real registry route.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use alien_bindings::{providers::kv::local::LocalKv, providers::storage::local::LocalStorage};
use alien_commands::dispatchers::NullCommandDispatcher;
use alien_commands::server::{CommandDispatcher, CommandRegistry, CommandServer};
use alien_commands::InMemoryCommandRegistry;
use alien_core::{ClientConfig, Platform, Stack};
use alien_error::AlienError;
use alien_manager::auth::Subject;
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::routes::charts::ChartSettings;
use alien_manager::routes::registry_proxy::{
    CredentialCache, PullValidationCache, RegistryRoutingTable,
};
use alien_manager::routes::AppState;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore, SqliteTokenStore,
};
use alien_manager::traits::{
    AuthValidator, CreateReleaseParams, CredentialResolver, DeploymentRecord, DeploymentStore,
    ReleaseStore, TokenStore,
};
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use flate2::read::GzDecoder;
use http::header;
use semver::Version;
use serde_json::Value;
use tower::ServiceExt;

const CHART: &str = "chart-versions";
const OLD_OPERATOR: &str = "ghcr.io/alienplatform/alien-operator:v3.3.57";
const NEW_OPERATOR: &str = "ghcr.io/alienplatform/alien-operator:v3.3.59";

/// Every request is the manager's own admin.
struct Admin;

#[async_trait]
impl AuthValidator for Admin {
    async fn validate(&self, _headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        Ok(Some(Subject::system()))
    }
}

struct NoCredentials;

#[async_trait]
impl CredentialResolver for NoCredentials {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        unreachable!("chart routes resolve no credentials")
    }
}

struct Stores {
    deployment_store: Arc<dyn DeploymentStore>,
    release_store: Arc<dyn ReleaseStore>,
    token_store: Arc<dyn TokenStore>,
    directory: tempfile::TempDir,
}

async fn stores() -> Stores {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDatabase::new(&directory.path().join("manager.db").to_string_lossy())
            .await
            .unwrap(),
    );
    Stores {
        deployment_store: Arc::new(SqliteDeploymentStore::new(db.clone())),
        release_store: Arc::new(SqliteReleaseStore::new(db.clone())),
        token_store: Arc::new(SqliteTokenStore::new(db)),
        directory,
    }
}

/// A manager over `stores` whose charts install `operator_image`.
async fn manager(stores: &Stores, operator_image: &str) -> AppState {
    let root = stores
        .directory
        .path()
        .join(operator_image.replace(['/', ':'], "_"));
    let kv: Arc<dyn alien_bindings::traits::Kv> =
        Arc::new(LocalKv::new(root.join("kv")).await.unwrap());
    let command_storage: Arc<dyn alien_bindings::traits::Storage> =
        Arc::new(LocalStorage::new(root.join("storage").to_string_lossy().to_string()).unwrap());
    let command_dispatcher: Arc<dyn CommandDispatcher> = Arc::new(NullCommandDispatcher);
    let command_registry: Arc<dyn CommandRegistry> = Arc::new(InMemoryCommandRegistry::default());
    AppState {
        deployment_store: stores.deployment_store.clone(),
        release_store: stores.release_store.clone(),
        project_store: Arc::new(alien_manager::providers::SingleWorkspaceProjects),
        token_store: stores.token_store.clone(),
        auth_validator: Arc::new(Admin),
        authz: Arc::new(OssAuthz),
        telemetry_backend: Arc::new(alien_manager::providers::NullTelemetryBackend),
        credential_resolver: Arc::new(NoCredentials),
        command_server: Arc::new(CommandServer::new(
            kv.clone(),
            command_storage,
            command_dispatcher,
            command_registry,
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
        registry_routing_table: Arc::new(
            RegistryRoutingTable::new(vec![]).expect("empty routing table is unambiguous"),
        ),
        import_registry: Arc::new(alien_infra::ImporterRegistry::built_in()),
        charts: Some(Arc::new(ChartSettings::new(
            Some(operator_image.to_string()),
            false,
        ))),
        release_channels: None,
        bundle_signing_key: None,
        bundle_sources: None,
        log_buffer: Arc::new(alien_manager::LogBuffer::new()),
        values: None,
        value_access: None,
        verify_limiter: Arc::new(alien_manager::values::VerifyLimiter::default()),
        signing: None,
    }
}

async fn create_release(stores: &Stores, stack_id: &str) {
    create_release_of(stores, Stack::new(stack_id.to_string()).build()).await;
}

async fn create_release_of(stores: &Stores, stack: Stack) {
    stores
        .release_store
        .create_release(
            &Subject::system(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(Platform::Kubernetes, stack)]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
                setup_templates: Default::default(),
                signed_stacks: Default::default(),
            },
        )
        .await
        .unwrap();
}

struct Pulled {
    status: StatusCode,
    digest: Option<String>,
    body: Vec<u8>,
}

impl Pulled {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("response is not JSON ({e}): {}", self.text()))
    }
}

async fn pull(state: &AppState, path: &str) -> Pulled {
    let request = Request::builder()
        .uri(format!("/v2/{path}"))
        .header(header::AUTHORIZATION, "Bearer admin")
        .body(Body::empty())
        .unwrap();
    let response = alien_manager::routes::registry_proxy::router()
        .with_state(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let digest = response
        .headers()
        .get("docker-content-digest")
        .map(|value| value.to_str().unwrap().to_string());
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    Pulled {
        status,
        digest,
        body,
    }
}

async fn tags(state: &AppState) -> Vec<String> {
    let pulled = pull(state, &format!("charts/{CHART}/tags/list")).await;
    assert_eq!(pulled.status, StatusCode::OK, "{}", pulled.text());
    let tags = pulled.json();
    assert_eq!(tags["name"], format!("charts/{CHART}"));
    tags["tags"]
        .as_array()
        .expect("tags is a list")
        .iter()
        .map(|tag| tag.as_str().expect("tag is a string").to_string())
        .collect()
}

/// The manifest digest a version resolves to.
async fn manifest_digest(state: &AppState, version: &str) -> String {
    let pulled = pull(state, &format!("charts/{CHART}/manifests/{version}")).await;
    assert_eq!(
        pulled.status,
        StatusCode::OK,
        "{version}: {}",
        pulled.text()
    );
    pulled
        .digest
        .expect("manifest responses carry their digest")
}

/// `Chart.yaml` and `values.yaml` of the chart archive a version resolves to.
async fn chart_files(state: &AppState, version: &str) -> (String, String) {
    let files = chart_archive(state, version).await;
    let file = |name: &str| {
        files
            .get(&format!("{CHART}/{name}"))
            .unwrap_or_else(|| panic!("the chart has no {name}"))
            .clone()
    };
    (file("Chart.yaml"), file("values.yaml"))
}

/// Every file of the chart archive a version resolves to, by path.
async fn chart_archive(state: &AppState, version: &str) -> HashMap<String, String> {
    let manifest = pull(state, &format!("charts/{CHART}/manifests/{version}")).await;
    assert_eq!(manifest.status, StatusCode::OK, "{}", manifest.text());
    let layer = manifest.json()["layers"][0]["digest"]
        .as_str()
        .expect("the manifest has a chart layer")
        .to_string();
    let archive = pull(state, &format!("charts/{CHART}/blobs/{layer}")).await;
    assert_eq!(archive.status, StatusCode::OK, "{}", archive.text());

    let mut files = HashMap::new();
    let mut tar = tar::Archive::new(GzDecoder::new(archive.body.as_slice()));
    for entry in tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().into_owned();
        let mut contents = String::new();
        entry.read_to_string(&mut contents).unwrap();
        files.insert(path, contents);
    }
    files
}

fn semver(version: &str) -> Version {
    Version::parse(version).unwrap_or_else(|e| panic!("chart version '{version}': {e}"))
}

#[tokio::test]
async fn a_new_operator_image_publishes_new_higher_chart_versions() {
    let stores = stores().await;
    create_release(&stores, CHART).await;
    // A release of another stack still counts in the release number.
    create_release(&stores, "another-stack").await;
    create_release(&stores, CHART).await;

    let before = manager(&stores, OLD_OPERATOR).await;
    let after = manager(&stores, NEW_OPERATOR).await;
    let before_tags = tags(&before).await;
    let after_tags = tags(&after).await;

    // Both managers list one version per release, oldest release first.
    assert_eq!(before_tags.len(), 2, "{before_tags:?}");
    assert_eq!(after_tags.len(), 2, "{after_tags:?}");
    for tag in &after_tags {
        assert!(
            !before_tags.contains(tag),
            "a new Operator image must publish new chart versions, but both managers list {tag} \
             (before: {before_tags:?}, after: {after_tags:?})"
        );
    }
    assert_eq!(before_tags, ["1.1.3003057", "1.3.3003057"]);
    assert_eq!(after_tags, ["1.1.3003059", "1.3.3003059"]);

    // Ordering, as Helm, Flux and Renovate compare chart versions.
    let [old_first, old_second] = [semver(&before_tags[0]), semver(&before_tags[1])];
    let [new_first, new_second] = [semver(&after_tags[0]), semver(&after_tags[1])];
    assert!(
        new_first > old_first,
        "{new_first} must sort above {old_first}"
    );
    assert!(
        new_second > old_second,
        "{new_second} must sort above {old_second}"
    );
    // A newer release sorts above an older one under any Operator image.
    assert!(
        old_second > new_first,
        "{old_second} must sort above {new_first}"
    );
    // Every version sorts above the `1.0.<n>` versions earlier managers served.
    let highest_earlier = Version::new(1, 0, u64::MAX);
    for version in [&old_first, &old_second, &new_first, &new_second] {
        assert!(
            *version > highest_earlier,
            "{version} must sort above {highest_earlier}"
        );
    }

    // A version always resolves to the same digest, also after a manager
    // upgrade: the manager stores a chart the first time it serves it, and a
    // manager with another Operator image serves the stored bytes.
    for (version, operator_tag) in [(&before_tags[0], "v3.3.57"), (&before_tags[1], "v3.3.57")] {
        let digest = manifest_digest(&before, version).await;
        assert_eq!(manifest_digest(&before, version).await, digest);
        assert_eq!(
            manifest_digest(&after, version).await,
            digest,
            "{version} must keep its digest after the manager upgrade"
        );
        let (_, upgraded_values) = chart_files(&after, version).await;
        assert!(
            upgraded_values.contains(&format!("    tag: \"{operator_tag}\"\n")),
            "{version} must keep installing Operator {operator_tag}:\n{upgraded_values}"
        );

        // The version installs the Operator image it names.
        let (chart_yaml, values_yaml) = chart_files(&before, version).await;
        assert!(
            chart_yaml.contains(&format!("version: {version}\n")),
            "{chart_yaml}"
        );
        assert!(
            values_yaml.contains(&format!("    tag: \"{operator_tag}\"\n")),
            "{version} must install Operator {operator_tag}:\n{values_yaml}"
        );
    }
    for version in &after_tags {
        let (_, values_yaml) = chart_files(&after, version).await;
        assert!(
            values_yaml.contains("    tag: \"v3.3.59\"\n"),
            "{version} must install Operator v3.3.59:\n{values_yaml}"
        );
    }

    // The `1.0.<n>` versions earlier managers served aren't served any more:
    // their content can't be reproduced, so they fail instead of resolving
    // to a chart with another Operator image.
    for manager in [&before, &after] {
        let earlier = pull(manager, &format!("charts/{CHART}/manifests/1.0.1")).await;
        assert_eq!(earlier.status, StatusCode::NOT_FOUND, "{}", earlier.text());
        assert!(
            earlier.text().contains("MANIFEST_UNKNOWN"),
            "{}",
            earlier.text()
        );
    }
}

#[tokio::test]
async fn the_first_stored_copy_of_a_version_wins() {
    use alien_manager::traits::StoredChart;

    let stores = stores().await;
    create_release(&stores, CHART).await;
    let release_id = stores
        .release_store
        .list_releases(&Subject::system())
        .await
        .unwrap()[0]
        .id
        .clone();
    let chart = |byte: u8| StoredChart {
        release_id: release_id.clone(),
        version: "1.1.3003057".to_string(),
        manifest: vec![byte],
        config: vec![byte],
        archive: vec![byte],
    };

    let first = stores
        .release_store
        .store_chart_if_absent(&Subject::system(), chart(1))
        .await
        .unwrap();
    // A second replica that rendered at the same time keeps the first copy.
    let second = stores
        .release_store
        .store_chart_if_absent(&Subject::system(), chart(2))
        .await
        .unwrap();
    assert_eq!(first, chart(1));
    assert_eq!(second, chart(1));
    let kept = stores
        .release_store
        .stored_charts(&Subject::system(), std::slice::from_ref(&release_id))
        .await
        .unwrap();
    assert_eq!(kept, [chart(1)]);
}

/// The chart carries the service account of the egress gateway Alien adds for an internet
/// sandbox: the Operator cannot create it, so a chart generated before that addition leaves the
/// gateway pods unable to start.
#[tokio::test]
async fn the_chart_of_an_internet_sandbox_carries_the_gateway_service_account() {
    use alien_core::{
        ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    };
    let stack = Stack::new(CHART.to_string())
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "example.invalid/agent:1".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: Some(300),
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let stores = stores().await;
    create_release_of(&stores, stack).await;
    let state = manager(&stores, OLD_OPERATOR).await;
    let version = tags(&state).await.remove(0);
    let files = chart_archive(&state, &version).await;
    let gateway = alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID;
    let values: serde_yaml::Value =
        serde_yaml::from_str(&files[&format!("{CHART}/values.yaml")]).expect("values.yaml is YAML");
    assert!(
        values["serviceAccounts"].get(gateway).is_some(),
        "the chart's serviceAccounts lack the gateway '{gateway}': {:?}",
        values["serviceAccounts"]
    );
}

/// A stack the preparation refuses is the caller's input, not a server fault: the route answers
/// with a client error that carries the preparation's own code and message.
#[tokio::test]
async fn a_stack_the_preparation_refuses_is_a_client_error_with_its_message() {
    use alien_core::{Container, ContainerCode, ContainerEgress, ResourceLifecycle, ResourceSpec};
    let api = Container::new("api".to_string())
        .code(ContainerCode::Image {
            image: "example.invalid/api:1".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .port(8080)
        .permissions("execution".to_string())
        .egress(ContainerEgress::Internet)
        .build();
    let stack = Stack::new(CHART.to_string())
        .add(api, ResourceLifecycle::Live)
        .build();
    let stores = stores().await;
    create_release_of(&stores, stack).await;
    let state = manager(&stores, OLD_OPERATOR).await;
    let version = tags(&state).await.remove(0);
    let pulled = pull(&state, &format!("charts/{CHART}/manifests/{version}")).await;
    assert_eq!(
        pulled.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        String::from_utf8_lossy(&pulled.body)
    );
    let body: Value = serde_json::from_slice(&pulled.body).unwrap();
    let error = &body["errors"][0];
    assert_eq!(error["code"], "VALIDATION_FAILED");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.to_lowercase().contains("egress"),
        "the preparation's reason must reach the caller: {message}"
    );
}
