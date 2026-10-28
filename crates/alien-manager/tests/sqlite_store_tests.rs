//! Comprehensive tests for SQLite store implementations.
//!
//! Each test gets a fresh in-memory SQLite database with migrations run,
//! exercising store operations through the trait interfaces.

use std::collections::HashMap;
use std::sync::Arc;

use alien_core::{DeploymentModel, DeploymentState, DeploymentStatus, Platform, StackSettings};
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::stores::sqlite::{
    migrations, SqliteCommandRegistry, SqliteDatabase, SqliteDeploymentStore, SqliteReleaseStore,
    SqliteTokenStore,
};

// Command-target resolution test imports.
use alien_commands::server::CommandRegistry;
use alien_core::{CommandDeliveryMode, CommandState, CommandTarget, CommandTargetType};
use alien_core::{
    Container, ContainerCode, Daemon, DaemonCode, ResourceLifecycle, ResourceSpec, RuntimeMetadata,
    Stack, StackState, Worker, WorkerCode,
};
use alien_manager::traits::deployment_store::*;
use alien_manager::traits::release_store::*;
use alien_manager::traits::token_store::*;

/// Workspace-admin subject used by every store test. The store doesn't
/// gate on it (Authz lives at the route layer), but every trait method
/// requires one — so we plumb the same one through.
fn test_subject() -> Subject {
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

/// Create a fresh database with all migrations applied.
/// Uses a temp file because WAL mode doesn't work with `:memory:` databases.
async fn fresh_db() -> Arc<SqliteDatabase> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    // Leak the tempdir so it lives for the test duration
    std::mem::forget(dir);
    Arc::new(SqliteDatabase::new(path.to_str().unwrap()).await.unwrap())
}

