//! AWS internet sandboxes in setup templates.
//!
//! A setup template is rendered before any deployment exists, so its stack is prepared with no
//! compute backend. These tests prepare a stack exactly that way, render it, and lint the result:
//! an internet sandbox's connector routes through the managed egress gateway, so the template has
//! to create the gateway's subnets and parameters, and must never reference ones it does not
//! declare.

use alien_cloudformation::{
    generate_cloudformation_template, test_utils::cfn_lint, to_yaml, CfEmitter, CfExpression,
    CfRegistry, CfResource, CfTemplate, CloudFormationOptions, CloudFormationTarget,
    RegistrationMode,
};
use alien_core::{
    import::EmitContext, sandbox_egress::has_sandbox_gateway, AwsManagementConfig, ComputeCluster,
    Container, ContainerCode, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, Network, NetworkSettings, PermissionProfile, Platform, ResourceLifecycle,
    ResourceSpec, Result, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings, StackState,
};
use alien_preflights::runner::PreflightRunner;

/// Compute clusters are rendered by a platform extension, not the built-in registry. Their own
/// resources are not what these tests are about, so this stands in for that extension with the
/// one resource the generator references: the node instance role.
struct ExtensionComputeCluster;

impl CfEmitter for ExtensionComputeCluster {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let prefix = ctx.name_for(ctx.resource_id).expect("cluster logical id");
        let mut role = CfResource::new(
            format!("{prefix}InstanceRole"),
            "AWS::IAM::Role".to_string(),
        );
        role.properties.insert(
            "AssumeRolePolicyDocument".to_string(),
            CfExpression::object([
                ("Version", CfExpression::from("2012-10-17")),
                (
                    "Statement",
                    CfExpression::list([CfExpression::object([
                        ("Effect", CfExpression::from("Allow")),
                        (
                            "Principal",
                            CfExpression::object([(
                                "Service",
                                CfExpression::from("ec2.amazonaws.com"),
                            )]),
                        ),
                        ("Action", CfExpression::from("sts:AssumeRole")),
                    ])]),
                ),
            ]),
        );
        Ok(vec![role])
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> Result<CfExpression> {
        Ok(CfExpression::object([(
            "clusterId",
            CfExpression::from("compute"),
        )]))
    }
}

#[derive(Clone, Copy, Debug)]
enum Shape {
    SandboxOnly,
    SandboxAndContainer,
}

fn settings() -> StackSettings {
    StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        ..StackSettings::default()
    }
}

fn stack(shape: Shape, egress: SandboxEgress) -> Stack {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
        })
        .egress(egress)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let mut stack = Stack::new("acme".to_string())
        .permission("execution", PermissionProfile::new())
        .add(sandbox, ResourceLifecycle::Live);
    if matches!(shape, Shape::SandboxAndContainer) {
        stack = stack.add(
            Container::new("api".to_string())
                .code(ContainerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:1".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.25".to_string(),
                    desired: "0.25".to_string(),
                })
                .memory(ResourceSpec {
                    min: "512Mi".to_string(),
                    desired: "512Mi".to_string(),
                })
                .replicas(1)
                .permissions("execution".to_string())
                .port(8080)
                .build(),
            ResourceLifecycle::Live,
        );
    }
    stack.build()
}

/// Prepare the stack the way a setup template is prepared: template preflights, then mutations
/// against a configuration that has no compute backend yet.
async fn prepare_for_setup_template(stack: Stack, settings: &StackSettings) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Aws)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(settings.clone())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: String::new(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    runner
        .apply_mutations(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("template mutations apply")
}

fn render(stack: &Stack, settings: StackSettings, description: &str) -> Result<CfTemplate> {
    let mut registry = CfRegistry::built_in();
    registry.register(
        ComputeCluster::RESOURCE_TYPE,
        Platform::Aws,
        ExtensionComputeCluster,
    );
    generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            registry: &registry,
            target: CloudFormationTarget::Aws,
            stack_settings: settings,
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::CustomResource {
                lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
                callback_url: None,
            },
            description: Some(description.to_string()),
        },
    )
}

/// The setup template for an internet sandbox carries the gateway's network, whether or not
/// the stack also has containers, and CloudFormation accepts it.
#[tokio::test]
async fn an_internet_sandbox_setup_template_carries_the_gateway_network() {
    for shape in [Shape::SandboxOnly, Shape::SandboxAndContainer] {
        let description = format!("internet sandbox setup template, {shape:?}");
        let settings = settings();
        let prepared =
            prepare_for_setup_template(stack(shape, SandboxEgress::Internet), &settings).await;
        assert!(
            has_sandbox_gateway(&prepared),
            "{description}: setup preparation must install the egress gateway"
        );

        let template = render(&prepared, settings, &description)
            .unwrap_or_else(|error| panic!("{description}: {error}"));
        let yaml = to_yaml(&template).expect("template serializes");
        cfn_lint(&yaml).assert_ok(&description);
        for (kind, name) in [
            ("resource", "SandboxGatewaySubnet1"),
            ("parameter", "SandboxGatewaySubnetIds"),
        ] {
            let present = match kind {
                "resource" => template.resources.keys().any(|id| id.ends_with(name)),
                _ => template.parameters.contains_key(name),
            };
            assert!(present, "{description}: gateway {kind} {name} is missing");
        }
    }
}

