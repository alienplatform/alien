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
    ManagementConfig, Network, NetworkSettings, PermissionProfile, Platform, ResourceDefinition,
    ResourceLifecycle, ResourceSpec, Result, Sandbox, SandboxCode, SandboxEgress,
    SandboxLifecyclePolicy, Stack, StackSettings, StackState,
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