/// Helper to create a deployment group and return its ID.
async fn create_test_group(store: &SqliteDeploymentStore) -> String {
    let group = store
        .create_deployment_group(
            &test_subject(),
            CreateDeploymentGroupParams {
                name: "test-group".to_string(),
                max_deployments: 100,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();
    group.id
}

/// Helper to create a minimal deployment with defaults.
async fn create_test_deployment(
    store: &SqliteDeploymentStore,
    group_id: &str,
    name: &str,
    platform: Platform,
) -> DeploymentRecord {
    create_test_deployment_with_settings(store, group_id, name, platform, StackSettings::default())
        .await
}

async fn create_test_deployment_with_settings(
    store: &SqliteDeploymentStore,
    group_id: &str,
    name: &str,
    platform: Platform,
    stack_settings: StackSettings,
) -> DeploymentRecord {
    store
        .create_deployment(
            &test_subject(),
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: name.to_string(),
                deployment_group_id: group_id.to_string(),
                platform,
                base_platform: None,
                stack_settings,
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
}

fn test_deployment_state(
    status: DeploymentStatus,
    stack_state: Option<StackState>,
) -> DeploymentState {
    DeploymentState {
        status,
        platform: Platform::Aws,
        current_release: None,
        target_release: None,
        stack_state,
        error: None,
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

fn test_reconcile_data(
    deployment_id: &str,
    session: &str,
    status: DeploymentStatus,
    stack_state: Option<StackState>,
) -> ReconcileData {
    ReconcileData {
        deployment_id: deployment_id.to_string(),
        session: session.to_string(),
        execution_claim: None,
        state: test_deployment_state(status, stack_state),
        update_heartbeat: false,
        suggested_delay_ms: None,
        heartbeats: vec![],
        observed_inventory_batches: vec![],
        capabilities: vec![],
        operator_version: None,
        operations_report: None,
    }
}

#[test]
fn reconcile_input_adds_an_optional_image_receipt_without_expanding_reconcile_data() {
    let data = test_reconcile_data(
        "dep_compatibility",
        "session",
        DeploymentStatus::Running,
        None,
    );
    let digest = format!("sha256:{}", "a".repeat(64));
    let report = alien_core::sync::OperatorImageReport {
        source: alien_core::sync::OperatorImageSource::Configured,
        package_id: None,
        package_version: None,
        image: format!("registry.example.test/operator@{digest}"),
        digest,
    };

    let (data, received_report) = ReconcileInput::builder(data)
        .operator_image(report.clone())
        .build()
        .into_parts();

    assert_eq!(data.deployment_id, "dep_compatibility");
    assert_eq!(received_report, Some(report));
}

// =============================================================================
// DeploymentStore tests
// =============================================================================

#[tokio::test]
async fn create_and_get_deployment() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let created = create_test_deployment(&store, &group_id, "my-deploy", Platform::Aws).await;

    assert!(
        created.id.starts_with("dep_"),
        "deployment ID should start with 'dep_' prefix, got: {}",
        created.id
    );
    assert_eq!(created.name, "my-deploy");
    assert_eq!(created.deployment_group_id, group_id);
    assert_eq!(created.platform, Platform::Aws);
    assert_eq!(created.status, "pending");
    assert!(!created.retry_requested);
    assert!(created.locked_by.is_none());

    // Get by ID
    let fetched = store
        .get_deployment(&test_subject(), &created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.name, "my-deploy");
    assert_eq!(fetched.platform, Platform::Aws);
    assert_eq!(fetched.status, "pending");
}

#[tokio::test]
async fn migrations_clear_only_recovered_resource_error_headlines() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let running = create_test_deployment(&store, &group_id, "running", Platform::Aws).await;
    let failed = create_test_deployment(&store, &group_id, "failed", Platform::Aws).await;
    let headline = serde_json::json!({
        "code": "DEPLOYMENT_FAILED",
        "message": "Deployment failed",
        "context": {
            "resource_errors": [],
            "total_resources": 1,
            "failed_resources": 1,
            "interrupted_resources": 0
        }
    });

    for (deployment, status) in [(&running, "running"), (&failed, "provisioning-failed")] {
        db.conn()
            .lock()
            .await
            .execute(
                "UPDATE deployments SET status = ?1, error = ?2 WHERE id = ?3",
                (status, headline.to_string(), deployment.id.as_str()),
            )
            .await
            .unwrap();
    }

    migrations::run_migrations(&db).await.unwrap();

    let running = store
        .get_deployment(&test_subject(), &running.id)
        .await
        .unwrap()
        .unwrap();
    let failed = store
        .get_deployment(&test_subject(), &failed.id)
        .await
        .unwrap()
        .unwrap();
    assert!(running.error.is_none());
    assert_eq!(failed.error, Some(headline));
}

/// Gated live resources resolve against the deployer's stored answers on
/// every reconcile, so losing them here would silently flip resources back
/// to their declared defaults.
#[tokio::test]
async fn input_values_survive_create_import_and_reimport() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let values = HashMap::from([
        ("enableAnalytics".to_string(), serde_json::json!(true)),
        ("tier".to_string(), serde_json::json!("pro")),
    ]);

    let created = store
        .create_deployment(
            &test_subject(),
            CreateDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "gated".to_string(),
                deployment_group_id: group_id.clone(),
                platform: Platform::Aws,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: None,
                environment_variables: None,
                public_subdomain: None,
                input_values: values.clone(),
                setup_item: None,
                deployment_token: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(created.input_values, values);
    let fetched = store
        .get_deployment(&test_subject(), &created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.input_values, values);

    let imported = store
        .create_with_state(
            &test_subject(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "gated-import".to_string(),
                deployment_group_id: group_id,
                platform: Platform::Aws,
                base_platform: None,
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(Platform::Aws),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                status: "provisioning".to_string(),
                current_release_id: None,
                desired_release_id: None,
                import_source: None,
                setup_metadata: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                deployment_token: None,
                management_config: None,
                input_values: values.clone(),
            },
        )
        .await
        .unwrap();
    let fetched = store
        .get_deployment(&test_subject(), &imported.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.input_values, values);

    // A re-import carries the deployer's edited answers and overwrites the
    // stored map — that's the edit surface for flipping gates.
    let edited = HashMap::from([("enableAnalytics".to_string(), serde_json::json!(false))]);
    let updated = store
        .update_imported_stack_state(
            &test_subject(),
            &imported.id,
            UpdateImportedDeploymentParams {
                stack_settings: StackSettings::default(),
                stack_state: StackState::new(Platform::Aws),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
                setup_metadata: None,
                current_release_id: None,
                setup_target: "test".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                activation_status: None,
                schedule_reconciliation: false,
                input_values: edited.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.input_values, edited);
}

#[tokio::test]
async fn list_by_status() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let dep1 = create_test_deployment(&store, &group_id, "dep-1", Platform::Aws).await;
    let dep2 = create_test_deployment(&store, &group_id, "dep-2", Platform::Aws).await;
    let _dep3 = create_test_deployment(&store, &group_id, "dep-3", Platform::Aws).await;

    // Move dep1 to "running" status via set_redeploy (which sets update-pending)
    // For a cleaner test, let's use the reconcile to set status, but that's complex.
    // Instead, use set_redeploy which sets "update-pending"
    store.set_redeploy(&test_subject(), &dep1.id).await.unwrap();
    store
        .set_delete_pending(&test_subject(), &dep2.id)
        .await
        .unwrap();

    // Filter by "pending" status
    let pending = store
        .list_deployments(
            &test_subject(),
            &DeploymentFilter {
                statuses: Some(vec!["pending".to_string()]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].name, "dep-3");

    // Filter by multiple statuses
    let mixed = store
        .list_deployments(
            &test_subject(),
            &DeploymentFilter {
                statuses: Some(vec![
                    "update-pending".to_string(),
                    "delete-pending".to_string(),
                ]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(mixed.len(), 2);
}

#[tokio::test]
async fn list_by_deployment_group() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);

    let group_a = store
        .create_deployment_group(
            &test_subject(),
            CreateDeploymentGroupParams {
                name: "group-a".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();

    let group_b = store
        .create_deployment_group(
            &test_subject(),
            CreateDeploymentGroupParams {
                name: "group-b".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();

    create_test_deployment(&store, &group_a.id, "dep-a1", Platform::Aws).await;
    create_test_deployment(&store, &group_a.id, "dep-a2", Platform::Aws).await;
    create_test_deployment(&store, &group_b.id, "dep-b1", Platform::Gcp).await;

    let group_a_deps = store
        .list_deployments(
            &test_subject(),
            &DeploymentFilter {
                deployment_group_id: Some(group_a.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(group_a_deps.len(), 2);

    let group_b_deps = store
        .list_deployments(
            &test_subject(),
            &DeploymentFilter {
                deployment_group_id: Some(group_b.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(group_b_deps.len(), 1);
    assert_eq!(group_b_deps[0].name, "dep-b1");
}

#[tokio::test]
async fn list_by_deployment_group_and_name() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);

    let group_a = store
        .create_deployment_group(
            &test_subject(),
            CreateDeploymentGroupParams {
                name: "group-a".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();

    let group_b = store
        .create_deployment_group(
            &test_subject(),
            CreateDeploymentGroupParams {
                name: "group-b".to_string(),
                max_deployments: 10,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();

    create_test_deployment(&store, &group_a.id, "api", Platform::Aws).await;
    create_test_deployment(&store, &group_a.id, "worker", Platform::Aws).await;
    create_test_deployment(&store, &group_b.id, "api", Platform::Gcp).await;

    let matches = store
        .list_deployments(
            &test_subject(),
            &DeploymentFilter {
                deployment_group_id: Some(group_a.id.clone()),
                name: Some("api".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].deployment_group_id, group_a.id);
    assert_eq!(matches[0].name, "api");
}

#[tokio::test]
async fn update_status() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // set_delete_pending
    store
        .set_delete_pending(&test_subject(), &dep.id)
        .await
        .unwrap();
    let fetched = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, "delete-pending");

    // set_delete_pending again should fail (already delete-pending)
    let result = store.set_delete_pending(&test_subject(), &dep.id).await;
    assert!(result.is_err());

    // Create another deployment for retry and redeploy
    let dep2 = create_test_deployment(&store, &group_id, "dep2", Platform::Aws).await;

    // set_retry_requested
    store
        .set_retry_requested(&test_subject(), &dep2.id)
        .await
        .unwrap();
    let fetched = store
        .get_deployment(&test_subject(), &dep2.id)
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.retry_requested);

    // set_redeploy
    let dep3 = create_test_deployment(&store, &group_id, "dep3", Platform::Aws).await;
    store.set_redeploy(&test_subject(), &dep3.id).await.unwrap();
    let fetched = store
        .get_deployment(&test_subject(), &dep3.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, "update-pending");
}

#[tokio::test]
async fn corrective_release_reaches_only_installed_automatic_setup_failures() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let mut deployments = Vec::new();
    for (name, grant) in [("automatic", true), ("manual", false)] {
        let dep = create_test_deployment_with_settings(
            &store,
            &group_id,
            name,
            Platform::Aws,
            StackSettings {
                setup_management: alien_core::SetupManagement::Automatic,
                ..Default::default()
            },
        )
        .await;
        // Report setup results through the store; only one requested grant was installed.
        store
            .reconcile(
                &test_subject(),
                ReconcileData {
                    deployment_id: dep.id.clone(),
                    session: "setup".to_string(),
                    execution_claim: None,
                    state: DeploymentState {
                        status: DeploymentStatus::InitialSetupFailed,
                        platform: Platform::Aws,
                        current_release: None,
                        target_release: None,
                        stack_state: None,
                        error: None,
                        environment_info: None,
                        runtime_metadata: Some(RuntimeMetadata {
                            automatic_setup: grant,
                            ..Default::default()
                        }),
                        retry_requested: false,
                        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
                    },
                    update_heartbeat: false,
                    heartbeats: vec![],
                    observed_inventory_batches: vec![],
                    suggested_delay_ms: None,
                    capabilities: vec![],
                    operator_version: None,
                    operations_report: None,
                },
            )
            .await
            .unwrap();
        deployments.push((dep.id, grant));
    }
    store
        .set_desired_release(&test_subject(), "corrective-release", Some(Platform::Aws))
        .await
        .unwrap();
    for (id, grant) in deployments {
        let dep = store
            .get_deployment(&test_subject(), &id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            dep.status,
            if grant {
                "update-pending"
            } else {
                "initial-setup-failed"
            }
        );
        assert_eq!(
            dep.desired_release_id.as_deref(),
            grant.then_some("corrective-release")
        );
    }
}

#[tokio::test]
async fn running_reconcile_cannot_erase_a_new_release_request() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let releases = SqliteReleaseStore::new(db);
    let group = create_test_group(&store).await;
    let mut ids = Vec::new();
    for name in ["old", "new"] {
        ids.push(
            releases
                .create_release(
                    &test_subject(),
                    CreateReleaseParams {
                        project_id: "default".to_string(),
                        stacks: HashMap::from([(
                            Platform::Local,
                            Stack::new(name.to_string()).build(),
                        )]),
                        git_commit_sha: None,
                        git_commit_ref: None,
                        git_commit_message: None,
                    },
                )
                .await
                .unwrap()
                .id,
        );
    }
    for (index, heartbeat_status) in [DeploymentStatus::Running, DeploymentStatus::RefreshFailed]
        .into_iter()
        .enumerate()
    {
        let deployment = create_test_deployment(
            &store,
            &group,
            &format!("release-race-{index}"),
            Platform::Local,
        )
        .await;
        let mut old =
            test_reconcile_data(&deployment.id, "heartbeat", DeploymentStatus::Running, None);
        old.state.platform = Platform::Local;
        old.state.current_release = Some(alien_core::ReleaseInfo {
            release_id: Some(ids[0].clone()),
            version: None,
            description: None,
            stack: Stack::new("old".to_string()).build(),
        });
        store.reconcile(&test_subject(), old.clone()).await.unwrap();
        // A heartbeat already holds the old Running state when a release arrives.
        store
            .set_desired_release(&test_subject(), &ids[1], None)
            .await
            .unwrap();
        old.state.status = heartbeat_status;
        store.reconcile(&test_subject(), old.clone()).await.unwrap();
        let pending = store
            .get_deployment(&test_subject(), &deployment.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pending.status, "update-pending");
        assert_eq!(pending.desired_release_id.as_deref(), Some(ids[1].as_str()));
        let acquired = store
            .acquire(
                &test_subject(),
                "update",
                &DeploymentFilter {
                    deployment_ids: Some(vec![deployment.id.clone()]),
                    ..Default::default()
                },
                1,
            )
            .await
            .unwrap();
        assert_eq!(acquired.len(), 1, "the update must still be scheduled");
        // The real update can complete; preserving the request must not trap Running.
        old.state.status = DeploymentStatus::Running;
        old.state.current_release.as_mut().unwrap().release_id = Some(ids[1].clone());
        let ready = store.reconcile(&test_subject(), old).await.unwrap().record;
        assert_eq!(ready.status, "running");
        assert!(ready.desired_release_id.is_none());
    }
}
#[tokio::test]
async fn new_release_reaches_failed_local_startup_without_changing_its_retry_phase() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let releases = SqliteReleaseStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let mut ids = vec![];
    for name in ["old", "new"] {
        ids.push(
            releases
                .create_release(
                    &test_subject(),
                    CreateReleaseParams {
                        project_id: "default".to_string(),
                        stacks: HashMap::from([
                            (Platform::Local, Stack::new(name.to_string()).build()),
                            (Platform::Aws, Stack::new(name.to_string()).build()),
                        ]),
                        git_commit_sha: None,
                        git_commit_ref: None,
                        git_commit_message: None,
                    },
                )
                .await
                .unwrap()
                .id,
        );
    }
    for (index, (platform, status, receives, expected)) in [
        (
            Platform::Local,
            "provisioning-failed",
            true,
            "provisioning-failed",
        ),
        (
            Platform::Local,
            "preflights-failed",
            true,
            "preflights-failed",
        ),
        (
            Platform::Local,
            "initial-setup-failed",
            true,
            "initial-setup-failed",
        ),
        (Platform::Local, "running", true, "update-pending"),
        (Platform::Local, "update-failed", true, "update-pending"),
        (Platform::Local, "refresh-failed", true, "update-pending"),
        (
            Platform::Aws,
            "provisioning-failed",
            false,
            "provisioning-failed",
        ),
        (Platform::Aws, "running", true, "update-pending"),
        (Platform::Local, "pending", false, "pending"),
        (Platform::Local, "delete-failed", false, "delete-failed"),
    ]
    .into_iter()
    .enumerate()
    {
        let deployment =
            create_test_deployment(&store, &group_id, &format!("case-{index}"), platform).await;
        let mut update = test_reconcile_data(
            &deployment.id,
            "agent-sync",
            serde_json::from_value(serde_json::json!(status)).unwrap(),
            None,
        );
        update.state.platform = platform;
        store.reconcile(&test_subject(), update).await.unwrap();
        store
            .set_deployment_desired_release(&test_subject(), &deployment.id, &ids[0])
            .await
            .unwrap();
        store
            .set_desired_release(&test_subject(), &ids[1], None)
            .await
            .unwrap();
        let found = store
            .get_deployment(&test_subject(), &deployment.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, expected, "{platform:?} {status}");
        assert_eq!(
            found.desired_release_id.as_deref(),
            Some(ids[usize::from(receives)].as_str()),
            "{platform:?} {status}"
        );
        assert!(
            !found.retry_requested,
            "release selection must not replace the explicit session retry"
        );
    }
}

#[tokio::test]
async fn acquire_filters_by_deployment_model() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let push_dep = create_test_deployment(&store, &group_id, "push-dep", Platform::Aws).await;
    let machines_dep =
        create_test_deployment(&store, &group_id, "machines-dep", Platform::Machines).await;
    let pull_dep = create_test_deployment_with_settings(
        &store,
        &group_id,
        "pull-dep",
        Platform::Aws,
        StackSettings {
            deployment_model: DeploymentModel::Pull,
            ..StackSettings::default()
        },
    )
    .await;

    let pull_acquired = store
        .acquire(
            &test_subject(),
            "pull-session",
            &DeploymentFilter {
                deployment_model: Some(DeploymentModel::Pull),
                ..DeploymentFilter::default()
            },
            10,
        )
        .await
        .unwrap();
    assert_eq!(pull_acquired.len(), 1);
    assert_eq!(pull_acquired[0].deployment.id, pull_dep.id);

    let push_acquired = store
        .acquire(
            &test_subject(),
            "push-session",
            &DeploymentFilter {
                deployment_model: Some(DeploymentModel::Push),
                ..DeploymentFilter::default()
            },
            10,
        )
        .await
        .unwrap();
    let push_ids = push_acquired
        .iter()
        .map(|acquired| acquired.deployment.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(push_ids.len(), 2);
    assert!(push_ids.contains(push_dep.id.as_str()));
    assert!(push_ids.contains(machines_dep.id.as_str()));
}

#[tokio::test]
async fn acquire_and_release() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    // Create a deployment in "pending" status (needs work)
    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Acquire should pick it up
    let acquired = store
        .acquire(
            &test_subject(),
            "session-1",
            &DeploymentFilter::default(),
            10,
        )
        .await
        .unwrap();

    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].deployment.id, dep.id);
    assert_eq!(
        acquired[0].deployment.locked_by.as_deref(),
        Some("session-1")
    );

    // Verify lock is persisted
    let fetched = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.locked_by.as_deref(), Some("session-1"));
    assert!(fetched.locked_at.is_some());

    // Release the lock
    store
        .release(&test_subject(), &dep.id, "session-1", None)
        .await
        .unwrap();

    let fetched = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert!(fetched.locked_by.is_none());
    assert!(fetched.locked_at.is_none());
}

#[tokio::test]
async fn heartbeat_acquires_refresh_failure_without_turning_it_into_a_work_retry() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group_id, "health", Platform::Aws).await;
    db.conn()
        .lock()
        .await
        .execute(
            "UPDATE deployments SET status = 'refresh-failed', retry_requested = 0 WHERE id = ?",
            [dep.id.as_str()],
        )
        .await
        .unwrap();

    for (statuses, expected_count) in [
        (vec!["refresh-failed".to_string()], 0),
        (vec!["running".to_string(), "refresh-failed".to_string()], 1),
    ] {
        let acquired = store
            .acquire(
                &test_subject(),
                "health-session",
                &DeploymentFilter {
                    statuses: Some(statuses),
                    deployment_ids: Some(vec![dep.id.clone()]),
                    ..Default::default()
                },
                1,
            )
            .await
            .unwrap();
        assert_eq!(acquired.len(), expected_count);
        if let Some(item) = acquired.first() {
            assert_eq!(item.deployment.status, "refresh-failed");
            assert!(!item.deployment.retry_requested);
        }
    }
}

#[tokio::test]
async fn acquire_does_not_pick_failed_status_without_retry_request() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    let sql = format!(
        "UPDATE deployments SET status = 'delete-failed', retry_requested = 0 WHERE id = '{}'",
        dep.id
    );
    db.conn().lock().await.execute(&sql, ()).await.unwrap();

    let acquired = store
        .acquire(
            &test_subject(),
            "session-1",
            &DeploymentFilter {
                statuses: Some(vec!["delete-failed".to_string()]),
                deployment_ids: Some(vec![dep.id.clone()]),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();

    assert!(
        acquired.is_empty(),
        "failed deployments without retry_requested must not be acquired"
    );
}

#[tokio::test]
async fn acquire_only_picks_running_when_explicitly_requested() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    let sql = format!(
        "UPDATE deployments SET status = 'running' WHERE id = '{}'",
        dep.id
    );
    db.conn().lock().await.execute(&sql, ()).await.unwrap();

    let default_acquired = store
        .acquire(
            &test_subject(),
            "default-session",
            &DeploymentFilter {
                deployment_ids: Some(vec![dep.id.clone()]),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();
    assert!(
        default_acquired.is_empty(),
        "normal deployment loop acquire must not pick stable running deployments"
    );

    let heartbeat_acquired = store
        .acquire(
            &test_subject(),
            "heartbeat-session",
            &DeploymentFilter {
                statuses: Some(vec!["running".to_string()]),
                deployment_ids: Some(vec![dep.id.clone()]),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();

    assert_eq!(heartbeat_acquired.len(), 1);
    assert_eq!(heartbeat_acquired[0].deployment.id, dep.id);
    assert_eq!(
        heartbeat_acquired[0].deployment.locked_by.as_deref(),
        Some("heartbeat-session")
    );
}

#[tokio::test]
async fn acquire_picks_failed_status_with_retry_request_once() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    let sql = format!(
        "UPDATE deployments SET status = 'delete-failed', retry_requested = 1 WHERE id = '{}'",
        dep.id
    );
    db.conn().lock().await.execute(&sql, ()).await.unwrap();

    let acquired = store
        .acquire(
            &test_subject(),
            "session-1",
            &DeploymentFilter {
                statuses: Some(vec!["delete-failed".to_string()]),
                deployment_ids: Some(vec![dep.id.clone()]),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();

    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].deployment.id, dep.id);
    assert!(
        acquired[0].deployment.retry_requested,
        "the acquired record must tell the state machine this tick is a retry"
    );

    let fetched = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !fetched.retry_requested,
        "acquisition must consume the persisted retry request"
    );
}

#[tokio::test]
async fn acquire_picks_active_status_without_retry_request() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    let acquired = store
        .acquire(
            &test_subject(),
            "session-1",
            &DeploymentFilter {
                statuses: Some(vec!["pending".to_string()]),
                deployment_ids: Some(vec![dep.id.clone()]),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();

    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].deployment.id, dep.id);
    assert!(!acquired[0].deployment.retry_requested);
}

#[tokio::test]
async fn acquire_resumes_waiting_without_retry() {
    for status in ["waiting-for-secrets", "waiting-for-machines"] {
        for explicit in [false, true] {
            let db = fresh_db().await;
            let store = SqliteDeploymentStore::new(db.clone());
            let group_id = create_test_group(&store).await;
            let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;
            db.conn()
                .lock()
                .await
                .execute(
                    "UPDATE deployments SET status = ?, retry_requested = 0 WHERE id = ?",
                    [status, dep.id.as_str()],
                )
                .await
                .unwrap();
            let acquired = store
                .acquire(
                    &test_subject(),
                    "secret-poll",
                    &DeploymentFilter {
                        statuses: explicit.then(|| vec![status.into()]),
                        deployment_ids: Some(vec![dep.id.clone()]),
                        ..Default::default()
                    },
                    1,
                )
                .await
                .unwrap();
            assert_eq!(acquired.len(), 1);
            assert_eq!(acquired[0].deployment.id, dep.id);
            assert!(!acquired[0].deployment.retry_requested);
        }
    }
}

#[tokio::test]
async fn concurrent_acquire() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let _dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // First session acquires
    let acquired_1 = store
        .acquire(
            &test_subject(),
            "session-1",
            &DeploymentFilter::default(),
            10,
        )
        .await
        .unwrap();
    assert_eq!(acquired_1.len(), 1);

    // Second session tries to acquire - should get nothing (already locked)
    let acquired_2 = store
        .acquire(
            &test_subject(),
            "session-2",
            &DeploymentFilter::default(),
            10,
        )
        .await
        .unwrap();
    assert_eq!(acquired_2.len(), 0);
}

#[tokio::test]
async fn explicit_acquire_classifies_unavailable_deployments_atomically() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;

    let available = create_test_deployment(&store, &group_id, "available", Platform::Aws).await;
    let contended = create_test_deployment(&store, &group_id, "contended", Platform::Aws).await;
    let deferred = create_test_deployment(&store, &group_id, "deferred", Platform::Aws).await;
    let wrong_status =
        create_test_deployment(&store, &group_id, "wrong-status", Platform::Aws).await;
    let wrong_platform =
        create_test_deployment(&store, &group_id, "wrong-platform", Platform::Gcp).await;
    let wrong_model = create_test_deployment_with_settings(
        &store,
        &group_id,
        "wrong-model",
        Platform::Aws,
        StackSettings {
            deployment_model: DeploymentModel::Pull,
            ..StackSettings::default()
        },
    )
    .await;

    let conn = db.conn().lock().await;
    let locked_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE deployments SET locked_by = 'other-session', locked_at = ?2 WHERE id = ?1",
        (contended.id.as_str(), locked_at.as_str()),
    )
    .await
    .unwrap();
    conn.execute(
        "UPDATE deployments SET next_step_after = '2999-01-01T00:00:00Z' WHERE id = ?",
        [deferred.id.as_str()],
    )
    .await
    .unwrap();
    conn.execute(
        "UPDATE deployments SET status = 'running' WHERE id = ?",
        [wrong_status.id.as_str()],
    )
    .await
    .unwrap();
    drop(conn);

    let result = store
        .acquire_with_reasons(
            &test_subject(),
            "diagnostic-session",
            &DeploymentFilter {
                deployment_ids: Some(vec![
                    available.id.clone(),
                    contended.id.clone(),
                    deferred.id.clone(),
                    wrong_status.id.clone(),
                    wrong_platform.id.clone(),
                    wrong_model.id.clone(),
                ]),
                statuses: Some(vec!["pending".to_string()]),
                platforms: Some(vec![Platform::Aws]),
                deployment_model: Some(DeploymentModel::Push),
                ..DeploymentFilter::default()
            },
            10,
        )
        .await
        .unwrap();

    assert_eq!(result.deployments.len(), 1);
    assert_eq!(result.deployments[0].deployment.id, available.id);

    let reasons = result
        .not_acquired
        .iter()
        .map(|item| (item.deployment_id.as_str(), item.reason))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        reasons.get(contended.id.as_str()),
        Some(&DeploymentAcquireUnavailableReason::Contended)
    );
    assert_eq!(
        reasons.get(deferred.id.as_str()),
        Some(&DeploymentAcquireUnavailableReason::Deferred)
    );
    assert_eq!(
        reasons.get(wrong_status.id.as_str()),
        Some(&DeploymentAcquireUnavailableReason::StatusMismatch)
    );
    assert_eq!(
        reasons.get(wrong_platform.id.as_str()),
        Some(&DeploymentAcquireUnavailableReason::PlatformMismatch)
    );
    assert_eq!(
        reasons.get(wrong_model.id.as_str()),
        Some(&DeploymentAcquireUnavailableReason::DeploymentModelMismatch)
    );
    assert!(result
        .not_acquired
        .iter()
        .find(|item| item.deployment_id == deferred.id)
        .and_then(|item| item.retry_after)
        .is_some());
    assert!(result
        .not_acquired
        .iter()
        .filter(|item| item.deployment_id != deferred.id)
        .all(|item| item.retry_after.is_none()));
}

#[tokio::test]
async fn explicit_acquire_reports_the_batch_limit_without_claiming_the_extra_row() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;
    let first = create_test_deployment(&store, &group_id, "first", Platform::Aws).await;
    let second = create_test_deployment(&store, &group_id, "second", Platform::Aws).await;

    let result = store
        .acquire_with_reasons(
            &test_subject(),
            "limited-session",
            &DeploymentFilter {
                deployment_ids: Some(vec![first.id.clone(), second.id.clone()]),
                ..DeploymentFilter::default()
            },
            1,
        )
        .await
        .unwrap();

    assert_eq!(result.deployments.len(), 1);
    assert_eq!(result.not_acquired.len(), 1);
    assert_eq!(
        result.not_acquired[0].reason,
        DeploymentAcquireUnavailableReason::LimitReached
    );
    assert_ne!(
        result.deployments[0].deployment.id,
        result.not_acquired[0].deployment_id
    );
}

#[tokio::test]
async fn stale_lock_broken() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Manually set a stale lock (locked_at older than 5 minutes)
    let stale_time = "2020-01-01T00:00:00+00:00";
    let lock_sql = format!(
        "UPDATE deployments SET locked_by = 'old-session', locked_at = '{}' WHERE id = '{}'",
        stale_time, dep.id
    );
    db.conn().lock().await.execute(&lock_sql, ()).await.unwrap();

    // New session should be able to acquire (stale lock gets broken)
    let acquired = store
        .acquire(
            &test_subject(),
            "new-session",
            &DeploymentFilter::default(),
            10,
        )
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);
    assert_eq!(
        acquired[0].deployment.locked_by.as_deref(),
        Some("new-session")
    );
}

