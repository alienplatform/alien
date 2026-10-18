//! Real Terraform plans with mocked providers exercise input evaluation without
//! cloud credentials or creating infrastructure.
use alien_core::{
    Network, NetworkSettings, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;

use super::helpers::{render, test_utils};

#[test]
fn mocked_network_plans_normalize_empty_ids_in_registration() {
    for authored in [
        NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        },
        NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-test".into(),
            public_subnet_ids: vec![],
            private_subnet_ids: vec![],
            security_group_ids: vec![],
        },
    ] {
        let is_static = matches!(authored, NetworkSettings::ByoVpcAws { .. });
        let stack = Stack::new("network-inputs".into())
            .add(
                Network::new("net".into())
                    .settings(authored.clone())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let module = render(
            &stack,
            TerraformTarget::Aws,
            StackSettings {
                network: Some(authored),
                ..Default::default()
            },
        );
        let mut files: test_utils::LinterFiles = module
            .iter()
            .filter(|(path, _)| path.ends_with(".tf"))
            .map(|(path, content)| (path.to_string(), content.to_string()))
            .collect();
        let mut tests = String::from(
            r#"
mock_provider "aws" {
  mock_data "aws_availability_zones" {
    defaults = { names = ["us-east-1a", "us-east-1b"], zone_ids = ["use1-az1", "use1-az2"] }
  }
  mock_data "aws_region" { defaults = { region = "us-east-1" } }
}
mock_provider "random" {}
variables {
  name = "network-inputs"
  token = "test"
  resource_prefix = "network-inputs"
  network_mode = "use-existing"
  vpc_id = "vpc-test"
}
"#,
        );
        for (index, (public, private, groups, expected_public, expected_private, expected_group)) in
            [
                (
                    r#"["", "  "]"#,
                    r#"[" subnet-private ", ""]"#,
                    r#"[""]"#,
                    "[]",
                    r#"["subnet-private"]"#,
                    "null",
                ),
                (
                    r#"[" subnet-public ", ""]"#,
                    "[]",
                    r#"["", " sg-first ", "sg-second"]"#,
                    r#"["subnet-public"]"#,
                    "[]",
                    r#""sg-first""#,
                ),
                (
                    "[]",
                    r#"["subnet-private"]"#,
                    "[]",
                    "[]",
                    r#"["subnet-private"]"#,
                    "null",
                ),
            ]
            .into_iter()
            .enumerate()
        {
            let static_vars = if is_static {
                format!("net_vpc_id = \"vpc-test\"\nnet_public_subnet_ids = {public}\nnet_private_subnet_ids = {private}\nnet_security_group_ids = {groups}")
            } else {
                String::new()
            };
            tests.push_str(&format!(r#"
run "network_{index}" {{
  command = plan
  variables {{
    public_subnet_ids = {public}
    private_subnet_ids = {private}
    security_group_ids = {groups}
    {static_vars}
  }}
  assert {{
    condition = jsonencode(local.deployment_resources[0].importData.publicSubnetIds) == jsonencode({expected_public})
    error_message = "Registration must not contain blank public subnet IDs."
  }}
  assert {{
    condition = jsonencode(local.deployment_resources[0].importData.privateSubnetIds) == jsonencode({expected_private})
    error_message = "Registration must not contain blank private subnet IDs."
  }}
  assert {{
    condition = local.deployment_resources[0].importData.securityGroupId == {expected_group}
    error_message = "An absent security group must be null; select the first real ID otherwise."
  }}
  assert {{
    condition = jsonencode(local.deployment_settings.network.publicSubnetIds) == jsonencode(local.deployment_resources[0].importData.publicSubnetIds) && jsonencode(local.deployment_settings.network.privateSubnetIds) == jsonencode(local.deployment_resources[0].importData.privateSubnetIds)
    error_message = "Stack settings and imported network topology must agree."
  }}
}}
"#));
        }
        files.insert("network.tftest.hcl".to_string(), tests);
        let run = test_utils::terraform_test(&files);
        // SKIP_LINTERS=1 is an explicit opt-out for machines without Terraform.
        if let test_utils::LinterStatus::Skipped(reason) = &run.status {
            eprintln!("skipping Terraform network plans: {reason}");
            return;
        }
        run.assert_ok(format!(
            "network ID normalization plans (static existing network: {is_static})"
        ));
    }
}

/// Upgrading an existing network must not replace its automatically allocated NAT addresses.
#[test]
fn gcp_nat_address_reservation_requires_gateway_opt_in() {
    for egress in [
        None,
        Some(SandboxEgress::None),
        Some(SandboxEgress::Internet),
    ] {
        let reserved = egress == Some(SandboxEgress::Internet);
        let mut stack = Stack::new("nat-compatibility".into()).add(
            Network::new("default-network".into())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 1,
                })
                .build(),
            ResourceLifecycle::Frozen,
        );
        if let Some(egress) = egress {
            stack = stack.add(
                Sandbox::new("box".into())
                    .code(SandboxCode::Image {
                        image: "ubuntu".into(),
                    })
                    .egress(egress)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Live,
            );
        }
        let module = render(
            &stack.build(),
            TerraformTarget::Gcp,
            StackSettings {
                network: Some(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 1,
                }),
                ..StackSettings::default()
            },
        );
        let mut files: test_utils::LinterFiles = module
            .iter()
            .filter(|(path, _)| path.ends_with(".tf"))
            .map(|(path, content)| (path.to_string(), content.to_string()))
            .collect();
        let allocation = if reserved { "MANUAL_ONLY" } else { "AUTO_ONLY" };
        let address_count = usize::from(reserved);
        files.insert(
            "nat.tftest.hcl".into(),
            format!(
                r#"
mock_provider "google" {{}}
mock_provider "random" {{}}
variables {{
  name = "nat-compatibility"
  token = "test"
  resource_prefix = "nat-compatibility"
  gcp_project = "test-project"
  gcp_region = "us-central1"
  network_mode = "create-new"
}}
run "nat_allocation" {{
  command = plan
  assert {{
    condition = google_compute_router_nat.default_network_nat[0].nat_ip_allocate_option == "{allocation}"
    error_message = "NAT address allocation must change only for an explicit gateway opt-in."
  }}
  assert {{
    condition = length(local.deployment_resources[0].importData.egressPublicIps) == {address_count}
    error_message = "Only reserved NAT addresses may be advertised as fixed outbound addresses."
  }}
}}
"#
            ),
        );
        test_utils::terraform_test(&files).assert_ok("GCP NAT allocation compatibility plan");
    }
}

