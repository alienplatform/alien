//! `POST /v1/initialize` under `OssAuthz` for every scope an agent or operator presents.
//! Real deployment-group, deployment and admin tokens go through `TokenDbValidator`; bearers
//! of the form `role:<deployment>` stand in for deployment-scoped roles OSS never mints, and
//! `dg-denied:<group>` for a deployment-group deployer whose `Authz` refuses sync and create.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use alien_error::AlienError;
use alien_manager::auth::{Authz, DeploymentCreateCtx, Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::token_db_validator::TokenDbValidator;
use alien_manager::providers::OssAuthz;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteTokenStore};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateTokenParams, DeploymentGroupRecord,
    DeploymentRecord, DeploymentStore, ReleaseRecord, TelemetrySignal, TokenStore, TokenType,
};
use alien_manager::AlienManagerBuilder;
use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

struct MixedValidator {
    tokens: TokenDbValidator,
}

#[async_trait]
impl AuthValidator for MixedValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let bearer = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        let role = match bearer.split_once(':') {
            Some(("viewer", _)) => Role::DeploymentViewer,
            Some(("telemetry", _)) => Role::DeploymentTelemetryWriter,
            Some(("resolver", _)) => Role::RemoteBindingResolver,
            Some(("planner", _)) => Role::ComputePlanner,
            Some(("manager", _)) => Role::DeploymentManager,
            Some(("dg-denied", group)) => {
                return Ok(Some(Subject {
                    kind: SubjectKind::ServiceAccount {
                        id: DENIED_GROUP_CALLER.to_string(),
                    },
                    workspace_id: "default".to_string(),
                    scope: Scope::DeploymentGroup {
                        project_id: "default".to_string(),
                        deployment_group_id: group.to_string(),
                    },
                    role: Role::DeploymentGroupDeployer,
                    bearer_token: bearer.to_string(),
                }))
            }
            Some(("project-viewer", _)) => {
                return Ok(Some(Subject {
                    kind: SubjectKind::ServiceAccount {
                        id: "project-viewer".to_string(),
                    },
                    workspace_id: "default".to_string(),
                    scope: Scope::Project {
                        project_id: "default".to_string(),
                    },
                    role: Role::ProjectViewer,
                    bearer_token: bearer.to_string(),
                }))
            }
            _ => return self.tokens.validate(headers).await,
        };
        let deployment_id = bearer.split_once(':').unwrap().1.to_string();
        Ok(Some(Subject {
            kind: SubjectKind::ServiceAccount {
                id: "stand-in".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Deployment {
                project_id: "default".to_string(),
                deployment_id,
            },
            role,
            bearer_token: bearer.to_string(),
        }))
    }
}

const DENIED_GROUP_CALLER: &str = "dg-denied";

fn denied(s: &Subject) -> bool {
    matches!(&s.kind, SubjectKind::ServiceAccount { id } if id == DENIED_GROUP_CALLER)
}

/// `OssAuthz`, except that the stand-in group caller may neither sync nor create a deployment.
struct DenyingAuthz;

