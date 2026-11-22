//! Who may read a deployment's vault value over HTTP. Reading needs the same access as writing.
//! The deployment's vault is a local one on disk, so a permitted read returns the stored value;
//! a refused read must stop before the credential resolver and the vault are touched.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::bindings::VaultBinding;
use alien_core::{
    ClientConfig, ManagementExternalId, Platform, Resource, ResourceStatus, StackResourceState,
    StackState, Vault,
};
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::config::ManagerConfig;
use alien_manager::providers::OssAuthz;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore};
use alien_manager::traits::{
    AuthValidator, CreateDeploymentGroupParams, CreateDeploymentParams, CredentialResolver,
    DeploymentStore,
};
use alien_manager::AlienManagerBuilder;
use async_trait::async_trait;
use axum::http::HeaderMap;

const VALUE: &str = "stored-value-1";

fn subject(token: &str, scope: Scope, role: Role, user: bool) -> Subject {
    Subject {
        kind: if user {
            SubjectKind::User {
                id: format!("user-{token}"),
                email: "user@example.com".to_string(),
                workspace_name: Some("ws-a".to_string()),
            }
        } else {
            SubjectKind::ServiceAccount {
                id: format!("sa-{token}"),
            }
        },
        workspace_id: "default".to_string(),
        scope,
        role,
        bearer_token: token.to_string(),
    }
}

struct ByToken(Vec<Subject>);

#[async_trait]
impl AuthValidator for ByToken {
    async fn validate(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<Subject>, alien_error::AlienError> {
        let token = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        Ok(self
            .0
            .iter()
            .find(|s| Some(s.bearer_token.as_str()) == token)
            .cloned())
    }
}

/// Resolves a local configuration and counts how often it was asked.
struct CountingResolver {
    state_dir: String,
    resolved: Arc<AtomicUsize>,
}

#[async_trait]
impl CredentialResolver for CountingResolver {
    async fn management_external_id(
        &self,
        _: &alien_manager::traits::DeploymentRecord,
    ) -> Result<Option<ManagementExternalId>, alien_error::AlienError> {
        Ok(None)
    }

    async fn resolve(
        &self,
        _: &alien_manager::traits::DeploymentRecord,
    ) -> Result<ClientConfig, alien_error::AlienError> {
        self.resolved.fetch_add(1, Ordering::SeqCst);
        Ok(ClientConfig::Local {
            state_directory: self.state_dir.clone(),
        })
    }
}

struct Server {
    url: String,
    deployment_id: String,
    resolved: Arc<AtomicUsize>,
    _dir: tempfile::TempDir,
}

async fn start(subjects: impl FnOnce(&str, &str) -> Vec<Subject>) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let vault_dir = dir.path().join("vault");
    let binding = VaultBinding::local("secrets", vault_dir.to_string_lossy());
    let mut stack_state = StackState::with_resource_prefix(Platform::Aws, "stack".to_string());
    stack_state.resources.insert(
        "secrets".to_string(),
        StackResourceState::builder()
            .resource_type(Vault::RESOURCE_TYPE.to_string())
            .status(ResourceStatus::Running)
            .config(Resource::new(Vault {
                id: "secrets".to_string(),
            }))
            .remote_binding_params(serde_json::to_value(&binding).unwrap())
            .dependencies(Vec::new())
            .build(),
    );

    // Seed the value through the same local vault the route will load.
    let seed = BindingsProvider::new(
        ClientConfig::Local {
            state_directory: dir.path().to_string_lossy().to_string(),
        },
        [(
            "secrets".to_string(),
            serde_json::to_value(&binding).unwrap(),
        )]
        .into(),
    )
    .unwrap();
    seed.load_vault("secrets")
        .await
        .unwrap()
        .set_secret("API_KEY", VALUE)
        .await
        .unwrap();

