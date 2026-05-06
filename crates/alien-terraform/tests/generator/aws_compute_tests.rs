//! AWS compute & artifacts — function / build / artifact-registry.
//!
//! `container-cluster` is a platform-only resource (Phase 6d moved its
//! emitter to `alien-terraformx`); the OSS suite asserts the dispatch
//! registry produces a typed `ImportRegistrationMissing` error if the
//! extension is absent.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, Build, CapacityGroup, ContainerCluster, ErrorData, Function, FunctionCode,
    Ingress, Platform, ResourceLifecycle, Stack, StackSettings,
};
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};

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
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_build", &module);
    assert_terraform_valid(&module, "aws_build");
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_function_basic", &module);
    assert_terraform_valid(&module, "aws_function_basic");
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    snapshot_module("aws_function_public", &module);
    assert_terraform_valid(&module, "aws_function_public");
}

#[test]
fn aws_container_cluster_without_platform_extension_errors_cleanly() {
    let stack = Stack::new("acme-cluster".to_string())
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
            ResourceLifecycle::Live,
        )
        .build();

    let registry = TfRegistry::built_in();
    let err = generate_terraform_module(
        &stack,
        TerraformTarget::Aws,
        TerraformOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            registration: None,
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
