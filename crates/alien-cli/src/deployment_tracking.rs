//! Deployment tracking and storage functionality for the Alien CLI
//!
//! This module handles securely storing deployment information (name, ID, API key)
//! and managing deployment registration with the platform.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::SdkResultExt;
use alien_platform_api::{
    types::{Subject, SubjectScope},
    Client as SdkClient,
};
#[cfg(debug_assertions)]
use dirs::config_dir;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::warn;

#[cfg(debug_assertions)]
use debug_keyring::{Entry, KeyringError};
#[cfg(not(debug_assertions))]
use keyring::{Entry, Error as KeyringError};

const SERVICE: &str = "alien-cli";
const DEPLOYMENTS_KEY: &str = "tracked_deployments";

/// Information about a tracked deployment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedDeployment {
    /// User-provided name for the deployment
    pub name: String,
    /// Deployment ID from the platform
    pub deployment_id: String,
    /// Deployment API key for authentication
    pub api_key: String,
    /// Workspace ID the deployment belongs to
    pub workspace_id: String,
    /// Project ID the deployment belongs to
    pub project_id: String,
}

/// All tracked deployments, keyed by their user-provided name
type TrackedDeployments = HashMap<String, TrackedDeployment>;

/// Persistence backend for the tracked-deployment registry.
trait TrackedDeploymentStore: Send + Sync {
    fn load(&self) -> Result<TrackedDeployments>;
    fn save(&self, deployments: &TrackedDeployments) -> Result<()>;
}

/// Deployment tracking manager
pub struct DeploymentTracker {
    /// All tracked deployments by name
    deployments: TrackedDeployments,
    store: Box<dyn TrackedDeploymentStore>,
}

impl DeploymentTracker {
    /// Create a new deployment tracker and load existing deployments
    pub fn new() -> Result<Self> {
        Self::with_store(Box::new(KeyringStore))
    }

    fn with_store(store: Box<dyn TrackedDeploymentStore>) -> Result<Self> {
        let deployments = store.load()?;
        Ok(Self { deployments, store })
    }

    /// Add a new deployment after validating it with the platform
    pub async fn add_deployment(
        &mut self,
        name: String,
        api_key: String,
        base_url: &str,
    ) -> Result<TrackedDeployment> {
        let deployment_info = validate_deployment_api_key(&api_key, base_url).await?;
        self.track(name, api_key, deployment_info)
    }

    /// Store an already-validated deployment key under `name`, replacing any entry there.
    pub fn track(
        &mut self,
        name: String,
        api_key: String,
        info: ValidatedDeploymentInfo,
    ) -> Result<TrackedDeployment> {
        let tracked = TrackedDeployment {
            name: name.clone(),
            deployment_id: info.deployment_id,
            api_key,
            workspace_id: info.workspace_id,
            project_id: info.project_id,
        };

        self.deployments.insert(name, tracked.clone());
        self.store.save(&self.deployments)?;

        Ok(tracked)
    }

    /// Get a tracked deployment by name
    pub fn get_deployment(&self, name: &str) -> Option<&TrackedDeployment> {
        self.deployments.get(name)
    }

    /// Return the tracked deployment only while the platform still accepts its stored key.
    ///
    /// An entry outlives platform-side deletion, so a dead one is dropped here and
    /// the caller registers a new deployment instead.
    pub async fn resolve_live_deployment(
        &mut self,
        name: &str,
        base_url: &str,
    ) -> Result<Option<TrackedDeployment>> {
        let Some(tracked) = self.deployments.get(name).cloned() else {
            return Ok(None);
        };

        if stored_key_is_live(&tracked, base_url).await? {
            return Ok(Some(tracked));
        }

        warn!(
            "Tracked deployment '{}' ({}) is no longer reachable with its stored key; dropping the local entry",
            name, tracked.deployment_id
        );
        self.remove_deployment(name)?;
        Ok(None)
    }

    /// List all tracked deployments
    pub fn list_deployments(&self) -> Vec<&TrackedDeployment> {
        self.deployments.values().collect()
    }

