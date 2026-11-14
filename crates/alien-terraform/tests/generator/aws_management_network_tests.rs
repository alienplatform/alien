//! The management role creates instances, security groups and load balancers only inside the
//! stack's own network: the module renders the network conditions with the VPC and subnets
//! Terraform knows on apply.

use super::helpers::{assert_terraform_valid, render, role_policy_documents};
use alien_core::{
    CapacityGroup, ComputeCluster, ManagementPermissions, Network, NetworkSettings,
    PermissionProfile, RemoteStackManagement, ResourceLifecycle, Stack, StackSettings,
};
use alien_terraform::TerraformTarget;
use hcl::eval::{Context as EvalContext, Evaluate, FuncArgs, FuncDef, ParamType};
use hcl::Value;
use serde_json::{json, Value as Json};

const CREATED_VPC: &str = "vpc-0c0000000000000a1";
const EXISTING_VPC: &str = "vpc-0e0000000000000b2";

fn stack(settings: NetworkSettings) -> Stack {
    stack_builder(settings).build()
}

fn stack_builder(settings: NetworkSettings) -> alien_core::StackBuilder {
    Stack::new("scope".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["compute-cluster/management", "container/provision"]),
        ))
        .add(
            Network::new("net".to_string()).settings(settings).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
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
                    scale_policy: None,
                    nested_virtualization: None,
                    data_disk_gib_range: None,
                })
                .build(),
            ResourceLifecycle::Live,
        )
}

fn strings(values: &[&str]) -> Value {
    Value::Array(values.iter().map(|value| Value::from(*value)).collect())
}

fn string_arg(args: &FuncArgs, index: usize) -> String {
    args[index].as_str().expect("string argument").to_string()
}

fn list_arg(value: &Value) -> Vec<Value> {
    value.as_array().expect("list argument").clone()
}

/// The Terraform functions the network expressions use.
fn functions(context: &mut EvalContext<'_>) {
    context.declare_func(
        "replace",
        FuncDef::builder()
            .params([ParamType::String, ParamType::String, ParamType::String])
            .build(|args| {
                Ok(Value::from(
                    string_arg(&args, 0).replace(&string_arg(&args, 1), &string_arg(&args, 2)),
                ))
            }),
    );
    context.declare_func(
        "concat",
        FuncDef::builder()
            .variadic_param(ParamType::Any)
            .build(|args| {
                Ok(Value::Array(
                    args.variadic_args().flat_map(list_arg).collect(),
                ))
            }),
    );
    context.declare_func(
        "compact",
        FuncDef::builder().param(ParamType::Any).build(|args| {
            Ok(Value::Array(
                list_arg(&args[0])
                    .into_iter()
                    .filter(|value| value.as_str() != Some(""))
                    .collect(),
            ))
        }),
    );
    context.declare_func(
        "trimspace",
        FuncDef::builder()
            .param(ParamType::String)
            .build(|args| Ok(Value::from(string_arg(&args, 0).trim()))),
    );
    context.declare_func(
        "length",
        FuncDef::builder()
            .param(ParamType::Any)
            .build(|args| Ok(Value::from(list_arg(&args[0]).len() as u64))),
    );
    context.declare_func(
        "substr",
        FuncDef::builder()
            .params([ParamType::String, ParamType::Number, ParamType::Number])
            .build(|args| {
                let text = string_arg(&args, 0);
                let offset = args[1].as_u64().expect("offset") as usize;
                let length = args[2].as_u64().expect("length") as usize;
                Ok(Value::from(
                    text.chars().skip(offset).take(length).collect::<String>(),
                ))
            }),
    );
}

/// The management policy documents as AWS receives them, for the given variable values.
fn management_documents(stack: &Stack, settings: NetworkSettings, vars: Json) -> Vec<Json> {
    let module = render(
        stack,
        TerraformTarget::Aws,
        StackSettings {
            network: Some(settings),
            ..StackSettings::default()
        },
    );
    assert_terraform_valid(&module, "management network scope");
    // The boundary is the ceiling setup installs and the runtime checks without a network, so
    // only the role's own policies name it.
    let boundary = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(_, contents)| hcl::parse(contents).unwrap().into_blocks())
        .find(|block| {
            block.identifier() == "resource"
                && block.labels()[0].as_str() == "aws_iam_policy"
                && block.labels()[1].as_str() == "management_permissions_boundary"
        })
        .expect("the module installs the boundary");
    let boundary = hcl::format::to_string(boundary.body()).unwrap();
    assert!(
        !boundary.contains("0a1e0000") && !boundary.contains("_network_"),
        "the boundary names no network: {boundary}"
    );
    let documents: Vec<_> = role_policy_documents(&module, "management")
        .into_iter()
        .filter(|(address, _)| address.contains("management_managed_policy"))
        .collect();
    assert!(!documents.is_empty());
    let context = evaluation_context(&module, vars);
    documents
        .into_iter()
        .map(|(address, document)| {
            let document = serde_json::to_value(
                document
                    .evaluate(&context)
                    .unwrap_or_else(|error| panic!("{address} evaluates: {error}")),
            )
            .unwrap();
            assert!(
                !document.to_string().contains("0a1e0000"),
                "{address} keeps a network placeholder: {document:#}"
            );
            document
        })
        .collect()
}

