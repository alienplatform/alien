use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_error::AlienError;
use alien_manager::auth::{Authz, DeploymentCreateCtx, Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::traits::{
    AuthValidator, DeploymentGroupRecord, DeploymentRecord, ReleaseRecord, TelemetrySignal,
};
use alien_manager::AlienManagerBuilder;
use async_trait::async_trait;
use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const OWN: &str = "artifacts/default-prj_a";
pub const OTHER: &str = "artifacts/default-prj_b";

pub fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

pub fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// `pusher-for-<p>`: ProjectDeveloper on p. `deploy-for-<p>`: a deployment token of p.
/// `group-for-<p>`: DeploymentGroupDeployer on a group of p. Accepts Bearer, and Basic with the
/// token as password (what `docker login` sends).
struct TestValidator;

#[async_trait]
impl AuthValidator for TestValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let Some(value) = headers.get(http::header::AUTHORIZATION) else {
            return Ok(None);
        };
        let value = value.to_str().unwrap();
        let token = if let Some(t) = value.strip_prefix("Bearer ") {
            t.to_string()
        } else if let Some(b) = value.strip_prefix("Basic ") {
            let decoded = base64::engine::general_purpose::STANDARD.decode(b).unwrap();
            let decoded = String::from_utf8(decoded).unwrap();
            decoded.split_once(':').unwrap().1.to_string()
        } else {
            return Ok(None);
        };
        let (scope, role) = if let Some(p) = token.strip_prefix("pusher-for-") {
            (
                Scope::Project {
                    project_id: p.to_string(),
                },
                Role::ProjectDeveloper,
            )
        } else if let Some(p) = token.strip_prefix("deploy-for-") {
            (
                Scope::Deployment {
                    project_id: p.to_string(),
                    deployment_id: "dep_1".to_string(),
                },
                Role::DeploymentManager,
            )
        } else if let Some(p) = token.strip_prefix("group-for-") {
            (
                Scope::DeploymentGroup {
                    project_id: p.to_string(),
                    deployment_group_id: "dg_1".to_string(),
                },
                Role::DeploymentGroupDeployer,
            )
        } else {
            return Ok(None);
        };
        Ok(Some(Subject {
            kind: SubjectKind::ServiceAccount {
                id: "test".to_string(),
            },
            workspace_id: "default".to_string(),
            scope,
            role,
            bearer_token: token,
        }))
    }
}

/// Push is allowed on the caller's own project only. `OssAuthz` cannot model this: it is
/// single-project and ignores the project id.
struct TestAuthz;

