//! GCP identity & network — service-account / remote-stack-management /
//! network (Create + ByoVpcGcp + UseDefault).

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, RemoteStackManagement,
    ResourceLifecycle, ServiceAccount, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;

#[test]
fn gcp_service_account_with_permission_set() {
    let sa = ServiceAccount::new("execution-sa".to_string())
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
    let stack = Stack::new("acme-iam".to_string())
        .add(sa, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_service_account", &module);
    assert_terraform_valid(&module, "gcp_service_account");
}

#[test]
fn gcp_remote_stack_management_role() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["worker/management", "storage/heartbeat"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    snapshot_module("gcp_remote_stack_management", &module);
    assert_terraform_valid(&module, "gcp_remote_stack_management");
}

#[test]
fn gcp_remote_stack_management_function_provision_role() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["worker/provision"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered.contains("\"run.services.create\""));
    assert!(rendered.contains("\"pubsub.topics.create\""));
    assert!(rendered.contains("\"storage.buckets.update\""));
    assert!(rendered.contains("google_project_iam_custom_role\" \"gcp_role_worker_provision\""));
    assert!(rendered.contains(
        "role_id     = format(\"role_%s_worker_provision\", substr(replace(lower(local.resource_prefix), \"-\", \"_\"), 0, 18))"
    ));
    assert!(!rendered.contains("roles/run.admin"));
    assert_terraform_valid(&module, "gcp_remote_stack_management_function_provision");
}

#[test]
fn gcp_network_create_two_subnets() {
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
    let module = render(&stack, TerraformTarget::Gcp, settings);
    snapshot_module("gcp_network_create", &module);
    assert_terraform_valid(&module, "gcp_network_create");
}

#[test]
fn gcp_network_byo_vpc_emits_data_lookups() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcGcp {
            network_name: "shared-vpc".to_string(),
            subnet_name: "workload-us-central1".to_string(),
            region: "us-central1".to_string(),
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-byo-gcp".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Gcp, settings);
    snapshot_module("gcp_network_byo_vpc", &module);
    assert_terraform_valid(&module, "gcp_network_byo_vpc");
}
