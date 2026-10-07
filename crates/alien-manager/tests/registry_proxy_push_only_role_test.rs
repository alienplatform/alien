//! A project-scoped push-only credential pushes a real image through the registry proxy, and
//! cannot pull or push under another project. The Authz grants `SandboxImagePusher` push on its
//! own project, as an embedder would; everything else falls through to `OssAuthz`.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use alien_error::AlienError;
use alien_manager::auth::{Authz, DeploymentCreateCtx, Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, DeploymentFilter,
    DeploymentGroupRecord, DeploymentRecord, DeploymentStore, ReleaseRecord, TelemetrySignal,
};
use alien_manager::AlienManagerBuilder;
use async_trait::async_trait;
use container_registry::ContainerRegistry;

const PUSHER_PREFIX: &str = "pusher-for-";

/// `Bearer pusher-for-<project>` authenticates as the push-only role on `<project>`.
struct PusherValidator;

#[async_trait]
impl AuthValidator for PusherValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let Some(value) = headers.get(http::header::AUTHORIZATION) else {
            return Ok(None);
        };
        let token = value
            .to_str()
            .unwrap()
            .strip_prefix("Bearer ")
            .expect("tests send bearer tokens only");
        let project_id = token
            .strip_prefix(PUSHER_PREFIX)
            .expect("tests send pusher tokens only");
        Ok(Some(Subject {
            kind: SubjectKind::ServiceAccount {
                id: "sandbox-image-pusher".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Project {
                project_id: project_id.to_string(),
            },
            role: Role::SandboxImagePusher,
            bearer_token: token.to_string(),
        }))
    }
}

struct PusherAuthz;

