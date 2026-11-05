//! The setup boundary of a stack whose workloads read many secret inputs.
//!
//! Setup preparation grants each consumer least-privilege reads of exactly the secret inputs it
//! is mapped to. Those grants name resources under the stack prefix, which the boundary's scoped
//! Allow already admits, so they must not grow the boundary. Copied in, a stack like this one
//! exceeded IAM's 6,144-character managed-policy limit and neither setup package rendered.

use super::helpers::{assert_terraform_valid, render_with_registry};
use alien_cloudformation::{
    generate_cloudformation_template, CfEmitter, CfExpression, CfRegistry, CfResource,
    CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    import::EmitContext, AwsManagementConfig, ComputeCluster, Container, ContainerCode,
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Key, ManagementConfig,
    NetworkSettings, PermissionProfile, PersistentStorage, Platform, ResourceLifecycle,
    ResourceRef, ResourceSpec, Result, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    Stack, StackInputDefinition, StackSettings, StackState, VolumeBackups,
};
use alien_permissions::{
    generators::{
        aws_boundary_scope_context, aws_stack_permissions_boundary, validate_aws_boundary_size,
    },
    PermissionContext,
};
use alien_preflights::runner::PreflightRunner;
use alien_terraform::{TerraformTarget, TfEmitter, TfFragment, TfRegistry};
use serde_json::{json, Value};

const MANAGED_POLICY_LIMIT: usize = 6_144;
const SECRET_INPUTS: usize = 24;
const CONSUMERS: usize = 4;

/// Compute clusters are rendered by an extension, not the built-in registries. Their own
/// resources are not what this test is about, so these stand in for one with the resource
/// CloudFormation references (the node instance role) and the import fields both read.
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

