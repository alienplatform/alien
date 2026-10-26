//! Isolated Azure storage qualification. See `azure_storage_live.md`.
#![cfg(feature = "azure")]

use std::{fs, path::PathBuf, sync::Arc};

use alien_core::{
    AzureClientConfig, AzureCredentials, AzureResourceGroup, AzureStorageAccount, ClientConfig,
    InitialSetupAuthority, LifecycleRule, Platform, ResourceLifecycle, ResourceStatus, Storage,
};
use alien_infra::{
    controller_test::SingleControllerExecutor, AzureResourceGroupController,
    AzureStorageAccountController, AzureStorageController, DefaultPlatformServiceProvider,
};
use alien_permissions::{
    generators::AzureRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    subscription_id: String,
    tenant_id: String,
    region: String,
    prefix: String,
    resource_group: String,
    account: String,
    setup_token_file: PathBuf,
    runtime_token_file: PathBuf,
    evidence_dir: PathBuf,
}

fn fixture() -> Fixture {
    let path = std::env::var("ALIEN_AZURE_STORAGE_FIXTURE").expect("isolated fixture required");
    let f: Fixture = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(f.prefix.starts_with("test-"));
    assert!(f.resource_group.starts_with(&f.prefix));
    f
}

fn config(f: &Fixture, setup: bool) -> ClientConfig {
    ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: f.subscription_id.clone(),
        tenant_id: f.tenant_id.clone(),
        region: Some(f.region.clone()),
        credentials: AzureCredentials::AccessToken {
            token: fs::read_to_string(if setup {
                &f.setup_token_file
            } else {
                &f.runtime_token_file
            })
            .unwrap()
            .trim()
            .into(),
        },
        service_overrides: None,
    }))
}

fn storage(public: bool, expiry: bool) -> Storage {
    Storage::new("assets".into())
        .public_read(public)
        .versioning(true)
        .cors_allowed_origins(vec!["https://example.test".into()])
        .lifecycle_rules(if expiry {
            vec![LifecycleRule {
                days: 30,
                prefix: Some("logs/".into()),
            }]
        } else {
            vec![]
        })
        .build()
}

#[test]
#[ignore = "exports exact grants for an isolated Azure test identity"]
fn export_grants() {
    let f = fixture();
    let context = PermissionContext::new()
        .with_subscription_id(&f.subscription_id)
        .with_resource_group(&f.resource_group)
        .with_storage_account_name(&f.account)
        .with_resource_name(&f.account)
        .with_stack_prefix(&f.prefix);
    let plans: Vec<_> = ["azure-storage-account/heartbeat", "storage/provision"]
        .iter()
        .map(|id| {
            AzureRuntimePermissionsGenerator::new()
                .generate_grant_plan(
                    get_permission_set(id).unwrap(),
                    BindingTarget::Stack,
                    &context,
                )
                .unwrap()
        })
        .collect();
    fs::write(
        f.evidence_dir.join("grants.json"),
        serde_json::to_vec_pretty(&plans).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "writes only the explicitly configured isolated storage account and container"]
async fn controller_phase() {
    let f = fixture();
    let phase = std::env::var("ALIEN_AZURE_STORAGE_PHASE").unwrap();
    let group = AzureResourceGroup::new("default-resource-group".into()).build();
    let account = AzureStorageAccount::new("default-storage-account".into()).build();
    let provider = Arc::new(DefaultPlatformServiceProvider::default());
    if matches!(
        phase.as_str(),
        "setup" | "remove-expiry" | "remove-all-expiry"
    ) {
        let controller = if phase == "setup" {
            AzureStorageAccountController::mock_ready(&f.account)
        } else {
            serde_json::from_slice(
                &fs::read(f.evidence_dir.join("account-checkpoint.json")).unwrap(),
            )
            .unwrap()
        };
        let mut run = SingleControllerExecutor::builder()
            .resource(account.clone())
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .controller(controller)
            .platform(Platform::Azure)
            .resource_prefix(&f.prefix)
            .client_config(config(&f, true))
            .service_provider(provider)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_stack_resource(storage(true, phase == "setup"), ResourceLifecycle::Live)
            .with_stack_resource(
                Storage::new("sibling".into())
                    .versioning(true)
                    .lifecycle_rules(if phase == "remove-all-expiry" {
                        vec![]
                    } else {
                        vec![LifecycleRule {
                            days: 60,
                            prefix: Some("backup/".into()),
                        }]
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .with_dependency(
                group,
                AzureResourceGroupController::mock_ready(&f.resource_group),
            )
            .real_delays()
            .build()
            .await
            .unwrap();
        run.update(account).unwrap();
        run.run_until_terminal().await.unwrap();
        assert_eq!(run.status(), ResourceStatus::Running);
        fs::write(
            f.evidence_dir.join("account-checkpoint.json"),
            serde_json::to_vec_pretty(
                run.internal_state::<AzureStorageAccountController>()
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    } else {
        assert!(matches!(
            phase.as_str(),
            "create" | "private" | "verify-removal" | "erase"
        ));
        let controller = if phase == "create" {
            AzureStorageController::default()
        } else {
            serde_json::from_slice(
                &fs::read(f.evidence_dir.join("container-checkpoint.json")).unwrap(),
            )
            .unwrap()
        };
        let mut run = SingleControllerExecutor::builder()
            .resource(storage(
                matches!(phase.as_str(), "create" | "private"),
                phase != "erase",
            ))
            .resource_lifecycle(ResourceLifecycle::Live)
            .controller(controller)
            .platform(Platform::Azure)
            .resource_prefix(&f.prefix)
            .client_config(config(&f, false))
            .service_provider(provider)
            .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
            .with_dependency(
                group,
                AzureResourceGroupController::mock_ready(&f.resource_group),
            )
            .with_dependency(
                account,
                AzureStorageAccountController::mock_ready(&f.account),
            )
            .real_delays()
            .build()
            .await
            .unwrap();
        match phase.as_str() {
            "private" => run.update(storage(false, true)).unwrap(),
            "verify-removal" => run.update(storage(false, false)).unwrap(),
            "erase" => run.delete().unwrap(),
            _ => (),
        }
        run.run_until_terminal().await.unwrap();
        assert_eq!(
            run.status(),
            if phase == "erase" {
                ResourceStatus::Deleted
            } else {
                ResourceStatus::Running
            }
        );
        fs::write(
            f.evidence_dir.join("container-checkpoint.json"),
            serde_json::to_vec_pretty(run.internal_state::<AzureStorageController>().unwrap())
                .unwrap(),
        )
        .unwrap();
    }
    fs::write(
        f.evidence_dir.join(format!("{phase}-passed")),
        b"controller reached expected terminal state\n",
    )
    .unwrap();
}
