//! AWS compute & artifacts — function / build / artifact-registry /
//! container-cluster.
//!
//! Mirrors the per-resource coverage in
//! `alien-terraform/tests/generator/aws_compute_tests.rs`. Each
//! scenario renders the full template via the built-in CFN registry,
//! runs `cfn-lint`, and snapshots the YAML for a complete audit
//! review.

use super::helpers::render_built_ins;
use alien_cloudformation::{generate_cloudformation_template, CfRegistry, RegistrationMode};
use alien_core::{
    ArtifactRegistry, Build, CapacityGroup, ContainerCluster, ErrorData, Function, FunctionCode,
    Ingress, Network, NetworkSettings, Platform, ResourceLifecycle, Stack, StackSettings,
};

#[test]
fn aws_artifact_registry_renders_ecr_repository() {
    let stack = Stack::new("acme-ecr".to_string())
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_artifact_registry",
    );
    insta::assert_snapshot!("aws_artifact_registry", yaml);
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
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_build",
    );
    insta::assert_snapshot!("aws_build", yaml);
}

#[test]
fn aws_function_basic_lambda() {
    let stack = Stack::new("acme-fn".to_string())
        .add(
            Function::new("api".to_string())
                .code(FunctionCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app:1".to_string(),
                })
                .permissions("execution".to_string())
                .timeout_seconds(30)
                .memory_mb(256)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_function_basic",
    );
    insta::assert_snapshot!("aws_function_basic", yaml);
}

#[test]
fn aws_function_public_ingress_emits_apigw_v2() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Function::new("public-api".to_string())
                .code(FunctionCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app:1".to_string(),
                })
                .permissions("execution".to_string())
                .ingress(Ingress::Public)
                .timeout_seconds(60)
                .memory_mb(512)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_function_public",
    );
    insta::assert_snapshot!("aws_function_public", yaml);
}

#[test]
fn aws_container_cluster_without_platform_extension_errors_cleanly() {
    // OSS no longer registers `ContainerCluster` — Phase 6c moved the
    // emitter to `alien-cloudformationx`. Plugins (or the platform
    // distribution) wire it back in via `register_platform_extensions`.
    // Confirm the OSS error path is the typed `ImportRegistrationMissing`
    // a third-party plugin author would also see when forgetting to
    // register their emitter.
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ContainerCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("m7g.large".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let registry = CfRegistry::built_in();
    let err = generate_cloudformation_template(
        &stack,
        alien_cloudformation::CloudFormationOptions {
            registry: &registry,
            stack_settings: settings,
            registration: RegistrationMode::OutputsFallback,
            description: None,
        },
    )
    .expect_err("OSS registry should not register container_cluster");

    match err.error.as_ref().expect("typed error") {
        ErrorData::ImportRegistrationMissing {
            resource_type,
            platform,
            ..
        } => {
            assert_eq!(resource_type.as_ref(), "container-cluster");
            assert_eq!(*platform, Platform::Aws);
        }
        other => panic!("expected ImportRegistrationMissing, got {other:?}"),
    }
}
