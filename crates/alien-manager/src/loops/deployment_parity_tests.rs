//! A push deployment of a stack without extensions, run through the manager's deployment loop:
//! what the loop saves is the same whether the manager can step extension controllers or not.
//!
//! With `ALIEN_PARITY_DIR` set, the saved deployment is also written there, with its ids and
//! times replaced, so a run on another revision of the manager can be compared byte for byte.

use super::{DeploymentLoop, ProcessOptions};
use crate::auth::Subject;
use crate::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore};
use crate::traits::deployment_store::{CreateDeploymentParams, DeploymentFilter, SuppliedStacks};
use crate::traits::{CredentialResolver, DeploymentStore, ResolvedCredentials, ServerBindings};
use alien_bindings::providers::{kv::local::LocalKv, storage::local::LocalStorage};
use alien_commands::{dispatchers::NullCommandDispatcher, InMemoryCommandRegistry};
use alien_core::{
    Platform, Resource, ResourceEntry, ResourceLifecycle, ServiceAccount, Stack, StackSettings,
    Worker, WorkerCode,
};
use alien_error::AlienError;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

struct TestCredentials;

#[async_trait]
impl CredentialResolver for TestCredentials {
    async fn management_external_id(
        &self,
        _: &crate::traits::DeploymentRecord,
    ) -> Result<Option<alien_core::ManagementExternalId>, AlienError> {
        Ok(None)
    }

    async fn resolve(
        &self,
        _: &crate::traits::DeploymentRecord,
    ) -> Result<alien_core::ClientConfig, AlienError> {
        Ok(alien_core::ClientConfig::Test)
    }

    async fn resolve_with_capability(
        &self,
        deployment: &crate::traits::DeploymentRecord,
    ) -> Result<ResolvedCredentials, AlienError> {
        Ok(ResolvedCredentials {
            client_config: self.resolve(deployment).await?,
            has_provision_capability: true,
        })
    }
}

/// A worker and a frozen identity: a live and a frozen resource, and no extension.
fn stack() -> Stack {
    let mut stack = Stack::new("app".to_string()).build();
    let entry = |config, lifecycle| ResourceEntry {
        enabled_when: None,
        config,
        lifecycle,
        dependencies: Vec::new(),
        display_name: None,
        description: None,
        remote_access: false,
    };
    stack.resources.insert(
        "identity".to_string(),
        entry(
            Resource::new(ServiceAccount::new("identity".to_string()).build()),
            ResourceLifecycle::Frozen,
        ),
    );
    stack.resources.insert(
        "api".to_string(),
        entry(
            Resource::new(
                Worker::new("api".to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("default".to_string())
                    .build(),
            ),
            ResourceLifecycle::Live,
        ),
    );
    stack
        .permissions
        .profiles
        .insert("default".to_string(), alien_core::PermissionProfile::new());
    stack
}

/// Ids, prefixes, times and the environment's hash differ from run to run; everything else
/// must not.
fn normalized(value: Value, deployment_id: &str) -> Value {
    match value {
        Value::String(text) if text == deployment_id => Value::String("<deployment>".into()),
        Value::String(text) if chrono::DateTime::parse_from_rfc3339(&text).is_ok() => {
            Value::String("<time>".into())
        }
        Value::String(text) => Value::String(without_random_hex(&text)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| normalized(item, deployment_id))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| match key.as_str() {
                    "resourcePrefix" => (key, Value::String("<prefix>".into())),
                    // A hash of the environment, which carries the deployment id.
                    "lastSyncedEnvVarsHash" => (key, Value::String("<hash>".into())),
                    _ => (key, normalized(value, deployment_id)),
                })
                .collect(),
        ),
        other => other,
    }
}

/// `text` with each run of 32 hex digits, a random id, replaced.
fn without_random_hex(text: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_hexdigit() {
            run.push(c);
            continue;
        }
        out.push_str(if run.len() == 32 { "<id>" } else { &run });
        run.clear();
        out.push(c);
    }
    out.pop();
    out
}

