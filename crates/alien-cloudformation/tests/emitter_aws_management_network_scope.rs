//! The management role's reach on a compute stack, per network mode.

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfRegistry, CfTemplate, CloudFormationOptions,
    RegistrationMode,
};
use alien_core::{
    CapacityGroup, ComputeCluster, Container, ContainerCode, ExposeProtocol, ManagementPermissions,
    Network, NetworkSettings, PermissionProfile, RemoteStackManagement, ResourceLifecycle,
    ResourceSpec, ServiceAccount, Stack, StackSettings,
};

fn render_json(network: NetworkSettings) -> serde_json::Value {
    serde_json::to_value(render(network)).unwrap()
}

fn render(network: NetworkSettings) -> CfTemplate {
    let stack = Stack::new("vpcscope".to_string())
        // The sets the management role of a compute stack carries (as in a 1.0.200 package).
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global([
                "compute-cluster/heartbeat",
                "compute-cluster/management",
                "container/provision",
            ]),
        ))
        .add(
            Network::new("default-network".to_string())
                .settings(network.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("m7g.large".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    nested_virtualization: None,
                    scale_policy: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Container::new("api".to_string())
                .cluster("compute".to_string())
                .code(ContainerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api:1.2.3".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "1".to_string(),
                    desired: "1".to_string(),
                })
                .memory(ResourceSpec {
                    min: "1Gi".to_string(),
                    desired: "1Gi".to_string(),
                })
                .public_endpoint(alien_core::PublicEndpoint {
                    name: "api".to_string(),
                    port: 8080,
                    protocol: ExposeProtocol::Http,
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let registry = CfRegistry::built_in();
    generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: StackSettings {
                network: Some(network),
                ..StackSettings::default()
            },
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("vpcscope".to_string()),
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .expect("template renders")
}

#[test]
fn dump_templates() {
    let Ok(dir) = std::env::var("VPCSCOPE_DUMP_DIR") else {
        return;
    };
    for (name, network) in [
        (
            "create",
            NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            },
        ),
        (
            "byo",
            NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-0123456789abcdef0".to_string(),
                public_subnet_ids: vec!["subnet-0aaaaaaaaaaaaaaaa".to_string()],
                private_subnet_ids: vec!["subnet-0bbbbbbbbbbbbbbbb".to_string()],
                security_group_ids: vec![],
            },
        ),
        ("default", NetworkSettings::UseDefault),
    ] {
        let yaml = to_yaml(&render(network)).unwrap();
        std::fs::write(format!("{dir}/compute-stack-{name}.yaml"), yaml).unwrap();
    }
}

/// Every statement of the management role's managed policies.
fn management_statements(template: &serde_json::Value) -> Vec<serde_json::Value> {
    let resources = template["Resources"].as_object().unwrap();
    let statements: Vec<serde_json::Value> = resources
        .iter()
        .filter(|(id, _)| id.starts_with("ManagementRoleManagementPolicy"))
        .flat_map(|(_, resource)| {
            resource["Properties"]["PolicyDocument"]["Statement"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect();
    assert!(
        !statements.is_empty(),
        "the template has management policies"
    );
    statements
}

fn actions(statement: &serde_json::Value) -> Vec<String> {
    match &statement["Action"] {
        serde_json::Value::String(action) => vec![action.clone()],
        serde_json::Value::Array(actions) => actions
            .iter()
            .map(|action| action.as_str().unwrap().to_string())
            .collect(),
        other => panic!("unexpected Action {other}"),
    }
}

/// The creates that place something in a network: the condition operator and key that name the
/// network, and whether that operator also needs `Null: false` (a `ForAllValues` test passes
/// when the request names nothing).
const NETWORK_CREATES: [(&str, &str, &str, bool); 4] = [
    (
        "ec2:CreateSecurityGroup",
        "StringLikeIfExists",
        "ec2:VpcID",
        false,
    ),
    (
        "elasticloadbalancing:CreateLoadBalancer",
        "ForAllValues:StringLike",
        "elasticloadbalancing:Subnet",
        true,
    ),
    (
        "autoscaling:CreateAutoScalingGroup",
        "ForAllValues:StringLike",
        "autoscaling:VPCZoneIdentifiers",
        true,
    ),
    ("ec2:RunInstances", "StringLike", "ec2:Vpc", false),
];

/// The network mode an install picks with the template's network parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Create,
    UseExisting,
    UseDefault,
}

/// What one branch of a condition value names: the `Ref`s it uses and its literal strings.
#[derive(Debug, Default)]
struct Named {
    refs: Vec<String>,
    literals: Vec<String>,
}

/// Resolves the template's `Fn::If`s for `mode` (every availability zone in use) and collects
/// what the value names. `Fn::Sub` templates are kept as literals with their variables resolved.
fn resolve(value: &serde_json::Value, mode: Mode, named: &mut Named) {
    use serde_json::Value;
    match value {
        Value::String(literal) => named.literals.push(literal.clone()),
        Value::Array(items) => items.iter().for_each(|item| resolve(item, mode, named)),
        Value::Object(map) => {
            assert_eq!(map.len(), 1, "unexpected network value {value}");
            let (function, args) = map.iter().next().unwrap();
            match function.as_str() {
                "Ref" => {
                    let name = args.as_str().unwrap();
                    if name != "AWS::NoValue" {
                        named.refs.push(name.to_string());
                    }
                }
                "Fn::If" => {
                    let condition = args[0].as_str().unwrap();
                    let taken = match condition {
                        "NetworkModeCreate" => mode == Mode::Create,
                        "NetworkModeUseExisting" => mode == Mode::UseExisting,
                        zone if zone.starts_with("NetworkCreateUseAz") => true,
                        other => panic!("unexpected condition {other} in a network value"),
                    };
                    resolve(&args[if taken { 1 } else { 2 }], mode, named);
                }
                "Fn::Split" | "Fn::Join" => resolve(&args[1], mode, named),
                "Fn::Sub" => match args {
                    Value::String(template) => named.literals.push(template.clone()),
                    Value::Array(parts) => {
                        named.literals.push(parts[0].as_str().unwrap().to_string());
                        for variable in parts[1].as_object().unwrap().values() {
                            resolve(variable, mode, named);
                        }
                    }
                    other => panic!("unexpected Fn::Sub {other}"),
                },
                other => panic!("unexpected {other} in a network value"),
            }
        }
        other => panic!("unexpected network value {other}"),
    }
}

/// For every network create management can make: asserts the network key sits under the
/// expected operator (and only there), with `Null: false` where the operator needs it, and
/// returns what the key names in `mode`, one entry per granting statement.
fn network_values(template: &serde_json::Value, mode: Mode) -> Vec<(String, Named)> {
    let statements = management_statements(template);
    let mut values = Vec::new();
    for (action, operator, key, needs_present) in NETWORK_CREATES {
        let granting: Vec<_> = statements
            .iter()
            .filter(|statement| statement["Effect"] == "Allow")
            .filter(|statement| actions(statement).iter().any(|a| a == action))
            // RunInstances also names images and volumes, which carry no network.
            .filter(|statement| {
                action != "ec2:RunInstances"
                    || statement["Resource"].to_string().contains(":subnet/")
            })
            .collect();
        assert!(!granting.is_empty(), "management can {action}");
        for statement in granting {
            let conditions = statement["Condition"].as_object().unwrap_or_else(|| {
                panic!("{action} has no condition: {statement}");
            });
            for (other, tests) in conditions {
                if other != operator && other != "Null" {
                    assert!(
                        tests.get(key).is_none(),
                        "{action} also tests {key} with {other}: {statement}"
                    );
                }
            }
            if needs_present {
                assert_eq!(
                    conditions["Null"][key], "false",
                    "{action} passes when the request names no network: {statement}"
                );
            }
            let value = conditions
                .get(operator)
                .and_then(|tests| tests.get(key))
                .unwrap_or_else(|| {
                    panic!("{action} is not conditioned on {operator} {key}: {statement}")
                });
            let mut named = Named::default();
            resolve(value, mode, &mut named);
            values.push((format!("{action} {key} ({mode:?}): {statement}"), named));
        }
    }
    // A group update may never name subnets: those are not checked against RunInstances.
    let updates: Vec<_> = statements
        .iter()
        .filter(|statement| {
            actions(statement)
                .iter()
                .any(|a| a == "autoscaling:UpdateAutoScalingGroup")
        })
        .collect();
    assert!(!updates.is_empty(), "management can update its groups");
    for statement in updates {
        assert_eq!(
            statement["Condition"]["Null"]["autoscaling:VPCZoneIdentifiers"], "true",
            "a group update can name subnets: {statement}"
        );
    }
    values
}

/// Asserts a branch names only the given references, at least one, and no wildcard of its own.
/// The only literals allowed are `Fn::Sub` templates that put a reference into a VPC ARN.
fn assert_names_only(values: &[(String, Named)], allowed: &[&str]) {
    for (what, named) in values {
        assert!(!named.refs.is_empty(), "names no network: {what}");
        for name in &named.refs {
            assert!(
                allowed.contains(&name.as_str()),
                "names {name}, not the stack's network: {what}"
            );
        }
        for literal in &named.literals {
            assert!(
                literal.ends_with(":vpc/${StackVpcId}") && !literal.contains('*'),
                "names the literal {literal:?}: {what}"
            );
        }
    }
}

const CREATED_NETWORK: [&str; 7] = [
    "DefaultNetworkVpc",
    "DefaultNetworkPublicSubnet1",
    "DefaultNetworkPublicSubnet2",
    "DefaultNetworkPublicSubnet3",
    "DefaultNetworkPrivateSubnet1",
    "DefaultNetworkPrivateSubnet2",
    "DefaultNetworkPrivateSubnet3",
];
const GIVEN_NETWORK: [&str; 3] = ["VpcId", "PublicSubnetIds", "PrivateSubnetIds"];

#[test]
fn a_created_network_scopes_management_to_its_vpc_and_subnets() {
    let template = render_json(NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    });
    // The template lets the installer pick the network mode; each branch names its own network.
    assert_names_only(&network_values(&template, Mode::Create), &CREATED_NETWORK);
    assert_names_only(
        &network_values(&template, Mode::UseExisting),
        &GIVEN_NETWORK,
    );
    // The default VPC is not known to the template: that branch alone stays a wildcard
    // (a documented exception).
    for (what, named) in network_values(&template, Mode::UseDefault) {
        assert!(named.refs.is_empty(), "{what}");
        assert!(
            named
                .literals
                .iter()
                .all(|literal| literal == "*" || literal.ends_with(":vpc/${StackVpcId}")),
            "{what}"
        );
    }
}

#[test]
fn an_existing_vpc_scopes_management_to_its_ids() {
    let template = render_json(NetworkSettings::ByoVpcAws {
        vpc_id: "vpc-0123456789abcdef0".to_string(),
        public_subnet_ids: vec!["subnet-0aaaaaaaaaaaaaaaa".to_string()],
        private_subnet_ids: vec!["subnet-0bbbbbbbbbbbbbbbb".to_string()],
        security_group_ids: vec![],
    });
    // No network-mode choice is left in this template: every mode resolves to the given IDs.
    for mode in [Mode::Create, Mode::UseExisting, Mode::UseDefault] {
        assert_names_only(&network_values(&template, mode), &GIVEN_NETWORK);
    }
}

/// Applies `change` to the condition of the first management statement granting `action`.
fn weaken(
    template: &serde_json::Value,
    action: &str,
    change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) -> serde_json::Value {
    let mut template = template.clone();
    let statement = template["Resources"]
        .as_object_mut()
        .unwrap()
        .iter_mut()
        .filter(|(id, _)| id.starts_with("ManagementRoleManagementPolicy"))
        .flat_map(|(_, resource)| {
            resource["Properties"]["PolicyDocument"]["Statement"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
        })
        .find(|statement| actions(statement).iter().any(|a| a == action))
        .unwrap();
    change(statement["Condition"].as_object_mut().unwrap());
    template
}

fn rejected(template: &serde_json::Value, mode: Mode, allowed: &[&str]) -> bool {
    std::panic::catch_unwind(|| assert_names_only(&network_values(template, mode), allowed))
        .is_err()
}

#[test]
fn the_network_checks_reject_a_weakened_policy() {
    let template = render_json(NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    });
    const SUBNET: &str = "elasticloadbalancing:Subnet";
    let use_existing_branch = |conditions: &mut serde_json::Map<String, serde_json::Value>| {
        // Fn::If [NetworkModeCreate, created, Fn::If [NetworkModeUseExisting, given, *]]
        conditions["ForAllValues:StringLike"][SUBNET]["Fn::If"][2]["Fn::If"][1] =
            serde_json::json!(["*"]);
    };
    let weakened = [
        // One branch allows any subnet while the other still names the stack's network.
        (
            weaken(
                &template,
                "elasticloadbalancing:CreateLoadBalancer",
                use_existing_branch,
            ),
            Mode::UseExisting,
            &GIVEN_NETWORK[..],
        ),
        // A request that names no subnet passes ForAllValues.
        (
            weaken(
                &template,
                "elasticloadbalancing:CreateLoadBalancer",
                |conditions| {
                    conditions.remove("Null");
                },
            ),
            Mode::Create,
            &CREATED_NETWORK[..],
        ),
        // The key keeps its references under an operator that matches anything else too.
        (
            weaken(&template, "ec2:CreateSecurityGroup", |conditions| {
                let value = conditions["StringLikeIfExists"]["ec2:VpcID"].clone();
                conditions.insert(
                    "ForAnyValue:StringLike".to_string(),
                    serde_json::json!({ "ec2:VpcID": [value, "*"] }),
                );
            }),
            Mode::Create,
            &CREATED_NETWORK[..],
        ),
    ];
    for (template, mode, allowed) in weakened {
        assert!(rejected(&template, mode, allowed), "accepted: {mode:?}");
    }
    assert!(!rejected(&template, Mode::Create, &CREATED_NETWORK));
}
