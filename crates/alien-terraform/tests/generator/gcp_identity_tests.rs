//! GCP identity & network — service-account / management /
//! network (Create + ByoVpcGcp + UseDefault).

use super::helpers::{assert_terraform_valid, gate_input, render, snapshot_module};
use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, RemoteStackManagement,
    ResourceLifecycle, ServiceAccount, Stack, StackSettings, Worker, WorkerCode,
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
fn gcp_service_account_disambiguates_stack_and_resource_role_bindings() {
    let sa = ServiceAccount::new("execution-sa".to_string())
        .stack_permission_set(
            alien_permissions::get_permission_set("vault/data-read")
                .expect("vault/data-read permission set")
                .clone(),
        )
        .build();
    let stack = Stack::new("acme-iam".to_string())
        .permission(
            "execution",
            PermissionProfile::new()
                .global(["vault/data-read"])
                .resource("alien-vault", ["vault/data-read"]),
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(sa, ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(rendered
        .contains("google_project_iam_member\" \"secretmanager_viewer_execution_sa_binding_0\""));
    assert!(rendered.contains(
        "google_project_iam_member\" \"secretmanager_viewer_alien_vault_vault_data_read_execution_sa_binding_0\""
    ));
    assert_terraform_valid(&module, "gcp_service_account_duplicate_vault_bindings");
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
    assert!(!rendered.contains("\"storage.buckets.update\""));
    assert!(rendered
        .contains("google_project_iam_custom_role\" \"gcp_role_manage_cloud_run_services\""));
    assert!(rendered.contains(
        "role_id     = format(\"role_%s_manage_cloud_run_services\", local.gcp_custom_role_prefix)"
    ));
    assert!(rendered.contains("gcp_manage_custom_roles"));
    assert!(!rendered.contains("roles/run.admin"));
    assert_terraform_valid(&module, "gcp_remote_stack_management_function_provision");
}

#[test]
fn gcp_custom_role_prefix_keeps_long_resource_prefixes_unique() {
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

    assert!(rendered.contains("length(replace(lower(local.resource_prefix), \"-\", \"_\")) <= 18"));
    assert!(rendered.contains("substr(sha256(local.resource_prefix), 0, 8)"));
    assert!(rendered.contains(
        "var.gcp_custom_role_prefix != \"\" ? substr(replace(lower(var.gcp_custom_role_prefix), \"-\", \"_\"), 0, 18)"
    ));
    assert_terraform_valid(&module, "gcp_unique_custom_role_prefix");
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

/// GCP's counterpart to the AWS and Azure management-grant gating: prove
/// there is nothing to gate. Resource-scoped management grants are emitted
/// only for a Kubernetes cluster, a type the gateability policy refuses, so a
/// gated worker can contribute no management grant at all — and the grants it
/// does get, through its permission profile, already carry its gate.
#[test]
fn gcp_emits_no_ungated_management_grant_for_a_gated_worker() {
    let stack = Stack::new("acme-mgmt-gcp".to_string())
        .inputs(vec![gate_input(
            "jobsEnabled",
            "Enable jobs",
            "Whether to run the jobs worker.",
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
                    image: "registry.example.com/app/jobs:1.2.3".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
            "jobsEnabled",
        )
        .build();

    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<Vec<_>>()
        .join("\n");

    // Every block naming the gated worker must carry its gate. A management
    // grant would name it by resource prefix, which no address-based scan can
    // see — this is the check that would have caught the AWS gap.
    let mut blocks_naming_the_worker = 0usize;
    for block in rendered.split("resource \"").skip(1) {
        if block.contains("-jobs") {
            blocks_naming_the_worker += 1;
            assert!(
                block.contains("var.input_jobs_enabled"),
                "a grant naming the gated worker must follow its gate:\n{block}"
            );
        }
    }
    // Today that loop finds nothing, and the reason is the finding: GCP's
    // resource-scoped management path handles only Kubernetes clusters, so a
    // Live worker contributes no management grant to name. AWS emits one and
    // needed gating; GCP has none to gate. If that ever changes, the loop
    // above turns from vacuous into the guard that catches it ungated.
    assert_eq!(
        blocks_naming_the_worker, 0,
        "GCP emitted a setup block naming a Live gated worker — it now needs the \
         same gating AWS has:\n{rendered}"
    );

    assert_terraform_valid(&module, "gcp_gated_worker_management_grants");
}
