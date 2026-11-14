//! The key `POST /v1/deployments/{id}/setup-credential` issues goes into the token
//! store the manager is built with, not into a store of the default's own.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alien_core::Platform;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteTokenStore};
use alien_manager::traits::{
    CreateDeploymentGroupParams, CreateTokenParams, DeploymentStore, TokenRecord, TokenStore,
    TokenType,
};
use alien_manager::AlienManagerBuilder;
use chrono::{DateTime, Utc};
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

fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// A token store of the embedder's own: it keeps its tokens in SQLite, as a
/// real one would keep them elsewhere, and counts the setup keys it is asked for.
struct CountingTokens {
    inner: SqliteTokenStore,
    setup_keys: AtomicUsize,
}

#[async_trait::async_trait]
impl TokenStore for CountingTokens {
    async fn create_token(
        &self,
        params: CreateTokenParams,
    ) -> Result<TokenRecord, alien_error::AlienError> {
        self.inner.create_token(params).await
    }
    async fn validate_token(
        &self,
        key_hash: &str,
    ) -> Result<Option<TokenRecord>, alien_error::AlienError> {
        self.inner.validate_token(key_hash).await
    }
    async fn delete_token(&self, id: &str) -> Result<(), alien_error::AlienError> {
        self.inner.delete_token(id).await
    }
    async fn list_tokens(&self) -> Result<Vec<TokenRecord>, alien_error::AlienError> {
        self.inner.list_tokens().await
    }
    async fn commands_key_generation(
        &self,
        deployment_id: &str,
    ) -> Result<u64, alien_error::AlienError> {
        self.inner.commands_key_generation(deployment_id).await
    }
    async fn create_commands_key(
        &self,
        deployment_id: &str,
        key_prefix: &str,
        key_hash: &str,
        generation: u64,
    ) -> Result<Option<TokenRecord>, alien_error::AlienError> {
        self.inner
            .create_commands_key(deployment_id, key_prefix, key_hash, generation)
            .await
    }
    async fn revoke_commands_keys(
        &self,
        deployment_id: &str,
    ) -> Result<(), alien_error::AlienError> {
        self.inner.revoke_commands_keys(deployment_id).await
    }
    async fn create_setup_token(
        &self,
        deployment_id: &str,
        key_prefix: &str,
        key_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<TokenRecord, alien_error::AlienError> {
        self.setup_keys.fetch_add(1, Ordering::SeqCst);
        self.inner
            .create_setup_token(deployment_id, key_prefix, key_hash, expires_at)
            .await
    }
}

#[tokio::test]
async fn the_setup_key_goes_into_the_token_store_the_manager_was_built_with() {
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
    let tokens = Arc::new(CountingTokens {
        inner: SqliteTokenStore::new(db.clone()),
        setup_keys: AtomicUsize::new(0),
    });
    let admin_token = format!("ax_admin_{}", uuid::Uuid::new_v4().simple());
    tokens
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: admin_token[..12].to_string(),
            key_hash: hash(&admin_token),
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
        .token_store(tokens.clone())
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
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let call = |path: String, bearer: Option<String>, body: Value| {
        let client = client.clone();
        let url = url.clone();
        async move {
            let mut request = client.post(format!("{url}{path}")).json(&body);
            if let Some(bearer) = bearer {
                request = request.bearer_auth(bearer);
            }
            let response = request.send().await.unwrap();
            let status = response.status();
            let text = response.text().await.unwrap();
            (
                status,
                serde_json::from_str::<Value>(&text).unwrap_or(Value::Null),
            )
        }
    };

    let (status, created) = call(
        "/v1/deployments".to_string(),
        Some(admin_token.clone()),
        json!({ "name": "customer", "platform": serde_json::to_value(Platform::Local).unwrap(), "deploymentGroupId": group_id }),
    )
    .await;
    assert!(status.is_success(), "{created}");
    let deployment_id = created["deployment"]["id"].as_str().unwrap().to_string();

    let (status, code) = call(
        format!("/v1/deployments/{deployment_id}/enrollment-code"),
        Some(admin_token.clone()),
        Value::Null,
    )
    .await;
    assert!(status.is_success(), "{code}");
    let operator_key = format!("ax_op_{}", uuid::Uuid::new_v4().simple());
    let (status, enrolled) = call(
        "/v1/identity/enroll".to_string(),
        None,
        json!({ "code": code["code"], "initialKeyHash": hash(&operator_key) }),
    )
    .await;
    assert!(status.is_success(), "{enrolled}");

    let (status, issued) = call(
        format!("/v1/deployments/{deployment_id}/setup-credential"),
        Some(operator_key),
        Value::Null,
    )
    .await;
    assert!(status.is_success(), "{issued}");
    assert_eq!(
        tokens.setup_keys.load(Ordering::SeqCst),
        1,
        "the key was written to another store"
    );
    let issued_token = issued["token"].as_str().expect("token");
    let record = tokens
        .validate_token(&hash(issued_token))
        .await
        .unwrap()
        .expect("the configured store holds the issued key");
    assert_eq!(record.token_type, TokenType::Setup);
}
