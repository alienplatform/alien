//! Shared CloudFormation generator-test fixtures.
//!
//! Every test in this directory funnels through [`render_template`] /
//! [`render_built_ins`] so the resulting `.snap` shows the complete
//! CloudFormation YAML and `cfn-lint` runs on every scenario. Snapshots
//! stay reviewable as a unit, the way a security team reads the template.

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfEmitter, CfExpression, CfRegistry, CfResource,
    CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    import::EmitContext, Platform, ResourceDefinition, ResourceLifecycle, ResourceRef,
    ResourceType, Result, Stack, StackSettings,
};
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Render a stack against the built-in registry and a chosen
/// registration mode. Runs `cfn-lint` and returns the YAML.
pub fn render_built_ins(
    stack: &Stack,
    settings: StackSettings,
    registration: RegistrationMode,
    description: &str,
) -> String {
    render_built_ins_target(
        stack,
        settings,
        registration,
        CloudFormationTarget::Aws,
        "aws",
        description,
    )
}

/// Render a stack against the built-in registry for a chosen package target.
pub fn render_built_ins_target(
    stack: &Stack,
    settings: StackSettings,
    registration: RegistrationMode,
    target: CloudFormationTarget,
    setup_target: &str,
    description: &str,
) -> String {
    let registry = CfRegistry::built_in();
    let template = generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            registry: &registry,
            target,
            stack_settings: settings,
            setup_target: setup_target.to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration,
            description: Some(description.to_string()),
        },
    )
    .expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok(description);
    yaml
}

/// Render a stack against a single-emitter sample registry. Useful for
/// generator-orchestration tests that don't care about per-resource
/// emitter shape (registration modes, parameter generation, output
/// chunking, plugin extension).
pub fn render_sample(
    stack: &Stack,
    settings: StackSettings,
    registration: RegistrationMode,
    description: &str,
) -> String {
    let registry = sample_registry();
    let template = generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            registry: &registry,
            target: CloudFormationTarget::Aws,
            stack_settings: settings,
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration,
            description: Some(description.to_string()),
        },
    )
    .expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok(description);
    yaml
}

/// Evaluate a template condition the way CloudFormation would, against
/// explicit parameter values with fallback to each parameter's `Default`.
/// Supports `Ref` and `Fn::Equals` — everything the permission-gate
/// feature emits.
pub fn evaluate_condition(
    template: &serde_json::Value,
    condition_name: &str,
    parameter_values: &[(&str, &str)],
) -> bool {
    let condition = &template["Conditions"][condition_name];
    let operands = condition["Fn::Equals"]
        .as_array()
        .unwrap_or_else(|| panic!("condition '{condition_name}' should be an Fn::Equals"));
    assert_eq!(
        operands.len(),
        2,
        "Fn::Equals in '{condition_name}' should have two operands"
    );

    let resolve = |operand: &serde_json::Value| -> String {
        if let Some(literal) = operand.as_str() {
            return literal.to_string();
        }
        let parameter = operand["Ref"].as_str().unwrap_or_else(|| {
            panic!("unsupported operand {operand} in condition '{condition_name}'")
        });
        if let Some((_name, value)) = parameter_values
            .iter()
            .find(|(name, _value)| *name == parameter)
        {
            return (*value).to_string();
        }
        template["Parameters"][parameter]["Default"]
            .as_str()
            .unwrap_or_else(|| panic!("parameter '{parameter}' has no provided value or Default"))
            .to_string()
    };

    resolve(&operands[0]) == resolve(&operands[1])
}

/// Sample registry that maps a single sample resource type to an
/// emitter that produces a tagged S3 bucket. The shape is small enough
/// to keep generator-level snapshot diffs focused on the orchestration
/// layer (parameters / conditions / outputs / chunking).
pub fn sample_registry() -> CfRegistry {
    let mut registry = CfRegistry::empty();
    registry.register(SampleResource::RESOURCE_TYPE, Platform::Aws, SampleEmitter);
    registry
}

/// Minimal sample stack with one [`SampleResource`].
pub fn sample_stack() -> Stack {
    Stack::new("demo".to_string())
        .add(
            SampleResource {
                id: "logs-bucket".to_string(),
            },
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// `Stack`-flavored helper for output-chunking tests — generates `n`
/// sample resources, each with a long id so the registration payload exceeds
/// the per-output byte budget.
pub fn many_sample_resources(n: usize) -> Stack {
    let mut stack = Stack::new("chunked".to_string());
    for index in 0..n {
        stack = stack.add(
            SampleResource {
                id: format!("chunked-resource-{index:03}-with-long-import-payload"),
            },
            ResourceLifecycle::Frozen,
        );
    }
    stack.build()
}

/// Sample resource type used across generator-orchestration tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SampleResource {
    pub id: String,
}

impl SampleResource {
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("sample-resource");
}

impl ResourceDefinition for SampleResource {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        vec![]
    }

    fn validate_update(&self, _new_config: &dyn ResourceDefinition) -> Result<()> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[derive(Debug)]
pub struct SampleEmitter;

impl CfEmitter for SampleEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let logical_id = ctx.name_for(ctx.resource_id).expect("logical id");
        let mut resource = CfResource::new(logical_id.to_string(), "AWS::S3::Bucket".to_string());
        resource.properties.insert(
            "BucketName".to_string(),
            CfExpression::sub(format!("${{AWS::StackName}}-{}", ctx.resource_id)),
        );
        Ok(vec![resource])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let logical_id = ctx.name_for(ctx.resource_id).expect("logical id");
        Ok(CfExpression::object([
            ("bucketName", CfExpression::ref_(logical_id)),
            ("bucketArn", CfExpression::get_att(logical_id, "Arn")),
        ]))
    }
}