    let db = Arc::new(
        SqliteDatabase::new(&dir.path().join("test.db").to_string_lossy())
            .await
            .unwrap(),
    );
    let store = Arc::new(SqliteDeploymentStore::new(db));
    let system = Subject::system();
    let group = store
        .create_deployment_group(
            &system,
            CreateDeploymentGroupParams {
                name: "group".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();
    let deployment = store
        .create_deployment(
            &system,
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "customer".to_string(),
                deployment_group_id: group.id,
                platform: Platform::Aws,
                base_platform: None,
                stack_settings: Default::default(),
                stack_state: Some(stack_state),
                environment_variables: None,
                public_subdomain: None,
                input_values: Default::default(),
                setup_item: None,
                deployment_token: None,
            },
        )
        .await
        .unwrap();

    let subjects = subjects(&deployment.id, &deployment.deployment_group_id);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}");
    let config = ManagerConfig {
        port,
        host: "127.0.0.1".to_string(),
        db_path: Some(dir.path().join("test.db")),
        state_dir: Some(dir.path().to_path_buf()),
        base_url: Some(url.clone()),
        disable_deployment_loop: true,
        disable_heartbeat_loop: true,
        response_signing_key: b"test-signing-key".to_vec(),
        ..ManagerConfig::default()
    };
    let resolved = Arc::new(AtomicUsize::new(0));
    let manager = AlienManagerBuilder::new(config)
        .deployment_store(store)
        .auth_validator(Arc::new(ByToken(subjects)))
        .authz(Arc::new(OssAuthz))
        .credential_resolver(Arc::new(CountingResolver {
            state_dir: dir.path().to_string_lossy().to_string(),
            resolved: resolved.clone(),
        }))
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
                deployment_id: deployment.id,
                resolved,
                _dir: dir,
            };
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("manager did not become healthy");
}

#[tokio::test]
async fn reading_a_vault_value_needs_the_access_that_writing_it_needs() {
    let server = start(|deployment, group| {
        let project = |id: &str| Scope::Project {
            project_id: id.to_string(),
        };
        let own_group = |group: &str| Scope::DeploymentGroup {
            project_id: "default".to_string(),
            deployment_group_id: group.to_string(),
        };
        let own = |dep: &str| Scope::Deployment {
            project_id: "default".to_string(),
            deployment_id: dep.to_string(),
        };
        vec![
            subject("key-admin", Scope::Workspace, Role::WorkspaceAdmin, false),
            subject("key-member", Scope::Workspace, Role::WorkspaceMember, false),
            subject("key-viewer", Scope::Workspace, Role::WorkspaceViewer, false),
            subject("user-admin", Scope::Workspace, Role::WorkspaceAdmin, true),
            subject("user-member", Scope::Workspace, Role::WorkspaceMember, true),
            subject("user-viewer", Scope::Workspace, Role::WorkspaceViewer, true),
            subject(
                "project-dev",
                project("default"),
                Role::ProjectDeveloper,
                false,
            ),
            subject(
                "project-viewer",
                project("default"),
                Role::ProjectViewer,
                false,
            ),
            subject(
                "group-token",
                own_group(group),
                Role::DeploymentGroupDeployer,
                false,
            ),
            subject(
                "other-group-token",
                own_group("dg_other"),
                Role::DeploymentGroupDeployer,
                false,
            ),
            subject(
                "deployment-token",
                own(deployment),
                Role::DeploymentManager,
                false,
            ),
            subject(
                "deployment-viewer",
                own(deployment),
                Role::DeploymentViewer,
                false,
            ),
            subject(
                "other-deployment-token",
                own("dep_other"),
                Role::DeploymentManager,
                false,
            ),
        ]
    })
    .await;
    let readers = [
        "key-admin",
        "key-member",
        "user-admin",
        "user-member",
        "project-dev",
        "group-token",
        "deployment-token",
    ];
    let refused = [
        "key-viewer",
        "user-viewer",
        "project-viewer",
        "deployment-viewer",
        "other-group-token",
        "other-deployment-token",
    ];
    let client = reqwest::Client::new();
    let url = format!(
        "{}/v1/deployments/{}/vault/secrets/secrets/API_KEY",
        server.url, server.deployment_id
    );
    for token in refused {
        let before = server.resolved.load(Ordering::SeqCst);
        let response = client.get(&url).bearer_auth(token).send().await.unwrap();
        let status = response.status().as_u16();
        let body = response.text().await.unwrap();
        assert!(status == 403 || status == 404, "{token}: {status} {body}");
        assert!(!body.contains(VALUE), "{token} must not see the value");
        assert_eq!(
            server.resolved.load(Ordering::SeqCst),
            before,
            "{token} was refused before any credential or vault was used"
        );
    }
    for token in readers {
        let response = client.get(&url).bearer_auth(token).send().await.unwrap();
        let status = response.status().as_u16();
        let body = response.text().await.unwrap();
        assert_eq!(status, 200, "{token}: {body}");
        assert!(body.contains(VALUE), "{token}: {body}");
    }
    // The write path is unchanged: the same callers are refused there.
    for token in [
        "key-viewer",
        "user-viewer",
        "project-viewer",
        "deployment-viewer",
    ] {
        let status = client
            .put(format!(
                "{}/v1/deployments/{}/vault/secrets/secrets/API_KEY",
                server.url, server.deployment_id
            ))
            .bearer_auth(token)
            .json(&serde_json::json!({ "value": "x" }))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16();
        assert_eq!(status, 403, "{token}");
    }
}
