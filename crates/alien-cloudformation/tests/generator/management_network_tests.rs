//! The management role's network-facing creates name the stack's own VPC and subnets, per
//! network mode. The network mode is a deploy-time parameter, so the values are template
//! expressions; no placeholder the permission generator rendered survives.

use super::helpers::{custom_resource_registration, render_built_ins_template};
use alien_cloudformation::{CfExpression, CloudFormationTarget};
use alien_core::{
    ManagementPermissions, Network, NetworkSettings, PermissionProfile, RemoteStackManagement,
    ResourceLifecycle, ResourceRef, SetupManagement, Stack, StackSettings,
};

fn management_policies(network: NetworkSettings) -> (String, String) {
    management_policies_with(network, SetupManagement::Manual)
}

fn management_policies_with(
    network: NetworkSettings,
    setup_management: SetupManagement,
) -> (String, String) {
    let (template, yaml) = management_template(network, setup_management, false);
    (policies_of(&template), yaml)
}

/// The stack setup renders, optionally with the executor edge from management to the network
/// that preflights add for direct setup.
fn management_template(
    network: NetworkSettings,
    setup_management: SetupManagement,
    management_waits_for_network: bool,
) -> (alien_cloudformation::CfTemplate, String) {
    let management = RemoteStackManagement::new("management".to_string()).build();
    let stack = Stack::new("vpcscope".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["compute-cluster/management", "container/provision"]),
        ))
        .add(
            Network::new("default-network".to_string())
                .settings(network.clone())
                .build(),
            ResourceLifecycle::Frozen,
        );
    let stack = if management_waits_for_network {
        stack.add_with_dependencies(
            management,
            ResourceLifecycle::Frozen,
            vec![ResourceRef::new(Network::RESOURCE_TYPE, "default-network")],
        )
    } else {
        stack.add(management, ResourceLifecycle::Frozen)
    }
    .build();
    render_built_ins_template(
        &stack,
        StackSettings {
            network: Some(network),
            setup_management,
            ..StackSettings::default()
        },
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "management network scope",
    )
}

fn policies_of(template: &alien_cloudformation::CfTemplate) -> String {
    let policies: Vec<&CfExpression> = template
        .resources
        .values()
        .filter(|resource| {
            resource.resource_type == "AWS::IAM::ManagedPolicy"
                && resource
                    .logical_id
                    .starts_with("ManagementRoleManagementPolicy")
        })
        .filter_map(|resource| resource.properties.get("PolicyDocument"))
        .collect();
    assert!(!policies.is_empty(), "management policies render");
    serde_json::to_string(&policies).expect("policies serialize")
}

/// Direct setup orders management after the network so its policy names the finished network.
/// Generated setup already orders the policies through their references to the network, so the
/// edge changes nothing in the template, in every network mode and for automatic packages,
/// which emit no network.
#[test]
fn management_waiting_for_the_network_leaves_the_template_unchanged() {
    for setup_management in [SetupManagement::Manual, SetupManagement::Automatic] {
        for network in [
            NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            },
            NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-0123456789abcdef0".to_string(),
                public_subnet_ids: vec![],
                private_subnet_ids: vec!["subnet-0bbbbbbbbbbbbbbbb".to_string()],
                security_group_ids: vec![String::new()],
            },
            NetworkSettings::UseDefault,
        ] {
            let (_, without) = management_template(network.clone(), setup_management, false);
            let (_, with) = management_template(network.clone(), setup_management, true);
            assert_eq!(without, with, "{setup_management:?} {network:?}");
        }
    }
}

fn dump(name: &str, yaml: &str) {
    if let Ok(dir) = std::env::var("VPCSCOPE_DUMP_DIR") {
        std::fs::write(format!("{dir}/alien-cfn-{name}.yaml"), yaml).expect("dump writes");
    }
}

fn assert_no_placeholder(policies: &str) {
    assert!(
        !policies.contains("0a1e000000000000"),
        "a placeholder survived: {policies}"
    );
}

#[test]
fn a_created_network_pins_management_to_its_vpc() {
    let (policies, yaml) = management_policies(NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    });
    dump("create", &yaml);
    assert_no_placeholder(&policies);
    for expected in [
        r#"vpc/${StackVpcId}"#,
        r#"{"Ref":"DefaultNetworkVpc"}"#,
        r#"{"Ref":"DefaultNetworkPublicSubnet1"}"#,
        r#"{"Ref":"DefaultNetworkPrivateSubnet1"}"#,
        r#""ec2:VpcID""#,
        r#""elasticloadbalancing:Subnet""#,
        r#""autoscaling:VPCZoneIdentifiers""#,
        // An existing VPC is named by the installer.
        r#"{"Ref":"VpcId"}"#,
    ] {
        assert!(
            policies.contains(expected),
            "{expected} missing: {policies}"
        );
    }
}

#[test]
fn an_existing_vpc_pins_management_to_the_named_ids() {
    let (policies, yaml) = management_policies(NetworkSettings::ByoVpcAws {
        vpc_id: "vpc-0123456789abcdef0".to_string(),
        public_subnet_ids: vec!["subnet-0aaaaaaaaaaaaaaaa".to_string()],
        private_subnet_ids: vec!["subnet-0bbbbbbbbbbbbbbbb".to_string()],
        security_group_ids: vec![String::new()],
    });
    dump("byo", &yaml);
    assert_no_placeholder(&policies);
    for expected in [
        r#"{"Ref":"VpcId"}"#,
        r#"{"Ref":"PublicSubnetIds"}"#,
        r#"{"Ref":"PrivateSubnetIds"}"#,
    ] {
        assert!(
            policies.contains(expected),
            "{expected} missing: {policies}"
        );
    }
}

/// CloudFormation cannot look up the account's default VPC, so that mode keeps the
/// account-wide reach.
#[test]
fn the_default_vpc_keeps_the_account_wide_reach() {
    let (policies, yaml) = management_policies(NetworkSettings::UseDefault);
    dump("default", &yaml);
    assert_no_placeholder(&policies);
    assert!(policies.contains(r#"vpc/${StackVpcId}"#), "{policies}");
    assert!(!policies.contains(r#"{"Ref":"VpcId"}"#), "{policies}");
}

/// An automatic package leaves the network to the runtime, so the template has no network to
/// name: management may launch and create security groups only in a VPC and subnets that carry
/// the stack's tags.
#[test]
fn a_network_management_creates_scopes_management_by_its_tags() {
    let (policies, yaml) = management_policies_with(
        NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        },
        SetupManagement::Automatic,
    );
    dump("automatic-create", &yaml);
    assert_no_placeholder(&policies);
    assert!(!policies.contains("DefaultNetwork"), "{policies}");
    assert!(!policies.contains(r#""Ref":"VpcId""#), "{policies}");
    let launch = r#"{"Action":["ec2:RunInstances"],"Condition":{"StringEquals":{"aws:ResourceTag/deployment":{"Fn::Sub":"${AWS::StackName}"}"#;
    assert!(
        policies.contains(launch),
        "launches need the stack's subnet tags: {policies}"
    );
}
