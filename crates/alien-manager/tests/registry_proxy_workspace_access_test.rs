//! The registry proxy grants image access by the project that owns the repository: a caller
//! reaches repositories of its own workspace's projects only, and a project-scoped caller its own
//! project only. Workspace `ws_a` owns `prj_a` and `prj_c`, workspace `ws_b` owns `prj_b`; all
//! three share one manager and one upstream registry.

#[allow(dead_code)]
#[path = "registry_proxy_validation/harness.rs"]
mod harness;

use std::collections::HashMap;
use std::sync::Arc;

use alien_core::{Platform, ReadinessProbe, ResourceLifecycle, Stack, Worker, WorkerCode};
use alien_error::AlienError;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::providers::OssAuthz;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, CreateReleaseParams,
    DeploymentStore, ReleaseStore,
};
use async_trait::async_trait;

use harness::*;

/// `prj_a` in `ws_a`: the callers' own project.
const OWN_REPO: &str = "artifacts/default-prj_a";
/// `prj_c` in `ws_a`: another project of the callers' workspace.
const SIBLING_REPO: &str = "artifacts/default-prj_c";
/// `prj_d` in `ws_a`: a project the deployment's release does not name.
const UNLISTED_REPO: &str = "artifacts/default-prj_d";
/// `prj_b` in `ws_b`.
const OTHER_WORKSPACE_REPO: &str = "artifacts/default-prj_b";

/// Tokens name their subject: `key.<ws>` is a workspace API key, `member.<ws>` a workspace member
/// user, `viewer.<ws>.<project>` a project viewer (what a telemetry query token carries),
/// `dev.<ws>.<project>` a project developer and `deploy.<ws>.<project>.<deployment>` a deployment
/// token.
struct TwoWorkspaceValidator;

