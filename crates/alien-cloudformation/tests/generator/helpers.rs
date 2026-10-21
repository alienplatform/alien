//! Shared CloudFormation generator-test fixtures.
//!
//! Every test in this directory funnels through [`render_template`] /
//! [`render_built_ins`] so the resulting `.snap` shows the complete
//! CloudFormation YAML and `cfn-lint` runs on every scenario. Snapshots
//! stay reviewable as a unit, the way a security team reads the template.

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfEmitter, CfExpression, CfRegistry, CfResource,
    CfTemplate, CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    import::EmitContext, Platform, ResourceDefinition, ResourceLifecycle, ResourceRef,
    ResourceType, Result, Stack, StackInputDefinition, StackSettings,
};
use serde::{Deserialize, Serialize};
use std::{any::Any, collections::HashMap};

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
    render_built_ins_template(
        stack,
        settings,
        registration,
        target,
        setup_target,
        description,
    )
    .1
}

/// Like [`render_built_ins_target`], but hands back the template too, for tests
/// that assert on its structure rather than on the rendered YAML.
pub fn render_built_ins_template(
    stack: &Stack,
    settings: StackSettings,
    registration: RegistrationMode,
    target: CloudFormationTarget,
    setup_target: &str,
    description: &str,
) -> (CfTemplate, String) {
    let template = try_render_built_ins(
        stack,
        settings,
        registration,
        target,
        setup_target,
        description,
    )
    .expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok(description);
    (template, yaml)
}

/// Render without asserting success, for the tests that assert on the refusal.
/// Skips `cfn-lint` because there may be no template to lint.
pub fn try_render_built_ins(
    stack: &Stack,
    settings: StackSettings,
    registration: RegistrationMode,
    target: CloudFormationTarget,
    setup_target: &str,
    description: &str,
) -> Result<CfTemplate> {
    let registry = CfRegistry::built_in();
    generate_cloudformation_template(
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
            management_trust: Default::default(),
        },
    )
}

/// The registration mode the `.enabled()` tests render under: the custom
/// resource that carries the payload registration deserializes.
pub fn custom_resource_registration() -> RegistrationMode {
    RegistrationMode::CustomResource {
        lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
        callback_url: None,
    }
}

/// A boolean deployer input, the shape `.enabled(input)` gates on.
pub fn gate_input(id: &str, label: &str, description: &str) -> StackInputDefinition {
    StackInputDefinition::deployer_boolean(id, label, description, Some(true))
}

/// The registration payload the custom resource carries, unresolved.
pub fn registration_payload(template: &CfTemplate) -> CfExpression {
    template
        .resources
        .get("DeploymentRegistration")
        .expect("registration custom resource")
        .properties
        .get("Resources")
        .expect("registration resource list")
        .clone()
}

/// What becomes of a list element that resolved to `AWS::NoValue`.
///
/// CloudFormation does not answer this the same way everywhere, and the
/// difference is invisible in a rendered template — it only appears once the
/// stack is live. Both rules below were read back off a real deployment.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Declined {
    /// The element disappears. Resource properties and `Fn::Join` behave this
    /// way; `Fn::Join` leaves no stray delimiter behind and yields `""` when
    /// every element goes.
    Removed,
    /// The element survives as a literal `null`. This is what `Fn::ToJsonString`
    /// does, and it is the whole reason the Outputs payload is built differently:
    /// registration deserializes every entry it receives, so a null fails the
    /// import rather than being skipped.
    Null,
}

