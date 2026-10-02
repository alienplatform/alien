//! A compute plan credential on its own deployment's scope reaches no route under `OssAuthz`.
//! Each request carries a well-formed body, so a refusal is the authorization gate rather than a
//! parse error.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use alien_error::AlienError;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, CreateReleaseParams,
    DeploymentStore, ReleaseStore,
};
use alien_manager::AlienManagerBuilder;
use async_trait::async_trait;
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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn a_compute_plan_credential_reaches_no_route_on_its_own_deployment() {
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
        .create_deployment(
            &admin(),
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "deployment".to_string(),
                deployment_group_id: group.id.clone(),
                platform: alien_core::Platform::Local,
                base_platform: None,
                stack_settings: Default::default(),
                stack_state: None,
                environment_variables: None,
                public_subdomain: None,
                input_values: Default::default(),
                setup_item: None,
                deployment_token: None,
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

    let probes: Vec<(Method, String, Option<serde_json::Value>)> = vec![
        (Method::GET, format!("/v1/deployments/{id}"), None),
        (Method::GET, format!("/v1/deployments/{id}/info"), None),
        (
            Method::POST,
            format!("/v1/deployments/{id}/delete"),
            Some(json!({ "action": "cleanup" })),
        ),
        (
            Method::POST,
            format!("/v1/deployments/{id}/retry"),
            Some(json!({})),
        ),
        (
            Method::POST,
            format!("/v1/deployments/{id}/redeploy"),
            Some(json!({})),
        ),
        (Method::GET, "/v1/deployments".to_string(), None),
        (
            Method::GET,
            format!("/v1/deployment-groups/{}", group.id),
            None,
        ),
        (Method::GET, "/v1/releases".to_string(), None),
        (Method::GET, "/v1/releases/latest".to_string(), None),
        (Method::GET, format!("/v1/releases/{}", release.id), None),
        (Method::GET, "/v1/build-config".to_string(), None),
        (Method::GET, "/v1/platforms".to_string(), None),
        (Method::GET, "/v1/tokens".to_string(), None),
        (Method::GET, "/v1/whoami".to_string(), None),
        (
            Method::GET,
            format!("/v1/deployments/{id}/vault/v/secrets/k"),
            None,
        ),
        (
            Method::POST,
            "/v1/sync/acquire".to_string(),
            Some(json!({ "session": "s", "deploymentModel": "pull", "deploymentIds": [id] })),
        ),
        (
            Method::POST,
            "/v1/sync".to_string(),
            Some(json!({ "deploymentId": id })),
        ),
        (
            Method::POST,
            "/v1/credentials/mint".to_string(),
            Some(json!({ "deploymentId": id, "resourceId": "r", "bindingName": "b" })),
        ),
        (
            Method::POST,
            "/v1/image-repositories".to_string(),
            Some(json!({ "projectId": "default", "platform": "local" })),
        ),
        (Method::POST, "/v1/logs".to_string(), Some(json!({}))),
        (
            Method::POST,
            "/v1/commands".to_string(),
            Some(json!({
                "deploymentId": id,
                "command": "run",
                "params": { "mode": "inline", "inlineBase64": "" },
            })),
        ),
        (
            Method::POST,
            "/v1/commands/leases".to_string(),
            Some(json!({
                "deploymentId": id,
                "target": serde_json::to_value(alien_core::CommandTarget::new(
                    "daemon-a",
                    alien_core::CommandTargetType::Daemon,
                ))
                .unwrap(),
            })),
        ),
        (
            Method::POST,
            "/v1/bindings/resolve".to_string(),
            Some(json!({ "deploymentId": id, "resourceId": "files" })),
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
        ),
    ];

    let mut leaked = Vec::new();
    for (method, path, body) in probes {
        let mut req = client
            .request(method.clone(), format!("{url}{path}"))
            .bearer_auth("planner");
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.unwrap();
        let status = resp.status();
        let text = resp.text().await.expect("read response body");
        let empty_list = status.is_success() && text == r#"{"items":[]}"#;
        if !matches!(status.as_u16(), 403 | 404) && !empty_list {
            leaked.push(format!("{method} {path} -> {status}: {text}"));
        }
    }

    let pull = client
        .get(format!("{url}/v2/artifacts/default/manifests/v1"))
        .bearer_auth("planner")
        .send()
        .await
        .unwrap();
    if pull.status().as_u16() != 403 {
        leaked.push(format!("registry pull -> {}", pull.status()));
    }

    let init = client
        .post(format!("{url}/v1/initialize"))
        .bearer_auth("planner")
        .json(&json!({ "initialDesiredRelease": "none" }))
        .send()
        .await
        .unwrap();
    let init_status = init.status();
    let init_body = init.text().await.expect("read response body");
    if init_status.as_u16() != 403 {
        leaked.push(format!("POST /v1/initialize -> {init_status}: {init_body}"));
    }

    assert!(
        leaked.is_empty(),
        "routes reachable:\n{}",
        leaked.join("\n")
    );
}
