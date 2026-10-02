//! A compute plan credential on its own deployment's scope gets nothing from any credentialed route
//! under `OssAuthz`: a 403, an empty list, or the OCI version check's empty body. Each request
//! carries a well-formed body, so a refusal is the authorization gate rather than a parse error.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use alien_error::AlienError;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateImportedDeploymentParams,
    CreateReleaseParams, DeploymentStore, ReleaseStore,
};
use alien_manager::AlienManagerBuilder;
use async_trait::async_trait;
use base64::Engine;
use hmac::Mac;
use reqwest::Method;
use serde_json::json;

struct PlannerValidator {
    deployment_id: String,
}

#[async_trait]
impl AuthValidator for PlannerValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let Some(value) = headers.get(http::header::AUTHORIZATION) else {
            return Ok(None);
        };
        let token = value.to_str().unwrap().trim_start_matches("Bearer ");
        if token == "admin" {
            return Ok(Some(admin()));
        }
        Ok(Some(Subject {
            kind: SubjectKind::ServiceAccount {
                id: "compute-planner".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Deployment {
                project_id: "default".to_string(),
                deployment_id: self.deployment_id.clone(),
            },
            role: Role::ComputePlanner,
            bearer_token: token.to_string(),
        }))
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

#[derive(Debug, Clone, Copy)]
enum Expect {
    Forbidden,
    EmptyList,
    /// A registry 403 carrying this text, since path and signature checks also return 403.
    Denied(&'static str),
    /// The OCI version check answers any authenticated caller.
    VersionCheck,
}

/// Signs as the registry proxy does, so the upload-session probe gets past the signature check.
fn sign_upload_session(key: &[u8], path: &str, repo: &str, expires: i64) -> String {
    type HmacSha256 = hmac::Hmac<sha2::Sha256>;
    let mut derive = HmacSha256::new_from_slice(key).unwrap();
    derive.update(b"registry-upload-session-signing");
    let mut mac = HmacSha256::new_from_slice(&derive.finalize().into_bytes()).unwrap();
    mac.update(format!("1\n{path}\n{repo}\n{expires}").as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn a_compute_plan_credential_gets_nothing_from_any_credentialed_route() {
    let state_dir = tempfile::tempdir().unwrap();
    let db_path = state_dir.path().join("test.db");
    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let store = SqliteDeploymentStore::new(db.clone());
    let release = SqliteReleaseStore::new(db)
        .create_release(
            &admin(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: std::collections::HashMap::from([(
                    alien_core::Platform::Local,
                    alien_core::Stack::new("planned".to_string())
                        .platforms(vec![alien_core::Platform::Local])
                        .add(
                            alien_core::Daemon::new("daemon-a".to_string())
                                .code(alien_core::DaemonCode::Image {
                                    image: "daemon:latest".to_string(),
                                })
                                .permissions("daemon-execution".to_string())
                                .commands_enabled(true)
                                .build(),
                            alien_core::ResourceLifecycle::Live,
                        )
                        .build(),
                )]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();
    let group = store
        .create_deployment_group(
            &admin(),
            CreateDeploymentGroupParams {
                name: "group".to_string(),
                max_deployments: 10,
            },
        )
        .await
        .unwrap();
    let deployment = store
        .create_with_state(
            &admin(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "deployment".to_string(),
                deployment_group_id: group.id.clone(),
                platform: alien_core::Platform::Local,
                base_platform: None,
                stack_settings: Default::default(),
                stack_state: alien_core::StackState::new(alien_core::Platform::Local),
                environment_info: None,
                runtime_metadata: Default::default(),
                status: "running".to_string(),
                current_release_id: Some(release.id.clone()),
                desired_release_id: None,
                import_source: None,
                setup_metadata: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                deployment_token: None,
                management_config: None,
                input_values: Default::default(),
            },
        )
        .await
        .unwrap();
    let id = deployment.id.clone();

    let port = free_port();
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
        .auth_validator(Arc::new(PlannerValidator {
            deployment_id: id.clone(),
        }))
        .authz(Arc::new(OssAuthz))
        .with_standalone_defaults(&toml_config)
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let server = tokio::spawn(async move { manager.start(addr).await.unwrap() });
    let client = reqwest::Client::new();

    let mut health = None;
    for _ in 0..50 {
        assert!(!server.is_finished(), "manager exited early");
        if let Ok(r) = client.get(format!("{url}/health")).send().await {
            if r.status().is_success() {
                health = Some(r.json::<serde_json::Value>().await.unwrap());
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let health = health.expect("manager became healthy");
    assert_eq!(health["computePlanCapability"], json!(true), "{health}");

    let setup = |method: Method, path: &str, body: Option<serde_json::Value>| {
        let req = client
            .request(method, format!("{url}{path}"))
            .bearer_auth("admin");
        let req = match body {
            Some(body) => req.json(&body),
            None => req,
        };
        let path = path.to_string();
        async move {
            let resp = req.send().await.unwrap();
            assert!(
                resp.status().is_success(),
                "setup {path}: {}",
                resp.status()
            );
            resp.json::<serde_json::Value>().await.unwrap()
        }
    };
    let command_id = setup(
        Method::POST,
        "/v1/commands",
        Some(json!({
            "deploymentId": id,
            "command": "run",
            "params": { "mode": "inline", "inlineBase64": "" },
        })),
    )
    .await["commandId"]
        .as_str()
        .unwrap()
        .to_string();
    let lease_target = serde_json::to_value(alien_core::CommandTarget::new(
        "daemon-a",
        alien_core::CommandTargetType::Daemon,
    ))
    .unwrap();
    let lease_id = setup(
        Method::POST,
        "/v1/commands/leases",
        Some(json!({ "deploymentId": id, "target": lease_target })),
    )
    .await["leases"][0]["leaseId"]
        .as_str()
        .unwrap()
        .to_string();
    setup(
        Method::POST,
        &format!("/v1/deployment-groups/{}/tokens", group.id),
        None,
    )
    .await;
    let token_id = setup(Method::GET, "/v1/tokens", None).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let upload_path = "/artifacts-uploads/namespaces/artifacts/repositories/default/uploads/u1";
    let upload_repo = "artifacts/default";
    let expires = chrono::Utc::now().timestamp() + 600;
    let upload_query = format!(
        "_alien_v=1&_alien_repo={}&_alien_exp={expires}&_alien_sig={}",
        urlencoding::encode(upload_repo),
        sign_upload_session(b"test-signing-key", upload_path, upload_repo, expires),
    );

    // /health, /install and /install.ps1 take no credentials.
    use Expect::*;
    let probes: Vec<(Method, String, Option<serde_json::Value>, Expect)> = vec![
        (
            Method::GET,
            format!("/v1/deployments/{id}"),
            None,
            Forbidden,
        ),
        (
            Method::GET,
            format!("/v1/deployments/{id}/info"),
            None,
            Forbidden,
        ),
        (
            Method::POST,
            format!("/v1/deployments/{id}/delete"),
            Some(json!({ "action": "cleanup" })),
            Forbidden,
        ),
        (
            Method::POST,
            format!("/v1/deployments/{id}/retry"),
            Some(json!({})),
            Forbidden,
        ),
        (
            Method::POST,
            format!("/v1/deployments/{id}/redeploy"),
            Some(json!({})),
            Forbidden,
        ),
        (Method::GET, "/v1/deployments".to_string(), None, Forbidden),
        (
            Method::POST,
            "/v1/deployments".to_string(),
            Some(json!({
                "name": "another",
                "platform": "local",
                "deploymentGroupId": group.id,
            })),
            Forbidden,
        ),
        (
            Method::GET,
            "/v1/deployment-groups".to_string(),
            None,
            EmptyList,
        ),
        (
            Method::POST,
            "/v1/deployment-groups".to_string(),
            Some(json!({ "name": "another" })),
            Forbidden,
        ),
        (
            Method::GET,
            format!("/v1/deployment-groups/{}", group.id),
            None,
            Forbidden,
        ),
        (
            Method::POST,
            format!("/v1/deployment-groups/{}/tokens", group.id),
            None,
            Forbidden,
        ),
        (Method::GET, "/v1/releases".to_string(), None, EmptyList),
        (
            Method::POST,
            "/v1/releases".to_string(),
            Some(json!({
                "projectId": "default",
                "stack": { "local": serde_json::to_value(&release.stacks[&alien_core::Platform::Local]).unwrap() },
            })),
            Forbidden,
        ),
        (
            Method::GET,
            "/v1/releases/latest".to_string(),
            None,
            Forbidden,
        ),
        (
            Method::GET,
            format!("/v1/releases/{}", release.id),
            None,
            Forbidden,
        ),
        (Method::GET, "/v1/build-config".to_string(), None, Forbidden),
        (Method::GET, "/v1/platforms".to_string(), None, Forbidden),
        (Method::GET, "/v1/tokens".to_string(), None, Forbidden),
        (
            Method::DELETE,
            format!("/v1/tokens/{token_id}"),
            None,
            Forbidden,
        ),
        (Method::GET, "/v1/whoami".to_string(), None, Forbidden),
        (
            Method::GET,
            format!("/v1/deployments/{id}/vault/secrets/secrets/k"),
            None,
            Forbidden,
        ),
        (
            Method::PUT,
            format!("/v1/deployments/{id}/vault/secrets/secrets/k"),
            Some(json!({ "value": "x" })),
            Forbidden,
        ),
        (
            Method::DELETE,
            format!("/v1/deployments/{id}/vault/secrets/secrets/k"),
            None,
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/initialize".to_string(),
            Some(json!({ "initialDesiredRelease": "none" })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/sync/acquire".to_string(),
            Some(json!({ "session": "s", "deploymentModel": "pull", "deploymentIds": [id] })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/sync/reconcile".to_string(),
            Some(json!({
                "deploymentId": id,
                "session": "s",
                "state": serde_json::to_value(alien_core::DeploymentState {
                    status: alien_core::DeploymentStatus::Running,
                    platform: alien_core::Platform::Local,
                    current_release: None,
                    target_release: None,
                    stack_state: None,
                    error: None,
                    environment_info: None,
                    runtime_metadata: None,
                    retry_requested: false,
                    protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                })
                .unwrap(),
            })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/sync/renew".to_string(),
            Some(json!({ "deploymentId": id, "session": "s" })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/sync/release".to_string(),
            Some(json!({ "deploymentId": id, "session": "s" })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/sync".to_string(),
            Some(json!({ "deploymentId": id })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/credentials/mint".to_string(),
            Some(json!({ "deploymentId": id, "resourceId": "r", "bindingName": "b" })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/image-repositories".to_string(),
            Some(json!({ "projectId": "default", "platform": "local" })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/logs".to_string(),
            Some(json!({})),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/traces".to_string(),
            Some(json!({})),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/metrics".to_string(),
            Some(json!({})),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/commands".to_string(),
            Some(json!({
                "deploymentId": id,
                "command": "run",
                "params": { "mode": "inline", "inlineBase64": "" },
            })),
            Forbidden,
        ),
        (
            Method::GET,
            format!("/v1/commands/{command_id}"),
            None,
            Forbidden,
        ),
        (
            Method::POST,
            format!("/v1/commands/{command_id}/upload-complete"),
            Some(json!({ "size": 0 })),
            Forbidden,
        ),
        (
            Method::PUT,
            format!("/v1/commands/{command_id}/response"),
            Some(serde_json::to_value(alien_core::CommandResponse::success(b"{}")).unwrap()),
            Forbidden,
        ),
        (
            Method::GET,
            format!("/v1/commands/{command_id}/payload"),
            None,
            Forbidden,
        ),
        (
            Method::PUT,
            format!("/v1/commands/{command_id}/payload"),
            Some(json!({})),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/commands/leases".to_string(),
            Some(json!({ "deploymentId": id, "target": lease_target })),
            Forbidden,
        ),
        (
            Method::POST,
            format!("/v1/commands/leases/{lease_id}/release"),
            None,
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/bindings/resolve".to_string(),
            Some(json!({ "deploymentId": id, "resourceId": "files" })),
            Forbidden,
        ),
        (
            Method::POST,
            "/v1/stack/import".to_string(),
            Some(
                serde_json::to_value(alien_core::import::StackImportRequest {
                    setup_import_format_version:
                        alien_core::import::CURRENT_SETUP_IMPORT_FORMAT_VERSION,
                    deployment_group_token: String::new(),
                    deployment_name: "deployment".to_string(),
                    resource_prefix: "deployment".to_string(),
                    source_kind: None,
                    setup_metadata: None,
                    release_id: Some(release.id.clone()),
                    platform: alien_core::Platform::Local,
                    base_platform: None,
                    region: "region".to_string(),
                    setup_target: "target".to_string(),
                    setup_fingerprint: "fingerprint".to_string(),
                    setup_fingerprint_version: 1,
                    stack_settings: Default::default(),
                    management_config: None,
                    input_values: Default::default(),
                    resources: vec![],
                })
                .unwrap(),
            ),
            Forbidden,
        ),
        (
            Method::GET,
            "/v2/artifacts/default/manifests/v1".to_string(),
            None,
            Denied("cannot pull"),
        ),
        (
            Method::POST,
            "/v2/artifacts/default/blobs/uploads/".to_string(),
            None,
            Denied("cannot push"),
        ),
        (
            Method::HEAD,
            "/v2/artifacts/default/manifests/v1".to_string(),
            None,
            Forbidden,
        ),
        (
            Method::PUT,
            "/v2/artifacts/default/manifests/v1".to_string(),
            Some(json!({})),
            Denied("cannot push"),
        ),
        (
            Method::PATCH,
            "/v2/artifacts/default/blobs/uploads/u1".to_string(),
            None,
            Denied("cannot push"),
        ),
        (
            Method::PUT,
            format!("{upload_path}?{upload_query}"),
            None,
            Denied("cannot push"),
        ),
        (
            Method::POST,
            format!("{upload_path}?{upload_query}"),
            None,
            Denied("cannot push"),
        ),
        (
            Method::PATCH,
            format!("{upload_path}?{upload_query}"),
            None,
            Denied("cannot push"),
        ),
        (Method::GET, "/v2".to_string(), None, VersionCheck),
        (Method::GET, "/v2/".to_string(), None, VersionCheck),
    ];

    let mut leaked = Vec::new();
    for (method, path, body, expect) in probes {
        let mut req = client
            .request(method.clone(), format!("{url}{path}"))
            .bearer_auth("planner");
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status().as_u16();
        let text = resp.text().await.expect("read response body");
        let ok = match expect {
            Forbidden => status == 403,
            EmptyList => {
                status == 200
                    && serde_json::from_str::<serde_json::Value>(&text)
                        .is_ok_and(|v| v["items"] == json!([]))
            }
            Denied(needle) => status == 403 && text.contains(needle),
            VersionCheck => status == 200 && text == "{}",
        };
        if !ok {
            leaked.push(format!(
                "{method} {path} expected {expect:?}, got {status}: {text}"
            ));
        }
    }

    let unrouted = client
        .get(format!("{url}/v1/no-such-route"))
        .bearer_auth("planner")
        .send()
        .await
        .unwrap();
    // No expectation accepts 404, so a mistyped probe path fails rather than passing.
    assert_eq!(unrouted.status().as_u16(), 404);
    assert_eq!(unrouted.text().await.unwrap(), "");

    assert!(
        leaked.is_empty(),
        "routes reachable:\n{}",
        leaked.join("\n")
    );
}
