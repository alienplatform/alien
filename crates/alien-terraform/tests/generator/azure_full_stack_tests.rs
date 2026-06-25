//! Full built-in Azure stack — every emitter wired together.
//!
//! The "audit walkthrough" snapshot. A security team reading this file
//! sees a complete Terraform module a customer could `terraform apply`.
//! `terraform fmt -check` + `terraform validate` run on every render.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup, AzureServiceBusNamespace,
    AzureStorageAccount, Build, Ingress, Kv, ManagementPermissions, Network, NetworkSettings,
    PermissionProfile, Queue, RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack,
    StackSettings, Storage, UpdatesMode, Vault, Worker, WorkerCode,
};
use alien_terraform::TerraformTarget;

#[test]
fn azure_full_stack_renders_audit_ready_module() {
    let execution_sa = ServiceAccount::new("execution-sa".to_string())
        .stack_permission_set(
            alien_permissions::get_permission_set("storage/data-read")
                .expect("storage/data-read permission set")
                .clone(),
        )
        .stack_permission_set(
            alien_permissions::get_permission_set("queue/data-write")
                .expect("queue/data-write permission set")
                .clone(),
        )
        .build();

    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.46.0.0/16".to_string()),
            availability_zones: 1,
        }),
        updates: UpdatesMode::ApprovalRequired,
        ..StackSettings::default()
    };

    let assets = Storage::new("assets".to_string()).versioning(true).build();
    let jobs = Queue::new("jobs".to_string()).build();
    let metadata = Kv::new("metadata".to_string()).build();
    let secrets = Vault::new("secrets".to_string()).build();

    let public_api = Worker::new("public-api".to_string())
        .code(WorkerCode::Image {
            image: "acmeprod.azurecr.io/api:1.2.3".to_string(),
        })
        .permissions("execution".to_string())
        .ingress(Ingress::Public)
        .timeout_seconds(60)
        .memory_mb(512)
        .environment([("RUST_LOG".to_string(), "info".to_string())].into())
        .link(&assets)
        .link(&metadata)
        .link(&secrets)
        .build();

    let worker = Worker::new("worker".to_string())
        .code(WorkerCode::Image {
            image: "acmeprod.azurecr.io/worker:1.2.3".to_string(),
        })
        .permissions("execution".to_string())
        .build();

    let stack = Stack::new("full-azure".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .global([
                    "worker/management",
                    "storage/heartbeat",
                    "queue/heartbeat",
                    "kv/heartbeat",
                ])
                .resource("secrets", ["vault/data-write"]),
        ))
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureStorageAccount::new("default-storage-account".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureServiceBusNamespace::new("default-service-bus-namespace".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            AzureContainerAppsEnvironment::new("default-container-apps-environment".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(execution_sa, ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(assets, ResourceLifecycle::Frozen)
        .add(jobs, ResourceLifecycle::Frozen)
        .add(metadata, ResourceLifecycle::Frozen)
        .add(secrets, ResourceLifecycle::Frozen)
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .environment([("PROFILE".to_string(), "release".to_string())].into())
                .build(),
            ResourceLifecycle::Live,
        )
        .add(public_api, ResourceLifecycle::Live)
        .add(worker, ResourceLifecycle::Live)
        .build();

    let module = render(&stack, TerraformTarget::Azure, settings);
    snapshot_module("azure_full_stack", &module);

    // The Container Apps environment must VNet-integrate so workers can reach private-only
    // resources (e.g. a Flexible Server reachable only via its private endpoint). Assert the
    // wiring literally — the snapshot's exact `=` alignment shifts whenever a sibling attribute is
    // added, so these literal checks are what actually guard the integration. Per-line whitespace
    // is normalized so a future sibling attribute can't silently break them.
    let rendered = module
        .iter()
        .map(|(_, contents)| contents.lines())
        .flatten()
        .map(str::trim)
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>();
    let has = |needle: &str| rendered.iter().any(|line| line == needle);

    assert!(
        has("infrastructure_subnet_id = azurerm_subnet.default_network_private.id"),
        "container apps environment must point its infrastructure subnet at the network private subnet"
    );
    assert!(
        has("internal_load_balancer_enabled = false"),
        "container apps environment must keep a public ingress while egressing through the VNet"
    );
    assert!(
        has("name = \"Microsoft.App/environments\""),
        "the private subnet must be delegated to Microsoft.App/environments for the env infra subnet"
    );

    assert_terraform_valid(&module, "azure_full_stack");
}
