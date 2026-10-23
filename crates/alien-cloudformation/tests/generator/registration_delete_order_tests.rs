//! Stack delete order: the registration custom resource is deleted first.
//!
//! Its delete callback tears down the resources created at runtime, and those
//! still use the setup's roles and network while they shut down. CloudFormation
//! deletes a resource only after everything that depends on it, so the
//! registration must depend on every other resource in the template, including
//! the ones behind a condition.

use super::helpers::{custom_resource_registration, render_built_ins_template};
use alien_cloudformation::{CfExpression, CfTemplate, CloudFormationTarget};
use alien_core::{
    HeartbeatsMode, Network, NetworkSettings, ResourceLifecycle, Stack, StackSettings, Storage,
    TelemetryMode, UpdatesMode,
};
use std::collections::BTreeSet;

const REGISTRATION: &str = "DeploymentRegistration";
/// The start signal is the one resource deliberately outside the registration's
/// dependency set: it must be created before anything else and is only a
/// progress report, so nothing it does outlives the stack.
const SETUP_STARTED: &str = "DeploymentSetupStarted";

/// The resources a resource depends on, as CloudFormation computes it:
/// `DependsOn` plus every `Ref`, `Fn::GetAtt` and `Fn::Sub` target in its
/// properties. Each dependency comes with the conditions of the `Fn::If`
/// branches it sits in.
fn dependencies(template: &CfTemplate, logical_id: &str) -> Vec<(String, BTreeSet<String>)> {
    let resource = &template.resources[logical_id];
    let mut found: Vec<(String, BTreeSet<String>)> = resource
        .depends_on
        .iter()
        .map(|id| (id.clone(), BTreeSet::new()))
        .collect();
    for value in resource.properties.values() {
        collect_references(value, &BTreeSet::new(), &mut found);
    }
    found.retain(|(id, _)| template.resources.contains_key(id));
    found
}

fn collect_references(
    expression: &CfExpression,
    guards: &BTreeSet<String>,
    found: &mut Vec<(String, BTreeSet<String>)>,
) {
    match expression {
        CfExpression::Object(fields) => {
            if let Some(CfExpression::List(branches)) = fields.get("Fn::If") {
                let [CfExpression::String(condition), when_true, when_false] = &branches[..] else {
                    panic!("malformed Fn::If: {branches:?}");
                };
                let mut guarded = guards.clone();
                guarded.insert(condition.clone());
                collect_references(when_true, &guarded, found);
                collect_references(when_false, guards, found);
                return;
            }
            if let Some(CfExpression::String(target)) = fields.get("Ref") {
                found.push((target.clone(), guards.clone()));
            }
            if let Some(CfExpression::List(target)) = fields.get("Fn::GetAtt") {
                if let Some(CfExpression::String(id)) = target.first() {
                    found.push((id.clone(), guards.clone()));
                }
            }
            if let Some(sub) = fields.get("Fn::Sub") {
                let text = match sub {
                    CfExpression::String(text) => text,
                    CfExpression::List(arguments) => match arguments.first() {
                        Some(CfExpression::String(text)) => text,
                        _ => panic!("malformed Fn::Sub: {arguments:?}"),
                    },
                    other => panic!("malformed Fn::Sub: {other:?}"),
                };
                for variable in text.split("${").skip(1) {
                    let name = variable.split('}').next().unwrap_or_default();
                    let id = name.split('.').next().unwrap_or_default();
                    found.push((id.to_string(), guards.clone()));
                }
            }
            for value in fields.values() {
                collect_references(value, guards, found);
            }
        }
        CfExpression::List(items) => {
            for item in items {
                collect_references(item, guards, found);
            }
        }
        _ => {}
    }
}

fn create_network_stack() -> (Stack, StackSettings) {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 3,
        }),
        updates: UpdatesMode::ApprovalRequired,
        telemetry: TelemetryMode::Off,
        heartbeats: HeartbeatsMode::Off,
        ..StackSettings::default()
    };
    let stack = Stack::new("delete-order".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network settings"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    (stack, settings)
}

#[test]
fn registration_is_deleted_before_every_setup_resource() {
    let (stack, settings) = create_network_stack();
    let (template, _) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "registration delete order",
    );

    let conditional: Vec<&String> = template
        .resources
        .iter()
        .filter_map(|(id, resource)| resource.condition.as_ref().map(|_| id))
        .collect();
    // Created networks are conditional, so the template must have routes and
    // subnet associations behind conditions for this test to mean anything.
    assert!(
        conditional
            .iter()
            .any(|id| template.resources[id.as_str()].resource_type == "AWS::EC2::Route"),
        "the created network should include conditional routes: {conditional:?}"
    );

    let dependencies = dependencies(&template, REGISTRATION);
    let depended_on: BTreeSet<&str> = dependencies.iter().map(|(id, _)| id.as_str()).collect();
    let deleted_alongside: Vec<&str> = template
        .resources
        .keys()
        .map(String::as_str)
        .filter(|id| *id != REGISTRATION && *id != SETUP_STARTED && !depended_on.contains(id))
        .collect();
    assert!(
        !depended_on.contains(SETUP_STARTED),
        "the registration must not wait for the start signal"
    );
    assert!(
        deleted_alongside.is_empty(),
        "CloudFormation would delete these while the registration's delete callback still \
         tears down runtime resources: {deleted_alongside:?}"
    );

    // A conditional resource may only be referenced inside a branch of its own
    // condition. Otherwise a stack whose condition is false fails to create.
    for (id, guards) in &dependencies {
        if let Some(condition) = &template.resources[id].condition {
            assert!(
                guards.contains(condition),
                "'{id}' is referenced outside its condition '{condition}' (guards {guards:?})"
            );
        }
    }
}