#[tokio::test]
async fn reconcile_succeeds_under_other_session_lock() {
    // Pull-mode `agent_sync` reconciles state without holding the manager
    // session's lock — the manager grants reconcile authority via the route
    // layer, not via the WHERE clause. This test guards against
    // re-introducing a `LockedBy = session` predicate on `reconcile`.
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Session A holds the lock.
    let acquired = store
        .acquire(
            &test_subject(),
            "session-A",
            &DeploymentFilter::default(),
            10,
        )
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].deployment.id, dep.id);

    // Session B (e.g. `"agent-sync"`) reconciles new state without acquiring.
    let state = DeploymentState {
        status: DeploymentStatus::Running,
        platform: Platform::Aws,
        current_release: None,
        target_release: None,
        stack_state: None,
        error: None,
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };
    store
        .reconcile(
            &test_subject(),
            ReconcileData {
                deployment_id: dep.id.clone(),
                session: "agent-sync".to_string(),
                execution_claim: None,
                state,
                update_heartbeat: false,
                heartbeats: vec![],
                observed_inventory_batches: vec![],
                suggested_delay_ms: None,
                capabilities: vec![],
                operator_version: None,
                operations_report: None,
            },
        )
        .await
        .expect("reconcile must succeed even when another session holds the lock");

    // The UPDATE applied — status is the new value.
    let fetched = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.status, "running");
    // The lock is unaffected — still held by session-A.
    assert_eq!(fetched.locked_by.as_deref(), Some("session-A"));
}

