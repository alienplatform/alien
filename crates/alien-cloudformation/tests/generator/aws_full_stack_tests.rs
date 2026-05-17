//! Full built-in stack — every AWS emitter wired together.
//!
//! The "audit walkthrough" snapshot. A security team reading this file
//! sees the complete CloudFormation template a customer would land in
//! their account. cfn-lint runs on every render.

use super::helpers::render_built_ins;
use alien_cloudformation::RegistrationMode;
use alien_core::{
    ArtifactRegistry, Build, Worker, WorkerCode, WorkerTrigger, Ingress, Kv,
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, Queue,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackSettings, Storage,
    UpdatesMode, Vault,
};

const LAMBDA_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:function:alien-import";

#[test]
fn aws_full_stack_with_create_network_renders_audit_ready_template() {
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
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/worker:1.2.3".to_string(),
        })
        .permissions("execution".to_string())
        .trigger(WorkerTrigger::queue(&jobs))
        .trigger(WorkerTrigger::schedule("*/5 * * * *"))
        .build();

    let stack = Stack::new("full-aws".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global([
                "worker/provision",
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
            ResourceLifecycle::Frozen,
        )
        .add(public_api, ResourceLifecycle::Live)
        .add(worker, ResourceLifecycle::Live)
        .build();

    let yaml = render_built_ins(
        &stack,
        settings,
        RegistrationMode::Both {
            lambda_arn: LAMBDA_ARN.to_string(),
            callback_url: None,
        },
        "aws full stack",
    );
    insta::assert_snapshot!("aws_full_stack", yaml);
}
