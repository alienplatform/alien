//! AWS identity & network — service-account / management /
//! network (Create + ByoVpcAws + UseDefault).

use super::helpers::{assert_terraform_valid, gate_input, render, snapshot_module};
use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, RemoteStackManagement,
    ResourceLifecycle, ServiceAccount, Stack, StackSettings, Worker, WorkerCode,
};
use alien_terraform::TerraformTarget;

#[test]
fn aws_service_account_with_permission_set() {
    let sa = ServiceAccount::new("execution-sa".to_string())
        .stack_permission_set(
            alien_permissions::get_permission_set("storage/data-read")
                .expect("storage/data-read permission set")
                .clone(),
        )
        .build();
    let stack = Stack::new("acme-iam".to_string())
        .add(sa, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_service_account", &module);
    assert_terraform_valid(&module, "aws_service_account");
}

#[test]
fn aws_remote_stack_management_role() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["worker/management", "storage/heartbeat"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_remote_stack_management", &module);
    assert_terraform_valid(&module, "aws_remote_stack_management");
}

#[test]
fn aws_remote_stack_management_skips_live_provision_sets() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("job", ["worker/provision", "worker/dispatch-command"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("job".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/job:1.2.3".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut rendered = String::new();
    for (_, contents) in module.iter() {
        rendered.push_str(contents);
        rendered.push('\n');
    }

    assert!(rendered.contains("lambda:InvokeFunction"));
    assert!(!rendered.contains("lambda:CreateFunction"));
    assert_terraform_valid(
        &module,
        "aws_remote_stack_management_skips_live_provision_sets",
    );
}

/// A management grant naming a gated resource must follow that resource's
/// gate. The shared managed policy is unconditional, so a grant left there
/// would outlive the very resource it names — the management role would keep
/// invoke rights on a function the deployer declined.
#[test]
fn aws_management_grants_for_a_gated_resource_carry_its_gate() {
    let stack = Stack::new("acme-mgmt".to_string())
        .inputs(vec![gate_input(
            "jobsEnabled",
            "Enable the job worker",
            "Whether to run the job worker.",
        )])
        .management(ManagementPermissions::extend(
            PermissionProfile::new().resource("jobs", ["worker/dispatch-command"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(
            Worker::new("jobs".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/jobs:1.2.3"
                        .to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
            "jobsEnabled",
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut main = String::new();
    for (_, contents) in module.iter() {
        main.push_str(contents);
        main.push('\n');
    }

    // The grant exists, and it is behind the worker's gate.
    let gated_policy = main
        .split("resource \"")
        .find(|block| block.starts_with("aws_iam_role_policy\" \"management_input_jobs_enabled\""))
        .unwrap_or_else(|| panic!("the gated management policy should render:\n{main}"));
    // HCL re-pads `=` to align with sibling attributes, so match the value.
    assert!(
        gated_policy.contains("var.input_jobs_enabled ? 1 : 0"),
        "the gated management policy must carry the worker's gate:\n{gated_policy}"
    );
    assert!(
        gated_policy.contains("lambda:InvokeFunction"),
        "the dispatch grant belongs in the gated policy:\n{gated_policy}"
    );

    // And it is not also sitting in the unconditional managed policy.
    for block in main.split("resource \"") {
        if block.starts_with("aws_iam_policy\" \"management_managed_policy") {
            assert!(
                !block.contains("lambda:InvokeFunction"),
                "a declinable resource's grant must not ride the unconditional \
                 managed policy:\n{block}"
            );
        }
    }

    assert_terraform_valid(&module, "aws_management_grants_for_a_gated_resource");
}

/// Two resources gated on one input land in one policy, each contributing the
/// same permission set and therefore the same statement id. IAM rejects a
/// policy document that repeats one, so the ids must be made unique — the
/// shared managed-policy path already does this, and the gated path has to
/// agree.
#[test]
fn management_grants_sharing_a_gate_get_unique_statement_ids() {
    let worker = |id: &str| {
        Worker::new(id.to_string())
            .code(WorkerCode::Image {
                image: format!("123456789012.dkr.ecr.us-east-1.amazonaws.com/app/{id}:1.2.3"),
            })
            .permissions("execution".to_string())
            .build()
    };
    let stack = Stack::new("acme-mgmt".to_string())
        .inputs(vec![gate_input(
            "jobsEnabled",
            "Enable the job workers",
            "Whether to run the job workers.",
        )])
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("jobs-a", ["worker/dispatch-command"])
                .resource("jobs-b", ["worker/dispatch-command"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add_enabled_when(worker("jobs-a"), ResourceLifecycle::Live, "jobsEnabled")
        .add_enabled_when(worker("jobs-b"), ResourceLifecycle::Live, "jobsEnabled")
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let mut rendered = String::new();
    for (_, contents) in module.iter() {
        rendered.push_str(contents);
        rendered.push('\n');
    }

    let gated_policy = rendered
        .split("resource \"")
        .find(|block| block.starts_with("aws_iam_role_policy\" \"management_input_jobs_enabled\""))
        .unwrap_or_else(|| panic!("the gated management policy should render:\n{rendered}"));

    let sids: Vec<&str> = gated_policy.match_indices("Sid = \"").map(|(i, _)| {
        let rest = &gated_policy[i + 7..];
        &rest[..rest.find('"').unwrap_or(0)]
    }).collect();
    assert_eq!(sids.len(), 2, "both workers contribute a statement: {sids:?}");
    assert_ne!(
        sids[0], sids[1],
        "IAM rejects a policy document with duplicate statement ids: {sids:?}"
    );

    assert_terraform_valid(&module, "management_grants_sharing_a_gate");
}

#[test]
fn aws_network_create_two_az() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-net".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, settings);
    snapshot_module("aws_network_create_two_az", &module);
    assert_terraform_valid(&module, "aws_network_create_two_az");
}

#[test]
fn aws_network_byo_vpc_emits_no_resources() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string()],
            private_subnet_ids: vec!["subnet-private-a".to_string()],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-byo".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, settings);
    snapshot_module("aws_network_byo_vpc", &module);
    assert_terraform_valid(&module, "aws_network_byo_vpc");
}