#[tokio::test]
async fn stale_reconcile_preserves_runtime_delete_status_and_latest_stack_state() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    for (index, (delete_status, expected_status)) in [
        (DeploymentStatus::DeletePending, "delete-pending"),
        (DeploymentStatus::Deleting, "deleting"),
        (DeploymentStatus::DeleteFailed, "delete-failed"),
    ]
    .into_iter()
    .enumerate()
    {
        let deployment = create_test_deployment(
            &store,
            &group_id,
            &format!("delete-race-{index}"),
            Platform::Aws,
        )
        .await;
        let session = format!("session-{index}");
        let acquired = store
            .acquire(
                &test_subject(),
                &session,
                &DeploymentFilter {
                    deployment_ids: Some(vec![deployment.id.clone()]),
                    ..Default::default()
                },
                1,
            )
            .await
            .unwrap();
        assert_eq!(acquired.len(), 1);

        store
            .set_delete_pending(&test_subject(), &deployment.id)
            .await
            .expect("delete intent is accepted while the runtime step owns its lease");
        if delete_status != DeploymentStatus::DeletePending {
            store
                .reconcile(
                    &test_subject(),
                    test_reconcile_data(&deployment.id, &session, delete_status, None),
                )
                .await
                .unwrap();
        }

        let latest_stack_state = StackState::new(Platform::Aws);
        let expected_resource_prefix = latest_stack_state.resource_prefix.clone();
        store
            .reconcile(
                &test_subject(),
                test_reconcile_data(
                    &deployment.id,
                    &session,
                    DeploymentStatus::Running,
                    Some(latest_stack_state),
                ),
            )
            .await
            .unwrap();

        let fetched = store
            .get_deployment(&test_subject(), &deployment.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, expected_status);
        assert_eq!(
            fetched
                .stack_state
                .expect("stale reconcile should persist its latest stack state")
                .resource_prefix,
            expected_resource_prefix
        );
        assert_eq!(fetched.locked_by.as_deref(), Some(session.as_str()));

        store
            .release(&test_subject(), &deployment.id, &session, None)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn checkpoint_after_mid_step_erase_preserves_new_resource_and_authorized_policy() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "mid-step-erase", Platform::Aws).await;
    let session = "runtime-step";
    let mut report = test_reconcile_data(&dep.id, session, DeploymentStatus::Running, None);
    report.state.runtime_metadata = Some(RuntimeMetadata::default());
    store
        .reconcile(&test_subject(), report.clone())
        .await
        .unwrap();
    let acquired = store
        .acquire(
            &test_subject(),
            session,
            &DeploymentFilter {
                deployment_ids: Some(vec![dep.id.clone()]),
                statuses: Some(vec!["running".into()]),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);
    assert_eq!(
        acquired[0]
            .deployment
            .runtime_metadata
            .as_ref()
            .unwrap()
            .deletion_policy,
        alien_core::DeletionPolicy::Retain
    );
    store
        .set_delete_pending_with_policy(&test_subject(), &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .expect("erase is accepted during the runtime lease");

    let mut stack = StackState::new(Platform::Aws);
    let mut bucket = alien_core::StackResourceState::new_pending(
        "storage".into(),
        alien_core::Resource::new(alien_core::Storage::new("data".into()).build()),
        None,
        Vec::new(),
    );
    bucket.status = alien_core::ResourceStatus::Running;
    bucket.lifecycle = Some(alien_core::ResourceLifecycle::Live);
    bucket.internal_state = Some(serde_json::json!({
        "type": "AwsStorageController", "_controllerStateVersion": 1,
        "state": "ready", "bucketName": "mid-step-data"
    }));
    stack.resources.insert("data".into(), bucket);
    let expected_stack = serde_json::to_value(&stack).unwrap();
    report.state.stack_state = Some(stack);
    let result = store.reconcile(&test_subject(), report).await;
    let saved = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&saved.stack_state).unwrap(),
        expected_stack,
        "completed cloud work must survive the mid-step policy change: {:?}",
        result.as_ref().err()
    );
    result.expect("checkpoint with the old policy is accepted");
    assert_eq!(saved.status, "delete-pending");
    assert_eq!(saved.locked_by.as_deref(), Some(session));
    assert_eq!(
        saved.runtime_metadata.unwrap().deletion_policy,
        alien_core::DeletionPolicy::Erase
    );
}

#[tokio::test]
async fn reconcile_hands_runtime_delete_progress_to_setup_lease() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;
    let deployment =
        create_test_deployment(&store, &group_id, "delete-progress", Platform::Aws).await;

    store
        .set_delete_pending(&test_subject(), &deployment.id)
        .await
        .unwrap();

    for (status, expected_status) in [
        (DeploymentStatus::Deleting, "deleting"),
        (DeploymentStatus::DeleteFailed, "delete-failed"),
        (DeploymentStatus::Deleting, "deleting"),
        (DeploymentStatus::TeardownRequired, "teardown-required"),
        (DeploymentStatus::Deleted, "deleted"),
    ] {
        if status == DeploymentStatus::Deleted {
            assert!(
                store
                    .reconcile(
                        &test_subject(),
                        test_reconcile_data(&deployment.id, "runtime-pull", status, None,)
                    )
                    .await
                    .is_err(),
                "runtime cannot finish setup-owned cleanup"
            );
            let acquired = store
                .acquire(
                    &test_subject(),
                    "delete-session",
                    &DeploymentFilter {
                        statuses: Some(vec!["teardown-required".into()]),
                        acquire_mode: Some(DeploymentAcquireMode::SetupTeardown),
                        setup_method: Some("cli".into()),
                        max_deployment_protocol_version: Some(
                            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                        ),
                        ..Default::default()
                    },
                    1,
                )
                .await
                .unwrap();
            assert_eq!(acquired.len(), 1);
            assert_eq!(acquired[0].deployment.id, deployment.id);
        }
        store
            .reconcile(
                &test_subject(),
                test_reconcile_data(&deployment.id, "delete-session", status, None),
            )
            .await
            .unwrap();

        let fetched = store
            .get_deployment(&test_subject(), &deployment.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.status, expected_status);
    }
}

#[tokio::test]
async fn legacy_deletion_resumes_after_protocol_upgrade() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;
    let deployment =
        create_test_deployment(&store, &group_id, "legacy-delete", Platform::Aws).await;
    store
        .set_delete_pending(&test_subject(), &deployment.id)
        .await
        .unwrap();
    // A deletion already underway when the manager was upgraded keeps its old protocol.
    db.conn()
        .lock()
        .await
        .execute(
            "UPDATE deployments SET status = 'deleting', deployment_protocol_version = 2 WHERE id = ?",
            [deployment.id.as_str()],
        )
        .await
        .unwrap();

    let saved = store
        .reconcile(
            &test_subject(),
            test_reconcile_data(
                &deployment.id,
                "delete-session",
                DeploymentStatus::Deleting,
                None,
            ),
        )
        .await
        .unwrap();
    assert_eq!(saved.record.status, "deleting");
    assert_eq!(
        saved.record.deployment_protocol_version,
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn reconcile_refreshes_owned_lock_lease() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    let acquired = store
        .acquire(
            &test_subject(),
            "session-A",
            &DeploymentFilter::default(),
            10,
        )
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);

    let stale_time = "2020-01-01T00:00:00+00:00";
    let lock_sql = format!(
        "UPDATE deployments SET locked_at = '{}' WHERE id = '{}'",
        stale_time, dep.id
    );
    db.conn().lock().await.execute(&lock_sql, ()).await.unwrap();

    let state = DeploymentState {
        status: DeploymentStatus::InitialSetup,
        platform: Platform::Aws,
        current_release: None,
        target_release: None,
        stack_state: None,
        error: None,
        environment_info: None,
        runtime_metadata: None,
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };
    store
        .reconcile(
            &test_subject(),
            ReconcileData {
                deployment_id: dep.id.clone(),
                session: "session-A".to_string(),
                execution_claim: None,
                state,
                update_heartbeat: false,
                heartbeats: vec![],
                observed_inventory_batches: vec![],
                suggested_delay_ms: None,
                capabilities: vec![],
                operator_version: None,
                operations_report: None,
            },
        )
        .await
        .expect("lock owner reconcile should refresh the lock lease");

    let fetched = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.locked_by.as_deref(), Some("session-A"));
    assert!(
        fetched.locked_at.unwrap().to_rfc3339().as_str() > stale_time,
        "reconcile by the lock owner should move locked_at forward"
    );
}

#[tokio::test]
async fn renew_lease_requires_the_active_session() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;
    let deployment = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    store
        .acquire(
            &test_subject(),
            "session-A",
            &DeploymentFilter::default(),
            1,
        )
        .await
        .expect("deployment should be acquired");
    store
        .renew_lease(&test_subject(), &deployment.id, "session-A", None)
        .await
        .expect("the active session should renew its lease");

    let error = store
        .renew_lease(&test_subject(), &deployment.id, "session-B", None)
        .await
        .expect_err("a different session must not renew the lease");
    assert_eq!(error.code, "DEPLOYMENT_LOCKED");
    assert!(!error.retryable);
}

#[tokio::test]
async fn suggested_delay_defers_reacquisition_until_external_work_wakes_it() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;
    let deployment = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    store
        .acquire(
            &test_subject(),
            "session-A",
            &DeploymentFilter::default(),
            1,
        )
        .await
        .expect("deployment should be acquired");
    let mut reconcile = test_reconcile_data(
        &deployment.id,
        "session-A",
        DeploymentStatus::InitialSetup,
        None,
    );
    reconcile.suggested_delay_ms = Some(60_000);
    store
        .reconcile(&test_subject(), reconcile)
        .await
        .expect("checkpoint should persist the suggested schedule");
    store
        .release(&test_subject(), &deployment.id, "session-A", None)
        .await
        .expect("deployment lease should release");

    let delayed = store
        .acquire(
            &test_subject(),
            "session-B",
            &DeploymentFilter::default(),
            1,
        )
        .await
        .expect("acquire should succeed without returning delayed work");
    assert!(delayed.is_empty());

    store
        .set_retry_requested(&test_subject(), &deployment.id)
        .await
        .expect("external retry should wake the deployment");
    let woken = store
        .acquire(
            &test_subject(),
            "session-B",
            &DeploymentFilter::default(),
            1,
        )
        .await
        .expect("woken deployment should be acquirable");
    assert_eq!(woken.len(), 1);
}

#[tokio::test]
async fn delete_deployment() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group_id = create_test_group(&store).await;

    let dep = create_test_deployment(&store, &group_id, "dep", Platform::Aws).await;

    // Verify it exists
    assert!(store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .is_some());

    // Delete it
    store
        .delete_deployment(&test_subject(), &dep.id)
        .await
        .unwrap();

    // Verify it's gone
    assert!(store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn group_count_computed() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);

    let group = store
        .create_deployment_group(
            &test_subject(),
            CreateDeploymentGroupParams {
                name: "counted-group".to_string(),
                max_deployments: 100,
                setup: Default::default(),
            },
        )
        .await
        .unwrap();

    // Initially 0
    let fetched = store
        .get_deployment_group(&test_subject(), &group.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.deployment_count, 0);

    // Add two deployments
    let dep1 = create_test_deployment(&store, &group.id, "dep-1", Platform::Aws).await;
    create_test_deployment(&store, &group.id, "dep-2", Platform::Gcp).await;

    let fetched = store
        .get_deployment_group(&test_subject(), &group.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.deployment_count, 2);

    // Delete one
    store
        .delete_deployment(&test_subject(), &dep1.id)
        .await
        .unwrap();

    let fetched = store
        .get_deployment_group(&test_subject(), &group.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.deployment_count, 1);

    // Verify list_deployment_groups also computes counts
    let groups = store.list_deployment_groups(&test_subject()).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].deployment_count, 1);
}

#[tokio::test]
async fn deployment_not_found() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);

    let result = store
        .get_deployment(&test_subject(), "dep_nonexistent")
        .await
        .unwrap();
    assert!(result.is_none());
}

// =============================================================================
// TokenStore tests
// =============================================================================