    /// Remove a tracked deployment
    pub fn remove_deployment(&mut self, name: &str) -> Result<Option<TrackedDeployment>> {
        let removed = self.deployments.remove(name);
        if removed.is_some() {
            self.store.save(&self.deployments)?;
        }
        Ok(removed)
    }
}

/// Whether the platform still authenticates the stored key as this exact deployment.
///
/// A deployment's API key is deleted with it, so rejection is the deletion signal.
/// Any other failure may be transient, and dropping an entry on one would create a
/// duplicate deployment.
async fn stored_key_is_live(tracked: &TrackedDeployment, base_url: &str) -> Result<bool> {
    let client = authenticated_client(&tracked.api_key, base_url)?;

    match client.whoami().send().await.into_sdk_error() {
        Ok(response) => Ok(matches!(
            response.into_inner(),
            Subject::ServiceAccountSubject(sa)
                if matches!(
                    &sa.scope,
                    SubjectScope::Deployment { deployment_id, .. }
                        if *deployment_id == tracked.deployment_id
                )
        )),
        Err(err) if err.http_status_code == Some(401) => Ok(false),
        Err(err) => Err(err).context(ErrorData::ApiRequestFailed {
            message: format!(
                "Could not confirm whether deployment '{}' still exists",
                tracked.name
            ),
            url: None,
        }),
    }
}

/// Information about a deployment token (deployment or deployment-group scoped)
#[derive(Debug, Clone)]
pub enum DeploymentToken {
    /// Deployment-scoped token (for existing deployments)
    Deployment {
        deployment_id: String,
        project_id: String,
        workspace_id: String,
    },
    /// Deployment-group-scoped token (for creating new deployments)
    DeploymentGroup {
        deployment_group_id: String,
        deployment_group_name: String,
        project_id: String,
        workspace_name: String,
        max_deployments: u32,
    },
}

/// Information returned from deployment validation
#[derive(Debug)]
pub struct ValidatedDeploymentInfo {
    pub deployment_id: String,
    pub workspace_id: String,
    pub project_id: String,
}

/// Build a platform SDK client that authenticates with `api_key`.
fn authenticated_client(api_key: &str, base_url: &str) -> Result<SdkClient> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let reqwest_client = Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(SdkClient::new_with_client(base_url, reqwest_client))
}

/// Validate a deployment token (agent or deployment-group scoped)
pub async fn validate_token(api_key: &str, base_url: &str) -> Result<DeploymentToken> {
    let sdk_client = authenticated_client(api_key, base_url)?;

    // Call whoami endpoint
    let response =
        sdk_client
            .whoami()
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Failed to validate token with platform".to_string(),
            })?;

    let subject = response.into_inner();

    // Extract token type and information based on the subject scope
    match subject {
        Subject::ServiceAccountSubject(sa) => {
            match sa.scope {
                // Deployment-scoped token: for existing deployments
                SubjectScope::Deployment {
                    deployment_id,
                    project_id,
                } => Ok(DeploymentToken::Deployment {
                    deployment_id,
                    project_id,
                    workspace_id: sa.workspace_id,
                }),
                // Deployment-group-scoped token: for creating new deployments
                SubjectScope::DeploymentGroup {
                    deployment_group_id,
                    project_id: _,
                } => {
                    // The group lookup keys on workspace name, not id (whoami provides the name).
                    let workspace_name = sa.workspace_name.clone().ok_or_else(|| {
                        AlienError::new(ErrorData::ValidationError {
                            field: "workspace".to_string(),
                            message: "token response is missing the workspace name".to_string(),
                        })
                    })?;
                    let deployment_group = fetch_deployment_group(
                        &deployment_group_id,
                        &workspace_name,
                        api_key,
                        base_url,
                    )
                    .await?;

                    Ok(DeploymentToken::DeploymentGroup {
                        deployment_group_id: deployment_group.id.to_string(),
                        deployment_group_name: deployment_group.name.to_string(),
                        project_id: deployment_group.project_id.to_string(),
                        workspace_name,
                        max_deployments: deployment_group.max_deployments.get() as u32,
                    })
                }
                _ => Err(AlienError::new(ErrorData::ValidationError {
                    field: "token".to_string(),
                    message: "Token must be deployment-scoped or deployment-group-scoped"
                        .to_string(),
                })),
            }
        }
        Subject::UserSubject(_) => Err(AlienError::new(ErrorData::ValidationError {
            field: "token".to_string(),
            message: "API key must be for a service account, not a user".to_string(),
        })),
    }
}