impl Authz for TestAuthz {
    fn can_push_image(&self, s: &Subject, project_id: &str, _repo_name: &str) -> bool {
        match &s.scope {
            Scope::Project { project_id: pid }
            | Scope::Deployment {
                project_id: pid, ..
            }
            | Scope::DeploymentGroup {
                project_id: pid, ..
            } => pid == project_id,
            _ => false,
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

/// One request as the upstream received it: method, raw path and raw query.
#[derive(Clone, Debug)]
pub struct Reached {
    pub method: String,
    pub path: String,
    pub query: String,
}

impl Reached {
    pub fn pairs(&self) -> Vec<(String, String)> {
        url::form_urlencoded::parse(self.query.as_bytes())
            .into_owned()
            .collect()
    }

    /// Whether any `mount`, `from` or `origin` key, in any letter case, reached the upstream.
    pub fn has_mount_params(&self) -> bool {
        self.pairs().iter().any(|(k, _)| {
            ["mount", "from", "origin"]
                .iter()
                .any(|p| k.eq_ignore_ascii_case(p))
        })
    }
}

pub type Seen = Arc<Mutex<Vec<Reached>>>;

pub fn reached_since(seen: &Seen, before: usize) -> Vec<Reached> {
    seen.lock().unwrap()[before..].to_vec()
}

/// A GAR-style upload Location, returned for an upload init whose query carries `gar=<variant>`.
fn gar_location(variant: &str) -> String {
    let segment = match variant {
        "encoded-slash" => "default-prj_a%2fx",
        "encoded-backslash" => "default-prj_a%5cx",
        _ => "default-prj_a",
    };
    format!("/artifacts-uploads/namespaces/artifacts/repositories/{segment}/uploads/AJ-x_y==")
}

/// Records `/v2/` and `/artifacts-uploads/` requests and answers like a registry: 201 to a mount
/// (any key case), 202 with a session Location to an upload init (the `location` query value when
/// given), 200 otherwise. Other paths get 404, so it never passes for a manager's `/health`.
pub async fn start_upstream() -> (String, Seen) {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let app =
        axum::Router::new().fallback(move |method: axum::http::Method, uri: axum::http::Uri| {
            let recorded = recorded.clone();
            async move {
                let path = uri.path().to_string();
                if !path.starts_with("/v2/") && !path.starts_with("/artifacts-uploads/") {
                    return (
                        axum::http::StatusCode::NOT_FOUND,
                        [("Location", "/unused".to_string())],
                    );
                }
                let reached = Reached {
                    method: method.to_string(),
                    path: path.clone(),
                    query: uri.query().unwrap_or_default().to_string(),
                };
                let pairs = reached.pairs();
                recorded.lock().unwrap().push(reached);
                let gar = pairs.iter().find(|(k, _)| k == "gar").map(|(_, v)| v);
                let given = pairs.iter().find(|(k, _)| k == "location").map(|(_, v)| v);
                if pairs.iter().any(|(k, _)| k.eq_ignore_ascii_case("mount")) {
                    (
                        axum::http::StatusCode::CREATED,
                        [("Location", format!("{path}{}", digest()))],
                    )
                } else if path.ends_with("/blobs/uploads/") {
                    let location = match (given, gar) {
                        (Some(location), _) => location.clone(),
                        (None, Some(variant)) => gar_location(variant),
                        (None, None) => format!("{path}session-1"),
                    };
                    (axum::http::StatusCode::ACCEPTED, [("Location", location)])
                } else {
                    (
                        axum::http::StatusCode::OK,
                        [("Location", "/unused".to_string())],
                    )
                }
            }
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("localhost:{}", addr.port()), seen)
}

pub struct Manager {
    pub url: String,
    pub port: u16,
    _state_dir: tempfile::TempDir,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A manager whose only artifact registry is the local binding at `registry_url`.
pub async fn start_manager(registry_url: String) -> Manager {
    start_manager_with_binding(registry_url, "ALIEN_ARTIFACT_REGISTRY_BINDING").await
}

/// Like `start_manager`, with the registry binding set through `binding_env`. A binding named
/// `artifacts` registers no route, so the manager serves it with an empty routing table.
pub async fn start_manager_with_binding(registry_url: String, binding_env: &str) -> Manager {
    let binding = alien_core::bindings::ArtifactRegistryBinding::local(registry_url, None);
    let mut env_map: HashMap<String, String> = std::env::vars().collect();
    env_map.insert(
        binding_env.to_string(),
        serde_json::to_string(&binding).unwrap(),
    );
    env_map.insert("ALIEN_DEPLOYMENT_TYPE".to_string(), "local".to_string());
    let bindings_provider = Arc::new(
        alien_bindings::BindingsProvider::from_env(env_map)
            .await
            .unwrap(),
    );
    let state_dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let url = format!("http://127.0.0.1:{port}");
    let config = ManagerConfig {
        port,
        host: "127.0.0.1".to_string(),
        db_path: Some(state_dir.path().join("test.db")),
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
        enable_local_log_ingest: false,
        allowed_origins: None,
        response_signing_key: b"test-signing-key".to_vec(),
    };
    let toml_config = alien_manager::standalone_config::ManagerTomlConfig::default();
    let manager = AlienManagerBuilder::new(config)
        .auth_validator(Arc::new(TestValidator))
        .authz(Arc::new(TestAuthz))
        .bindings_provider(bindings_provider)
        .with_standalone_defaults(&toml_config)
        .await
        .unwrap()
        .build()
        .await
        .unwrap();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let server = tokio::spawn(async move { manager.start(addr).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let mut ready = false;
    for _ in 0..50 {
        // A failed bind ends the task; surface its error instead of waiting out the loop.
        if server.is_finished() {
            server.await.unwrap();
            panic!("manager on port {port} stopped");
        }
        if client
            .get(format!("{url}/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "manager on port {port} never became healthy");
    // `/health` must have been answered by this manager, not one that took the port.
    assert!(!server.is_finished(), "manager on port {port} stopped");
    Manager {
        url,
        port,
        _state_dir: state_dir,
    }
}

pub struct Response {
    pub status: u16,
    pub head: String,
    pub body: String,
}

impl Response {
    pub fn error_code(&self) -> String {
        let body: serde_json::Value = serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("error body is not JSON ({e}): {}", self.body));
        body["errors"][0]["code"].as_str().unwrap().to_string()
    }

    /// The Location header as a path and query on the manager.
    pub fn location(&self, port: u16) -> String {
        let location = self
            .head
            .lines()
            .find_map(|l| {
                l.strip_prefix("location: ")
                    .or_else(|| l.strip_prefix("Location: "))
            })
            .unwrap_or_else(|| panic!("no Location: {}", self.head))
            .trim();
        location
            .split_once(&format!("127.0.0.1:{port}"))
            .map_or(location, |(_, rest)| rest)
            .to_string()
    }
}

/// Sends the request target verbatim, so no client normalizes it first. Panics when the
/// connection closes without a status line: every request must get an answer.
pub async fn raw(
    port: u16,
    method: &str,
    target: impl AsRef<[u8]>,
    token: Option<&str>,
    body: &[u8],
) -> Response {
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let mut request = format!("{method} ").into_bytes();
    request.extend_from_slice(target.as_ref());
    request.extend_from_slice(
        format!(
            " HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    );
    request.extend_from_slice(body);
    let target = String::from_utf8_lossy(target.as_ref());
    let exchange = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream.write_all(&request).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        response
    };
    let response = tokio::time::timeout(Duration::from_secs(10), exchange)
        .await
        .unwrap_or_else(|_| panic!("{method} {target}: no response within 10s"));
    let text = String::from_utf8_lossy(&response).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("{method} {target}: no response"));
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Response {
        status,
        head: head.to_string(),
        body: body.to_string(),
    }
}

/// `raw` with a Bearer token and no body.
pub async fn send(port: u16, method: &str, target: &str, token: &str) -> Response {
    raw(port, method, target, Some(token), b"").await
}