#[tokio::test]
async fn create_and_validate_token() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    let token = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_abc".to_string(),
            key_hash: "sha256_hash_abc".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    assert!(token.id.starts_with("tok_"));
    assert_eq!(token.token_type, TokenType::Admin);
    assert_eq!(token.key_prefix, "ax_admin_abc");
    assert_eq!(token.key_hash, "sha256_hash_abc");

    // Validate by hash
    let validated = store
        .validate_token("sha256_hash_abc")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(validated.id, token.id);
    assert_eq!(validated.token_type, TokenType::Admin);
}

#[tokio::test]
async fn invalid_hash_rejected() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_abc".to_string(),
            key_hash: "correct_hash".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    let result = store.validate_token("wrong_hash").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn list_tokens() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_1".to_string(),
            key_hash: "hash_1".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    store
        .create_token(CreateTokenParams {
            token_type: TokenType::DeploymentGroup,
            key_prefix: "ax_dg_2".to_string(),
            key_hash: "hash_2".to_string(),
            deployment_group_id: Some("dg_123".to_string()),
            deployment_id: None,
        })
        .await
        .unwrap();

    let tokens = store.list_tokens().await.unwrap();
    assert_eq!(tokens.len(), 2);
}

#[tokio::test]
async fn delete_token() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    let token = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_del".to_string(),
            key_hash: "hash_del".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();

    // Verify it exists
    assert!(store.validate_token("hash_del").await.unwrap().is_some());

    // Delete it
    store.delete_token(&token.id).await.unwrap();

    // Verify it's gone
    assert!(store.validate_token("hash_del").await.unwrap().is_none());
}

#[tokio::test]
async fn delete_nonexistent_token_errors() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    let result = store.delete_token("tok_nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn token_types() {
    let db = fresh_db().await;
    let store = SqliteTokenStore::new(db);

    // Admin
    let admin = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Admin,
            key_prefix: "ax_admin_".to_string(),
            key_hash: "hash_admin".to_string(),
            deployment_group_id: None,
            deployment_id: None,
        })
        .await
        .unwrap();
    assert_eq!(admin.token_type, TokenType::Admin);

    // DeploymentGroup
    let dg = store
        .create_token(CreateTokenParams {
            token_type: TokenType::DeploymentGroup,
            key_prefix: "ax_dg_".to_string(),
            key_hash: "hash_dg".to_string(),
            deployment_group_id: Some("dg_123".to_string()),
            deployment_id: None,
        })
        .await
        .unwrap();
    assert_eq!(dg.token_type, TokenType::DeploymentGroup);
    assert_eq!(dg.deployment_group_id.as_deref(), Some("dg_123"));

    // Deployment
    let dep = store
        .create_token(CreateTokenParams {
            token_type: TokenType::Deployment,
            key_prefix: "ax_deploy_".to_string(),
            key_hash: "hash_dep".to_string(),
            deployment_group_id: Some("dg_123".to_string()),
            deployment_id: Some("dep_456".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(dep.token_type, TokenType::Deployment);
    assert_eq!(dep.deployment_id.as_deref(), Some("dep_456"));

    // All three are round-trippable through validate
    let validated_admin = store.validate_token("hash_admin").await.unwrap().unwrap();
    assert_eq!(validated_admin.token_type, TokenType::Admin);

    let validated_dg = store.validate_token("hash_dg").await.unwrap().unwrap();
    assert_eq!(validated_dg.token_type, TokenType::DeploymentGroup);

    let validated_dep = store.validate_token("hash_dep").await.unwrap().unwrap();
    assert_eq!(validated_dep.token_type, TokenType::Deployment);
}

// =============================================================================
// ReleaseStore tests
// =============================================================================

#[tokio::test]
async fn create_and_get_release() {
    let db = fresh_db().await;
    let store = SqliteReleaseStore::new(db);
    let subject = test_subject();

    let release = store
        .create_release(
            &subject,
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(
                    Platform::Aws,
                    alien_core::Stack::new("my-stack".to_string()).build(),
                )]),
                git_commit_sha: Some("abc123".to_string()),
                git_commit_ref: Some("refs/heads/main".to_string()),
                git_commit_message: Some("Initial commit".to_string()),
            },
        )
        .await
        .unwrap();

    assert!(release.id.starts_with("rel_"));
    assert_eq!(release.workspace_id, "default");
    assert_eq!(release.project_id, "default");
    assert_eq!(release.stacks.get(&Platform::Aws).unwrap().id, "my-stack");
    assert_eq!(release.git_commit_sha.as_deref(), Some("abc123"));
    assert_eq!(release.git_commit_ref.as_deref(), Some("refs/heads/main"));
    assert_eq!(
        release.git_commit_message.as_deref(),
        Some("Initial commit")
    );

    // Get by ID — round-trip through SQL must preserve every field.
    let fetched = store
        .get_release(&subject, &release.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.id, release.id);
    assert_eq!(fetched.workspace_id, "default");
    assert_eq!(fetched.project_id, "default");
    assert_eq!(fetched.stacks.get(&Platform::Aws).unwrap().id, "my-stack");
    assert_eq!(fetched.git_commit_sha.as_deref(), Some("abc123"));
    assert_eq!(fetched.git_commit_ref.as_deref(), Some("refs/heads/main"));
    assert_eq!(
        fetched.git_commit_message.as_deref(),
        Some("Initial commit")
    );
}

/// Regression: a release with several platform stacks must round-trip
/// every key. Previously the writer wrote a `platform` discriminator
/// while the reader interpreted it as legacy single-Stack format,
/// breaking every fresh release.
#[tokio::test]
async fn create_and_get_release_multi_platform() {
    let db = fresh_db().await;
    let store = SqliteReleaseStore::new(db);
    let subject = test_subject();

    let release = store
        .create_release(
            &subject,
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([
                    (
                        Platform::Aws,
                        alien_core::Stack::new("aws-stack".to_string()).build(),
                    ),
                    (
                        Platform::Gcp,
                        alien_core::Stack::new("gcp-stack".to_string()).build(),
                    ),
                    (
                        Platform::Azure,
                        alien_core::Stack::new("azure-stack".to_string()).build(),
                    ),
                ]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();

    let fetched = store
        .get_release(&subject, &release.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.stacks.len(), 3);
    assert_eq!(fetched.stacks.get(&Platform::Aws).unwrap().id, "aws-stack");
    assert_eq!(fetched.stacks.get(&Platform::Gcp).unwrap().id, "gcp-stack");
    assert_eq!(
        fetched.stacks.get(&Platform::Azure).unwrap().id,
        "azure-stack"
    );

    // get_latest_release must read it back identically.
    let latest = store.get_latest_release(&subject).await.unwrap().unwrap();
    assert_eq!(latest.id, release.id);
    assert_eq!(latest.stacks.len(), 3);
}

#[tokio::test]
async fn latest_release() {
    let db = fresh_db().await;
    let store = SqliteReleaseStore::new(db);
    let subject = test_subject();

    // No releases yet
    assert!(store.get_latest_release(&subject).await.unwrap().is_none());

    let _rel1 = store
        .create_release(
            &subject,
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(
                    Platform::Local,
                    alien_core::Stack::new("stack-v1".to_string()).build(),
                )]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: Some("first".to_string()),
            },
        )
        .await
        .unwrap();

    // Small sleep to ensure distinct timestamps
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let rel2 = store
        .create_release(
            &subject,
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(
                    Platform::Local,
                    alien_core::Stack::new("stack-v2".to_string()).build(),
                )]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: Some("second".to_string()),
            },
        )
        .await
        .unwrap();

    // Latest should be the second one
    let latest = store.get_latest_release(&subject).await.unwrap().unwrap();
    assert_eq!(latest.id, rel2.id);
    assert_eq!(latest.stacks.get(&Platform::Local).unwrap().id, "stack-v2");
}

#[tokio::test]
async fn release_not_found() {
    let db = fresh_db().await;
    let store = SqliteReleaseStore::new(db);
    let subject = test_subject();

    let result = store
        .get_release(&subject, "rel_nonexistent")
        .await
        .unwrap();
    assert!(result.is_none());
}

// =============================================================================
// SqliteCommandRegistry target resolution + migration
// =============================================================================

fn worker(id: &str, commands_enabled: bool) -> Worker {
    let b = Worker::new(id.to_string())
        .code(WorkerCode::Image {
            image: "worker:latest".to_string(),
        })
        .permissions("execution".to_string());
    if commands_enabled {
        b.commands_enabled(true).build()
    } else {
        b.build()
    }
}

fn container(id: &str, commands_enabled: bool) -> Container {
    let b = Container::new(id.to_string())
        .code(ContainerCode::Image {
            image: "container:latest".to_string(),
        })
        .cpu(ResourceSpec {
            min: "0.5".to_string(),
            desired: "1".to_string(),
        })
        .memory(ResourceSpec {
            min: "512Mi".to_string(),
            desired: "1Gi".to_string(),
        })
        .port(8080)
        .permissions("container-execution".to_string());
    if commands_enabled {
        b.commands_enabled(true).build()
    } else {
        b.build()
    }
}

fn daemon(id: &str, commands_enabled: bool) -> Daemon {
    let b = Daemon::new(id.to_string())
        .code(DaemonCode::Image {
            image: "daemon:latest".to_string(),
        })
        .permissions("daemon-execution".to_string());
    if commands_enabled {
        b.commands_enabled(true).build()
    } else {
        b.build()
    }
}