/// The module's network locals and the values Terraform knows on apply, for the given
/// variable values.
fn evaluation_context(module: &alien_terraform::ModuleFiles, vars: Json) -> EvalContext<'static> {
    let mut context = EvalContext::new();
    functions(&mut context);
    for (name, value) in [
        (
            "data",
            json!({"aws_region": {"current": {"region": "us-west-2"}}, "aws_caller_identity": {"current": {"account_id": "123456789012"}}}),
        ),
        ("var", vars),
        ("aws_vpc", json!({"net": [{"id": CREATED_VPC}]})),
        (
            "aws_subnet",
            json!({
                "net_public": [{"id": "subnet-0c00000000000pub1"}, {"id": "subnet-0c00000000000pub2"}],
                "net_private": [{"id": "subnet-0c0000000000priv1"}, {"id": "subnet-0c0000000000priv2"}],
            }),
        ),
    ] {
        context.declare_var(name, hcl::to_value(value).unwrap());
    }
    // The network locals, from the module.
    let mut locals = serde_json::Map::new();
    locals.insert("resource_prefix".to_string(), json!("scope"));
    for (path, contents) in module.iter() {
        if !path.ends_with(".tf") {
            continue;
        }
        for block in hcl::parse(contents).unwrap().into_blocks() {
            if block.identifier() != "locals" {
                continue;
            }
            for attribute in block.body().attributes() {
                if attribute.key().contains("_network_") {
                    let value = attribute.expr().evaluate(&context).unwrap_or_else(|error| {
                        panic!("local.{} evaluates: {error}", attribute.key())
                    });
                    locals.insert(
                        attribute.key().to_string(),
                        serde_json::to_value(value).unwrap(),
                    );
                }
            }
        }
    }
    context.declare_var("local", hcl::to_value(Json::Object(locals)).unwrap());
    context
}

/// Every value of `key` in any condition of a statement that allows `action`.
fn condition_values(documents: &[Json], action: &str, key: &str) -> Vec<Json> {
    let mut values = Vec::new();
    for statement in documents
        .iter()
        .flat_map(|document| document["Statement"].as_array().unwrap())
    {
        let actions = match &statement["Action"] {
            Json::String(single) => vec![single.clone()],
            Json::Array(many) => many
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect(),
            _ => Vec::new(),
        };
        if statement["Effect"] != "Allow" || !actions.iter().any(|candidate| candidate == action) {
            continue;
        }
        if let Some(conditions) = statement["Condition"].as_object() {
            for operator in conditions.values() {
                if let Some(value) = operator.get(key) {
                    values.push(value.clone());
                }
            }
        }
    }
    values
}

/// The policy names nine subnet entries; those past the stack's subnets are blank.
fn assert_scoped(documents: &[Json], vpc: &str, subnets: &[&str]) {
    let mut padded: Vec<&str> = subnets.to_vec();
    padded.resize(9, "");
    let subnets = json!(padded);
    let vpc_arn = json!(format!("arn:aws:ec2:us-west-2:123456789012:vpc/{vpc}"));
    let launches = condition_values(documents, "ec2:RunInstances", "ec2:Vpc");
    assert!(!launches.is_empty(), "instance launches name the VPC");
    assert!(
        launches.iter().all(|value| value == &vpc_arn),
        "{launches:?}"
    );
    let groups = condition_values(documents, "ec2:CreateSecurityGroup", "ec2:VpcID");
    assert!(!groups.is_empty(), "security groups name the VPC");
    assert!(groups.iter().all(|value| value == vpc), "{groups:?}");
    for (action, key) in [
        (
            "elasticloadbalancing:CreateLoadBalancer",
            "elasticloadbalancing:Subnet",
        ),
        (
            "autoscaling:CreateAutoScalingGroup",
            "autoscaling:VPCZoneIdentifiers",
        ),
    ] {
        let values = condition_values(documents, action, key);
        let named: Vec<_> = values.iter().filter(|value| value.is_array()).collect();
        assert!(!named.is_empty(), "{action} names the subnets");
        assert!(
            named.iter().all(|value| **value == subnets),
            "{action}: {values:?}"
        );
    }
}

