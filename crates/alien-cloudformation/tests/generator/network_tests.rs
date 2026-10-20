//! Network axis — `Create` (full topology) vs `ByoVpcAws` (parameter-driven).

use super::helpers::{render_built_ins, render_built_ins_template, render_sample, sample_stack};
use alien_cloudformation::{CloudFormationTarget, RegistrationMode};
use alien_core::{
    HeartbeatsMode, Network, NetworkSettings, ResourceLifecycle, Stack, StackSettings,
    TelemetryMode, UpdatesMode,
};

#[test]
fn create_network_three_az_emits_full_topology() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 3,
        }),
        updates: UpdatesMode::ApprovalRequired,
        telemetry: TelemetryMode::Off,
        heartbeats: HeartbeatsMode::Off,
        ..StackSettings::default()
    };

    let stack = Stack::new("network-create".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network settings"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        settings,
        RegistrationMode::OutputsFallback,
        "create network 3 az",
    );
    insta::assert_snapshot!("network_create_three_az", yaml);
}

#[test]
fn byo_vpc_aws_uses_parameter_driven_subnet_ids() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string(), "subnet-public-b".to_string()],
            private_subnet_ids: vec![
                "subnet-private-a".to_string(),
                "subnet-private-b".to_string(),
            ],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        }),
        ..StackSettings::default()
    };

    let yaml = render_sample(
        &sample_stack(),
        settings,
        RegistrationMode::OutputsFallback,
        "byo vpc aws",
    );
    insta::assert_snapshot!("network_byo_vpc_aws", yaml);
}

#[test]
fn optional_security_groups_accept_an_empty_default() {
    let settings = StackSettings {
        network: Some(NetworkSettings::UseDefault),
        ..StackSettings::default()
    };
    let stack = Stack::new("default-network".to_string()).build();

    let (template, _) = render_built_ins_template(
        &stack,
        settings,
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Aws,
        "https://api.example.com",
        "default vpc optional security groups",
    );

    let parameter = template
        .parameters
        .get("SecurityGroupIds")
        .expect("shared network inputs include optional security groups");

    // AWS validates typed `List<AWS::EC2::SecurityGroup::Id>` defaults before it evaluates
    // conditions. An empty default therefore rejects `use-default` stacks even though this
    // parameter is only consumed by `use-existing`. `CommaDelimitedList` preserves list-valued
    // `Ref` behavior while allowing the unused input to remain empty.
    assert_eq!(parameter.parameter_type, "CommaDelimitedList");
    assert_eq!(parameter.default, Some("".into()));
}

/// With no network settings, the network preflight adds `default-network` for resources that need
/// a VPC. The template builds that network, so it must declare the inputs the network reads;
/// otherwise CreateStack rejects refs to undeclared parameters.
#[test]
fn network_added_by_the_preflight_declares_its_inputs() {
    let stack = Stack::new("auto-network".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(NetworkSettings::Create {
                    cidr: None,
                    availability_zones: 2,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    // cfn-lint runs inside the render and fails on any ref to an undeclared parameter.
    let (template, _yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        CloudFormationTarget::Aws,
        "aws",
        "network added by the preflight",
    );
    for parameter in ["VpcCidr", "AvailabilityZones"] {
        assert!(
            template.parameters.contains_key(parameter),
            "{parameter} is declared"
        );
    }
}

/// A stack can name its own existing VPC rather than take one from the settings. The template
/// reads that VPC from the same inputs either way, defaulting to the stack's IDs, as Terraform does.
#[test]
fn existing_vpc_declared_by_the_stack_renders_as_its_settings() {
    let existing = NetworkSettings::ByoVpcAws {
        vpc_id: "vpc-0123456789abcdef0".to_string(),
        public_subnet_ids: vec!["subnet-public-a".to_string(), "subnet-public-b".to_string()],
        private_subnet_ids: vec![
            "subnet-private-a".to_string(),
            "subnet-private-b".to_string(),
        ],
        security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
    };
    let stack = Stack::new("existing-vpc".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(existing.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    // cfn-lint runs inside each render and fails on any ref to an undeclared parameter.
    let render = |settings| {
        render_built_ins_template(
            &stack,
            settings,
            RegistrationMode::OutputsFallback,
            CloudFormationTarget::Aws,
            "aws",
            "existing vpc declared by the stack",
        )
        .1
    };
    assert_eq!(
        render(StackSettings::default()),
        render(StackSettings {
            network: Some(existing),
            ..StackSettings::default()
        })
    );
}