#[async_trait]
impl AuthValidator for TwoWorkspaceValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<Subject>, AlienError> {
        let Some(value) = headers.get(http::header::AUTHORIZATION) else {
            return Ok(None);
        };
        let Some(token) = value.to_str().unwrap().strip_prefix("Bearer ") else {
            return Ok(None);
        };
        let parts: Vec<&str> = token.split('.').collect();
        let service_account = SubjectKind::ServiceAccount {
            id: token.to_string(),
        };
        let (kind, scope, role) = match parts.as_slice() {
            ["key", _] => (service_account, Scope::Workspace, Role::WorkspaceAdmin),
            ["member", _] => (
                SubjectKind::User {
                    id: token.to_string(),
                    email: "member@example.com".to_string(),
                    workspace_name: None,
                },
                Scope::Workspace,
                Role::WorkspaceMember,
            ),
            ["viewer", _, project] => (
                service_account,
                Scope::Project {
                    project_id: project.to_string(),
                },
                Role::ProjectViewer,
            ),
            ["dev", _, project] => (
                service_account,
                Scope::Project {
                    project_id: project.to_string(),
                },
                Role::ProjectDeveloper,
            ),
            ["deploy", _, project, deployment] => (
                service_account,
                Scope::Deployment {
                    project_id: project.to_string(),
                    deployment_id: deployment.to_string(),
                },
                Role::DeploymentManager,
            ),
            _ => return Ok(None),
        };
        Ok(Some(Subject {
            kind,
            workspace_id: parts[1].to_string(),
            scope,
            role,
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

fn worker(id: &str, image: &str) -> Worker {
    Worker::new(id.to_string())
        .code(WorkerCode::Image {
            image: image.to_string(),
        })
        .permissions("execution".to_string())
        .memory_mb(256)
        .timeout_seconds(30)
        .expect("literal Worker timeout is within supported range")
        .commands_enabled(false)
        .readiness_probe(ReadinessProbe::default())
        .build()
}

struct Setup {
    manager: Manager,
    seen: Seen,
    /// The token of a `prj_a` deployment whose release names images in `prj_c` and `prj_b`.
    deployment_token: String,
}

async fn setup() -> Setup {
    let (upstream, seen) = start_upstream().await;
    let state_dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDatabase::new(&state_dir.path().join("test.db").to_string_lossy())
            .await
            .unwrap(),
    );
    let deployments = SqliteDeploymentStore::new(db.clone());
    let releases = SqliteReleaseStore::new(db);
    let stack = Stack::new("stack".to_string())
        .add(
            worker("sibling", &format!("registry.example/{SIBLING_REPO}:v1")),
            ResourceLifecycle::Live,
        )
        .add(
            worker(
                "other-workspace",
                &format!("registry.example/{OTHER_WORKSPACE_REPO}:v1"),
            ),
            ResourceLifecycle::Live,
        )
        .build();
    let release = releases
        .create_release(
            &admin(),
            CreateReleaseParams {
                project_id: "prj_a".to_string(),
                stacks: HashMap::from([(Platform::Local, stack)]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();
    let group = deployments
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
    let deployment = deployments
        .create_deployment(
            &admin(),
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "deployment".to_string(),
                deployment_group_id: group.id,
                platform: Platform::Local,
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
    deployments
        .set_desired_release(&admin(), &release.id, Some(Platform::Local))
        .await
        .unwrap();
    let manager = start_manager_with_auth(
        upstream,
        "ALIEN_ARTIFACT_REGISTRY_BINDING",
        state_dir,
        Arc::new(TwoWorkspaceValidator),
        Arc::new(OssAuthz),
    )
    .await;
    Setup {
        manager,
        seen,
        deployment_token: format!("deploy.ws_a.prj_a.{}", deployment.id),
    }
}

fn pull_paths(repo: &str) -> Vec<String> {
    let digest = digest();
    vec![
        format!("/v2/{repo}/manifests/v1"),
        format!("/v2/{repo}/manifests/{digest}"),
        format!("/v2/{repo}/blobs/{digest}"),
        format!("/v2/{repo}/tags/list"),
        format!("/v2/{repo}/referrers/{digest}"),
    ]
}

fn push_requests(repo: &str) -> Vec<(&'static str, String)> {
    vec![
        ("POST", format!("/v2/{repo}/blobs/uploads/")),
        ("PATCH", format!("/v2/{repo}/blobs/uploads/session-1")),
        ("PUT", format!("/v2/{repo}/blobs/uploads/session-1?digest={}", digest())),
        ("PUT", format!("/v2/{repo}/manifests/v1")),
    ]
}

/// Sends the request and returns its status, with whether it reached the upstream registry.
async fn status_and_reach(setup: &Setup, method: &str, target: &str, token: &str) -> (u16, bool) {
    let before = setup.seen.lock().unwrap().len();
    let response = send(setup.manager.port, method, target, token).await;
    (
        response.status,
        !reached_since(&setup.seen, before).is_empty(),
    )
}

async fn assert_denied(setup: &Setup, method: &str, target: &str, token: &str) {
    let (status, reached) = status_and_reach(setup, method, target, token).await;
    assert_eq!(status, 403, "{token} {method} {target}");
    assert!(!reached, "{token} {method} {target} reached the upstream");
}

async fn assert_forwarded(setup: &Setup, method: &str, target: &str, token: &str) {
    let (status, reached) = status_and_reach(setup, method, target, token).await;
    assert!(
        (200..300).contains(&status),
        "{token} {method} {target}: {status}"
    );
    assert!(reached, "{token} {method} {target} did not reach the upstream");
}

/// Workspace keys, workspace users and project tokens of `ws_a` cannot read `ws_b`'s images.
#[tokio::test]
async fn pull_requires_repository_in_subject_workspace() {
    let setup = setup().await;
    for token in [
        "key.ws_a",
        "member.ws_a",
        "viewer.ws_a.prj_a",
        "dev.ws_a.prj_a",
    ] {
        for method in ["GET", "HEAD"] {
            for target in pull_paths(OTHER_WORKSPACE_REPO) {
                assert_denied(&setup, method, &target, token).await;
            }
        }
    }
}

/// Neither a workspace writer nor a project developer of `ws_a` can write `ws_b`'s images.
#[tokio::test]
async fn push_to_other_workspace_project_is_denied() {
    let setup = setup().await;
    for token in ["key.ws_a", "member.ws_a", "dev.ws_a.prj_a"] {
        for (method, target) in push_requests(OTHER_WORKSPACE_REPO) {
            assert_denied(&setup, method, &target, token).await;
        }
    }
}

/// A blob mount copies from its source repository, so a source the caller cannot pull is
/// forwarded as a plain upload.
#[tokio::test]
async fn mount_from_other_workspace_repository_is_not_forwarded() {
    let setup = setup().await;
    for token in ["key.ws_a", "member.ws_a", "dev.ws_a.prj_a"] {
        let target = format!(
            "/v2/{OWN_REPO}/blobs/uploads/?mount={}&from={}",
            digest(),
            enc(OTHER_WORKSPACE_REPO)
        );
        let before = setup.seen.lock().unwrap().len();
        let response = send(setup.manager.port, "POST", &target, token).await;
        let reached = reached_since(&setup.seen, before);
        assert_eq!(response.status, 202, "{token}: {}", response.body);
        assert_eq!(reached.len(), 1, "{token}");
        assert!(!reached[0].has_mount_params(), "{token}: {reached:?}");
    }
}

/// A project token reaches its own project only, even within its workspace.
#[tokio::test]
async fn project_scoped_caller_reaches_only_its_project() {
    let setup = setup().await;
    let token = "dev.ws_a.prj_a";
    for target in pull_paths(SIBLING_REPO) {
        assert_denied(&setup, "GET", &target, token).await;
    }
    for (method, target) in push_requests(SIBLING_REPO) {
        assert_denied(&setup, method, &target, token).await;
    }
}

/// A project viewer token, such as the one minted to query telemetry, does not pull images, not
/// even from its own project.
#[tokio::test]
async fn project_viewer_cannot_pull_images() {
    let setup = setup().await;
    for target in pull_paths(OWN_REPO) {
        assert_denied(&setup, "GET", &target, "viewer.ws_a.prj_a").await;
    }
}

/// A release that names another workspace's image does not open it to the deployment.
#[tokio::test]
async fn deployment_release_cannot_open_other_workspace_repository() {
    let setup = setup().await;
    for target in pull_paths(OTHER_WORKSPACE_REPO) {
        assert_denied(&setup, "GET", &target, &setup.deployment_token).await;
    }
}

/// Own-workspace callers keep pulling, pushing and mounting within their projects, and a
/// deployment keeps pulling its own project's images and those its release names.
#[tokio::test]
async fn own_workspace_image_access_reaches_upstream() {
    let setup = setup().await;
    for token in ["key.ws_a", "member.ws_a", "dev.ws_a.prj_a"] {
        for target in pull_paths(OWN_REPO) {
            assert_forwarded(&setup, "GET", &target, token).await;
            assert_forwarded(&setup, "HEAD", &target, token).await;
        }
        for (method, target) in push_requests(OWN_REPO) {
            assert_forwarded(&setup, method, &target, token).await;
        }
        let mount = format!(
            "/v2/{OWN_REPO}/copy/blobs/uploads/?mount={}&from={}",
            digest(),
            enc(OWN_REPO)
        );
        let before = setup.seen.lock().unwrap().len();
        let response = send(setup.manager.port, "POST", &mount, token).await;
        let reached = reached_since(&setup.seen, before);
        assert_eq!(response.status, 201, "{token}: {}", response.body);
        assert!(reached[0].has_mount_params(), "{token}: {reached:?}");
    }
    for token in ["key.ws_a", "member.ws_a"] {
        for target in pull_paths(SIBLING_REPO) {
            assert_forwarded(&setup, "GET", &target, token).await;
        }
        for (method, target) in push_requests(SIBLING_REPO) {
            assert_forwarded(&setup, method, &target, token).await;
        }
    }

    let deployment = setup.deployment_token.as_str();
    for target in pull_paths(OWN_REPO)
        .into_iter()
        .chain(pull_paths(&format!("{OWN_REPO}/api")))
        .chain(pull_paths(SIBLING_REPO))
    {
        assert_forwarded(&setup, "GET", &target, deployment).await;
    }
    for target in pull_paths(UNLISTED_REPO) {
        assert_denied(&setup, "GET", &target, deployment).await;
    }
    for (method, target) in push_requests(OWN_REPO) {
        assert_denied(&setup, method, &target, deployment).await;
    }
}