#[test]
fn aws_management_policy_names_the_network_setup_creates() {
    let settings = NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    let documents = management_documents(
        &stack(settings.clone()),
        settings,
        json!({"network_mode": "create-new", "vpc_id": "", "public_subnet_ids": [], "private_subnet_ids": []}),
    );
    assert_scoped(
        &documents,
        CREATED_VPC,
        &[
            "subnet-0c00000000000pub1",
            "subnet-0c00000000000pub2",
            "subnet-0c0000000000priv1",
            "subnet-0c0000000000priv2",
        ],
    );
}

#[test]
fn aws_management_policy_names_an_existing_network() {
    let settings = NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    let documents = management_documents(
        &stack(settings.clone()),
        settings,
        json!({
            "network_mode": "use-existing",
            "vpc_id": EXISTING_VPC,
            "public_subnet_ids": [" subnet-0e00000000000pub1", ""],
            "private_subnet_ids": ["subnet-0e0000000000priv1"],
        }),
    );
    assert_scoped(
        &documents,
        EXISTING_VPC,
        &["subnet-0e00000000000pub1", "subnet-0e0000000000priv1"],
    );
}

/// Blank subnet lists name no subnet; they never widen to any subnet.
#[test]
fn aws_management_policy_with_blank_existing_subnets_names_none() {
    let settings = NetworkSettings::ByoVpcAws {
        vpc_id: EXISTING_VPC.to_string(),
        public_subnet_ids: vec![],
        private_subnet_ids: vec![],
        security_group_ids: vec![String::new()],
    };
    let documents = management_documents(
        &stack(settings.clone()),
        settings,
        json!({
            "net_vpc_id": EXISTING_VPC,
            "net_public_subnet_ids": [""],
            "net_private_subnet_ids": [],
            "net_security_group_ids": [""],
        }),
    );
    assert_scoped(&documents, EXISTING_VPC, &[]);
}

/// Direct setup orders management after the network so its policy names the finished network.
/// The module already orders the policies through their references to the network, so the edge
/// changes nothing, in every network mode and for automatic packages, which emit no network.
#[test]
fn aws_management_waiting_for_the_network_leaves_the_module_unchanged() {
    for setup_management in [
        alien_core::SetupManagement::Manual,
        alien_core::SetupManagement::Automatic,
    ] {
        for settings in [
            NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            },
            NetworkSettings::ByoVpcAws {
                vpc_id: EXISTING_VPC.to_string(),
                public_subnet_ids: vec![],
                private_subnet_ids: vec!["subnet-0e0000000000priv1".to_string()],
                security_group_ids: vec![String::new()],
            },
            NetworkSettings::UseDefault,
        ] {
            let module = |waits: bool| {
                let mut stack = stack(settings.clone());
                if waits {
                    stack
                        .resources
                        .get_mut("management")
                        .expect("management")
                        .dependencies
                        .push(alien_core::ResourceRef::new(Network::RESOURCE_TYPE, "net"));
                }
                render(
                    &stack,
                    TerraformTarget::Aws,
                    StackSettings {
                        network: Some(settings.clone()),
                        setup_management,
                        ..StackSettings::default()
                    },
                )
            };
            let (without, with) = (module(false), module(true));
            for (path, contents) in without.iter() {
                assert_eq!(
                    Some(contents),
                    with.files.get(path).map(String::as_str),
                    "{path}: {setup_management:?} {settings:?}"
                );
            }
            assert_eq!(without, with, "{setup_management:?} {settings:?}");
        }
    }
}