/// Minimal model of how CloudFormation resolves a rendered payload at deploy
/// time: `Fn::If` picks a branch, `AWS::NoValue` deletes whatever holds it, and
/// the string-producing intrinsics collapse to text.
///
/// The `AWS::NoValue`-removes-a-list-element half of this is not a guess. The
/// shipped AWS network emitter already relies on it: `subnet_refs` renders
/// `publicSubnetIds` as a three-element list whose 2nd and 3rd elements are
/// `Fn::If(<az condition>, Ref, AWS::NoValue)`, those AZ conditions are driven
/// by the deploy-time `AvailabilityZones` parameter, and the value lands in
/// `AwsNetworkImportData.public_subnet_ids: Vec<String>`. A deployer choosing 2
/// AZs would break that path today if `AWS::NoValue` produced a null element
/// rather than removing it.
///
/// `Fn::Sub` is modelled only in its variable-map form, and substitution is a
/// single pass — a `${...}` left inside a substituted value stays literal, which
/// is what CloudFormation does. Intrinsics with no local answer (`Ref`,
/// `Fn::GetAtt`, plain `Fn::Sub`) are left as-is; they carry no `id` and cannot
/// be null, so they do not affect what these tests assert.
pub fn resolve(
    expression: &CfExpression,
    conditions: &HashMap<&str, bool>,
    declined: Declined,
) -> Option<CfExpression> {
    if *expression == CfExpression::no_value() {
        return None;
    }

    match expression {
        CfExpression::Object(fields) => {
            if let Some(CfExpression::List(branches)) = fields.get("Fn::If") {
                let [CfExpression::String(condition), when_true, when_false] = &branches[..] else {
                    panic!("malformed Fn::If: {branches:?}");
                };
                let answer = *conditions
                    .get(condition.as_str())
                    .unwrap_or_else(|| panic!("no answer supplied for condition '{condition}'"));
                let taken = if answer { when_true } else { when_false };
                return resolve(taken, conditions, declined);
            }

            if let Some(value) = fields.get("Fn::ToJsonString") {
                let value = resolve(value, conditions, Declined::Null)
                    .expect("Fn::ToJsonString argument should survive resolution");
                let text = serde_json::to_string(&value).expect("resolved value should serialize");
                return Some(CfExpression::String(text));
            }

            if let Some(CfExpression::List(arguments)) = fields.get("Fn::Join") {
                let [CfExpression::String(delimiter), CfExpression::List(items)] = &arguments[..]
                else {
                    panic!("malformed Fn::Join: {arguments:?}");
                };
                let parts = items
                    .iter()
                    .filter_map(|item| resolve(item, conditions, Declined::Removed))
                    .map(|item| match item {
                        CfExpression::String(text) => text,
                        other => placeholder(&other).unwrap_or_else(|| {
                            panic!("Fn::Join element should be a string: {other:?}")
                        }),
                    })
                    .collect::<Vec<_>>();
                return Some(CfExpression::String(parts.join(delimiter)));
            }

            if let Some(CfExpression::List(arguments)) = fields.get("Fn::Sub") {
                let [CfExpression::String(template), CfExpression::Object(variables)] =
                    &arguments[..]
                else {
                    panic!("malformed Fn::Sub: {arguments:?}");
                };
                let mut text = template.clone();
                for (name, value) in variables {
                    let value = resolve(value, conditions, Declined::Removed)
                        .expect("Fn::Sub variable should survive resolution");
                    let CfExpression::String(value) = value else {
                        panic!("Fn::Sub variable '{name}' should be a string: {value:?}");
                    };
                    text = text.replace(&format!("${{{name}}}"), &value);
                }
                return Some(CfExpression::String(text));
            }

            Some(CfExpression::Object(
                fields
                    .iter()
                    .filter_map(|(key, value)| {
                        resolve(value, conditions, declined).map(|value| (key.clone(), value))
                    })
                    .collect(),
            ))
        }
        CfExpression::List(items) => Some(CfExpression::List(
            items
                .iter()
                .filter_map(|item| match resolve(item, conditions, declined) {
                    Some(value) => Some(value),
                    None => (declined == Declined::Null).then_some(CfExpression::Null),
                })
                .collect(),
        )),
        other => Some(other.clone()),
    }
}