/// Build a running deployment whose current release contains `stack`, and
/// return a registry wired to the same stores plus the deployment id.
async fn registry_with_release(
    stack: Stack,
    platform: Platform,
    stack_settings: StackSettings,
) -> (SqliteCommandRegistry, String) {
    let db = fresh_db().await;
    let dep_store = Arc::new(SqliteDeploymentStore::new(db.clone()));
    let rel_store = Arc::new(SqliteReleaseStore::new(db.clone()));
    let group_id = create_test_group(&dep_store).await;

    let release = rel_store
        .create_release(
            &test_subject(),
            CreateReleaseParams {
                project_id: "default".to_string(),
                stacks: HashMap::from([(platform, stack)]),
                git_commit_sha: None,
                git_commit_ref: None,
                git_commit_message: None,
            },
        )
        .await
        .unwrap();

    let dep = dep_store
        .create_with_state(
            &test_subject(),
            CreateImportedDeploymentParams {
                deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                name: "cmd-target-dep".to_string(),
                deployment_group_id: group_id,
                platform,
                base_platform: None,
                stack_settings,
                stack_state: StackState::new(platform),
                environment_info: None,
                runtime_metadata: RuntimeMetadata::default(),
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

    let registry = SqliteCommandRegistry::new(db, dep_store, rel_store);
    (registry, dep.id)
}

#[tokio::test]
async fn resolve_target_shorthand_single_worker_push() {
    // One command-enabled worker on AWS with the default (Push) deployment
    // model resolves via shorthand and derives Push delivery.
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .add(worker("w-off", false), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let resolved = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap();
    assert_eq!(
        resolved.target,
        CommandTarget::new("w1", CommandTargetType::Worker)
    );
    assert_eq!(resolved.delivery_mode, CommandDeliveryMode::Push);
}

#[tokio::test]
async fn resolve_target_explicit_daemon_is_pull() {
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .add(daemon("d1", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let resolved = registry
        .resolve_target(&dep_id, "test-command", Some("d1"))
        .await
        .unwrap();
    assert_eq!(
        resolved.target,
        CommandTarget::new("d1", CommandTargetType::Daemon)
    );
    // Daemons are always Pull, even on a push-capable platform/model.
    assert_eq!(resolved.delivery_mode, CommandDeliveryMode::Pull);
}

#[tokio::test]
async fn resolve_target_ambiguous_is_409() {
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .add(worker("w2", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let err = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap_err();
    assert_eq!(err.code, "COMMAND_TARGET_AMBIGUOUS");
    assert_eq!(err.http_status_code, Some(409));
}

#[tokio::test]
async fn resolve_target_none_is_422() {
    let stack = Stack::new("s".to_string())
        .add(worker("w-off", false), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let err = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap_err();
    assert_eq!(err.code, "NO_COMMAND_TARGETS");
    assert_eq!(err.http_status_code, Some(422));
}

#[tokio::test]
async fn resolve_target_unknown_is_404() {
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let err = registry
        .resolve_target(&dep_id, "test-command", Some("nope"))
        .await
        .unwrap_err();
    assert_eq!(err.code, "COMMAND_TARGET_NOT_FOUND");
    assert_eq!(err.http_status_code, Some(404));

    // An explicit empty id never falls back to shorthand.
    let err = registry
        .resolve_target(&dep_id, "test-command", Some(""))
        .await
        .unwrap_err();
    assert_eq!(err.code, "COMMAND_TARGET_NOT_FOUND");
}

#[tokio::test]
async fn resolve_target_worker_on_kubernetes_is_pull() {
    // K8s manager delivery is Pull to the in-cluster operator, which then
    // pushes the command into the Worker runtime.
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Kubernetes, StackSettings::default()).await;

    let resolved = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap();
    assert_eq!(resolved.delivery_mode, CommandDeliveryMode::Pull);
}

#[tokio::test]
async fn resolve_target_worker_on_local_is_push() {
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Local, StackSettings::default()).await;

    let resolved = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap();
    assert_eq!(resolved.delivery_mode, CommandDeliveryMode::Push);
}

#[tokio::test]
async fn resolve_target_worker_pull_model_is_pull() {
    // Push-capable platform, but the stack uses the Pull deployment model.
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .build();
    let settings = StackSettings {
        deployment_model: DeploymentModel::Pull,
        ..StackSettings::default()
    };
    let (registry, dep_id) = registry_with_release(stack, Platform::Aws, settings).await;

    let resolved = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap();
    assert_eq!(resolved.delivery_mode, CommandDeliveryMode::Pull);
}

#[tokio::test]
async fn resolve_target_container_always_pull() {
    let stack = Stack::new("s".to_string())
        .add(container("c1", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let resolved = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap();
    assert_eq!(
        resolved.target,
        CommandTarget::new("c1", CommandTargetType::Container)
    );
    assert_eq!(resolved.delivery_mode, CommandDeliveryMode::Pull);
}

#[tokio::test]
async fn resolve_target_colon_id_is_invalid() {
    // A requested target id containing ':' would break the pending-index /
    // idempotency key grammar; the shared guard rejects it with a typed error
    // through the SQLite resolve path, identically to the in-memory registry.
    let stack = Stack::new("s".to_string())
        .add(worker("w1", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let err = registry
        .resolve_target(&dep_id, "test-command", Some("w1:pending:1"))
        .await
        .unwrap_err();
    assert_eq!(err.code, "COMMAND_TARGET_ID_INVALID");
    assert_eq!(err.http_status_code, Some(400));
}

// NB: the fail-fast guard for a deployment missing stack_settings is exercised
// by an in-crate unit test (`resolve_delivery_mode_fails_fast_without_stack_settings`
// in stores::sqlite::command_registry) — the `stack_settings` column is NOT NULL,
// so that invariant-violation state cannot be reached through this store's
// public API, only through the private derivation helper.

#[tokio::test]
async fn create_command_round_trips_target_columns() {
    let stack = Stack::new("s".to_string())
        .add(daemon("d1", true), ResourceLifecycle::Live)
        .build();
    let (registry, dep_id) =
        registry_with_release(stack, Platform::Aws, StackSettings::default()).await;

    let resolved = registry
        .resolve_target(&dep_id, "test-command", None)
        .await
        .unwrap();
    let expected = CommandTarget::new("d1", CommandTargetType::Daemon);
    let metadata = registry
        .create_command(
            &dep_id,
            "sync-data",
            &resolved,
            CommandState::Pending,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(metadata.target, expected);
    assert_eq!(metadata.delivery_mode, CommandDeliveryMode::Pull);

    let status = registry
        .get_command_status(&metadata.command_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.target, expected);

    let envelope = registry
        .get_command_metadata(&metadata.command_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(envelope.target, expected);
    assert_eq!(envelope.delivery_mode, CommandDeliveryMode::Pull);
}

#[tokio::test]
async fn commands_migration_is_idempotent() {
    let db = fresh_db().await;
    // `SqliteDatabase::new` already ran migrations once; running them again
    // (target-column ALTERs included) must succeed via duplicate-column
    // tolerance.
    migrations::run_migrations(&db).await.unwrap();
    migrations::run_migrations(&db).await.unwrap();
}

#[tokio::test]
async fn protocol_upgrade_is_durable_idempotent_and_fences_old_store_writes() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let deployment =
        create_test_deployment(&store, &group, "protocol-upgrade", Platform::Aws).await;
    // Model a persisted checkpoint from the old writer.
    db.conn()
        .lock()
        .await
        .execute(
            "UPDATE deployments SET deployment_protocol_version = 1 WHERE id = ?",
            [deployment.id.as_str()],
        )
        .await
        .unwrap();
    let report = test_reconcile_data(
        &deployment.id,
        "upgrade",
        DeploymentStatus::Provisioning,
        None,
    );
    for _ in 0..2 {
        let saved = store
            .reconcile(&test_subject(), report.clone())
            .await
            .unwrap();
        assert_eq!(
            saved.record.deployment_protocol_version,
            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        );
        assert_eq!(saved.record.status, "provisioning");
    }
    for version in [
        0,
        1,
        2,
        3,
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION + 1,
    ] {
        let mut stale = report.clone();
        stale.state.protocol_version = version;
        stale.state.status = DeploymentStatus::Deleted;
        assert!(store.reconcile(&test_subject(), stale).await.is_err());
        let saved = store
            .get_deployment(&test_subject(), &deployment.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.deployment_protocol_version,
            alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
        );
        assert_eq!(saved.status, "provisioning");
    }
    let old_actor = store
        .acquire(
            &test_subject(),
            "old-actor",
            &DeploymentFilter {
                max_deployment_protocol_version: Some(
                    alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION - 1,
                ),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert!(old_actor.is_empty());
}

#[tokio::test]
async fn deletion_policy_is_fenced_and_preserves_recovery_metadata() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "policy", Platform::Aws).await;
    db.conn().lock().await.execute(&format!("UPDATE deployments SET status = 'running', locked_by = 'active', runtime_metadata = '{{\"vaultCredentialsRevoked\":true}}' WHERE id = '{}'", dep.id), ()).await.unwrap();
    store
        .set_delete_pending(&test_subject(), &dep.id)
        .await
        .unwrap();
    let retained = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    let metadata = retained.runtime_metadata.unwrap();
    assert_eq!(metadata.deletion_policy, alien_core::DeletionPolicy::Retain);
    assert!(metadata.vault_credentials_revoked);
    assert_eq!(
        retained.deployment_protocol_version,
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
    );
    store
        .set_delete_pending_with_policy(&test_subject(), &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .expect_err("pending retain cannot become erase");
    db.conn()
        .lock()
        .await
        .execute(
            &format!(
                "UPDATE deployments SET status = 'delete-failed' WHERE id = '{}'",
                dep.id
            ),
            (),
        )
        .await
        .unwrap();
    store
        .set_delete_pending_with_policy(&test_subject(), &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .expect_err("failed retain retries cannot change policy");
    db.conn()
        .lock()
        .await
        .execute(
            &format!(
                "UPDATE deployments SET status = 'retention-required', locked_by = NULL WHERE id = '{}'",
                dep.id
            ),
            (),
        )
        .await
        .unwrap();
    store
        .set_delete_pending_with_policy(&test_subject(), &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .expect("explicit erase after retention handoff");
    let erased = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(erased.status, "delete-pending");
    let metadata = erased.runtime_metadata.unwrap();
    assert_eq!(metadata.deletion_policy, alien_core::DeletionPolicy::Erase);
    assert!(metadata.vault_credentials_revoked);
}

#[tokio::test]
async fn reconcile_cannot_replace_authorized_deletion_policy() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "report-policy", Platform::Aws).await;
    store
        .set_delete_pending_with_policy(&test_subject(), &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .unwrap();
    let mut report = test_reconcile_data(&dep.id, "actor", DeploymentStatus::Deleting, None);
    report.state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
    for metadata in [
        None,
        Some(RuntimeMetadata {
            deletion_policy: alien_core::DeletionPolicy::Retain,
            ..Default::default()
        }),
    ] {
        report.state.runtime_metadata = metadata;
        store
            .reconcile(&test_subject(), report.clone())
            .await
            .expect("report progress is accepted without changing authorized policy");
        let current = store
            .get_deployment(&test_subject(), &dep.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.status, "deleting");
        assert_eq!(
            current.runtime_metadata.unwrap().deletion_policy,
            alien_core::DeletionPolicy::Erase
        );
    }
}

#[tokio::test]
async fn forgetting_keeps_installed_data_recovery_until_erased() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "recovery", Platform::Aws).await;
    let mut stack = StackState::new(Platform::Aws);
    let mut resource = alien_core::StackResourceState::new_pending(
        "storage".to_string(),
        alien_core::Resource::new(alien_core::Storage::new("data".to_string()).build()),
        Some(ResourceLifecycle::Frozen),
        Vec::new(),
    );
    resource.status = alien_core::ResourceStatus::Running;
    stack.resources.insert("data".into(), resource);
    let saved = serde_json::to_string(&stack).unwrap();
    db.conn()
        .lock()
        .await
        .execute(
            "UPDATE deployments SET stack_state = ? WHERE id = ?",
            (saved.clone(), dep.id.clone()),
        )
        .await
        .unwrap();
    store
        .delete_deployment(&test_subject(), &dep.id)
        .await
        .expect_err("force must not orphan an installed durable resource");
    let reloaded = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_string(&reloaded.stack_state.unwrap()).unwrap(),
        saved
    );
    stack.resources.get_mut("data").unwrap().status = alien_core::ResourceStatus::Deleted;
    db.conn().lock().await.execute(
        "UPDATE deployments SET stack_state = ?, status = 'deleted', locked_by = 'finishing-actor' WHERE id = ?",
        (serde_json::to_string(&stack).unwrap(), dep.id.clone()),
    ).await.unwrap();
    store
        .delete_deployment(&test_subject(), &dep.id)
        .await
        .unwrap();
    assert!(store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn legacy_teardown_retain_request_fences_old_actors() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "legacy-retain", Platform::Aws).await;
    db.conn().lock().await.execute(
        "UPDATE deployments SET status = 'teardown-required', deployment_protocol_version = 1 WHERE id = ?",
        [dep.id.clone()],
    ).await.unwrap();
    let old = store
        .acquire(
            &test_subject(),
            "old",
            &DeploymentFilter {
                statuses: Some(vec!["teardown-required".to_string()]),
                max_deployment_protocol_version: Some(3),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert!(
        old.is_empty(),
        "legacy persisted deletion also requires a new actor"
    );
    let mut old_report = test_reconcile_data(&dep.id, "old", DeploymentStatus::Deleted, None);
    old_report.state.protocol_version = 1;
    assert!(store.reconcile(&test_subject(), old_report).await.is_err());
    store
        .set_delete_pending(&test_subject(), &dep.id)
        .await
        .unwrap();
    let saved = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, "delete-pending");
    assert_eq!(
        saved.deployment_protocol_version,
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
    );
    assert_eq!(
        saved.runtime_metadata.unwrap().deletion_policy,
        alien_core::DeletionPolicy::Retain
    );
    let old = store
        .acquire(
            &test_subject(),
            "old",
            &DeploymentFilter {
                max_deployment_protocol_version: Some(3),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert!(
        old.is_empty(),
        "old actors cannot acquire the retained inventory"
    );
}

#[tokio::test]
async fn concurrent_erase_cannot_be_overwritten_by_a_stale_pull_report() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "pull-policy-race", Platform::Aws).await;
    store
        .set_delete_pending(&test_subject(), &dep.id)
        .await
        .unwrap();
    let mut report = test_reconcile_data(&dep.id, "pull", DeploymentStatus::Deleting, None);
    report.state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
    let subject = test_subject();
    let mut reconcile = Box::pin(store.reconcile(&subject, report));
    // Queue the initial read first, then the competing write. The connection
    // mutex is FIFO: the report's eventual UPDATE runs after erase is saved.
    let held = db.conn().lock().await;
    assert!(futures::poll!(&mut reconcile).is_pending());
    let mut erase = Box::pin(async {
        let connection = db.conn().lock().await;
        connection.execute(
            "UPDATE deployments SET runtime_metadata = '{\"deletionPolicy\":\"erase\"}' WHERE id = ?",
            [dep.id.as_str()],
        ).await.unwrap();
    });
    assert!(futures::poll!(&mut erase).is_pending());
    drop(held);
    let (result, ()) = tokio::join!(reconcile, erase);
    assert!(
        result.is_err(),
        "stale retain report must reload after erase wins"
    );
    let current = store
        .get_deployment(&subject, &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.deployment_protocol_version,
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
    );
    assert_eq!(current.status, "delete-pending");
    assert_eq!(
        current.runtime_metadata.unwrap().deletion_policy,
        alien_core::DeletionPolicy::Erase
    );
}

#[tokio::test]
async fn retention_handoff_is_skipped_by_discovery_and_acquirable_for_setup() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "retain-handoff", Platform::Aws).await;
    store
        .set_delete_pending(&test_subject(), &dep.id)
        .await
        .unwrap();
    db.conn()
        .lock()
        .await
        .execute(
            "UPDATE deployments SET status = 'retention-required' WHERE id = ?",
            [dep.id.as_str()],
        )
        .await
        .unwrap();
    let mut filter = DeploymentFilter {
        statuses: Some(vec!["retention-required".into()]),
        max_deployment_protocol_version: Some(alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION),
        deployment_model: Some(DeploymentModel::Push),
        ..Default::default()
    };
    assert!(store
        .acquire(
            &test_subject(),
            "runtime",
            &DeploymentFilter {
                max_deployment_protocol_version: Some(
                    alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
                ),
                ..Default::default()
            },
            1
        )
        .await
        .unwrap()
        .is_empty());
    filter.acquire_mode = Some(DeploymentAcquireMode::SetupTeardown);
    filter.setup_method = Some("cli".into());
    let acquired = store
        .acquire(&test_subject(), "setup", &filter, 1)
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].deployment.id, dep.id);
}

#[tokio::test]
async fn setup_retention_checkpoint_survives_late_pull_reports_and_reopens_for_erase() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db);
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "setup-retain-owner", Platform::Aws).await;
    let subject = test_subject();
    store.set_delete_pending(&subject, &dep.id).await.unwrap();
    let mut stale = test_reconcile_data(&dep.id, "pull", DeploymentStatus::RetentionRequired, None);
    stale.state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
    stale.state.runtime_metadata = Some(RuntimeMetadata::default());
    store.reconcile(&subject, stale.clone()).await.unwrap();
    let acquired = store
        .acquire(
            &subject,
            "setup",
            &DeploymentFilter {
                statuses: Some(vec!["retention-required".into()]),
                acquire_mode: Some(DeploymentAcquireMode::SetupTeardown),
                setup_method: Some("cli".into()),
                max_deployment_protocol_version: Some(
                    alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                ),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert_eq!(acquired.len(), 1);
    // A setup owner must finish before a separate erase can change its policy.
    let erase = store
        .set_delete_pending_with_policy(&subject, &dep.id, alien_core::DeletionPolicy::Erase)
        .await;
    let error = erase.expect_err("erase must wait for setup cleanup");
    assert_eq!(error.code, "DEPLOYMENT_SETUP_CLEANUP_IN_PROGRESS");
    assert!(error.retryable);
    let mut cleanup = stale.clone();
    cleanup.session = "setup".into();
    cleanup
        .state
        .runtime_metadata
        .as_mut()
        .unwrap()
        .vault_credentials_revoked = true;
    store.reconcile(&subject, cleanup.clone()).await.unwrap();
    assert!(
        store.reconcile(&subject, stale.clone()).await.is_err(),
        "pull report does not own setup cleanup"
    );
    let ignored = store
        .get_deployment(&subject, &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ignored.status, "retention-required");
    assert!(ignored.runtime_metadata.unwrap().vault_credentials_revoked);
    cleanup.state.status = DeploymentStatus::Retained;
    store.reconcile(&subject, cleanup).await.unwrap();
    let ignored = store.reconcile(&subject, stale).await.unwrap().record;
    assert_eq!(ignored.status, "retained");
    assert!(ignored.runtime_metadata.unwrap().vault_credentials_revoked);
    let error = store
        .set_delete_pending_with_policy(&subject, &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .expect_err("retained still holds the setup lease until release");
    assert_eq!(error.code, "DEPLOYMENT_SETUP_CLEANUP_IN_PROGRESS");
    assert!(error.retryable);
    store
        .release(&subject, &dep.id, "setup", None)
        .await
        .unwrap();
    store
        .set_delete_pending_with_policy(&subject, &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .unwrap();
    let reopened = store
        .get_deployment(&subject, &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, "teardown-required");
    assert_eq!(
        reopened.runtime_metadata.unwrap().deletion_policy,
        alien_core::DeletionPolicy::Erase
    );
    let acquired = store
        .acquire(
            &subject,
            "setup-erase",
            &DeploymentFilter {
                statuses: Some(vec!["teardown-required".into()]),
                acquire_mode: Some(DeploymentAcquireMode::SetupTeardown),
                setup_method: Some("cli".into()),
                max_deployment_protocol_version: Some(
                    alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                ),
                ..Default::default()
            },
            1,
        )
        .await
        .unwrap();
    assert_eq!(
        acquired.len(),
        1,
        "later erase is owned by setup, whose credentials still exist"
    );
}

#[tokio::test]
async fn erase_expires_stale_setup_leases_and_survives_late_checkpoints() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let subject = test_subject();
    let group = create_test_group(&store).await;
    for status in [
        "retained",
        "retention-required",
        "teardown-required",
        "teardown-failed",
    ] {
        let dep = create_test_deployment(&store, &group, status, Platform::Aws).await;
        let mut stack = StackState::new(Platform::Aws);
        let mut resource = alien_core::StackResourceState::new_pending(
            "storage".to_string(),
            alien_core::Resource::new(alien_core::Storage::new("data".to_string()).build()),
            Some(ResourceLifecycle::Frozen),
            Vec::new(),
        );
        resource.status = alien_core::ResourceStatus::Running;
        stack.resources.insert("data".into(), resource);
        db.conn()
            .lock()
            .await
            .execute(
                "UPDATE deployments SET stack_state = ? WHERE id = ?",
                (serde_json::to_string(&stack).unwrap(), dep.id.clone()),
            )
            .await
            .unwrap();
        store.set_delete_pending(&subject, &dep.id).await.unwrap();
        db.conn().lock().await.execute(
            "UPDATE deployments SET status = ?, locked_by = 'dead-setup', locked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
            [status, dep.id.as_str()],
        ).await.unwrap();
        let error = store
            .set_delete_pending_with_policy(&subject, &dep.id, alien_core::DeletionPolicy::Erase)
            .await
            .unwrap_err();
        assert_eq!(error.code, "DEPLOYMENT_SETUP_CLEANUP_IN_PROGRESS");
        assert_eq!(error.http_status_code, Some(409));
        assert!(error.retryable);
        db.conn().lock().await.execute(
            "UPDATE deployments SET locked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-6 minutes') WHERE id = ?",
            [dep.id.as_str()],
        ).await.unwrap();
        store
            .set_delete_pending_with_policy(&subject, &dep.id, alien_core::DeletionPolicy::Erase)
            .await
            .expect("stale setup lease must not fence erase forever");
        let saved = store
            .get_deployment(&subject, &dep.id)
            .await
            .unwrap()
            .unwrap();
        let expected_status = if status == "retained" {
            "teardown-required"
        } else {
            "delete-pending"
        };
        assert_eq!(saved.status, expected_status);
        assert!(saved.locked_by.is_none());
        assert!(saved.locked_at.is_none());
        assert_eq!(
            saved.runtime_metadata.unwrap().deletion_policy,
            alien_core::DeletionPolicy::Erase
        );
        let mut late = test_reconcile_data(&dep.id, "dead-setup", DeploymentStatus::Retained, None);
        late.state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
        late.state.runtime_metadata = Some(RuntimeMetadata::default());
        late.state.stack_state = Some(StackState::new(Platform::Aws));
        for reported_status in [
            DeploymentStatus::Retained,
            DeploymentStatus::RetentionRequired,
        ] {
            late.state.status = reported_status;
            let error = store
                .reconcile(&subject, late.clone())
                .await
                .err()
                .expect("expired setup session cannot report retention over erase");
            assert_eq!(error.http_status_code, Some(400));
            let unchanged = store
                .get_deployment(&subject, &dep.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(unchanged.status, expected_status);
            assert_eq!(
                unchanged.runtime_metadata.unwrap().deletion_policy,
                alien_core::DeletionPolicy::Erase
            );
            assert_eq!(
                serde_json::to_value(unchanged.stack_state).unwrap(),
                serde_json::to_value(&stack).unwrap()
            );
        }
        if status == "retained" {
            late.state.status = DeploymentStatus::TeardownFailed;
            let error = store
                .reconcile(&subject, late)
                .await
                .err()
                .expect("expired session must lose setup ownership");
            assert!(error.message.contains("active lease"));
        }
        let saved = store
            .get_deployment(&subject, &dep.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.status, expected_status);
        assert_eq!(
            saved.runtime_metadata.unwrap().deletion_policy,
            alien_core::DeletionPolicy::Erase
        );
        assert!(saved.locked_by.is_none());
        assert!(saved.locked_at.is_none());
    }
}

#[tokio::test]
async fn erase_waits_for_each_setup_cleanup_status_to_release_its_lease() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    for status in ["retention-required", "teardown-required", "teardown-failed"] {
        let dep = create_test_deployment(&store, &group, status, Platform::Aws).await;
        store
            .set_delete_pending(&test_subject(), &dep.id)
            .await
            .unwrap();
        db.conn()
            .lock()
            .await
            .execute(
                "UPDATE deployments SET status = ? WHERE id = ?",
                [status, dep.id.as_str()],
            )
            .await
            .unwrap();
        let acquired = store
            .acquire(
                &test_subject(),
                "setup",
                &DeploymentFilter {
                    deployment_ids: Some(vec![dep.id.clone()]),
                    statuses: Some(vec![status.into()]),
                    acquire_mode: Some(DeploymentAcquireMode::SetupTeardown),
                    setup_method: Some("cli".into()),
                    max_deployment_protocol_version: Some(
                        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                    ),
                    ..Default::default()
                },
                1,
            )
            .await
            .unwrap();
        assert_eq!(acquired.len(), 1);
        let error = store
            .set_delete_pending_with_policy(
                &test_subject(),
                &dep.id,
                alien_core::DeletionPolicy::Erase,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "DEPLOYMENT_SETUP_CLEANUP_IN_PROGRESS");
        assert!(error.retryable);
        let saved = store
            .get_deployment(&test_subject(), &dep.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.status, status);
        assert_eq!(saved.locked_by.as_deref(), Some("setup"));
        assert_eq!(
            saved.runtime_metadata.unwrap().deletion_policy,
            alien_core::DeletionPolicy::Retain
        );
        store
            .release(&test_subject(), &dep.id, "setup", None)
            .await
            .unwrap();
        store
            .set_delete_pending_with_policy(
                &test_subject(),
                &dep.id,
                alien_core::DeletionPolicy::Erase,
            )
            .await
            .unwrap();
        let saved = store
            .get_deployment(&test_subject(), &dep.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.status, "delete-pending");
        assert_eq!(
            saved.runtime_metadata.unwrap().deletion_policy,
            alien_core::DeletionPolicy::Erase
        );
    }
}

#[tokio::test]
async fn erase_reopens_retained_inventory_even_if_policy_already_says_erase() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "retained-erase", Platform::Aws).await;
    store
        .set_delete_pending_with_policy(&test_subject(), &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .unwrap();
    // Older reports could leave this inconsistent state; explicit erase repairs it.
    db.conn()
        .lock()
        .await
        .execute(
            "UPDATE deployments SET status = 'retained' WHERE id = ?",
            [dep.id.as_str()],
        )
        .await
        .unwrap();
    store
        .set_delete_pending_with_policy(&test_subject(), &dep.id, alien_core::DeletionPolicy::Erase)
        .await
        .unwrap();
    let reopened = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, "teardown-required");
    assert_eq!(
        reopened.runtime_metadata.unwrap().deletion_policy,
        alien_core::DeletionPolicy::Erase
    );
}

#[tokio::test]
async fn concurrent_completed_retention_cannot_be_overwritten_by_a_pull_report() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "pull-retained-race", Platform::Aws).await;
    let subject = test_subject();
    store.set_delete_pending(&subject, &dep.id).await.unwrap();
    let mut stale = test_reconcile_data(&dep.id, "pull", DeploymentStatus::RetentionRequired, None);
    stale.state.protocol_version = alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION;
    let mut reconcile = Box::pin(store.reconcile(&subject, stale));
    let held = db.conn().lock().await;
    assert!(futures::poll!(&mut reconcile).is_pending());
    let mut complete = Box::pin(async {
        let connection = db.conn().lock().await;
        connection.execute(
            "UPDATE deployments SET status = 'retained', runtime_metadata = '{\"vaultCredentialsRevoked\":true}' WHERE id = ?",
            [dep.id.as_str()],
        ).await.unwrap();
    });
    assert!(futures::poll!(&mut complete).is_pending());
    drop(held);
    let (result, ()) = tokio::join!(reconcile, complete);
    assert!(
        result.is_err(),
        "a report that read before setup completion must reload"
    );
    let current = store
        .get_deployment(&subject, &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.status, "retained");
    assert!(current.runtime_metadata.unwrap().vault_credentials_revoked);
}

mod retention_handoff_tests {
    use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt};
    use alien_core::{
        ClientConfig, DeploymentConfig, DeploymentState, DeploymentStatus,
        EnvironmentVariablesSnapshot, ExternalBindings, Platform, ResourceLifecycle,
        ResourceStatus, RuntimeMetadata, StackSettings, StackState,
    };
    use alien_deployment::{
        installed_durable_resource_ids,
        loop_contract::{LoopOperation, LoopOutcome},
        runner::RunnerPolicy,
        setup_teardown::run_setup_teardown_after_handoff,
        transport::{DeploymentLoopTransport, StepReconcileResult},
    };
    use alien_error::AlienError;
    use alien_infra::MockPlatformServiceProvider;
    use std::sync::Arc;

    struct SetupTransport;
    #[async_trait::async_trait]
    impl DeploymentLoopTransport for SetupTransport {
        async fn reconcile_step(
            &self,
            _id: &str,
            _state: &DeploymentState,
            _config: &DeploymentConfig,
            _heartbeat: bool,
            _delay: Option<u64>,
            _heartbeats: Vec<alien_core::ResourceHeartbeat>,
            _inventory: Vec<alien_core::ObservedInventoryBatch>,
        ) -> Result<StepReconcileResult, AlienError> {
            Ok(StepReconcileResult {
                state: None,
                config: None,
            })
        }
    }
    #[test]
    fn failed_bucket_without_physical_identity_is_not_installed_data() {
        let mut stack = StackState::with_resource_prefix(Platform::Aws, "test".into());
        let mut bucket = alien_core::StackResourceState::new_pending(
            "storage".into(),
            alien_core::Resource::new(alien_core::Storage::new("data".into()).build()),
            None,
            Vec::new(),
        );
        bucket.status = ResourceStatus::ProvisionFailed;
        bucket.lifecycle = Some(ResourceLifecycle::Live);
        bucket.internal_state = Some(serde_json::json!({
            "type": "AwsStorageController", "state": "createFailed", "_controllerStateVersion": 1
        }));
        stack.resources.insert("data".into(), bucket);
        assert!(installed_durable_resource_ids(&stack).is_empty());
        // An interrupted request is cleaned up by the controller's ownership-checked
        // delete path; it is not evidence that an installed bucket holds user data.
        stack
            .resources
            .get_mut("data")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()["requestedBucketName"] = serde_json::json!("test-data");
        assert!(installed_durable_resource_ids(&stack).is_empty());
        stack
            .resources
            .get_mut("data")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()["bucketName"] = serde_json::json!("test-data");
        assert_eq!(installed_durable_resource_ids(&stack), vec!["data"]);
    }

    #[tokio::test]
    async fn setup_erase_removes_retained_live_bucket() {
        let mut stack = StackState::with_resource_prefix(Platform::Aws, "test".into());
        let mut bucket = alien_core::StackResourceState::new_pending(
            "storage".into(),
            alien_core::Resource::new(alien_core::Storage::new("data".into()).build()),
            None,
            Vec::new(),
        );
        bucket.lifecycle = Some(ResourceLifecycle::Live);
        bucket.status = ResourceStatus::Running;
        bucket.internal_state = Some(serde_json::json!({
            "type": "AwsStorageController", "_controllerStateVersion": 1,
            "state": "ready", "bucketName": "test-data"
        }));
        stack.resources.insert("data".into(), bucket);
        let mut state = DeploymentState::builder()
            .status(DeploymentStatus::TeardownRequired)
            .platform(Platform::Aws)
            .stack_state(stack)
            .runtime_metadata(RuntimeMetadata {
                deletion_policy: alien_core::DeletionPolicy::Erase,
                ..Default::default()
            })
            .retry_requested(false)
            .protocol_version(alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION)
            .build();
        let mut s3 = alien_aws_clients::s3::MockS3Api::new();
        s3.expect_get_bucket_tagging().times(1).returning(|name| {
            assert_eq!(name, "test-data");
            Ok(alien_core::standard_resource_tags("test", "data"))
        });
        s3.expect_empty_bucket().times(1).returning(|name| {
            assert_eq!(name, "test-data");
            Ok(())
        });
        s3.expect_delete_bucket().times(1).returning(|name| {
            assert_eq!(name, "test-data");
            Ok(())
        });
        let s3 = Arc::new(s3);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_s3_client()
            .returning(move |_| Ok(s3.clone()));
        let mut config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();
        let result = run_setup_teardown_after_handoff(
            &mut state,
            &mut config,
            &ClientConfig::Aws(Box::new(AwsClientConfig::mock())),
            "test",
            &RunnerPolicy {
                operation: LoopOperation::Delete,
                ..Default::default()
            },
            &SetupTransport,
            Some(Arc::new(provider)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.loop_result.outcome, LoopOutcome::Success);
        assert_eq!(state.status, DeploymentStatus::Deleted);
        assert_eq!(
            state.stack_state.as_ref().unwrap().resources["data"].status,
            ResourceStatus::Deleted
        );
    }
}

#[tokio::test]
async fn deletion_is_accepted_while_a_runtime_step_holds_the_lease() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "locked-delete", Platform::Aws).await;
    db.conn()
        .lock()
        .await
        .execute(
            "UPDATE deployments SET status = 'running', locked_by = 'active' WHERE id = ?",
            [dep.id.as_str()],
        )
        .await
        .unwrap();
    store
        .set_delete_pending(&test_subject(), &dep.id)
        .await
        .unwrap();
    let saved = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, "delete-pending");
    assert_eq!(saved.locked_by.as_deref(), Some("active"));
    assert_eq!(
        saved.deployment_protocol_version,
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
    );
    assert_eq!(
        saved.runtime_metadata.unwrap().deletion_policy,
        alien_core::DeletionPolicy::Retain
    );
}