/// The policy names at most nine subnets. A network with more fails the plan on the management
/// role's precondition instead of leaving the extra subnets out of the policy.
#[test]
fn aws_management_policy_refuses_more_subnets_than_it_names() {
    let subnets = |count: usize| -> Vec<String> {
        (1..=count)
            .map(|index| format!("subnet-0e000000000000{index:03}"))
            .collect()
    };
    let existing = NetworkSettings::ByoVpcAws {
        vpc_id: EXISTING_VPC.to_string(),
        public_subnet_ids: vec![],
        private_subnet_ids: vec!["subnet-0e0000000000priv1".to_string()],
        security_group_ids: vec![String::new()],
    };
    let created = NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    for (settings, vars) in [
        (
            existing,
            Box::new(|count: usize, _mode: &str| {
                json!({
                    "net_vpc_id": EXISTING_VPC,
                    "net_public_subnet_ids": [""],
                    "net_private_subnet_ids": subnets(count),
                    "net_security_group_ids": [""],
                })
            }) as Box<dyn Fn(usize, &str) -> Json>,
        ),
        (
            created,
            Box::new(|count: usize, mode: &str| {
                json!({
                    "network_mode": mode,
                    "vpc_id": EXISTING_VPC,
                    "public_subnet_ids": [],
                    "private_subnet_ids": subnets(count),
                })
            }),
        ),
    ] {
        let module = render(
            &stack(settings.clone()),
            TerraformTarget::Aws,
            StackSettings {
                network: Some(settings.clone()),
                ..StackSettings::default()
            },
        );
        assert_terraform_valid(&module, "management subnet limit");
        let role = module
            .iter()
            .filter(|(path, _)| path.ends_with(".tf"))
            .flat_map(|(_, contents)| hcl::parse(contents).unwrap().into_blocks())
            .find(|block| {
                block.identifier() == "resource"
                    && block.labels()[0].as_str() == "aws_iam_role"
                    && block.labels()[1].as_str() == "management"
            })
            .expect("the module creates the management role");
        let condition = role
            .body()
            .blocks()
            .filter(|block| block.identifier() == "lifecycle")
            .flat_map(|lifecycle| lifecycle.body().blocks())
            .filter(|block| block.identifier() == "precondition")
            .flat_map(|precondition| precondition.body().attributes())
            .find(|attribute| attribute.key() == "condition")
            .expect("the management role checks the subnet count")
            .expr()
            .clone();
        // The check reads the installer's variables only, never the network's resources.
        let text = hcl::format::to_string(&condition).unwrap();
        assert!(
            !text.contains("aws_subnet") && !text.contains("local."),
            "{text}"
        );
        for (count, mode, allowed) in [
            (1, "use-existing", true),
            (9, "use-existing", true),
            (10, "use-existing", false),
            // Subnets setup creates never exceed the limit; the variables are not used.
            (10, "create-new", true),
        ] {
            if matches!(settings, NetworkSettings::ByoVpcAws { .. }) && mode != "use-existing" {
                continue;
            }
            let context = evaluation_context(&module, vars(count, mode));
            assert_eq!(
                condition.evaluate(&context).unwrap(),
                Value::Bool(allowed),
                "{settings:?}: {count} subnets, {mode}"
            );
        }
    }
}

/// On a Kubernetes target the module creates no sandbox gateway subnets and declares no
/// variable for them (the sandbox runs in the cluster), so the management policy must not name
/// them either. With an internet sandbox and its gateway in the stack, the module still
/// validates and names only the network's own subnets.
#[test]
fn aws_management_policy_on_eks_names_no_sandbox_gateway_subnets() {
    use alien_core::{
        sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, Container, ContainerCode, ResourceSpec, Sandbox,
        SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    };
    for settings in [
        NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        },
        NetworkSettings::ByoVpcAws {
            vpc_id: EXISTING_VPC.to_string(),
            public_subnet_ids: vec![],
            private_subnet_ids: vec!["subnet-0e0000000000priv1".to_string()],
            security_group_ids: vec![String::new()],
        },
    ] {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "s3://vpcscope-artifacts/agents/bundle.zip".to_string(),
                bundled_from: None,
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let gateway = Container::new(SANDBOX_EGRESS_GATEWAY_ID.to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "ghcr.io/example/gateway:1".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.25".to_string(),
                desired: "0.25".to_string(),
            })
            .memory(ResourceSpec {
                min: "256Mi".to_string(),
                desired: "256Mi".to_string(),
            })
            .permissions("execution".to_string())
            .build();
        let stack = stack_builder(settings.clone())
            .add(sandbox, ResourceLifecycle::Frozen)
            .add(gateway, ResourceLifecycle::Live)
            .build();
        assert!(alien_core::sandbox_egress::has_sandbox_gateway(&stack));
        let module = render(
            &stack,
            TerraformTarget::Eks,
            StackSettings {
                network: Some(settings.clone()),
                ..StackSettings::default()
            },
        );
        assert_terraform_valid(&module, "management network scope on EKS");
        let text = module
            .iter()
            .filter(|(path, _)| path.ends_with(".tf"))
            .map(|(_, contents)| contents.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("_network_subnet_ids"),
            "{settings:?}: the policy names the network"
        );
        assert!(
            !text.contains("sandbox_gateway"),
            "{settings:?}: an EKS module names gateway subnets it does not have"
        );
    }
}
