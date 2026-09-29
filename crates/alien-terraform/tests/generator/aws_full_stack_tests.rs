//! Full built-in AWS stack — every emitter wired together.
//!
//! The "audit walkthrough" snapshot. A security team reading this file
//! sees a complete Terraform module a customer could `terraform apply`.
//! `terraform fmt -check` + `terraform validate` run on every render.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, Build, Kv, ManagementPermissions, Network, NetworkSettings,
    PermissionProfile, Queue, RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack,
    StackSettings, Storage, UpdatesMode, Vault, Worker, WorkerCode, WorkerTrigger,
};
use alien_terraform::TerraformTarget;

#[test]
fn aws_full_stack_renders_audit_ready_module() {
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
            availability_zones: 2,
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
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api:1.2.3".to_string(),
        })
        .permissions("execution".to_string())
        .public_endpoint(alien_core::WorkerPublicEndpoint {
            name: "api".to_string(),
            host_label: None,
            wildcard_subdomains: false,
        })
        .timeout_seconds(60)
        .expect("literal Worker timeout is within supported range")
        .memory_mb(512)
        .environment([("RUST_LOG".to_string(), "info".to_string())].into())
        .link(&assets)
        .link(&metadata)
        .link(&secrets)
        .trigger(WorkerTrigger::storage(&assets, vec!["created".to_string()]))
        .build();

    let worker = Worker::new("worker".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/worker:1.2.3".to_string(),
        })
        .permissions("execution".to_string())
        .trigger(WorkerTrigger::queue(&jobs))
        .trigger(WorkerTrigger::schedule("*/5 * * * *"))
        .build();

    let stack = Stack::new("full-aws".to_string())
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

    let module = render(&stack, TerraformTarget::Aws, settings);
    snapshot_module("aws_full_stack", &module);
    assert_terraform_valid(&module, "aws_full_stack");

    // Lambda deletes a worker's Hyperplane ENI with the function's role, so
    // every role must outlive the subnets and security group the ENI sits in.
    let network: hcl::Body = hcl::from_str(
        module
            .get("default_network.tf")
            .expect("default_network.tf should render"),
    )
    .expect("default_network.tf parses");
    for (resource_type, label) in [
        ("aws_subnet", "default_network_private"),
        ("aws_security_group", "default_network_workload"),
    ] {
        let block = network
            .blocks()
            .find(|block| {
                block.identifier() == "resource"
                    && block
                        .labels()
                        .iter()
                        .map(|label| label.as_str())
                        .collect::<Vec<_>>()
                        == [resource_type, label]
            })
            .unwrap_or_else(|| panic!("{resource_type}.{label} should render"));
        let depends_on = block
            .body()
            .attributes()
            .find(|attribute| attribute.key() == "depends_on")
            .unwrap_or_else(|| panic!("{resource_type}.{label} must declare depends_on"));
        let rendered = hcl::format::to_string(depends_on.expr()).expect("depends_on formats");
        for role in ["aws_iam_role.execution_sa", "aws_iam_role.management"] {
            assert!(
                rendered.contains(role),
                "{resource_type}.{label} must be destroyed before {role}: {rendered}"
            );
        }
    }
    assert!(
        !network.blocks().any(|block| block
            .labels()
            .iter()
            .any(|label| label.as_str() == "default_network_public")
            && block
                .body()
                .attributes()
                .any(|attribute| attribute.key() == "depends_on")),
        "public subnets do not host worker ENIs"
    );
}
