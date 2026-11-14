//! Which release a deployment is sent to when it reports Running, over HTTP
//! against a standalone manager with the SQLite stores.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use alien_core::{DeploymentState, DeploymentStatus, Platform, ReleaseInfo};
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseChannelStore, SqliteReleaseStore,
    SqliteTokenStore,
};
use alien_manager::traits::release_store::*;
use alien_manager::traits::{
    CreateDeploymentGroupParams, CreateTokenParams, DeploymentStore, ReleaseChannelStore,
    TokenStore, TokenType, DEFAULT_CHANNEL,
};
use alien_manager::AlienManagerBuilder;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn admin() -> Subject {
    Subject {
        kind: SubjectKind::ServiceAccount {
            id: "test".to_string(),
        },
        workspace_id: "default".to_string(),
        scope: Scope::Workspace,
        role: Role::WorkspaceAdmin,
        bearer_token: String::new(),
    }
}

struct Server {
    url: String,
    token: String,
    group_id: String,
    /// The older release.
    r1: String,
    /// The newer release.
    r2: String,
    _dir: tempfile::TempDir,
}

/// A manager whose production channel points at the newer of two releases.
async fn start() -> Server {
    start_with_channel_on(1).await
}

/// A manager with two releases whose production channel points at
/// `ids[channel_release]`.
async fn start_with_channel_on(channel_release: usize) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}");

    let db_path = dir.path().join("test.db");
    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let token_store: Arc<dyn TokenStore> = Arc::new(SqliteTokenStore::new(db.clone()));
    let token = format!("ax_admin_{}", uuid::Uuid::new_v4().simple());
    token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: token[..12].to_string(),
            key_hash: format!("{:x}", Sha256::digest(token.as_bytes())),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    let group_id = SqliteDeploymentStore::new(db.clone())
        .create_deployment_group(
            &admin(),
            CreateDeploymentGroupParams {
                name: "group".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap()
        .id;
    let releases = SqliteReleaseStore::new(db.clone());
    let mut ids = Vec::new();
    for _ in 0..2 {
        ids.push(
            releases
                .create_release(
                    &admin(),
                    CreateReleaseParams {
                        project_id: "default".to_string(),
                        stacks: HashMap::from([(
                            Platform::Local,
                            alien_core::Stack::new("app".to_string()).build(),
                        )]),
                        git_commit_sha: None,
                        git_commit_ref: None,
                        git_commit_message: None,
                        setup_templates: Default::default(),
                        signed_stacks: Default::default(),
                    },
                )
                .await
                .unwrap()
                .id,
        );
    }
    SqliteReleaseChannelStore::new(db.clone())
        .set_channel_release(DEFAULT_CHANNEL, &ids[channel_release])
        .await
        .unwrap();

    let config = ManagerConfig {
        port,
        host: "127.0.0.1".to_string(),
        db_path: Some(db_path),
        state_dir: Some(dir.path().to_path_buf()),
        deployment_interval_secs: 999,
        heartbeat_interval_secs: 999,
        self_heartbeat_interval_secs: 999,
        otlp_endpoint: None,
        base_url: Some(url.clone()),
        releases_url: None,
        targets: vec![],
        supported_aws_regions: Vec::new(),
        disable_deployment_loop: true,
        disable_heartbeat_loop: true,
        enable_local_log_ingest: false,
        stop_local_containers_on_shutdown: false,
        allowed_origins: None,
        response_signing_key: b"test-signing-key".to_vec(),
    };
    let manager = AlienManagerBuilder::new(config)
        .token_store(token_store)
        .ephemeral_state_dir(false)
        .with_standalone_defaults(&Default::default())
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
    tokio::spawn(async move { manager.start_with_listener(listener).await.unwrap() });

    let client = reqwest::Client::new();
    for _ in 0..50 {
        if client
            .get(format!("{url}/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Server {
                url,
                token,
                group_id,
                r1: ids[0].clone(),
                r2: ids[1].clone(),
                _dir: dir,
            };
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("manager did not become healthy");
}

impl Server {
    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert!(status.is_success(), "{path} returned {status}: {text}");
        serde_json::from_str(&text).unwrap()
    }

    /// Create a deployment, as `POST /v1/deployments` does.
    async fn create_deployment(&self, pinned_release_id: Option<&str>) -> String {
        let created = self
            .call(
                reqwest::Method::POST,
                "/v1/deployments",
                Some(json!({
                    "name": "customer",
                    "platform": "local",
                    "deploymentGroupId": self.group_id,
                    "pinnedReleaseId": pinned_release_id,
                })),
            )
            .await;
        created["deployment"]["id"].as_str().unwrap().to_string()
    }

    /// Report the deployment Running on `release`, as its runtime does.
    async fn report_running(&self, deployment_id: &str, release: &str) {
        let state = DeploymentState {
            status: DeploymentStatus::Running,
            platform: Platform::Local,
            current_release: Some(ReleaseInfo {
                release_id: Some(release.to_string()),
                version: None,
                description: None,
                stack: alien_core::Stack::new("app".to_string()).build(),
                signed: None,
                signer_lists: Vec::new(),
            }),
            target_release: None,
            stack_state: None,
            error: None,
            environment_info: None,
            runtime_metadata: None,
            retry_requested: false,
            protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        };
        self.call(
            reqwest::Method::POST,
            "/v1/sync/reconcile",
            Some(json!({
                "deploymentId": deployment_id,
                "session": "runtime",
                "state": state,
            })),
        )
        .await;
    }

    /// The release the deployment is asked to run next, and its status.
    async fn desired(&self, deployment_id: &str) -> (Option<String>, String) {
        let deployment = self
            .call(
                reqwest::Method::GET,
                &format!("/v1/deployments/{deployment_id}"),
                None,
            )
            .await;
        (
            deployment["desiredReleaseId"].as_str().map(str::to_string),
            deployment["status"].as_str().unwrap().to_string(),
        )
    }
}

#[tokio::test]
async fn a_deployment_that_finishes_installing_is_sent_the_release_promoted_meanwhile() {
    let s = start_with_channel_on(0).await;
    let id = s.create_deployment(None).await;
    assert_eq!(s.desired(&id).await.0.as_deref(), Some(s.r1.as_str()));

    // The release published while it installs skips it.
    s.call(
        reqwest::Method::POST,
        &format!("/v1/releases/{}/promote", s.r2),
        Some(json!({ "channel": DEFAULT_CHANNEL })),
    )
    .await;
    assert_eq!(s.desired(&id).await.0.as_deref(), Some(s.r1.as_str()));

    s.report_running(&id, &s.r1).await;
    assert_eq!(
        s.desired(&id).await,
        (Some(s.r2.clone()), "update-pending".to_string())
    );
}

#[tokio::test]
async fn a_deployment_created_with_a_pinned_release_stays_on_it() {
    let s = start().await;
    let id = s.create_deployment(Some(&s.r1)).await;
    assert_eq!(s.desired(&id).await.0.as_deref(), Some(s.r1.as_str()));

    // The production channel points at r2, but the caller asked for r1.
    s.report_running(&id, &s.r1).await;
    assert_eq!(s.desired(&id).await, (None, "running".to_string()));
}
