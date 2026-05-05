//! Integration tests for imported deployment persistence and acquisition.
//!
//! Distribution-imported deployments — produced by the
//! `POST /v1/stack/import` endpoint — start at `provisioning` so the manager
//! can complete layer-3 lifecycle work with management credentials. The test
//! exercises the `DeploymentStore::acquire` contract directly with the same
//! `active_work_statuses` filter that the real loop uses, plus a direct
//! `get_deployment` round-trip to assert the persistence shape.

use std::sync::Arc;

use alien_core::import::data::AwsStorageImportData;
use alien_core::import::ImportContext;
use alien_core::import::ImportSourceKind;
use alien_core::{
    AwsManagementConfig, ManagementConfig, Platform, Resource, ResourceEntry, ResourceLifecycle,
    ResourceStatus, StackSettings, StackState, Storage,
};
use alien_infra::ResourceImporter;

use alien_manager::auth::Subject;
use alien_manager::stores::sqlite::{SqliteDatabase, SqliteDeploymentStore};
use alien_manager::traits::{
    CreateDeploymentGroupParams, CreateImportedDeploymentParams, DeploymentFilter, DeploymentStore,
};

/// `active_work_statuses` is private to `loops::deployment`, so mirror it
/// here. The test fails loudly (via the explicit list and the assertion that
/// "running" is excluded) if the production set ever drops `running` or
/// adds it.
fn active_work_statuses() -> Vec<String> {
    vec![
        "pending".to_string(),
        "initial-setup".to_string(),
        "provisioning".to_string(),
        "update-pending".to_string(),
        "updating".to_string(),
        "delete-pending".to_string(),
        "deleting".to_string(),
    ]
}

async fn make_deployment_store() -> (Arc<dyn DeploymentStore>, String) {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    std::mem::forget(tmp);

    let db = Arc::new(
        SqliteDatabase::new(&db_path.to_string_lossy())
            .await
            .unwrap(),
    );
    let store: Arc<dyn DeploymentStore> = Arc::new(SqliteDeploymentStore::new(db));

    let dg = store
        .create_deployment_group(
            &Subject::system(),
            CreateDeploymentGroupParams {
                name: "imported-loop-group".to_string(),
                max_deployments: 100,
            },
        )
        .await
        .unwrap();

    (store, dg.id)
}

fn make_imported_state(resource_id: &str, bucket: &str) -> StackState {
    let mut stack_state = StackState::new(Platform::Aws);

    let storage = Storage::new(resource_id.to_string()).build();
    let resource = Resource::new(storage);

    let import_data = AwsStorageImportData {
        bucket_name: bucket.to_string(),
        bucket_arn: format!("arn:aws:s3:::{}", bucket),
    };

    let resource_entry = ResourceEntry {
        config: resource.clone(),
        lifecycle: ResourceLifecycle::Frozen,
        dependencies: Vec::new(),
        remote_access: false,
    };

    let stack_settings = StackSettings::default();
    let management_config = ManagementConfig::Aws(AwsManagementConfig {
        managing_role_arn: "arn:aws:iam::123456789012:role/AlienManager".to_string(),
    });

    let ctx = ImportContext {
        resource_id,
        resource: &resource_entry,
        platform: Platform::Aws,
        region: "us-east-1",
        management_config: &management_config,
        stack_settings: &stack_settings,
    };

    // Drive the AWS storage importer end-to-end so the test exercises the
    // real `make_imported_state` plumbing instead of hand-rolling a synthetic
    // resource state. Any change to the importer (e.g. a new field on
    // AwsStorageController) shows up as a test failure here.
    let importer = alien_infra::AwsStorageImporter;
    let resource_state = importer.import(import_data, &ctx).unwrap();

    stack_state
        .resources
        .insert(resource_id.to_string(), resource_state);
    stack_state
}