/// Terraform must reuse the recorded address when the same setup is applied again.
#[test]
fn repeated_cloud_setup_preserves_the_reserved_nat_address() {
    for (target, provider, address, variables) in [
        (TerraformTarget::Gcp, "google", "google_compute_address.default_network_nat_ip[0].address",
         "gcp_project = \"test-project\"\ngcp_region = \"us-central1\"\nnetwork_mode = \"create-new\""),
        (TerraformTarget::Azure, "azurerm", "azurerm_public_ip.default_network_nat_pip.ip_address",
         "azure_resource_group_name = \"test-group\"\nazure_location = \"eastus\"\nazure_subscription_id = \"00000000-0000-0000-0000-000000000001\""),
    ] {
        let settings = NetworkSettings::Create { cidr: None, availability_zones: 1 };
        let mut stack = Stack::new("nat-reapply".into()).add(
            Network::new("default-network".into()).settings(settings.clone()).build(),
            ResourceLifecycle::Frozen,
        );
        if matches!(target, TerraformTarget::Gcp) {
            stack = stack.add(Sandbox::new("box".into())
                .code(SandboxCode::Image { image: "ubuntu".into() })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy { max_lifetime_seconds: None, idle_pause_seconds: None })
                .build(), ResourceLifecycle::Live);
        }
        let module = render(&stack.build(), target, StackSettings { network: Some(settings), ..Default::default() });
        let mut files: test_utils::LinterFiles = module.iter()
            .filter(|(path, _)| path.ends_with(".tf"))
            .map(|(path, content)| (path.to_string(), content.to_string())).collect();
        files.insert("nat_address.tf".into(), format!("output \"nat_address\" {{ value = {address} }}"));
        let provider_mocks = if matches!(target, TerraformTarget::Azure) {
            r#"
  mock_resource "azurerm_nat_gateway" { defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000001/resourceGroups/test-group/providers/Microsoft.Network/natGateways/nat" } }
  mock_resource "azurerm_public_ip" { defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000001/resourceGroups/test-group/providers/Microsoft.Network/publicIPAddresses/nat" } }
  mock_resource "azurerm_subnet" { defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000001/resourceGroups/test-group/providers/Microsoft.Network/virtualNetworks/net/subnets/private" } }
  mock_resource "azurerm_network_security_group" { defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000001/resourceGroups/test-group/providers/Microsoft.Network/networkSecurityGroups/workload" } }
"#
        } else { "" };
        files.insert("nat_reapply.tftest.hcl".into(), format!(r#"
mock_provider "{provider}" {{ {provider_mocks} }}
mock_provider "random" {{}}
variables {{
  name = "nat-reapply"
  token = "test"
  resource_prefix = "nat-reapply"
  {variables}
}}
run "first" {{ command = apply }}
run "repeat" {{
  command = apply
  assert {{
    condition = output.nat_address == run.first.nat_address
    error_message = "Reapplying setup replaced the reserved NAT address."
  }}
}}
"#));
        test_utils::terraform_test(&files).assert_ok("cloud NAT address survives repeated setup");
    }
}
