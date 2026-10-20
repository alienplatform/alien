//! Real Terraform plans with mocked providers exercise input evaluation without
//! cloud credentials or creating infrastructure.
use alien_core::{Network, NetworkSettings, ResourceLifecycle, Stack, StackSettings};
use alien_terraform::TerraformTarget;

use super::helpers::{assert_terraform_valid, render, test_utils};

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

/// With no network settings, the network preflight adds `default-network` for resources that need
/// a VPC. The module builds that network, so it must declare the variables the network reads.
#[test]
fn network_added_by_the_preflight_declares_its_variables() {
    let stack = Stack::new("auto-network".into())
        .add(
            Network::new("default-network".into())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());

    assert_terraform_valid(&module, "network added by the preflight");
}
