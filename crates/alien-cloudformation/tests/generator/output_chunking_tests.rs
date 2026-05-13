//! CFN Outputs chunking + source-function rejection.
//!
//! `AWS::CloudFormation::Output` values cap at 4 KB; the generator
//! splits the resolved import payload into `DeploymentResources0` / `...1` /
//! `…N` chunks once the assembled JSON exceeds a budget. Reviewers see
//! the chunk shape via these tests, not via a full template snapshot
//! (which would include 80+ buckets — not useful to read).

use super::helpers::{many_sample_resources, sample_registry, sample_stack};
use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfExpression, CfOutput, CloudFormationOptions,
    RegistrationMode,
};
use alien_core::{
    Function, FunctionCode, ResourceLifecycle, Stack, StackSettings, ToolchainConfig,
};

const OUTPUT_RESOURCES: &str = "DeploymentResources";

#[test]
fn outputs_fallback_chunks_large_resource_payload() {
    let stack = many_sample_resources(80);
    let registry = sample_registry();
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("chunked resources".to_string()),
        },
    )
    .expect("template should render");

    let chunk_keys = template
        .outputs
        .keys()
        .filter(|key| key.starts_with(OUTPUT_RESOURCES))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        chunk_keys.len() > 1,
        "expected multiple resource output chunks, got {chunk_keys:?}"
    );
    assert!(
        !template.outputs.contains_key(OUTPUT_RESOURCES),
        "base DeploymentResources output should be replaced by chunked outputs"
    );
    for (index, key) in chunk_keys.iter().enumerate() {
        assert_eq!(key, &format!("{OUTPUT_RESOURCES}{index}"));
    }

    let resource_count = chunk_keys
        .iter()
        .map(|key| chunked_output_item_count(&template.outputs[key]))
        .sum::<usize>();
    assert_eq!(resource_count, 80);

    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("chunked outputs");
}

#[test]
fn small_payload_emits_single_resources_output() {
    let registry = sample_registry();
    let template = generate_cloudformation_template(
        &sample_stack(),
        CloudFormationOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
        },
    )
    .expect("template should render");

    assert!(
        template.outputs.contains_key(OUTPUT_RESOURCES),
        "small payloads should keep a single DeploymentResources output"
    );
    assert!(
        !template
            .outputs
            .keys()
            .any(|key| key.starts_with("DeploymentResources0")),
        "small payloads should not chunk"
    );
}

#[test]
fn source_function_returns_typed_error() {
    let function = Function::new("source-fn".to_string())
        .code(FunctionCode::Source {
            src: ".".to_string(),
            toolchain: ToolchainConfig::Rust {
                binary_name: "app".to_string(),
            },
        })
        .permissions("execution".to_string())
        .build();
    let stack = Stack::new("source-stack".to_string())
        .add(function, ResourceLifecycle::Live)
        .build();
    let registry = sample_registry();
    let error = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
        },
    )
    .expect_err("source function should be rejected");

    assert!(
        error
            .to_string()
            .contains("CloudFormation templates require a pre-built image"),
        "{error}"
    );
}

fn chunked_output_item_count(output: &CfOutput) -> usize {
    let CfExpression::Object(value) = &output.value else {
        panic!("resource chunk output should use Fn::ToJsonString");
    };
    let Some(CfExpression::List(items)) = value.get("Fn::ToJsonString") else {
        panic!("resource chunk output should serialize a list");
    };
    items.len()
}