impl Authz for DenyingAuthz {
    fn can_sync_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        !denied(s) && OssAuthz.can_sync_deployment(s, d)
    }
    fn can_create_deployment(&self, s: &Subject, c: DeploymentCreateCtx<'_>) -> bool {
        !denied(s) && OssAuthz.can_create_deployment(s, c)
    }
    fn can_create_release(&self, s: &Subject, p: &str) -> bool {
        OssAuthz.can_create_release(s, p)
    }
    fn can_read_release(&self, s: &Subject, r: &ReleaseRecord) -> bool {
        OssAuthz.can_read_release(s, r)
    }
    fn can_export_release(&self, s: &Subject, r: &ReleaseRecord) -> bool {
        OssAuthz.can_export_release(s, r)
    }
    fn can_read_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_read_deployment(s, d)
    }
    fn can_update_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_update_deployment(s, d)
    }
    fn can_resolve_remote_binding(
        &self,
        s: &Subject,
        d: &DeploymentRecord,
        k: alien_core::remote_bindings::RemoteBindingKind,
        r: &str,
    ) -> bool {
        OssAuthz.can_resolve_remote_binding(s, d, k, r)
    }
    fn can_delete_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_delete_deployment(s, d)
    }
    fn can_create_deployment_group(&self, s: &Subject, p: &str) -> bool {
        OssAuthz.can_create_deployment_group(s, p)
    }
    fn can_read_deployment_group(&self, s: &Subject, g: &DeploymentGroupRecord) -> bool {
        OssAuthz.can_read_deployment_group(s, g)
    }
    fn can_update_deployment_group(&self, s: &Subject, g: &DeploymentGroupRecord) -> bool {
        OssAuthz.can_update_deployment_group(s, g)
    }
    fn can_delete_deployment_group(&self, s: &Subject, g: &DeploymentGroupRecord) -> bool {
        OssAuthz.can_delete_deployment_group(s, g)
    }
    fn can_dispatch_command(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_dispatch_command(s, d)
    }
    fn can_read_command(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_read_command(s, d)
    }
    fn can_read_command_context(
        &self,
        s: &Subject,
        c: &alien_commands::server::CommandAccessContext,
    ) -> bool {
        OssAuthz.can_read_command_context(s, c)
    }
    fn can_receive_command(
        &self,
        s: &Subject,
        d: &DeploymentRecord,
        t: &alien_core::CommandTarget,
    ) -> bool {
        OssAuthz.can_receive_command(s, d, t)
    }
    fn can_execute_command_context(
        &self,
        s: &Subject,
        c: &alien_commands::server::CommandAccessContext,
    ) -> bool {
        OssAuthz.can_execute_command_context(s, c)
    }
    fn can_acquire_deployments(&self, s: &Subject, d: &[DeploymentRecord]) -> bool {
        OssAuthz.can_acquire_deployments(s, d)
    }
    fn can_ingest_telemetry(&self, s: &Subject, t: TelemetrySignal) -> bool {
        OssAuthz.can_ingest_telemetry(s, t)
    }
    fn can_push_image(&self, s: &Subject, p: &str, r: &str) -> bool {
        OssAuthz.can_push_image(s, p, r)
    }
    fn can_provision_image_repository(&self, s: &Subject, p: &str) -> bool {
        OssAuthz.can_provision_image_repository(s, p)
    }
    fn can_act_on_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_act_on_deployment(s, d)
    }
}

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

async fn seed_token(
    store: &SqliteTokenStore,
    token_type: TokenType,
    group: Option<&str>,
) -> String {
    let raw = format!(
        "{}{}",
        token_type.prefix(),
        "0123456789abcdef0123456789abcdef01234567"
    );
    store
        .create_token(CreateTokenParams {
            token_type,
            key_prefix: raw[..12].to_string(),
            key_hash: hex::encode(Sha256::digest(raw.as_bytes())),
            deployment_group_id: group.map(ToString::to_string),
            deployment_id: None,
        })
        .await
        .unwrap();
    raw
}