impl TfEmitter for ExtensionComputeCluster {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> Result<hcl::Expression> {
        Ok(hcl::Expression::from(hcl::Object::from([(
            hcl::ObjectKey::from("clusterId"),
            hcl::Expression::from("compute"),
        )])))
    }
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

/// Workloads that each read a different subset of many installer-provided secrets, an internet
/// sandbox (which brings the egress gateway and its secrets), and a volume key whose
/// provider-assigned ARN the boundary must list exactly.
fn stack() -> Stack {
    let consumer = |index: usize| format!("api-{index}");
    let mut stack = Stack::new("acme".to_string()).add(
        Key::new("volumes".to_string()).build(),
        ResourceLifecycle::Frozen,
    );
    for index in 0..CONSUMERS {
        let container = Container::new(consumer(index))
            .code(ContainerCode::Image {
                image: "registry.example.com/api:1".to_string(),
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
            .permissions(consumer(index))
            .port(8080);
        let container = if index == 0 {
            container
                .persistent_storage(PersistentStorage {
                    size: "10Gi".to_string(),
                    mount_path: "/data".to_string(),
                    encryption_key: Some(ResourceRef::new(Key::RESOURCE_TYPE, "volumes")),
                    backups: VolumeBackups::default(),
                })
                .build()
        } else {
            container.build()
        };
        stack = stack
            .permission(consumer(index), PermissionProfile::new())
            .add(container, ResourceLifecycle::Live);
    }
    let mut stack = stack
        .add(
            Sandbox::new("agents".to_string())
                .code(SandboxCode::Image {
                    image: "s3://example-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
                    bundled_from: None,
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
    for input in 0..SECRET_INPUTS {
        // Each consumer reads every secret from its own index on, so their grants differ.
        let targets: Vec<_> = (0..CONSUMERS)
            .filter(|index| input % CONSUMERS >= *index)
            .map(consumer)
            .collect();
        let definition: StackInputDefinition = serde_json::from_value(json!({
            "id": format!("applicationSetting{input}"),
            "kind": "secret",
            "providedBy": ["deployer"],
            "required": true,
            "label": format!("Application setting {input}"),
            "description": "A credential the installer provides.",
            "env": [{"name": format!("APPLICATION_SETTING_{input}"), "targetResources": targets}],
        }))
        .expect("secret input definition");
        stack.inputs.push(definition);
    }
    stack
}

/// Prepare the stack the way a setup package is prepared: template preflights, then mutations
/// against a configuration that has no deployment yet.
async fn prepare(stack: Stack) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Aws)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(settings())
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

/// The boundary as setup sizes it: the longest prefix and region, and distinct key ARNs.
fn sized_boundary(stack: &Stack) -> Value {
    let base = PermissionContext::new()
        .with_stack_prefix("s".repeat(40))
        .with_aws_region("r".repeat(16))
        .with_aws_account_id("123456789012")
        .with_managing_role_arn(format!(
            "arn:aws:iam::210987654321:role/{}",
            "r".repeat(2019)
        ))
        .with_managing_account_id("210987654321");
    aws_stack_permissions_boundary(stack, &base, |_, scope| {
        aws_boundary_scope_context(stack, &base, scope, |scope| {
            Ok(
                (stack.resources[scope].config.resource_type() == Key::RESOURCE_TYPE).then(|| {
                    format!(
                        "arn:aws:kms:{}:123456789012:key/00000000-0000-0000-0000-000000000000",
                        "r".repeat(16)
                    )
                }),
            )
        })
    })
    .expect("the boundary fits the managed policy limit")
}

/// Statements of the given effect that name `needle` in any value.
fn statements_naming<'a>(boundary: &'a Value, effect: &str, needle: &str) -> Vec<&'a Value> {
    boundary["Statement"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["Effect"] == effect && s.to_string().contains(needle))
        .collect()
}

#[tokio::test]
async fn many_secret_consumers_fit_the_boundary_in_both_setup_packages() {
    let prepared = prepare(stack()).await;
    let deployer_reads = prepared
        .permissions
        .profiles
        .values()
        .flat_map(|profile| profile.0.values().flatten())
        .filter(|reference| reference.id() == "vault/deployer-secrets-read")
        .count();
    assert_eq!(
        deployer_reads, CONSUMERS,
        "preparation grants every consumer its secret reads"
    );

    validate_aws_boundary_size(&prepared).expect("setup sizing accepts the boundary");
    let boundary = sized_boundary(&prepared);
    let size = boundary
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .count();
    assert!(
        size <= MANAGED_POLICY_LIMIT * 9 / 10,
        "boundary is {size} characters"
    );
    // The consumers' reads are admitted by the scoped Allow, not copied.
    assert!(statements_naming(&boundary, "Allow", "-deployer-secrets-").is_empty());
    let scoped = &boundary["Statement"][0];
    assert_eq!(scoped["NotAction"], "iam:*");
    assert!(scoped["Resource"]
        .as_array()
        .unwrap()
        .contains(&json!(format!(
            "arn:aws:ssm:{}:123456789012:parameter/{}-*",
            "r".repeat(16),
            "s".repeat(40)
        ))));
    // The provider-assigned key ARN and the gateway's protective Deny are kept.
    assert!(scoped.to_string().contains(":key/00000000-0000-0000-0000-"));
    assert_eq!(
        statements_naming(&boundary, "Deny", "-sandbox-egress-gateway-config").len(),
        1
    );

    let mut cf_registry = CfRegistry::built_in();
    cf_registry.register(
        ComputeCluster::RESOURCE_TYPE,
        Platform::Aws,
        ExtensionComputeCluster,
    );
    generate_cloudformation_template(
        &prepared,
        CloudFormationOptions {
            registry: &cf_registry,
            source_stack: &stack(),
            target: CloudFormationTarget::Aws,
            stack_settings: settings(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::CustomResource {
                lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
                callback_url: None,
            },
            description: None,
            management_trust: Default::default(),
        },
    )
    .expect("the CloudFormation setup template renders");

    let mut tf_registry = TfRegistry::built_in();
    tf_registry.register(
        ComputeCluster::RESOURCE_TYPE,
        Platform::Aws,
        ExtensionComputeCluster,
    );
    let module = render_with_registry(&prepared, TerraformTarget::Aws, settings(), &tf_registry)
        .expect("the Terraform setup module renders");
    assert_terraform_valid(&module, "many secret consumers");
}
