//! Who holds a pull deployment's lock around the end of runtime cleanup, over HTTP against a
//! standalone manager with the SQLite stores: the Operator reports the handoff, the destroy
//! command acquires the setup teardown, the Operator syncs again.

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
    tokens: Arc<dyn TokenStore>,
    _dir: tempfile::TempDir,
}

async fn start() -> Server {
    let channel_release = 1usize;
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
                            Platform::Kubernetes,
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
        lease_wait_probe_interval_secs: 1,
        max_open_lease_waits: 5_000,
        max_open_lease_waits_per_target: 8,
        max_open_lease_waits_per_deployment: 64,
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
        .token_store(token_store.clone())
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
                tokens: token_store,
                _dir: dir,
            };
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("manager did not become healthy");
}

impl Server {
    async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
        self.send_as(&self.token, method, path, body).await
    }

    async fn send_as(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Value {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let text = response.text().await.unwrap();
        assert!(status.is_success(), "{path} returned {status}: {text}");
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    fn state(&self, status: &str) -> Value {
        json!({
            "platform": "kubernetes",
            "status": status,
            "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        })
    }

    /// A pull Kubernetes deployment, running, then asked to delete (retain).
    async fn deleting_deployment(&self) -> String {
        let created = self
            .send(
                reqwest::Method::POST,
                "/v1/deployments",
                Some(json!({
                    "name": "example",
                    "platform": "kubernetes",
                    "deploymentGroupId": self.group_id,
                    "stackSettings": {"deploymentModel": "pull"},
                })),
            )
            .await;
        let id = created["deployment"]["id"].as_str().unwrap().to_string();
        let running = DeploymentState {
            status: DeploymentStatus::Running,
            platform: Platform::Kubernetes,
            current_release: Some(ReleaseInfo {
                release_id: Some(self.r2.clone()),
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
        self.send(
            reqwest::Method::POST,
            "/v1/sync/reconcile",
            Some(json!({"deploymentId": id, "session": "runtime", "state": running})),
        )
        .await;
        self.send(
            reqwest::Method::POST,
            &format!("/v1/deployments/{id}/delete"),
            Some(json!({"action": "retain"})),
        )
        .await;
        id
    }

    /// One sync of the Operator, as it runs it: its own session, execution claims supported,
    /// and the state it last reached.
    async fn operator_syncs(&self, id: &str, status: &str) -> Value {
        self.operator_syncs_with(&self.token, "pull-operator-1", id, status)
            .await
    }

    async fn operator_syncs_with(
        &self,
        token: &str,
        session: &str,
        id: &str,
        status: &str,
    ) -> Value {
        self.send_as(
            token,
            reqwest::Method::POST,
            "/v1/sync",
            Some(json!({
                "deploymentId": id,
                "session": session,
                "supportsExecutionClaims": true,
                "maxSupportedDeploymentProtocolVersion":
                    alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION,
                "currentState": self.state(status),
            })),
        )
        .await
    }

    /// The destroy command's acquire of the setup teardown.
    async fn command_acquires(&self, id: &str, session: &str, mode: Option<&str>) -> usize {
        let mut body = json!({
            "session": session,
            "deploymentIds": [id],
            "setupMethod": "helm",
            "statuses": ["retention-required", "teardown-required", "teardown-failed"],
            "deploymentModel": "pull",
            "maxSupportedDeploymentProtocolVersion": alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION,
        });
        if let Some(mode) = mode {
            body["acquireMode"] = json!(mode);
        }
        let answer = self
            .send(reqwest::Method::POST, "/v1/sync/acquire", Some(body))
            .await;
        answer["deployments"].as_array().unwrap().len()
    }
}

#[tokio::test]
async fn the_command_takes_the_deployment_the_operator_handed_off_and_keeps_it() {
    let s = start().await;
    let id = s.deleting_deployment().await;

    // The Operator runs the cleanup, then reports the end of it, and keeps syncing with the
    // status it reached while nobody has taken the deployment yet: it must not hold the lock
    // (it asks for its deployment by that status).
    s.operator_syncs(&id, "delete-pending").await;
    for _ in 0..3 {
        s.operator_syncs(&id, "retention-required").await;
    }

    // The command takes it at once.
    assert_eq!(
        s.command_acquires(&id, "command", Some("setup-teardown"))
            .await,
        1
    );

    // The Operator keeps syncing: it neither takes the lock nor gives the command's up.
    for _ in 0..3 {
        s.operator_syncs(&id, "retention-required").await;
    }
    assert_eq!(
        s.command_acquires(&id, "someone-else", Some("setup-teardown"))
            .await,
        0,
        "the command still holds the lease"
    );
}

/// An acquire that names no mode (older clients) takes the deployment the Operator handed off.
#[tokio::test]
async fn an_acquire_without_a_mode_takes_a_handed_off_deployment() {
    let s = start().await;
    let id = s.deleting_deployment().await;
    s.operator_syncs(&id, "delete-pending").await;
    s.operator_syncs(&id, "retention-required").await;
    assert_eq!(s.command_acquires(&id, "older-client", None).await, 1);
}

/// A token scoped to the one deployment, as the Operator holds it: it reports the handoff and the
/// command still takes the deployment.
#[tokio::test]
async fn an_operator_with_a_deployment_scoped_token_hands_off() {
    let s = start().await;
    let id = s.deleting_deployment().await;
    let token = format!("ax_dep_{}", uuid::Uuid::new_v4().simple());
    s.tokens
        .create_token(CreateTokenParams {
            token_type: TokenType::Deployment,
            key_prefix: token[..12].to_string(),
            key_hash: format!("{:x}", Sha256::digest(token.as_bytes())),
            deployment_group_id: None,
            deployment_id: Some(id.clone()),
        })
        .await
        .unwrap();
    s.operator_syncs_with(&token, "pull-operator-1", &id, "delete-pending")
        .await;
    for _ in 0..3 {
        s.operator_syncs_with(&token, "pull-operator-1", &id, "retention-required")
            .await;
    }
    assert_eq!(
        s.command_acquires(&id, "command", Some("setup-teardown"))
            .await,
        1
    );
}

/// An Operator that reports a handoff for a deployment nobody asked to delete hands nothing over:
/// the record is `running` before the report, so the setup owner cannot take it.
#[tokio::test]
async fn a_handoff_report_without_a_delete_request_hands_nothing_over() {
    let s = start().await;
    let id = s.deleting_deployment().await;
    // A second deployment that is running and was never asked to delete.
    let other = {
        let created = s
            .send(
                reqwest::Method::POST,
                "/v1/deployments",
                Some(json!({
                    "name": "other",
                    "platform": "kubernetes",
                    "deploymentGroupId": s.group_id,
                    "stackSettings": {"deploymentModel": "pull"},
                })),
            )
            .await;
        created["deployment"]["id"].as_str().unwrap().to_string()
    };
    let _ = id;
    s.operator_syncs(&other, "running").await;
    // Repeated reports must not move the record: a second report would otherwise find the
    // stored status equal to the reported one.
    for round in 1..=3 {
        s.operator_syncs(&other, "teardown-required").await;
        let record = s
            .send(
                reqwest::Method::GET,
                &format!("/v1/deployments/{other}"),
                None,
            )
            .await;
        assert_eq!(record["status"], "running", "report {round}: {record}");
        assert_eq!(
            s.command_acquires(&other, "command", Some("setup-teardown"))
                .await,
            0,
            "report {round}: no delete request, the setup owner does not get the deployment"
        );
    }
}

/// A session other than the lock's holder cannot give the lock away: another Operator session
/// reports the handoff while the first one still holds it.
#[tokio::test]
async fn a_session_that_does_not_hold_the_lock_releases_nothing() {
    let s = start().await;
    let id = s.deleting_deployment().await;
    // The first Operator session takes the deployment to run the cleanup and holds the lock.
    s.operator_syncs(&id, "delete-pending").await;
    // A different session reports the handoff.
    let response = reqwest::Client::new()
        .post(format!("{}/v1/sync", s.url))
        .bearer_auth(&s.token)
        .json(&json!({
            "deploymentId": id,
            "session": "pull-operator-2",
            "supportsExecutionClaims": true,
            "maxSupportedDeploymentProtocolVersion":
                alien_core::MAX_SUPPORTED_DEPLOYMENT_PROTOCOL_VERSION,
            "currentState": s.state("retention-required"),
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    assert_eq!(
        s.command_acquires(&id, "command", Some("setup-teardown"))
            .await,
        0,
        "the first session still holds the lock"
    );
}

/// A retained deployment reopened for an erase, through the real manager and the SQLite store:
/// the stale `retained` report of the Operator gives no lock away, the command that took the
/// reopened deletion finds the recorded erase under the lock, a run that does not ask for erase
/// is refused with the lock given back, and a run that does finishes the acquire.
#[tokio::test]
async fn an_erase_reopen_is_checked_against_the_recorded_policy_under_the_lock() {
    use alien_core::DeploymentModel;
    use alien_deployment::manager_api_transport::{
        acquire_setup_delete_deployment, SetupDeleteAcquireOutcome,
    };

    let s = start().await;
    let id = s.deleting_deployment().await;
    let client = {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", s.token).parse().unwrap(),
        );
        alien_manager_api::Client::new_with_client(
            &s.url,
            reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .unwrap(),
        )
    };

    // The first deletion (retain) runs to the end: the Operator hands off, the command takes the
    // deployment and records it as retained.
    s.operator_syncs(&id, "delete-pending").await;
    s.operator_syncs(&id, "retention-required").await;
    let first = acquire_setup_delete_deployment(
        &client,
        &client,
        &id,
        "command-1",
        DeploymentModel::Pull,
        "helm",
        false,
    )
    .await
    .expect("acquire");
    assert!(matches!(first, SetupDeleteAcquireOutcome::Acquired { .. }));
    s.send(
        reqwest::Method::POST,
        "/v1/sync/reconcile",
        Some(json!({"deploymentId": id, "session": "command-1", "state": s.state("retained")})),
    )
    .await;
    s.send(
        reqwest::Method::POST,
        "/v1/sync/release",
        Some(json!({"deploymentId": id, "session": "command-1"})),
    )
    .await;

    // Erase reopens it: delete-pending again, with the erase recorded.
    s.send(
        reqwest::Method::POST,
        &format!("/v1/deployments/{id}/delete"),
        Some(json!({"action": "erase"})),
    )
    .await;
    let record = s
        .send(reqwest::Method::GET, &format!("/v1/deployments/{id}"), None)
        .await;
    assert_eq!(record["status"], "delete-pending", "{record}");
    assert_eq!(record["runtimeMetadata"]["deletionPolicy"], "erase");

    // The Operator still reports `retained` from before: it gives no lock away, and the
    // command is not offered the deployment yet.
    s.operator_syncs(&id, "retained").await;
    assert_eq!(
        s.command_acquires(&id, "probe", Some("setup-teardown"))
            .await,
        0
    );

    // The Operator finishes its cleanup and reports the handoff.
    s.operator_syncs(&id, "teardown-required").await;

    // A run that does not ask for erase takes the lock, reads the recorded erase under it, gives
    // the lock back and refuses.
    let refused = acquire_setup_delete_deployment(
        &client,
        &client,
        &id,
        "command-2",
        DeploymentModel::Pull,
        "helm",
        false,
    )
    .await
    .expect("acquire");
    let SetupDeleteAcquireOutcome::PolicyRefused { message } = refused else {
        panic!("a rerun without --erase-data must be refused");
    };
    assert!(message.contains("requested with data erase"), "{message}");

    // The matching run proceeds: the lock the refused run gave back is free.
    let accepted = acquire_setup_delete_deployment(
        &client,
        &client,
        &id,
        "command-3",
        DeploymentModel::Pull,
        "helm",
        true,
    )
    .await
    .expect("acquire");
    assert!(
        matches!(accepted, SetupDeleteAcquireOutcome::Acquired { .. }),
        "the run that asks for erase takes the deployment"
    );
}
