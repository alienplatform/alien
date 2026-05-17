//! AWS identity & network — service-account / remote-stack-management /
//! network (Create + ByoVpcAws + UseDefault).

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, RemoteStackManagement,
    ResourceLifecycle, ServiceAccount, Stack, StackSettings,
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