/// Fetch deployment group details from the API
async fn fetch_deployment_group(
    deployment_group_id: &str,
    workspace_name: &str,
    api_key: &str,
    base_url: &str,
) -> Result<alien_platform_api::types::GetDeploymentGroupResponse> {
    let sdk_client = authenticated_client(api_key, base_url)?;

    // Fetch deployment group
    let response = sdk_client
        .get_deployment_group()
        .id(deployment_group_id)
        .workspace(workspace_name)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_group".to_string(),
            message: format!("Failed to fetch deployment group: {}", deployment_group_id),
        })?;

    Ok(response.into_inner())
}

/// Validate a deployment API key by calling the whoami endpoint
async fn validate_deployment_api_key(
    api_key: &str,
    base_url: &str,
) -> Result<ValidatedDeploymentInfo> {
    let token = validate_token(api_key, base_url).await?;

    match token {
        DeploymentToken::Deployment {
            deployment_id,
            workspace_id,
            project_id,
        } => Ok(ValidatedDeploymentInfo {
            deployment_id,
            workspace_id,
            project_id,
        }),
        DeploymentToken::DeploymentGroup { .. } => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "Expected deployment token, got deployment-group token".to_string(),
            }))
        }
    }
}

/// The real registry: one keyring entry holding every tracked deployment.
struct KeyringStore;

impl KeyringStore {
    fn entry() -> Result<Entry> {
        Entry::new(SERVICE, DEPLOYMENTS_KEY)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to create keyring entry for deployments".to_string(),
            })
    }
}

impl TrackedDeploymentStore for KeyringStore {
    fn load(&self) -> Result<TrackedDeployments> {
        match Self::entry()?.get_password() {
            Ok(data) => {
                serde_json::from_str(&data)
                    .into_alien_error()
                    .context(ErrorData::JsonError {
                        operation: "deserialize".to_string(),
                        reason: "Failed to parse tracked deployments data".to_string(),
                    })
            }
            Err(KeyringError::NoEntry) => Ok(TrackedDeployments::new()),
            // Reading an empty registry from a store that is merely unreadable would
            // make the next save wipe every deployment key it holds.
            Err(err) => Err(err)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to read tracked deployments from the keyring".to_string(),
                }),
        }
    }

    fn save(&self, deployments: &TrackedDeployments) -> Result<()> {
        let data = serde_json::to_string(deployments)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "serialize".to_string(),
                reason: "Failed to serialize tracked deployments data".to_string(),
            })?;

        Self::entry()?
            .set_password(&data)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to store tracked deployments in keyring".to_string(),
            })?;

        Ok(())
    }
}

/// Simple file-based keyring for debug builds to avoid macOS keychain prompts
#[cfg(debug_assertions)]
mod debug_keyring {
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    #[derive(Debug)]
    pub enum KeyringError {
        /// Nothing has been stored under this service and user yet.
        NoEntry,
        /// The backing file could not be read, parsed, or written.
        Unusable(String),
    }

