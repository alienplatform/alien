//! Real Terraform plans with mocked providers exercise input evaluation without
//! cloud credentials or creating infrastructure.
use alien_core::{
    Container, ContainerCode, Network, NetworkSettings, ResourceLifecycle, ResourceSpec, Sandbox,
    SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;

use super::helpers::{render, test_utils, try_render};

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

/// Generation refuses subnet layouts that cannot hold the dedicated sandbox allocation.
#[test]
fn azure_sandbox_network_rejects_small_or_non_ipv4_networks() {
    for (cidr, supported) in [
        ("10.46.0.0/16", true),
        ("10.46.0.0/20", false),
        ("fd00::/16", false),
    ] {
        let stack = Stack::new("sandbox-network".into())
            .add(
                Network::new("net".into())
                    .settings(NetworkSettings::Create {
                        cidr: Some(cidr.into()),
                        availability_zones: 1,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Sandbox::new("box".into())
                    .code(SandboxCode::Image {
                        image: "ubuntu".into(),
                    })
                    .egress(SandboxEgress::Internet)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Container::new("sandbox-egress-gateway".into())
                    .code(ContainerCode::Image {
                        image: "gateway:test".into(),
                    })
                    .cpu(ResourceSpec {
                        min: "0.25".into(),
                        desired: "0.25".into(),
                    })
                    .memory(ResourceSpec {
                        min: "256Mi".into(),
                        desired: "256Mi".into(),
                    })
                    .permissions("gateway".into())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        let result =
            super::helpers::try_render(&stack, TerraformTarget::Azure, StackSettings::default());
        if supported {
            result.expect("the default network can allocate the dedicated subnet");
        } else {
            let error = result.expect_err("unsafe subnet allocation must fail before apply");
            assert!(
                error.to_string().contains("IPv4 VNet /16 or larger"),
                "{error}"
            );
        }
    }
}

#[test]
fn aks_sandboxes_do_not_require_a_native_azure_subnet() {
    let settings = NetworkSettings::ByoVnetAzure {
        vnet_resource_id: "/subscriptions/test/resourceGroups/test/providers/Microsoft.Network/virtualNetworks/test".into(),
        public_subnet_name: "public".into(),
        private_subnet_name: "private".into(),
        application_gateway_subnet_name: None,
        private_endpoint_subnet_name: None,
    };
    let stack = Stack::new("aks-sandbox".into())
        .add(
            Network::new("net".into())
                .settings(settings.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Sandbox::new("box".into())
                .code(SandboxCode::Image {
                    image: "ubuntu".into(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    super::helpers::try_render(
        &stack,
        TerraformTarget::Aks,
        StackSettings {
            network: Some(settings),
            ..Default::default()
        },
    )
    .expect("pods do not consume native Azure sandbox subnets");
    let error =
        super::helpers::try_render(&stack, TerraformTarget::Azure, StackSettings::default())
            .expect_err("native sandboxes still require a dedicated setup-owned subnet");
    assert!(
        error.to_string().contains("dedicated BYO sandbox subnets"),
        "{error}"
    );
}

/// A linker must select its own delegated range rather than every Sandbox range.
#[test]
fn azure_sandbox_source_handoff_keeps_resource_ranges_distinct() {
    let settings = NetworkSettings::Create {
        cidr: Some("10.61.0.0/16".into()),
        availability_zones: 1,
    };
    let mut stack = Stack::new("linked-sources".into()).add(
        Network::new("default-network".into())
            .settings(settings.clone())
            .build(),
        ResourceLifecycle::Frozen,
    );
    for (id, egress) in [
        ("alpha", SandboxEgress::Internet),
        ("beta", SandboxEgress::Internet),
        ("legacy", SandboxEgress::Allow),
    ] {
        stack = stack.add(
            Sandbox::new(id.into())
                .code(SandboxCode::Image {
                    image: "ubuntu".into(),
                })
                .egress(egress)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        );
    }
    let stack = stack
        .add(
            Container::new("sandbox-egress-gateway".into())
                .code(ContainerCode::Image {
                    image: "gateway:test".into(),
                })
                .cpu(ResourceSpec {
                    min: "0.25".into(),
                    desired: "0.25".into(),
                })
                .memory(ResourceSpec {
                    min: "256Mi".into(),
                    desired: "256Mi".into(),
                })
                .permissions("gateway".into())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(
        &stack,
        TerraformTarget::Azure,
        StackSettings {
            network: Some(settings.clone()),
            ..Default::default()
        },
    );
    let mut files: test_utils::LinterFiles = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(path, content)| (path.to_string(), content.to_string()))
        .collect();
    files.insert("sources.tftest.hcl".into(), r#"
mock_provider "azurerm" {}
mock_provider "azapi" {}
mock_provider "random" {}
variables {
  name = "linked-sources"
  token = "test"
  resource_prefix = "linked-sources"
  azure_resource_group_name = "test-group"
  azure_location = "eastus"
  azure_subscription_id = "00000000-0000-0000-0000-000000000001"
}
run "source_ranges" {
  command = plan
  assert {
    condition = toset(azurerm_network_security_rule.default_network_sandbox_alpha_gateway.destination_port_ranges) == toset(["3128", "3129"])
    error_message = "The link opens both gateway Container ports while admin requests remain authenticated."
  }
  assert {
    condition = jsonencode(local.deployment_resources[0].importData.sandboxNetwork.sandboxCidrsByResource) == jsonencode({alpha = ["10.61.16.0/23"], beta = ["10.61.18.0/23"]})
    error_message = "Each new Internet Sandbox must keep its own exact source range; legacy sandboxes must not acquire one."
  }
  assert {
    condition = length(local.deployment_resources[0].importData.sandboxNetwork.sandboxSubnetIds) == 2
    error_message = "The handoff must contain exactly the two opt-in delegated subnets."
  }
}
"#.into());
    test_utils::terraform_test(&files).assert_ok("Azure exact Sandbox source handoff");

    let mut without_gateway = stack;
    without_gateway
        .resources
        .shift_remove("sandbox-egress-gateway");
    let error = try_render(
        &without_gateway,
        TerraformTarget::Azure,
        StackSettings {
            network: Some(settings),
            ..Default::default()
        },
    )
    .err()
    .expect("new Internet without a gateway must not emit a native fallback");
    assert!(error
        .to_string()
        .contains("requires the deployment gateway"));
}