/// Text standing in for a deploy-time string inside `Fn::Join`: `Ref: Name` is
/// `${Name}` and `Fn::GetAtt: [Id, Attr]` is `${Id.Attr}`.
fn placeholder(expression: &CfExpression) -> Option<String> {
    let CfExpression::Object(fields) = expression else {
        return None;
    };
    match (fields.get("Ref"), fields.get("Fn::GetAtt")) {
        (Some(CfExpression::String(name)), None) => Some(format!("${{{name}}}")),
        (None, Some(CfExpression::List(target))) => match &target[..] {
            [CfExpression::String(id), CfExpression::String(attribute)] => {
                Some(format!("${{{id}.{attribute}}}"))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Resource ids present in a resolved registration payload, in order.
pub fn entry_ids(payload: &CfExpression) -> Vec<String> {
    let CfExpression::List(entries) = payload else {
        panic!("registration payload should be a list: {payload:?}");
    };
    entries
        .iter()
        .map(|entry| {
            let CfExpression::Object(fields) = entry else {
                panic!("registration entry should be an object: {entry:?}");
            };
            match fields.get("id").expect("registration entry id") {
                CfExpression::String(id) => id.clone(),
                other => panic!("registration entry id should be a string: {other:?}"),
            }
        })
        .collect()
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
            management_trust: Default::default(),
        },
    )
    .expect("template should render");
    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok(description);
    yaml
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

/// Evaluates a JSON-text output the way CloudFormation does and parses the text.
///
/// Outputs are built from core intrinsics (see `json_text`), so this follows
/// `Fn::Join`, `Fn::If`, `Fn::Sub` and `AWS::NoValue` (a dropped `Fn::Join`
/// element). A deploy-time value it cannot know renders as a placeholder:
/// `Ref: Name` as `"${Name}"` and `Fn::GetAtt: [Id, Attr]` as `"${Id.Attr}"`.
pub fn output_json(
    template: &serde_json::Value,
    output: &str,
    conditions: &HashMap<&str, bool>,
) -> serde_json::Value {
    let text = evaluate_text(&template["Outputs"][output]["Value"], conditions)
        .unwrap_or_else(|| panic!("output '{output}' should resolve"));
    let serde_json::Value::String(text) = text else {
        panic!("output '{output}' should resolve to text: {text}");
    };
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("output '{output}' is not valid JSON ({error}): {text}"))
}

fn evaluate_text(
    value: &serde_json::Value,
    conditions: &HashMap<&str, bool>,
) -> Option<serde_json::Value> {
    use serde_json::Value;
    let eval = |value: &Value| evaluate_text(value, conditions);
    let text = |value: &Value| match evaluate_text(value, conditions) {
        Some(Value::String(text)) => text,
        other => panic!("expected text, got {other:?} from {value}"),
    };
    match value {
        Value::Object(fields) => {
            if let Some(name) = fields.get("Ref") {
                let name = name.as_str().expect("Ref name");
                return (name != "AWS::NoValue").then(|| Value::String(format!("${{{name}}}")));
            }
            if let Some(Value::Array(target)) = fields.get("Fn::GetAtt") {
                return Some(Value::String(format!(
                    "${{{}.{}}}",
                    target[0].as_str().expect("logical id"),
                    target[1].as_str().expect("attribute")
                )));
            }
            if let Some(Value::Array(arguments)) = fields.get("Fn::If") {
                let condition = arguments[0].as_str().expect("condition name");
                let answer = *conditions
                    .get(condition)
                    .unwrap_or_else(|| panic!("no answer supplied for condition '{condition}'"));
                return eval(&arguments[if answer { 1 } else { 2 }]);
            }
            if let Some(Value::Array(arguments)) = fields.get("Fn::Join") {
                let delimiter = arguments[0].as_str().expect("Fn::Join delimiter");
                let items = match eval(&arguments[1]) {
                    Some(Value::Array(items)) => items,
                    // A list parameter: its single placeholder stands for the whole list.
                    Some(Value::String(item)) => vec![Value::String(item)],
                    other => panic!("Fn::Join takes a list: {other:?}"),
                };
                let items: Vec<String> = items.iter().map(&text).collect();
                return Some(Value::String(items.join(delimiter)));
            }
            if let Some(sub) = fields.get("Fn::Sub") {
                return Some(Value::String(match sub {
                    Value::String(template) => template.clone(),
                    Value::Array(arguments) => {
                        let mut template = arguments[0].as_str().expect("template").to_string();
                        for (name, value) in arguments[1].as_object().expect("variables") {
                            template = template.replace(&format!("${{{name}}}"), &text(value));
                        }
                        template
                    }
                    other => panic!("malformed Fn::Sub: {other}"),
                }));
            }
            assert!(
                !fields.keys().any(|key| key.starts_with("Fn::")),
                "unsupported expression in a JSON-text output: {value}"
            );
            Some(Value::Object(
                fields
                    .iter()
                    .filter_map(|(key, value)| eval(value).map(|value| (key.clone(), value)))
                    .collect(),
            ))
        }
        Value::Array(items) => Some(Value::Array(items.iter().filter_map(eval).collect())),
        other => Some(other.clone()),
    }
}