    impl std::fmt::Display for KeyringError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::NoEntry => write!(f, "No entry found"),
                Self::Unusable(message) => write!(f, "{}", message),
            }
        }
    }

    impl std::error::Error for KeyringError {}

    pub struct Entry {
        service: String,
        user: String,
        path: PathBuf,
    }

    impl Entry {
        pub fn new(service: &str, user: &str) -> std::result::Result<Self, KeyringError> {
            Ok(Self::at(service, user, default_keyring_path()))
        }

        fn at(service: &str, user: &str, path: PathBuf) -> Self {
            Self {
                service: service.to_string(),
                user: user.to_string(),
                path,
            }
        }

        pub fn set_password(&self, password: &str) -> std::result::Result<(), KeyringError> {
            let mut store = self.load_store()?;
            store.insert(self.key(), password.to_string());
            self.save_store(&store)
        }

        pub fn get_password(&self) -> std::result::Result<String, KeyringError> {
            self.load_store()?
                .get(&self.key())
                .cloned()
                .ok_or(KeyringError::NoEntry)
        }

        fn key(&self) -> String {
            format!("{}:{}", self.service, self.user)
        }

        fn load_store(&self) -> std::result::Result<HashMap<String, String>, KeyringError> {
            if !self.path.exists() {
                return Ok(HashMap::new());
            }
            let content = fs::read_to_string(&self.path).map_err(|e| {
                KeyringError::Unusable(format!(
                    "Failed to read keyring file {}: {}",
                    self.path.display(),
                    e
                ))
            })?;
            // Treating an unparseable file as empty would let the next write
            // replace every credential it holds.
            serde_json::from_str(&content).map_err(|e| {
                KeyringError::Unusable(format!(
                    "Failed to parse keyring file {}: {}",
                    self.path.display(),
                    e
                ))
            })
        }

        fn save_store(
            &self,
            store: &HashMap<String, String>,
        ) -> std::result::Result<(), KeyringError> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir).map_err(|e| {
                    KeyringError::Unusable(format!("Failed to create config dir: {}", e))
                })?;
            }
            let content = serde_json::to_string_pretty(store).map_err(|e| {
                KeyringError::Unusable(format!("Failed to serialize keyring: {}", e))
            })?;
            alien_core::file_utils::write_secret_file(&self.path, content.as_bytes()).map_err(
                |e| {
                    KeyringError::Unusable(format!(
                        "Failed to write keyring file {}: {}",
                        self.path.display(),
                        e
                    ))
                },
            )?;
            Ok(())
        }
    }

    fn default_keyring_path() -> PathBuf {
        super::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("alien")
            .join("cli-keyring.json")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn unparseable_file_is_an_error_rather_than_an_empty_store() {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = dir.path().join("cli-keyring.json");
            let entry = Entry::at("alien-cli", "tracked_deployments", path.clone());

            entry.set_password("stored").expect("write entry");
            assert_eq!(
                entry.get_password().expect("read entry"),
                "stored",
                "round trip should return what was stored"
            );

            fs::write(&path, "{ not json").expect("corrupt the file");
            let error = entry
                .get_password()
                .expect_err("a corrupt store must not read as empty");
            assert!(
                matches!(error, KeyringError::Unusable(_)),
                "expected an unusable store, got {error:?}"
            );

            entry
                .set_password("replacement")
                .expect_err("a corrupt store must not be overwritten");
            assert_eq!(
                fs::read_to_string(&path).expect("file still readable"),
                "{ not json",
                "the corrupt file must be left untouched"
            );
        }

        #[test]
        fn missing_entry_is_distinct_from_a_broken_store() {
            let dir = tempfile::tempdir().expect("temp dir");
            let entry = Entry::at(
                "alien-cli",
                "tracked_deployments",
                dir.path().join("cli-keyring.json"),
            );

            let error = entry.get_password().expect_err("nothing stored yet");
            assert!(
                matches!(error, KeyringError::NoEntry),
                "expected NoEntry, got {error:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router,
    };
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    const NAME: &str = "sandbox";
    const DEPLOYMENT_ID: &str = "dep_00000000000000000000000001";
    const REPLACEMENT_DEPLOYMENT_ID: &str = "dep_00000000000000000000000002";
    const PROJECT_ID: &str = "prj_00000000000000000000000001";
    const WORKSPACE_ID: &str = "ws_000000000000000000000001";
    const API_KEY: &str = "ax_dep_stored";

    /// Registry backed by a temp file, so no test touches the developer's real keyring.
    struct FileStore {
        path: PathBuf,
    }

    impl TrackedDeploymentStore for FileStore {
        fn load(&self) -> Result<TrackedDeployments> {
            if !self.path.exists() {
                return Ok(TrackedDeployments::new());
            }
            let data = std::fs::read_to_string(&self.path)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to read the test registry".to_string(),
                })?;
            serde_json::from_str(&data)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to parse the test registry".to_string(),
                })
        }

        fn save(&self, deployments: &TrackedDeployments) -> Result<()> {
            let data = serde_json::to_string(deployments)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to serialize the test registry".to_string(),
                })?;
            std::fs::write(&self.path, data).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Failed to write the test registry".to_string(),
                },
            )
        }
    }

    fn tracker_at(path: &Path) -> DeploymentTracker {
        DeploymentTracker::with_store(Box::new(FileStore {
            path: path.to_path_buf(),
        }))
        .expect("registry should load")
    }

    fn registry_with_entry() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("registry.json");

        let tracked = tracker_at(&path)
            .track(
                NAME.to_string(),
                API_KEY.to_string(),
                ValidatedDeploymentInfo {
                    deployment_id: DEPLOYMENT_ID.to_string(),
                    workspace_id: WORKSPACE_ID.to_string(),
                    project_id: PROJECT_ID.to_string(),
                },
            )
            .expect("entry should persist");
        assert_eq!(tracked.deployment_id, DEPLOYMENT_ID);

        (dir, path)
    }

    #[derive(Clone, Copy)]
    enum Whoami {
        /// The key still authenticates as this deployment.
        Deployment(&'static str),
        /// The key authenticates, but not as a deployment.
        DeploymentGroup,
        /// The key is rejected — what the platform does once the deployment is deleted.
        Rejected,
        /// The platform is broken or unreachable.
        ServerError,
    }

    async fn whoami_handler(
        State((reply, calls)): State<(Whoami, Arc<AtomicUsize>)>,
    ) -> axum::response::Response {
        calls.fetch_add(1, Ordering::SeqCst);
        match reply {
            Whoami::Deployment(deployment_id) => Json(serde_json::json!({
                "kind": "serviceAccount",
                "id": "sa_test",
                "workspaceId": WORKSPACE_ID,
                "role": "deployment.manager",
                "scope": {
                    "type": "deployment",
                    "deploymentId": deployment_id,
                    "projectId": PROJECT_ID,
                },
            }))
            .into_response(),
            Whoami::DeploymentGroup => Json(serde_json::json!({
                "kind": "serviceAccount",
                "id": "sa_test",
                "workspaceId": WORKSPACE_ID,
                "role": "deployment-group.deployer",
                "scope": {
                    "type": "deployment-group",
                    "deploymentGroupId": "dg_00000000000000000000000001",
                    "projectId": PROJECT_ID,
                },
            }))
            .into_response(),
            Whoami::Rejected => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "code": "UNAUTHORIZED",
                    "message": "Invalid API key",
                    "internal": false,
                })),
            )
                .into_response(),
            Whoami::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "code": "INTERNAL_ERROR",
                    "message": "Database unavailable",
                    "internal": true,
                })),
            )
                .into_response(),
        }
    }

    /// Serve `/v1/whoami` on loopback, returning the base URL and a probe counter.
    async fn fake_platform(reply: Whoami) -> (String, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/v1/whoami", get(whoami_handler))
            .with_state((reply, calls.clone()));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        (format!("http://{addr}"), calls)
    }

    #[tokio::test]
    async fn live_entry_is_resolved_and_kept() {
        let (_dir, path) = registry_with_entry();
        let (base_url, calls) = fake_platform(Whoami::Deployment(DEPLOYMENT_ID)).await;

        let resolved = tracker_at(&path)
            .resolve_live_deployment(NAME, &base_url)
            .await
            .expect("resolution should succeed")
            .expect("a live entry should resolve");

        assert_eq!(resolved.deployment_id, DEPLOYMENT_ID);
        assert_eq!(resolved.api_key, API_KEY);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "exactly one probe call");
        assert!(
            tracker_at(&path).get_deployment(NAME).is_some(),
            "a live entry must stay in the registry"
        );
    }

    #[tokio::test]
    async fn deleted_deployment_drops_the_entry_and_falls_through_to_registration() {
        let (_dir, path) = registry_with_entry();
        let (base_url, calls) = fake_platform(Whoami::Rejected).await;

        let resolved = tracker_at(&path)
            .resolve_live_deployment(NAME, &base_url)
            .await
            .expect("resolution should succeed");

        assert!(
            resolved.is_none(),
            "a rejected key must resolve to nothing so the caller registers a new deployment"
        );
        assert!(
            tracker_at(&path).get_deployment(NAME).is_none(),
            "the entry must be gone from the registry, not merely bypassed"
        );

        let second_run = tracker_at(&path)
            .resolve_live_deployment(NAME, &base_url)
            .await
            .expect("resolution should succeed");
        assert!(
            second_run.is_none(),
            "the second run must reach the same register-new path as the first"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the dropped entry must not be probed again"
        );
    }

    #[tokio::test]
    async fn key_that_no_longer_names_this_deployment_drops_the_entry() {
        for reply in [
            Whoami::Deployment(REPLACEMENT_DEPLOYMENT_ID),
            Whoami::DeploymentGroup,
        ] {
            let (_dir, path) = registry_with_entry();
            let (base_url, _calls) = fake_platform(reply).await;

            let resolved = tracker_at(&path)
                .resolve_live_deployment(NAME, &base_url)
                .await
                .expect("resolution should succeed");

            assert!(
                resolved.is_none(),
                "a key that authenticates as something else must not resolve this entry"
            );
            assert!(tracker_at(&path).get_deployment(NAME).is_none());
        }
    }

    #[tokio::test]
    async fn unreachable_platform_keeps_the_entry_and_fails() {
        let (_dir, path) = registry_with_entry();
        let (base_url, _calls) = fake_platform(Whoami::ServerError).await;

        let error = tracker_at(&path)
            .resolve_live_deployment(NAME, &base_url)
            .await
            .expect_err("a platform failure must not be mistaken for a deleted deployment");

        assert_eq!(error.code, "API_REQUEST_FAILED");
        assert!(
            tracker_at(&path).get_deployment(NAME).is_some(),
            "dropping the entry here would create a duplicate deployment on the next run"
        );
    }

    #[tokio::test]
    async fn untracked_name_resolves_without_calling_the_platform() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("registry.json");
        let (base_url, calls) = fake_platform(Whoami::Rejected).await;

        let resolved = tracker_at(&path)
            .resolve_live_deployment(NAME, &base_url)
            .await
            .expect("resolution should succeed");

        assert!(resolved.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn tracking_the_same_name_twice_replaces_the_stored_key() {
        let (_dir, path) = registry_with_entry();

        tracker_at(&path)
            .track(
                NAME.to_string(),
                "ax_dep_rotated".to_string(),
                ValidatedDeploymentInfo {
                    deployment_id: REPLACEMENT_DEPLOYMENT_ID.to_string(),
                    workspace_id: WORKSPACE_ID.to_string(),
                    project_id: PROJECT_ID.to_string(),
                },
            )
            .expect("re-tracking should persist");

        let reloaded = tracker_at(&path);
        let entry = reloaded
            .get_deployment(NAME)
            .expect("the name should still be tracked");
        assert_eq!(entry.api_key, "ax_dep_rotated");
        assert_eq!(entry.deployment_id, REPLACEMENT_DEPLOYMENT_ID);
        assert_eq!(reloaded.list_deployments().len(), 1);
    }

    #[test]
    fn removing_an_entry_persists() {
        let (_dir, path) = registry_with_entry();

        let removed = tracker_at(&path)
            .remove_deployment(NAME)
            .expect("removal should persist")
            .expect("the entry should have been there");
        assert_eq!(removed.deployment_id, DEPLOYMENT_ID);

        assert!(tracker_at(&path).get_deployment(NAME).is_none());
    }
}