impl Authz for PusherAuthz {
    fn can_push_image(&self, s: &Subject, project_id: &str, repo_name: &str) -> bool {
        match (&s.scope, s.role) {
            (Scope::Project { project_id: pid }, Role::SandboxImagePusher) => pid == project_id,
            _ => OssAuthz.can_push_image(s, project_id, repo_name),
        }
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
    fn can_create_deployment(&self, s: &Subject, c: DeploymentCreateCtx<'_>) -> bool {
        OssAuthz.can_create_deployment(s, c)
    }
    fn can_read_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_read_deployment(s, d)
    }
    fn can_update_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_update_deployment(s, d)
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
    fn can_sync_deployment(&self, s: &Subject, d: &DeploymentRecord) -> bool {
        OssAuthz.can_sync_deployment(s, d)
    }
    fn can_acquire_deployments(&self, s: &Subject, d: &[DeploymentRecord]) -> bool {
        OssAuthz.can_acquire_deployments(s, d)
    }
    fn can_ingest_telemetry(&self, s: &Subject, t: TelemetrySignal) -> bool {
        OssAuthz.can_ingest_telemetry(s, t)
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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Fails with the server's own error when it exits early, e.g. when another test took its port.
async fn wait_for_health(
    client: &reqwest::Client,
    url: &str,
    server: &mut tokio::task::JoinHandle<()>,
) {
    for _ in 0..50 {
        if server.is_finished() {
            panic!("Manager exited before becoming healthy: {:?}", server.await);
        }
        if client
            .get(format!("{url}/health"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("Manager did not become healthy at {url}");
}

#[tokio::test]
async fn a_push_only_credential_pushes_but_cannot_pull_or_cross_projects() {
    let running = ContainerRegistry::builder()
        .build_for_testing()
        .run_in_background();
    let registry_url = format!("localhost:{}", running.bound_addr().port());

    let binding = alien_core::bindings::ArtifactRegistryBinding::local(registry_url, None);
    let binding_json = serde_json::to_string(&binding).unwrap();
    let mut env_map: HashMap<String, String> = std::env::vars().collect();
    env_map.insert("ALIEN_ARTIFACTS_BINDING".to_string(), binding_json);
    env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());
    let bindings_provider = Arc::new(
        alien_bindings::BindingsProvider::from_env(env_map)
            .await
            .unwrap(),
    );

    let state_dir = tempfile::tempdir().unwrap();
    let db_path = state_dir.path().join("test.db");
    // A deployment in the pusher's own project, so the refused reads below have something to leak.
    let store = SqliteDeploymentStore::new(Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    ));
    let group = store
        .create_deployment_group(
            &admin(),
            CreateDeploymentGroupParams {
                name: "group".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();
    store
        .create_deployment(
            &admin(),
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "deployment".to_string(),
                deployment_group_id: group.id,
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
    let port = free_port();
    let manager_url = format!("http://127.0.0.1:{port}");
    let config = ManagerConfig {
        port,
        host: "127.0.0.1".to_string(),
        db_path: Some(db_path),
        state_dir: Some(state_dir.path().to_path_buf()),
        deployment_interval_secs: 999,
        heartbeat_interval_secs: 999,
        self_heartbeat_interval_secs: 999,
        otlp_endpoint: None,
        base_url: Some(manager_url.clone()),
        releases_url: None,
        targets: vec![],
        supported_aws_regions: Vec::new(),
        supports_aws_setup_node_identity: false,
        disable_deployment_loop: true,
        disable_heartbeat_loop: true,
        enable_local_log_ingest: false,
        allowed_origins: None,
        response_signing_key: b"test-signing-key".to_vec(),
    };
    let toml_config = alien_manager::standalone_config::ManagerTomlConfig::default();
    let manager = AlienManagerBuilder::new(config)
        .auth_validator(Arc::new(PusherValidator))
        .authz(Arc::new(PusherAuthz))
        .bindings_provider(bindings_provider)
        .with_standalone_defaults(&toml_config)
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let mut server = tokio::spawn(async move { manager.start(addr).await.unwrap() });
    let client = reqwest::Client::new();
    wait_for_health(&client, &manager_url, &mut server).await;

    // No route matches `artifacts/...` here, so the proxy attributes it to "default".
    let own = format!("{PUSHER_PREFIX}default");
    let other = format!("{PUSHER_PREFIX}prj_other");
    let repo = "artifacts/sandbox-image";

    // A real dockdash push, so any GET or HEAD it made would fail on the pull refusal.
    let layer = dockdash::Layer::builder()
        .unwrap()
        .data("usr/local/bin/agent", b"#!/bin/sh\n", None)
        .unwrap()
        .build()
        .await
        .unwrap();
    let out = tempfile::tempdir().unwrap();
    let host = format!("127.0.0.1:{port}");
    let target = format!("{host}/{repo}:v1");
    let (image, _) = dockdash::Image::builder()
        .platform("linux", &dockdash::Arch::Amd64)
        .layer(layer)
        .entrypoint(vec!["/usr/local/bin/agent".to_string()])
        .output_to(out.path().join("image.oci.tar"))
        .output_name_and_tag(&target)
        .build()
        .await
        .unwrap();
    let push = |token: String| dockdash::PushOptions {
        auth: dockdash::RegistryAuth::Bearer(token),
        protocol: dockdash::ClientProtocol::Http,
        ..Default::default()
    };

    image
        .push(&target, &push(own.clone()))
        .await
        .expect("the push-only credential must complete a real push");
    // Pushed again over layers that already exist, as a rebuild of the same image does.
    image
        .push(&target, &push(own.clone()))
        .await
        .expect("a repeat push must also complete");

    let err = image
        .push(&target, &push(other.clone()))
        .await
        .expect_err("another project's credential must not push here");
    assert!(
        err.to_string().contains("403") || err.to_string().to_lowercase().contains("denied"),
        "{err}"
    );

    let status = |req: reqwest::RequestBuilder| async move { req.send().await.unwrap().status() };
    assert_eq!(
        status(client.get(format!("{manager_url}/v2/")).bearer_auth(&own)).await,
        200
    );
    for method in [reqwest::Method::GET, reqwest::Method::HEAD] {
        for path in [
            format!("{repo}/manifests/v1"),
            format!("{repo}/blobs/sha256:{}", "0".repeat(64)),
            format!("{repo}/tags/list"),
            format!("{repo}/blobs/uploads/some-session"),
        ] {
            let got = status(
                client
                    .request(method.clone(), format!("{manager_url}/v2/{path}"))
                    .bearer_auth(&own),
            )
            .await;
            assert_eq!(got, 403, "{method} /v2/{path} must be refused, got {got}");
        }
        // `_catalog` names no repository, so it is refused for every caller.
        let got = status(
            client
                .request(method.clone(), format!("{manager_url}/v2/_catalog"))
                .bearer_auth(&own),
        )
        .await;
        assert_eq!(got, 400, "{method} /v2/_catalog must be refused, got {got}");
    }
    // Paths the router decodes into separators, or that hold an empty segment, are refused.
    for (method, path) in [
        (
            reqwest::Method::POST,
            format!(
                "{repo}/blobs/uploads/%3Fmount=sha256:{}%26from=artifacts/other",
                "0".repeat(64)
            ),
        ),
        (
            reqwest::Method::PUT,
            "artifacts/other/manifests/..%2F..%2Fsandbox-image/manifests/v2".to_string(),
        ),
        (
            reqwest::Method::GET,
            "artifacts/other/manifests/..%5C..%5Csandbox-image/manifests/v1".to_string(),
        ),
        (
            reqwest::Method::PUT,
            "artifacts//sandbox-image/manifests/v2".to_string(),
        ),
    ] {
        let got = status(
            client
                .request(method.clone(), format!("{manager_url}/v2/{path}"))
                .bearer_auth(&own),
        )
        .await;
        assert_eq!(got, 400, "{method} /v2/{path} must be refused, got {got}");
    }
    // The GAR upload-session route has no GET/HEAD handler at all.
    let got = status(
        client
            .get(format!(
                "{manager_url}/artifacts-uploads/namespaces/p/repositories/r/uploads/x"
            ))
            .bearer_auth(&own),
    )
    .await;
    assert_eq!(got, 405);

    let listed = store
        .list_deployments(&admin(), &DeploymentFilter::default())
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].project_id, "default");
    let deployments: serde_json::Value = client
        .get(format!("{manager_url}/v1/deployments"))
        .bearer_auth(&own)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(deployments["items"], serde_json::json!([]), "{deployments}");
    let got = status(
        client
            .post(format!("{manager_url}/v1/initialize"))
            .bearer_auth(&own)
            .json(&serde_json::json!({ "initialDesiredRelease": "none" })),
    )
    .await;
    // It can read no deployment, so it is told none exists.
    assert_eq!(got, 400, "the pusher must not be assigned to a deployment");
    let got = status(
        client
            .post(format!("{manager_url}/v1/image-repositories"))
            .bearer_auth(&own)
            .json(&serde_json::json!({ "projectId": "default", "platform": "aws" })),
    )
    .await;
    assert_eq!(got, 403, "the pusher must not provision a repository");

    drop(running);
}
