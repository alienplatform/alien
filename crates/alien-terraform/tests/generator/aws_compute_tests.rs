//! AWS compute & artifacts — function / build / artifact-registry.
//!
//! `compute-cluster` is a platform-only resource (Phase 6d moved its
//! emitter to `alien-terraformx`); the OSS suite asserts the dispatch
//! registry produces a typed `ImportRegistrationMissing` error if the
//! extension is absent.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    import::EmitContext, ArtifactRegistry, Build, CapacityGroup, ComputeCluster, ErrorData,
    Network, NetworkSettings, Platform, ResourceLifecycle, Result, Stack, StackSettings, Worker,
    WorkerCode,
};
use alien_terraform::{
    block::{attr, resource_block},
    emitters::aws::helpers::{private_subnet_ids_expr, required_label, vpc_id_expr},
    expr, generate_terraform_module, TerraformOptions, TerraformTarget, TfEmitter, TfFragment,
    TfRegistry,
};
use hcl::expr::Expression;

#[test]
fn aws_artifact_registry_renders_ecr_repository() {
    let stack = Stack::new("acme-ecr".to_string())
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_artifact_registry", &module);
    assert_terraform_valid(&module, "aws_artifact_registry");
}

#[test]
fn aws_build_renders_codebuild_project() {
    let stack = Stack::new("acme-build".to_string())
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .environment([("PROFILE".to_string(), "release".to_string())].into())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_build", &module);
    assert_terraform_valid(&module, "aws_build");
}

/// A `Build` role must stay unassumable by the sandbox image builder.
///
/// `sandbox/provision` passes `role/<prefix>-*-build`, and a `Build` resource emits a role wearing
/// that same shape. What stops it being passed into `CreateMicrovmImage` is this trust policy:
/// Lambda cannot assume a role that does not name it. That is the whole reason the grant is safe
/// for `Build`, and until now it was written down only in a comment — adding lambda here would
/// open a path from provisioning to whatever this role can read, with nothing failing.
#[test]
fn the_build_role_cannot_be_assumed_by_the_sandbox_image_builder() {
    let stack = Stack::new("acme-build-trust".to_string())
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    let rendered: String = module
        .files
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        rendered.contains("codebuild.amazonaws.com"),
        "the build role has to trust the service that actually runs it:\n{rendered}"
    );
    let build_role = rendered
        .split("resource \"aws_iam_role\"")
        .find(|block| block.contains("codebuild.amazonaws.com"))
        .expect("the build role must render");
    assert!(
        !build_role.contains("lambda.amazonaws.com"),
        "a build role Lambda can assume is passable into the sandbox image build:\n{build_role}"
    );
}

#[test]
fn aws_function_basic_lambda() {
    let stack = Stack::new("acme-fn".to_string())
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app:1".to_string(),
                })
                .permissions("execution".to_string())
                .timeout_seconds(30)
                .expect("literal Worker timeout is within supported range")
                .memory_mb(256)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_function_basic", &module);
    assert_terraform_valid(&module, "aws_function_basic");
}

#[test]
fn aws_function_public_ingress_emits_apigw_v2() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Worker::new("public-api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app:1".to_string(),
                })
                .permissions("execution".to_string())
                .public_endpoint(alien_core::WorkerPublicEndpoint {
                    name: "api".to_string(),
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .timeout_seconds(60)
                .expect("literal Worker timeout is within supported range")
                .memory_mb(512)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_function_public", &module);
    assert_terraform_valid(&module, "aws_function_public");
}

#[test]
fn aws_container_cluster_without_platform_extension_errors_cleanly() {
    let stack = Stack::new("acme-cluster".to_string())
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
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let registry = TfRegistry::built_in();
    let err = generate_terraform_module(
        &stack,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: None,
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: vec!["us-east-1".to_string(), "eu-west-1".to_string()],
        },
    )
    .expect_err("OSS registry should not register container_cluster");

    match err.error.as_ref().expect("typed error") {
        ErrorData::ImportRegistrationMissing {
            resource_type,
            platform,
            ..
        } => {
            assert_eq!(resource_type.as_ref(), "compute-cluster");
            assert_eq!(*platform, Platform::Aws);
        }
        other => panic!("expected ImportRegistrationMissing, got {other:?}"),
    }
}

/// Stand-in for an extension compute-cluster emitter: it places its machines in the stack's
/// network purely through the shared network helpers, the way out-of-crate emitters do.
struct NetworkConsumerEmitter;

impl TfEmitter for NetworkConsumerEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let label = required_label(ctx)?;
        Ok(TfFragment::default()
            .with_resource(resource_block(
                "aws_security_group",
                label,
                [attr("vpc_id", vpc_id_expr(ctx))],
            ))
            .with_resource(resource_block(
                "aws_lb",
                label,
                [
                    attr("internal", Expression::Bool(true)),
                    attr("subnets", private_subnet_ids_expr(ctx)),
                ],
            )))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([(
            "securityGroupId",
            expr::traversal(["aws_security_group", label, "id"]),
        )]))
    }
}

/// A container stack on the account's default VPC gets a compute cluster plus a `UseDefault`
/// network. The network helpers resolve to the default-VPC data sources, so the network emitter
/// must declare them for every consumer, not only for databases and EKS. `terraform validate`
/// rejects any reference to an undeclared data source, which is the failure this pins; the
/// dynamic network render runs alongside as the passing baseline.
#[test]
fn aws_compute_cluster_network_references_resolve_for_every_network_mode() {
    for (scenario, network) in [
        ("static_default_vpc", NetworkSettings::UseDefault),
        (
            "dynamic_network",
            NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            },
        ),
    ] {
        let stack = Stack::new("acme-cluster".to_string())
            .add(
                Network::new("default-network".to_string())
                    .settings(network.clone())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ComputeCluster::new("compute".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let mut registry = TfRegistry::built_in();
        registry.register(
            ComputeCluster::RESOURCE_TYPE,
            Platform::Aws,
            NetworkConsumerEmitter,
        );
        let module = generate_terraform_module(
            &stack,
            TerraformTarget::Aws,
            TerraformOptions {
                display_name: None,
                registry: &registry,
                stack_settings: StackSettings {
                    network: Some(network),
                    ..StackSettings::default()
                },
                registration: None,
                helm_install: None,
                supported_aws_regions: Vec::new(),
            },
        )
        .expect("module should render");

        assert_terraform_valid(&module, scenario);
    }
}