#[tokio::test]
async fn initialize_admits_sync_callers_and_refuses_other_deployment_roles() {
    let state_dir = tempfile::tempdir().unwrap();
    let db_path = state_dir.path().join("test.db");
    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let group = SqliteDeploymentStore::new(db.clone())
        .create_deployment_group(
            &admin(),
            CreateDeploymentGroupParams {
                name: "group".to_string(),
                max_deployments: 10,
            },
        )
        .await
        .unwrap();
    let token_store = Arc::new(SqliteTokenStore::new(db));
    let dg_token = seed_token(&token_store, TokenType::DeploymentGroup, Some(&group.id)).await;
    let admin_token = seed_token(&token_store, TokenType::Admin, None).await;

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}");
    let config = ManagerConfig {
        port,
        host: "127.0.0.1".to_string(),
        db_path: Some(db_path),
        state_dir: Some(state_dir.path().to_path_buf()),
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
        enable_local_log_ingest: true,
        allowed_origins: None,
        response_signing_key: b"test-signing-key".to_vec(),
    };
    let toml_config = alien_manager::standalone_config::ManagerTomlConfig::default();
    let manager = AlienManagerBuilder::new(config)
        .auth_validator(Arc::new(MixedValidator {
            tokens: TokenDbValidator::new(token_store.clone()),
        }))
        .authz(Arc::new(DenyingAuthz))
        .with_standalone_defaults(&toml_config)
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let server = tokio::spawn(async move { manager.start(addr).await.unwrap() });
    let client = reqwest::Client::new();
    let mut ready = false;
    for _ in 0..50 {
        assert!(!server.is_finished(), "manager exited early");
        if let Ok(r) = client.get(format!("{url}/health")).send().await {
            if r.status().is_success() {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "manager never became healthy");

    let init = |bearer: String| {
        let client = client.clone();
        let url = url.clone();
        async move {
            let resp = client
                .post(format!("{url}/v1/initialize"))
                .bearer_auth(bearer)
                .json(&json!({ "name": "agent-a", "initialDesiredRelease": "none" }))
                .send()
                .await
                .unwrap();
            let status = resp.status().as_u16();
            let body: Value = resp.json().await.unwrap();
            (status, body)
        }
    };

    // A deployment-group token registers the agent and receives a deployment token.
    let (status, body) = init(dg_token.clone()).await;
    assert_eq!(status, 201, "{body}");
    let deployment_id = body["deploymentId"].as_str().unwrap().to_string();
    let deployment_token = body["token"].as_str().unwrap().to_string();

    // The agent restarts with that deployment token (DeploymentManager) and gets its own id back.
    let (status, body) = init(deployment_token.clone()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["deploymentId"], json!(deployment_id), "{body}");
    assert!(body.get("token").is_none(), "{body}");

    // Re-running with the group token is idempotent on the name.
    let (status, body) = init(dg_token.clone()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["deploymentId"], json!(deployment_id), "{body}");

    // A group caller that `Authz` may not sync gets no token for the existing deployment, and one
    // it may not create for gets no new deployment.
    let (status, body) = init(format!("dg-denied:{}", group.id)).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(
        body["message"],
        json!("Caller cannot initialize a deployment in this group"),
        "{body}"
    );
    assert!(body.get("token").is_none(), "{body}");
    // A refused caller learns nothing about the existing deployment's prefix: 403 before the 400.
    let rerun_with_prefix = |bearer: String| {
        client
            .post(format!("{url}/v1/initialize"))
            .bearer_auth(bearer)
            .json(&json!({
                "name": "agent-a",
                "initialDesiredRelease": "none",
                "resourcePrefix": "otherprefix",
            }))
            .send()
    };
    let resp = rerun_with_prefix(dg_token.clone()).await.unwrap();
    assert_eq!(resp.status().as_u16(), 400);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["message"],
        json!("Deployment already uses a different resource prefix"),
        "{body}"
    );
    let resp = rerun_with_prefix(format!("dg-denied:{}", group.id))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["message"],
        json!("Caller cannot initialize a deployment in this group"),
        "{body}"
    );
    let resp = client
        .post(format!("{url}/v1/initialize"))
        .bearer_auth(format!("dg-denied:{}", group.id))
        .json(&json!({ "name": "agent-b", "initialDesiredRelease": "none" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["message"],
        json!("Caller cannot initialize a deployment in this group"),
        "{body}"
    );

    // A workspace admin token is assigned to the existing deployment.
    let (status, body) = init(admin_token.clone()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["deploymentId"], json!(deployment_id), "{body}");

    // Whoami keeps serving every caller that is not a compute plan credential.
    for (bearer, scope_type) in [
        (deployment_token.clone(), "deployment"),
        (dg_token.clone(), "deployment-group"),
        (admin_token.clone(), "workspace"),
        (format!("viewer:{deployment_id}"), "deployment"),
        (format!("telemetry:{deployment_id}"), "deployment"),
        ("project-viewer:".to_string(), "project"),
    ] {
        let resp = client
            .get(format!("{url}/v1/whoami"))
            .bearer_auth(&bearer)
            .send()
            .await
            .unwrap();
        let status = resp.status().as_u16();
        let body: Value = resp.json().await.unwrap();
        assert_eq!(status, 200, "{bearer}: {body}");
        assert_eq!(body["scope"]["type"], json!(scope_type), "{bearer}: {body}");
    }
    let resp = client
        .get(format!("{url}/v1/whoami"))
        .bearer_auth(format!("planner:{deployment_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], json!("FORBIDDEN"), "{body}");

    let (status, body) = init(format!("manager:{deployment_id}")).await;
    assert_eq!(status, 200, "{body}");

    // A project viewer may read the deployment but not sync it: 403 from the project branch.
    let (project_status, project_body) = init("project-viewer:".to_string()).await;
    assert_eq!(project_status, 403, "{project_body}");
    assert_eq!(project_body["code"], json!("FORBIDDEN"), "{project_body}");

    for role in ["viewer", "telemetry", "resolver", "planner"] {
        let (status, body) = init(format!("{role}:{deployment_id}")).await;
        assert_eq!(status, 403, "{role}: {body}");
        assert_eq!(body["code"], json!("FORBIDDEN"), "{role}: {body}");
        assert_eq!(
            body["message"],
            json!("Caller cannot initialize this deployment"),
            "{role}: {body}"
        );
        assert!(body.get("deploymentModel").is_none(), "{role}: {body}");
    }
}