#[tokio::test]
async fn imported_deployment_round_trips_through_sqlite_with_import_source() {
    let (store, dg_id) = make_deployment_store().await;

    let stack_state = make_imported_state("assets", "acme-imports");

    let created = store
        .create_with_state(
            &Subject::system(),
            CreateImportedDeploymentParams {
                name: "imported-cf-us-east-1".to_string(),
                deployment_group_id: dg_id.clone(),
                platform: Platform::Aws,
                stack_settings: StackSettings::default(),
                stack_state: stack_state.clone(),
                status: "provisioning".to_string(),
                current_release_id: None,
                import_source: Some(ImportSourceKind::CloudFormation),
                deployment_token: None,
                management_config: None,
            },
        )
        .await
        .unwrap();

    let fetched = store
        .get_deployment(&Subject::system(), &created.id)
        .await
        .unwrap()
        .expect("imported deployment must persist");

    assert_eq!(fetched.status, "provisioning");
    assert_eq!(
        fetched.import_source,
        Some(ImportSourceKind::CloudFormation)
    );
    let fetched_state = fetched
        .stack_state
        .as_ref()
        .expect("stack_state must be persisted, not nulled");
    let resource = fetched_state
        .resources
        .get("assets")
        .expect("resource id must round-trip");
    assert_eq!(resource.status, ResourceStatus::Running);
}

#[tokio::test]
async fn loop_acquire_picks_up_imported_deployments_in_provisioning_status() {
    let (store, dg_id) = make_deployment_store().await;

    let stack_state = make_imported_state("assets", "acme-imports");
    let created = store
        .create_with_state(
            &Subject::system(),
            CreateImportedDeploymentParams {
                name: "imported-cf-us-east-1".to_string(),
                deployment_group_id: dg_id,
                platform: Platform::Aws,
                stack_settings: StackSettings::default(),
                stack_state,
                status: "provisioning".to_string(),
                current_release_id: None,
                import_source: Some(ImportSourceKind::CloudFormation),
                deployment_token: None,
                management_config: None,
            },
        )
        .await
        .unwrap();

    let acquired = store
        .acquire(
            &Subject::system(),
            "test-session",
            &DeploymentFilter {
                statuses: Some(active_work_statuses()),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();

    let acquired_ids: Vec<String> = acquired.iter().map(|a| a.deployment.id.clone()).collect();
    assert!(
        acquired_ids.contains(&created.id),
        "imported deployments at provisioning status must be acquired so the \
         manager can complete layer-3 lifecycle work. acquired = {:?}",
        acquired_ids
    );
}

#[tokio::test]
async fn imported_deployment_appears_when_promoted_to_update_pending() {
    // Sanity check the inverse: once a release roll bumps the deployment to
    // update-pending, it MUST flow through the normal active-work pipeline so
    // the manager's update path can apply config changes. The skip is
    // strictly scoped to the `running` / `refresh-failed` rest states.

    let (store, dg_id) = make_deployment_store().await;

    let stack_state = make_imported_state("assets", "acme-imports");
    let created = store
        .create_with_state(
            &Subject::system(),
            CreateImportedDeploymentParams {
                name: "imported-cf-us-east-1".to_string(),
                deployment_group_id: dg_id,
                platform: Platform::Aws,
                stack_settings: StackSettings::default(),
                stack_state,
                // Force into update-pending to mirror the post-release-roll state.
                status: "update-pending".to_string(),
                current_release_id: None,
                import_source: Some(ImportSourceKind::CloudFormation),
                deployment_token: None,
                management_config: None,
            },
        )
        .await
        .unwrap();

    let acquired = store
        .acquire(
            &Subject::system(),
            "test-session",
            &DeploymentFilter {
                statuses: Some(active_work_statuses()),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();

    let acquired_ids: Vec<String> = acquired.iter().map(|a| a.deployment.id.clone()).collect();
    assert!(
        acquired_ids.contains(&created.id),
        "deployment in update-pending status MUST be acquired so the update \
         path can run preflights + executor on the imported state. \
         acquired = {:?}",
        acquired_ids
    );
}
