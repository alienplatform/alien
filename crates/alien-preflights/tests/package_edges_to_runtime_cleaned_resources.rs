//! Runtime cleanup deletes the resources an automatic setup package does not own. An edge from a
//! package resource (management role, vault, resource group, service activation) to one of those
//! resources keeps it from being deleted while the package resource stays, so every such edge must
//! be a known one that deletion treats as ordering only. This lists the edges the preflights add.

use std::collections::BTreeSet;

use alien_core::SetupManagement;
use alien_core::{
    removed_by_runtime_cleanup, AwsManagementConfig, AzureManagementConfig, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, GcpManagementConfig, Kv, ManagementConfig,
    Network, NetworkSettings, PermissionsConfig, Platform, Queue, Redis, ResourceLifecycle, Stack,
    StackSettings, StackState, Storage, Vault, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;

fn management(platform: Platform) -> ManagementConfig {
    match platform {
        Platform::Aws => ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "arn:aws:iam::111122223333:role/alien-management".to_string(),
        }),
        Platform::Gcp => ManagementConfig::Gcp(GcpManagementConfig {
            service_account_email: "manager@example.iam.gserviceaccount.com".to_string(),
            federation_trust: None,
        }),
        _ => ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: "management-tenant".to_string(),
            oidc_issuer: "https://issuer.example".to_string(),
            oidc_subject: "system:serviceaccount:alien:manager".to_string(),
        }),
    }
}

#[tokio::test]
async fn package_resources_wait_only_for_other_package_resources_or_known_ordering_edges() {
    let mut edges = BTreeSet::new();
    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "image:latest".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let stack = Stack::new("edges".to_string())
            .add(worker, ResourceLifecycle::Live)
            .add(
                Storage::new("data".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Queue::new("jobs".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Kv::new("cache-kv".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Vault::new("secrets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Redis::new("cache".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Network::new("default-network".to_string())
                    .settings(NetworkSettings::Create {
                        cidr: None,
                        availability_zones: 2,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .permissions(PermissionsConfig::new())
            .build();
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings {
                setup_management: SetupManagement::Automatic,
                ..Default::default()
            })
            .management_config(management(platform))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: String::new(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let prepared = PreflightRunner::new()
            .apply_mutations(stack, &StackState::new(platform), &config)
            .await
            .unwrap();
        for (id, entry) in &prepared.resources {
            let source_type = entry.config.resource_type();
            if removed_by_runtime_cleanup(
                source_type.as_ref(),
                entry.lifecycle,
                SetupManagement::Automatic,
                platform,
            ) {
                continue;
            }
            for dependency in &entry.dependencies {
                let target = &prepared.resources[dependency.id()];
                if removed_by_runtime_cleanup(
                    target.config.resource_type().as_ref(),
                    target.lifecycle,
                    SetupManagement::Automatic,
                    platform,
                ) {
                    edges.insert(format!(
                        "{platform:?}: {source_type} '{id}' -> {} '{}'",
                        dependency.resource_type,
                        dependency.id()
                    ));
                }
            }
        }
    }
    // Deletion treats the management role's network read as ordering only
    // (`StackExecutor::permission_only_dependency`). Any other edge listed here would keep a
    // runtime-cleaned resource from being deleted: add it there, with a test, or drop the edge.
    assert_eq!(
        edges.into_iter().collect::<Vec<_>>(),
        vec!["Aws: remote-stack-management 'management' -> network 'default-network'"]
    );
}
