//! Parameter-only stack updates.
//!
//! Customers scale and reconfigure an installed stack by changing a parameter on
//! the existing template (`--use-previous-template`, or "Use existing template"
//! in the console). That only works when the stored template still references
//! the parameter. The `AWS::LanguageExtensions` transform resolves every
//! parameter to a literal when it processes a template, and CloudFormation
//! reuses that processed copy, so such an update reported "no changes".
//!
//! To check a rendered template against AWS: create a stack from it, run
//! `aws cloudformation update-stack --use-previous-template` changing only
//! `ComputeStatefulMachines`, and confirm the change set modifies the
//! registration's `StackSettings`.

use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfEmitter, CfExpression, CfRegistry, CfResource,
    CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    import::EmitContext, CapacityGroup, CapacityGroupScalePolicy, ComputeChoiceRange,
    ComputeCluster, ComputePoolSelection, ComputeSettings, Network, NetworkSettings, Platform,
    ResourceLifecycle, Result, Stack, StackSettings, Storage, Worker, WorkerCode,
};
use serde_json::{json, Value};

/// Stands in for the platform's compute-cluster emitter, which this crate does
/// not ship. The parameters under test come from the generator, not the emitter.
struct ComputeClusterEmitter;

impl CfEmitter for ComputeClusterEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let logical_id = ctx.name_for(ctx.resource_id).expect("logical id");
        Ok(vec![CfResource::new(
            logical_id.to_string(),
            "AWS::CloudFormation::WaitConditionHandle".to_string(),
        )])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let logical_id = ctx.name_for(ctx.resource_id).expect("logical id");
        Ok(CfExpression::object([(
            "clusterId",
            CfExpression::ref_(logical_id),
        )]))
    }
}

fn range(min: u32, max: u32, default: u32) -> ComputeChoiceRange {
    ComputeChoiceRange { min, max, default }
}

fn group(group_id: &str, scale_policy: CapacityGroupScalePolicy) -> CapacityGroup {
    CapacityGroup {
        group_id: group_id.to_string(),
        instance_type: None,
        profile: None,
        min_size: 1,
        max_size: 4,
        scale_policy: Some(scale_policy),
        nested_virtualization: None,
    }
}

/// True when `value` reads parameter `name` through `Ref` or `Fn::Sub`.
fn references(value: &Value, name: &str) -> bool {
    match value {
        Value::Object(fields) => {
            fields.get("Ref").and_then(Value::as_str) == Some(name)
                || match fields.get("Fn::Sub") {
                    Some(Value::String(template)) => template.contains(&format!("${{{name}}}")),
                    Some(Value::Array(arguments)) => arguments.first().is_some_and(|template| {
                        template
                            .as_str()
                            .is_some_and(|template| template.contains(&format!("${{{name}}}")))
                    }) || arguments.iter().skip(1).any(|value| references(value, name)),
                    _ => false,
                }
                || fields.values().any(|value| references(value, name))
        }
        Value::Array(items) => items.iter().any(|value| references(value, name)),
        _ => false,
    }
}

#[test]
fn every_parameter_stays_live_for_updates_that_reuse_the_template() {
    let network = NetworkSettings::Create {
        cidr: None,
        availability_zones: 2,
    };
    let storage = Storage::new("data".to_string()).build();
    let stack = Stack::new("parameter-updates".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(network.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(group(
                    "stateful",
                    CapacityGroupScalePolicy::Fixed {
                        machines: range(2, 4, 2),
                    },
                ))
                .capacity_group(group(
                    "general",
                    CapacityGroupScalePolicy::Autoscale {
                        min: range(1, 2, 1),
                        max: range(2, 4, 3),
                    },
                ))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "example.com/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .link(&storage)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(storage, ResourceLifecycle::Frozen)
        .build();
    let settings = StackSettings {
        network: Some(network),
        compute: Some(ComputeSettings {
            containers: Default::default(),
            pools: [
                (
                    "stateful".to_string(),
                    ComputePoolSelection::Fixed {
                        machines: 2,
                        machine: Some("m7g.large".to_string()),
                        failure_domains: None,
                    },
                ),
                (
                    "general".to_string(),
                    ComputePoolSelection::Autoscale {
                        min: 1,
                        max: 3,
                        machine: Some("m7g.large".to_string()),
                        failure_domains: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        }),
        ..StackSettings::default()
    };
    let mut registry = CfRegistry::built_in();
    registry.register(
        ComputeCluster::RESOURCE_TYPE,
        Platform::Aws,
        ComputeClusterEmitter,
    );

    for registration in [
        RegistrationMode::Both {
            lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
            callback_url: None,
        },
        RegistrationMode::OutputsFallback,
    ] {
        let template = generate_cloudformation_template(
            &stack,
            CloudFormationOptions {
                registry: &registry,
                target: CloudFormationTarget::Aws,
                stack_settings: settings.clone(),
                setup_target: "aws".to_string(),
                setup_fingerprint: "test".to_string(),
                setup_fingerprint_version: 1,
                registration,
                description: Some("parameter updates".to_string()),
            },
        )
        .expect("template should render");

        assert!(
            template.transform.is_empty(),
            "a transform resolves parameters into the stored template: {:?}",
            template.transform
        );
        let rendered = serde_json::to_value(&template).expect("template serializes");
        let rendered_text = rendered.to_string();
        for function in ["Fn::ToJsonString", "Fn::Length", "Fn::ForEach"] {
            assert!(
                !rendered_text.contains(function),
                "{function} needs the transform"
            );
        }
        // `ManagingAccountId` is read only by resources that pull images from the
        // management account, and this stack has none.
        for name in template
            .parameters
            .keys()
            .filter(|name| name.as_str() != "ManagingAccountId")
        {
            assert!(
                ["Resources", "Outputs", "Conditions", "Rules"]
                    .iter()
                    .any(|section| references(&rendered[section], name)),
                "parameter '{name}' is never read, so changing it cannot update the stack"
            );
        }

        let compute = match rendered["Resources"].get("DeploymentRegistration") {
            Some(registration) => registration["Properties"]["StackSettings"]["compute"].clone(),
            None => {
                let settings = super::helpers::output_json(
                    &rendered,
                    "DeploymentStackSettings",
                    &[
                        ("NetworkModeCreate", true),
                        ("HasVpcCidr", false),
                        ("HasDomainName", false),
                    ]
                    .into_iter()
                    .collect(),
                );
                settings["compute"].clone()
            }
        };
        let live = |name: &str| match rendered["Resources"].get("DeploymentRegistration") {
            Some(_) => json!({ "Ref": name }),
            None => json!(format!("${{{name}}}")),
        };
        assert_eq!(compute["pools"]["stateful"]["machines"], live("ComputeStatefulMachines"));
        assert_eq!(compute["pools"]["stateful"]["machine"], live("ComputeStatefulMachine"));
        assert_eq!(compute["pools"]["general"]["min"], live("ComputeGeneralMin"));
        assert_eq!(compute["pools"]["general"]["max"], live("ComputeGeneralMax"));
        assert_eq!(compute["pools"]["general"]["machine"], live("ComputeGeneralMachine"));

        let yaml = to_yaml(&template).expect("template should serialize");
        alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("parameter updates");
    }
}
