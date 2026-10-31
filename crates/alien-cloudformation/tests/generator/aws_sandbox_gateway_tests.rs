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
    Key, ManagementConfig, Network, NetworkSettings, PermissionProfile, PersistentStorage,
    Platform, ResourceLifecycle, ResourceRef, ResourceSpec, Result, Sandbox, SandboxCode,
    SandboxEgress, SandboxLifecyclePolicy, Stack, StackSettings, StackState, Storage,
    VolumeBackups, Worker, WorkerCode,
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
pub(super) async fn prepare_for_setup_template(stack: Stack, settings: &StackSettings) -> Stack {
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
            management_trust: Default::default(),
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

/// A profile granted on an internet sandbox, by a Worker's link or directly. Setup preparation
/// adds the gateway Container and makes the sandbox depend on the deployment secrets Vault, whose
/// grants it adds to the same profile. The generator folds the sandbox's and the Vault's grants on
/// that role into one managed policy, which must not inherit the sandbox's ordering after the
/// Vault's grant as a dependency on itself: CloudFormation refuses a template with a cycle.
#[tokio::test]
async fn a_profile_granted_an_internet_sandbox_renders_without_a_dependency_cycle() {
    let sandbox = || {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build()
    };
    let linking_worker = Stack::new("acme".to_string())
        .permission("execution", PermissionProfile::new())
        .add(sandbox(), ResourceLifecycle::Live)
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .link(&sandbox())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let granted_profile = Stack::new("acme".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("agents", ["sandbox/execute"]),
        )
        .add(sandbox(), ResourceLifecycle::Live)
        .build();

    for (description, stack) in [
        ("a worker linking an internet sandbox", linking_worker),
        ("a profile granted an internet sandbox", granted_profile),
    ] {
        let settings = settings();
        let prepared = prepare_for_setup_template(stack, &settings).await;
        assert!(
            has_sandbox_gateway(&prepared),
            "{description}: gateway installed"
        );

        let template = render(&prepared, settings, description)
            .unwrap_or_else(|error| panic!("{description}: {error}"));
        for (logical_id, resource) in &template.resources {
            assert!(
                !resource.depends_on.contains(logical_id),
                "{description}: {logical_id} depends on itself"
            );
        }
        cfn_lint(&to_yaml(&template).expect("template serializes")).assert_ok(description);
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

/// Every statement attached to `role` in the rendered template, from its inline policies and
/// from the standalone and managed policies that name it.
fn role_statements(template: &CfTemplate, role: &str) -> Vec<serde_json::Value> {
    let role_ref = serde_json::json!({ "Ref": role });
    let mut statements = Vec::new();
    for (logical_id, resource) in &template.resources {
        let properties = serde_json::to_value(&resource.properties).expect("properties serialize");
        let documents: Vec<serde_json::Value> = match resource.resource_type.as_str() {
            "AWS::IAM::Role" if logical_id == role => properties["Policies"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|policy| policy["PolicyDocument"].clone())
                .collect(),
            "AWS::IAM::Policy" | "AWS::IAM::ManagedPolicy"
                if properties["Roles"]
                    .as_array()
                    .is_some_and(|roles| roles.contains(&role_ref)) =>
            {
                vec![properties["PolicyDocument"].clone()]
            }
            _ => Vec::new(),
        };
        for document in documents {
            statements.extend(
                document["Statement"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    statements
}

fn as_strings(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(items) => items.iter().flat_map(as_strings).collect(),
        serde_json::Value::Null => Vec::new(),
        other => vec![other.to_string()],
    }
}

fn actions(statement: &serde_json::Value) -> Vec<String> {
    as_strings(&statement["Action"])
        .into_iter()
        .map(|action| action.trim_matches('"').to_string())
        .collect()
}

/// A deployment with its own key: a bucket encrypted with it, an application that reads and
/// writes the bucket and drives an internet sandbox, and the vendor's management identity. The
/// management role may check the bucket's health and write the deployment's secrets vault, but
/// it may not use the key to decrypt, may read nothing from the vault, and may only see that the
/// gateway's replica secret exists. Setup generates that secret; the gateway reads it. The
/// application keeps its own key and vault access.
#[tokio::test]
async fn the_management_role_cannot_decrypt_or_read_the_deployments_secrets() {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    let stack = Stack::new("acme".to_string())
        .permission(
            "api",
            PermissionProfile::new()
                .resource("files", ["storage/data-read", "storage/data-write"])
                .global(["sandbox/execute", "sandbox/management"]),
        )
        .permission("none", PermissionProfile::new())
        .add(
            Key::new("cell-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("files".to_string())
                .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "cell-key"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(sandbox, ResourceLifecycle::Live)
        .add(
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
                .permissions("api".to_string())
                .port(8080)
                .build(),
            ResourceLifecycle::Live,
        )
        .add(
            Container::new("redis".to_string())
                .code(ContainerCode::Image {
                    image: "redis:7.4-alpine".to_string(),
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
                .stateful(true)
                .persistent_storage(PersistentStorage {
                    size: "10Gi".to_string(),
                    mount_path: "/data".to_string(),
                    encryption_key: Some(ResourceRef::new(Key::RESOURCE_TYPE, "cell-key")),
                    backups: VolumeBackups::default(),
                })
                .permissions("none".to_string())
                .port(6379)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let settings = settings();
    let prepared = prepare_for_setup_template(stack, &settings).await;
    let template = render(&prepared, settings, "deployment key and internet sandbox")
        .expect("template renders");
    cfn_lint(&to_yaml(&template).expect("template serializes"))
        .assert_ok("deployment key and internet sandbox");

    let gateway_config = "${AWS::StackName}-secrets-sandbox-egress-gateway-config";
    let management = role_statements(&template, "ManagementRole");
    assert!(!management.is_empty(), "the management role has grants");
    for statement in management.iter().filter(|s| s["Effect"] == "Allow") {
        let actions = actions(statement);
        let resources = as_strings(&statement["Resource"]);
        let key_use = actions.iter().any(|action| {
            matches!(
                action.as_str(),
                "kms:Decrypt" | "kms:GenerateDataKey" | "kms:GenerateDataKey*" | "kms:*"
            )
        });
        assert!(
            !key_use,
            "management may not decrypt with a key: {statement}"
        );
        let reads_parameters = actions
            .iter()
            .any(|action| action.starts_with("ssm:GetParameter") || action == "ssm:*");
        if reads_parameters {
            for resource in &resources {
                assert!(
                    !resource.contains(":parameter/"),
                    "management may read no parameter: {statement}"
                );
            }
        }
        let secret_access = actions
            .iter()
            .filter(|action| action.starts_with("secretsmanager:"))
            .collect::<Vec<_>>();
        assert!(
            secret_access
                .iter()
                .all(|action| *action == "secretsmanager:DescribeSecret"),
            "management may only see that the replica secret exists: {statement}"
        );
    }
    let denies = |wanted: &[&str], resource: &str| {
        management.iter().any(|statement| {
            let actions = actions(statement);
            statement["Effect"] == "Deny"
                && wanted
                    .iter()
                    .all(|action| actions.iter().any(|granted| granted == action))
                && statement["Resource"].to_string().contains(resource)
        })
    };
    assert!(
        denies(
            &[
                "ssm:GetParameter",
                "ssm:GetParameters",
                "ssm:GetParameterHistory"
            ],
            "${AWS::StackName}-secrets-*"
        ),
        "management is explicitly denied reading the secrets vault: {management:#?}"
    );
    assert!(
        denies(
            &[
                "secretsmanager:GetSecretValue",
                "secretsmanager:PutSecretValue"
            ],
            "${AWS::StackName}-sandbox-egress-replica-secret-??????"
        ),
        "management is explicitly denied the replica secret's value: {management:#?}"
    );
    let replica_secret = &template.resources["SandboxEgressReplicaSecret"];
    assert_eq!(replica_secret.resource_type, "AWS::SecretsManager::Secret");
    let replica_secret = serde_json::to_value(&replica_secret.properties).unwrap();
    assert_eq!(
        replica_secret["Name"],
        serde_json::json!({"Fn::Sub": "${AWS::StackName}-sandbox-egress-replica-secret"})
    );
    assert!(replica_secret["GenerateSecretString"]["PasswordLength"].as_u64() >= Some(32));
    assert!(
        management
            .iter()
            .any(|statement| statement["Effect"] == "Deny"
                && actions(statement).contains(&"ssm:GetParametersByPath".to_string())),
        "management is explicitly denied reading parameters by path"
    );
    assert!(
        management
            .iter()
            .any(|statement| statement["Effect"] == "Allow"
                && actions(statement).iter().any(|a| a == "ssm:PutParameter")
                && statement["Resource"].to_string().contains("-secrets-")),
        "management still writes the secrets vault"
    );

    assert!(
        management
            .iter()
            .any(|statement| statement["Effect"] == "Allow"
                && actions(statement).contains(&"kms:GenerateDataKeyWithoutPlaintext".to_string())
                && statement["Condition"]
                    .to_string()
                    .contains("kms:ViaService")),
        "management still creates volumes encrypted with the key"
    );

    let api = role_statements(&template, "ApiSaRole");
    assert!(
        api.iter().any(|statement| statement["Effect"] == "Allow"
            && actions(statement).contains(&"kms:Decrypt".to_string())
            && statement["Condition"].is_null()),
        "the application decrypts its objects: {api:#?}"
    );
    let gateway = template
        .resources
        .iter()
        .find(|(id, resource)| {
            resource.resource_type == "AWS::IAM::Role" && id.contains("EgressGateway")
        })
        .map(|(id, _)| id.clone())
        .expect("gateway role renders");
    assert!(
        role_statements(&template, &gateway)
            .iter()
            .any(|statement| {
                statement["Effect"] == "Allow"
                    && actions(statement).contains(&"ssm:GetParameter".to_string())
                    && statement["Resource"].to_string().contains(gateway_config)
            }),
        "the gateway reads its configuration"
    );
    assert!(
        role_statements(&template, &gateway)
            .iter()
            .any(|statement| {
                statement["Effect"] == "Allow"
                    && actions(statement).contains(&"secretsmanager:GetSecretValue".to_string())
                    && statement["Resource"]
                        .to_string()
                        .contains("${AWS::StackName}-sandbox-egress-replica-secret-??????")
            }),
        "the gateway reads its replica secret"
    );
}