#[tokio::test]
async fn deleting_a_protocol_one_gcp_installation_keeps_setup_teardown_routing() {
    let db = fresh_db().await;
    let store = SqliteDeploymentStore::new(db.clone());
    let group = create_test_group(&store).await;
    let dep = create_test_deployment(&store, &group, "legacy-gcp", Platform::Gcp).await;
    let mut stack = StackState::with_resource_prefix(Platform::Gcp, "legacy".into());
    let mut management = alien_core::StackResourceState::new_pending(
        "management".into(),
        alien_core::Resource::new(
            alien_core::RemoteStackManagement::new("management".into()).build(),
        ),
        None,
        vec![],
    );
    management.status = alien_core::ResourceStatus::Running;
    management.lifecycle = Some(ResourceLifecycle::Frozen);
    management.outputs = Some(alien_core::ResourceOutputs::new(
        alien_core::RemoteStackManagementOutputs {
            aws_permissions_boundary_arn: None,
            management_resource_id: "manager@test-project.iam.gserviceaccount.com".into(),
            access_configuration: "manager@test-project.iam.gserviceaccount.com".into(),
            legacy_remote_bindings_access: None,
        },
    ));
    stack.resources.insert("management".into(), management);
    db.conn().lock().await.execute(
        "UPDATE deployments SET status = 'running', deployment_protocol_version = 1, stack_state = ? WHERE id = ?",
        [serde_json::to_string(&stack).unwrap(), dep.id.clone()],
    ).await.unwrap();
    store
        .set_delete_pending(&test_subject(), &dep.id)
        .await
        .unwrap();
    let saved = store
        .get_deployment(&test_subject(), &dep.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.deployment_protocol_version,
        alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION
    );
    let state = DeploymentState::builder()
        .status(DeploymentStatus::DeletePending)
        .platform(Platform::Gcp)
        .protocol_version(saved.deployment_protocol_version)
        .stack_state(saved.stack_state.unwrap())
        .runtime_metadata(saved.runtime_metadata.unwrap())
        .retry_requested(false)
        .build();
    assert_eq!(
        alien_deployment::destroy_without_runtime(&state),
        Some(DeploymentStatus::TeardownRequired)
    );
}