/// A stack that reaches the generator with an internet sandbox but no gateway was not prepared.
/// Rendering it would reference gateway subnets and parameters the template never declares, so
/// the generator refuses and names the sandbox.
#[test]
fn an_internet_sandbox_without_its_gateway_is_refused() {
    let unprepared = Stack::new("acme".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings().network.expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .build();

    let error = render(&unprepared, settings(), "unprepared internet sandbox")
        .expect_err("an internet sandbox without its gateway must not render");
    assert_eq!(error.code, "SANDBOX_INTERNET_GATEWAY_MISSING");
    assert!(error.message.contains("'agents'"), "{}", error.message);
}

/// Legacy declarations never get a gateway, with or without containers in the stack.
#[tokio::test]
async fn a_legacy_sandbox_setup_template_has_no_gateway() {
    for shape in [Shape::SandboxOnly, Shape::SandboxAndContainer] {
        for egress in [SandboxEgress::None, SandboxEgress::Allow] {
            let settings = settings();
            let prepared =
                prepare_for_setup_template(stack(shape, egress.clone()), &settings).await;
            assert!(
                !has_sandbox_gateway(&prepared),
                "{shape:?} {egress:?}: legacy egress must not install the gateway"
            );
        }
    }
}

/// Evaluates a condition the way CloudFormation does on create and update. Only the functions
/// the gateway condition uses are supported; `Fn::Select` past the end of a list is the error
/// CloudFormation fails the operation with.
fn evaluate_condition(
    expression: &serde_json::Value,
    lists: &std::collections::HashMap<&str, Vec<&str>>,
) -> std::result::Result<serde_json::Value, String> {
    use serde_json::Value;
    let eval = |value: &Value| evaluate_condition(value, lists);
    let Value::Object(fields) = expression else {
        return Ok(expression.clone());
    };
    let (function, arguments) = fields.iter().next().expect("intrinsic");
    let arguments = |count: usize| -> Vec<Value> {
        let items = match arguments {
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        };
        assert_eq!(items.len(), count, "{function} arguments: {items:?}");
        items
    };
    let text = |value: Value| match value {
        Value::String(text) => text,
        other => panic!("expected text, got {other}"),
    };
    Ok(match function.as_str() {
        "Ref" => {
            let name = fields["Ref"].as_str().expect("Ref name");
            Value::from(lists[name].clone())
        }
        "Fn::And" => Value::Bool(
            match &fields["Fn::And"] {
                Value::Array(items) => items.clone(),
                other => panic!("Fn::And takes a list: {other}"),
            }
            .iter()
            .map(|item| eval(item).map(|value| value.as_bool().expect("condition")))
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .all(|value| value),
        ),
        "Fn::Not" => Value::Bool(!eval(&arguments(1)[0])?.as_bool().expect("condition")),
        "Fn::Equals" => {
            let [left, right] = &arguments(2)[..] else {
                unreachable!()
            };
            Value::Bool(eval(left)? == eval(right)?)
        }
        "Fn::Join" => {
            let [delimiter, items] = &arguments(2)[..] else {
                unreachable!()
            };
            let Value::Array(items) = eval(items)? else {
                panic!("Fn::Join takes a list")
            };
            let items = items
                .iter()
                .map(|item| eval(item).map(text))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Value::String(items.join(delimiter.as_str().expect("delimiter")))
        }
        "Fn::Split" => {
            let [delimiter, source] = &arguments(2)[..] else {
                unreachable!()
            };
            text(eval(source)?)
                .split(delimiter.as_str().expect("delimiter"))
                .map(Value::from)
                .collect()
        }
        "Fn::Select" => {
            let [index, items] = &arguments(2)[..] else {
                unreachable!()
            };
            let index = index.as_u64().expect("index") as usize;
            let Value::Array(items) = eval(items)? else {
                panic!("Fn::Select takes a list")
            };
            items.get(index).cloned().ok_or_else(|| {
                format!("Fn::Select cannot select nonexistent value at index {index}")
            })?
        }
        other => panic!("unsupported condition function {other}"),
    })
}

/// `SandboxGatewayHasThirdSubnet` is evaluated on every create and update, including create-new
/// installs that leave the gateway CIDRs empty. It must resolve for any list and be true only for
/// exactly three entries, which is when the template adds the third subnet's proxy rule.
#[tokio::test]
async fn the_third_gateway_subnet_condition_resolves_for_any_cidr_list() {
    let settings = settings();
    let prepared = prepare_for_setup_template(
        stack(Shape::SandboxOnly, SandboxEgress::Internet),
        &settings,
    )
    .await;
    let template = render(&prepared, settings, "third gateway subnet condition")
        .expect("internet sandbox template renders");
    let condition = serde_json::to_value(&template.conditions["SandboxGatewayHasThirdSubnet"])
        .expect("condition serializes");

    // A cleared CommaDelimitedList parameter resolves to one empty entry.
    for (cidrs, expected) in [
        (vec![""], false),
        (vec!["10.0.10.0/24"], false),
        (vec!["10.0.10.0/24", "10.0.11.0/24"], false),
        (vec!["10.0.10.0/24", "10.0.11.0/24", "10.0.12.0/24"], true),
        (
            vec![
                "10.0.10.0/24",
                "10.0.11.0/24",
                "10.0.12.0/24",
                "10.0.13.0/24",
            ],
            false,
        ),
    ] {
        let lists = std::collections::HashMap::from([("SandboxGatewaySubnetCidrs", cidrs.clone())]);
        let value = evaluate_condition(&condition, &lists)
            .unwrap_or_else(|error| panic!("{cidrs:?}: the condition must resolve: {error}"));
        assert_eq!(value, serde_json::Value::Bool(expected), "{cidrs:?}");
    }
}
