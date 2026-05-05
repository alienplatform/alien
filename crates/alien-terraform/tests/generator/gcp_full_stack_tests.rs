//! Full built-in GCP stack — every emitter wired together.
//!
//! The "audit walkthrough" snapshot. A security team reading this file
//! sees a complete Terraform module a customer could `terraform apply`.
//! `terraform fmt -check` + `terraform validate` run on every render.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, Build, Function, FunctionCode, FunctionTrigger, Ingress, Kv,
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, Queue,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage,
    UpdatesMode, Vault,
};
use alien_terraform::TerraformTarget;

#[test]
fn gcp_full_stack_renders_audit_ready_module() {
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
            cidr: Some("10.44.0.0/16".to_string()),
            availability_zones: 1,
        }),
        updates: UpdatesMode::ApprovalRequired,
        ..StackSettings::default()
    };

    let assets = Storage::new("assets".to_string()).versioning(true).build();
    let jobs = Queue::new("jobs".to_string()).build();
    let metadata = Kv::new("metadata".to_string()).build();
    let secrets = Vault::new("secrets".to_string()).build();

    let public_api = Function::new("public-api".to_string())
        .code(FunctionCode::Image {
            image: "us-central1-docker.pkg.dev/proj/app/api:1.2.3".to_string(),
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

    let worker = Function::new("worker".to_string())
        .code(FunctionCode::Image {
            image: "us-central1-docker.pkg.dev/proj/app/worker:1.2.3".to_string(),
        })
        .permissions("execution".to_string())
        .trigger(FunctionTrigger::queue(&jobs))
        .trigger(FunctionTrigger::schedule("*/5 * * * *"))
        .trigger(FunctionTrigger::storage(
            &assets,
            vec!["created".to_string()],
        ))
        .build();

    let stack = Stack::new("full-gcp".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global([
                "function/management",
                "storage/heartbeat",
                "queue/heartbeat",
                "kv/heartbeat",
                "vault/data-write",
            ]),
        ))
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network settings"))
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

    let module = render(&stack, TerraformTarget::Gcp, settings);
    snapshot_module("gcp_full_stack", &module);
    assert_terraform_valid(&module, "gcp_full_stack");
}
