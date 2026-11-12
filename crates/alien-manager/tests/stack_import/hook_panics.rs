//! A hook that panics during a stack import answers that import with a 500 that keeps the
//! panic's text to the server, and does not unwind the handler (which would fail `oneshot`).

use super::*;
use alien_core::import::ImportContext;
use alien_infra::AwsStorageImporter;

struct PanicsOnImport;

impl alien_core::import::ResourceImporter for PanicsOnImport {
    type ImportData = AwsStorageImportData;
    fn import(
        &self,
        _: AwsStorageImportData,
        _: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        panic!("secret import detail")
    }
}

struct PanicsOnMerge;

impl alien_core::import::ResourceImporter for PanicsOnMerge {
    type ImportData = AwsStorageImportData;
    fn import(
        &self,
        data: AwsStorageImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        alien_core::import::ResourceImporter::import(&AwsStorageImporter, data, ctx)
    }
    fn merge_reimport(
        &self,
        _: StackResourceState,
        _: StackResourceState,
        _: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        panic!("secret merge detail")
    }
}

fn on_import(r: &mut alien_infra::ImporterRegistry) {
    r.register(Storage::RESOURCE_TYPE, Platform::Aws, PanicsOnImport);
}

fn on_merge(r: &mut alien_infra::ImporterRegistry) {
    r.register(Storage::RESOURCE_TYPE, Platform::Aws, PanicsOnMerge);
}

#[tokio::test]
async fn an_importer_that_panics_answers_500_without_its_message() {
    let mut fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    fixture.state.host_registrations = alien_infra::HostRegistrations {
        importers: on_import,
        ..alien_infra::HostRegistrations::NONE
    };
    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");
    let (status, json) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json:#}");
    assert!(
        !json.to_string().contains("secret import detail"),
        "{json:#}"
    );
    // The same router keeps answering after the panic.
    let (again, _) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(again, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn a_reimport_merge_that_panics_answers_500_without_its_message() {
    let mut fixture = make_fixture(Some(stack_with_storage("assets"))).await;
    fixture.state.host_registrations = alien_infra::HostRegistrations {
        importers: on_merge,
        ..alien_infra::HostRegistrations::NONE
    };
    let body = aws_s3_import_request("acme-prod", "us-east-1", "assets", "acme-imports");
    let (s1, j1) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(s1, StatusCode::CREATED, "{j1:#}");
    let first: StackImportResponse = serde_json::from_value(j1).unwrap();
    let imported = fixture
        .deployment_store
        .get_deployment(
            &alien_manager::auth::Subject::system(),
            &first.deployment_id,
        )
        .await
        .unwrap()
        .unwrap();
    fixture
        .deployment_store
        .reconcile(
            &alien_manager::auth::Subject::system(),
            ReconcileData {
                deployment_id: imported.id,
                session: "test-reconcile".to_string(),
                execution_claim: None,
                state: DeploymentState {
                    status: DeploymentStatus::Running,
                    platform: imported.platform,
                    current_release: Some(ReleaseInfo {
                        release_id: fixture.release_id.clone(),
                        version: None,
                        description: None,
                        stack: stack_with_storage("assets"),
                    }),
                    target_release: None,
                    stack_state: imported.stack_state,
                    error: None,
                    environment_info: imported.environment_info,
                    runtime_metadata: imported.runtime_metadata,
                    retry_requested: false,
                    protocol_version: imported.deployment_protocol_version,
                },
                update_heartbeat: false,
                suggested_delay_ms: None,
                heartbeats: vec![],
                observed_inventory_batches: vec![],
                capabilities: vec![],
                operator_version: None,
                operations_report: None,
            },
        )
        .await
        .expect("stable before re-import");
    let mut body = body;
    body.resources[0].import_data = serde_json::to_value(AwsStorageImportData {
        bucket_name: "acme-imports-v2".to_string(),
        bucket_arn: "arn:aws:s3:::acme-imports-v2".to_string(),
    })
    .unwrap();
    let (s2, j2) = post_import(&fixture, Some(&fixture.dg_token), &body).await;
    assert_eq!(s2, StatusCode::INTERNAL_SERVER_ERROR, "{j2:#}");
    assert!(!j2.to_string().contains("secret merge detail"), "{j2:#}");
}
