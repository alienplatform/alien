//! ALIEN-219: DefaultCommandDispatcher routes by the envelope's target.
//!
//! The dispatcher no longer scans the release stack for a commands-enabled
//! worker — the envelope names the exact Worker resource, and only Worker
//! targets ever reach the push path. These tests verify both invariants.

use std::sync::Arc;

use alien_commands::server::CommandDispatcher;
use alien_core::{
    BodySpec, ClientConfig, CommandTarget, CommandTargetType, Envelope, Platform,
    PresignedOperation, PresignedRequest, ResponseHandling, StackSettings, StackState,
};
use alien_error::AlienError;
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::commands::DefaultCommandDispatcher;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore};
use alien_manager::traits::deployment_store::*;

fn subject() -> Subject {
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

/// Credential resolver that must never be invoked — both dispatcher tests fail
/// before the dispatch reaches credential resolution.
struct UnusedResolver;

#[async_trait::async_trait]
impl alien_manager::traits::CredentialResolver for UnusedResolver {
    async fn resolve(&self, _deployment: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
        panic!("credential resolution must not be reached in these tests");
    }
}

async fn fresh_db() -> Arc<SqliteDatabase> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    std::mem::forget(dir);
    Arc::new(SqliteDatabase::new(path.to_str().unwrap()).await.unwrap())
}

/// Create a running AWS deployment (empty stack state) and return its id.
async fn running_deployment(store: &SqliteDeploymentStore) -> String {
    let group = store
        .create_deployment_group(
            &subject(),
            CreateDeploymentGroupParams {
                name: "g".to_string(),
                max_deployments: 10,
            },
        )
        .await
        .unwrap();

    store
        .create_with_state(
            &subject(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "dep".to_string(),
                deployment_group_id: group.id,
                platform: Platform::Aws,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(Platform::Aws),
                environment_info: None,
                runtime_metadata: Default::default(),
                status: "running".to_string(),
                current_release_id: None,
                desired_release_id: None,
                import_source: None,
                setup_metadata: None,
                setup_target: "aws".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                deployment_token: None,
                management_config: None,
                input_values: Default::default(),
            },
        )
        .await
        .unwrap()
        .id
}

fn envelope(deployment_id: &str, target: CommandTarget) -> Envelope {
    Envelope {
        protocol: "arc.v1".to_string(),
        deployment_id: deployment_id.to_string(),
        target,
        command_id: "cmd_test".to_string(),
        attempt: 1,
        trace_context: None,
        deadline: None,
        command: "sync".to_string(),
        params: BodySpec::inline(b"{}"),
        response_handling: ResponseHandling {
            max_inline_bytes: 1000,
            submit_response_url: "http://localhost/submit".to_string(),
            storage_upload_request: PresignedRequest::new_local(
                "unused".to_string(),
                PresignedOperation::Put,
                "unused".to_string(),
                chrono::Utc::now(),
            ),
        },
    }
}

#[tokio::test]
async fn non_worker_target_is_rejected_before_dispatch() {
    let db = fresh_db().await;
    let store = Arc::new(SqliteDeploymentStore::new(db));
    let dep_id = running_deployment(&store).await;

    let dispatcher = DefaultCommandDispatcher::new(store, Arc::new(UnusedResolver));

    // A Container target reaching the push dispatcher is a routing bug (they
    // are always Pull) — it must be rejected loudly, not dispatched.
    let env = envelope(
        &dep_id,
        CommandTarget::new("c1", CommandTargetType::Container),
    );
    let err = dispatcher.dispatch(&env).await.unwrap_err();
    assert_eq!(err.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        err.message.contains("pull delivery") || err.message.contains("Container"),
        "unexpected message: {}",
        err.message
    );
}

#[tokio::test]
async fn worker_target_output_lookup_is_keyed_by_envelope_target() {
    let db = fresh_db().await;
    let store = Arc::new(SqliteDeploymentStore::new(db));
    let dep_id = running_deployment(&store).await;

    let dispatcher = DefaultCommandDispatcher::new(store, Arc::new(UnusedResolver));

    // The worker id comes straight from the envelope target; with an empty
    // stack state the outputs lookup fails, and the error names that exact id
    // (proving the dispatcher no longer scans for a commands-enabled worker).
    let env = envelope(
        &dep_id,
        CommandTarget::new("worker-xyz", CommandTargetType::Worker),
    );
    let err = dispatcher.dispatch(&env).await.unwrap_err();
    assert!(
        err.message.contains("worker-xyz"),
        "error should name the envelope's target worker, got: {}",
        err.message
    );
}