/// Runs the stack through the loop of a manager that `extend` sets up, until the deployment
/// runs, and returns what the store holds of it.
async fn run(extend: impl FnOnce(DeploymentLoop) -> DeploymentLoop) -> Value {
    // The secrets digest key the manager builder installs.
    alien_deployment::set_ephemeral_secrets_digest_key();
    let temp = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDatabase::new(temp.path().join("manager.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    let store = Arc::new(SqliteDeploymentStore::new(db.clone()));
    let bindings = Arc::new(ServerBindings {
        local_queues: Arc::new(alien_local::LocalQueueService::default()),
        kv: Arc::new(LocalKv::new(temp.path().join("kv")).await.unwrap()),
        command_storage: Arc::new(
            LocalStorage::new(temp.path().join("storage").to_string_lossy().to_string()).unwrap(),
        ),
        command_dispatcher: Arc::new(NullCommandDispatcher),
        command_registry: Arc::new(InMemoryCommandRegistry::default()),
        artifact_registry: None,
        bindings_provider: None,
        target_bindings_providers: Default::default(),
        registry_routing_table: Arc::new(
            crate::routes::registry_proxy::RegistryRoutingTable::new(vec![]).unwrap(),
        ),
    });
    let deployment_loop = extend(DeploymentLoop::new(
        Arc::new(crate::config::ManagerConfig::default()),
        store.clone(),
        Arc::new(SqliteReleaseStore::new(db.clone())),
        Arc::new(TestCredentials),
        bindings,
        None,
    ));
    let caller = Subject::system();
    let id = store
        .create_deployment(
            &caller,
            CreateDeploymentParams {
                name: "plain".to_string(),
                deployment_group_id: "dg_test".to_string(),
                platform: Platform::Test,
                deployment_protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                base_platform: None,
                stack_settings: StackSettings {
                    deployment_model: alien_core::DeploymentModel::Push,
                    ..Default::default()
                },
                stack_state: None,
                environment_variables: None,
                public_subdomain: None,
                input_values: Default::default(),
                setup_item: None,
                deployment_token: None,
            },
        )
        .await
        .unwrap()
        .id;
    let filter = DeploymentFilter {
        deployment_ids: Some(vec![id.clone()]),
        ..Default::default()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let stored = store.get_deployment(&caller, &id).await.unwrap().unwrap();
        if stored.status == "running" || std::time::Instant::now() > deadline {
            let saved = serde_json::json!({
                "status": stored.status,
                "error": stored.error,
                "stackState": stored.stack_state,
                "runtimeMetadata": stored.runtime_metadata,
                "environmentInfo": stored.environment_info,
            });
            return normalized(saved, &id);
        }
        let Some(mut item) = store
            .acquire(&caller, "manager-session", &filter, 1)
            .await
            .unwrap()
            .pop()
        else {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            continue;
        };
        item.deployment.desired_release_id = Some("rel_plain".to_string());
        item.deployment.supplied_stacks = Some(SuppliedStacks {
            desired: Some(stack()),
            current: None,
        });
        deployment_loop
            .process_deployment(
                item.deployment,
                item.execution_claim,
                "manager-session",
                ProcessOptions::deployment_tick(),
            )
            .await;
    }
}

#[tokio::test]
async fn a_stack_without_extensions_runs_the_same_with_extension_controllers() {
    let with_controllers = run(|deployment_loop| {
        deployment_loop.with_extension_controllers(Some(Arc::new(
            crate::extension_controllers::ExtensionControllers::new(
                "https://manager.example.com".to_string(),
                Arc::new(ed25519_compact::KeyPair::from_seed(
                    ed25519_compact::Seed::new([9; 32]),
                )),
                Arc::new(TestCredentials),
            ),
        )))
    })
    .await;
    let without = run(|deployment_loop| deployment_loop).await;

    if let Ok(dir) = std::env::var("ALIEN_PARITY_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("deployment-loop.json"),
            serde_json::to_vec_pretty(&with_controllers).unwrap(),
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("deployment-loop-without.json"),
            serde_json::to_vec_pretty(&without).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        with_controllers["status"], "running",
        "{with_controllers:#}"
    );
    assert_eq!(with_controllers, without);
}
