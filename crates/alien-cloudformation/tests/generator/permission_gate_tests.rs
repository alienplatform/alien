//! Permission-gate scenarios — a gated permission set exists in the template
//! only under a Condition keyed on its deployer input, while ungated sets
//! render unconditionally. The gate is fail-closed: the Condition is false
//! (and the policy therefore absent) unless the input matches.

use super::helpers::{evaluate_condition, render_built_ins};
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Kv, PermissionGate, PermissionProfile, PermissionsConfig, Queue, ResourceLifecycle,
    ServiceAccount, Stack, StackInputDefaultValue, StackInputDefinition, StackInputKind,
    StackInputProvider, StackInputValidation, StackSettings, Storage,
};
use serde_json::Value;

fn enum_input(id: &str, values: &[&str], default: &str) -> StackInputDefinition {
    StackInputDefinition {
        id: id.to_string(),
        kind: StackInputKind::Enum,
        provided_by: vec![StackInputProvider::Deployer],
        required: false,
        label: id.to_string(),
        description: "Gate input.".to_string(),
        placeholder: None,
        default: Some(StackInputDefaultValue::String(default.to_string())),
        platforms: None,
        validation: Some(StackInputValidation {
            min_length: None,
            max_length: None,
            pattern: None,
            format: None,
            min: None,
            max: None,
            values: Some(values.iter().map(|v| v.to_string()).collect()),
            min_items: None,
            max_items: None,
        }),
        env: vec![],
    }
}

fn boolean_input(id: &str, default: bool) -> StackInputDefinition {
    StackInputDefinition {
        id: id.to_string(),
        kind: StackInputKind::Boolean,
        provided_by: vec![StackInputProvider::Deployer],
        required: false,
        label: id.to_string(),
        description: "Gate input.".to_string(),
        placeholder: None,
        default: Some(StackInputDefaultValue::Boolean(default)),
        platforms: None,
        validation: None,
        env: vec![],
    }
}

fn gated_stack() -> Stack {
    let profile = PermissionProfile::new()
        .resource("metadata", ["kv/data-write"])
        .resource("jobs", ["queue/data-write"])
        .resource("assets", ["storage/data-read"]);

    let service_account =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("permission sets should resolve");

    let mut permissions = PermissionsConfig::new().with_profile("execution", profile);
    permissions.gates = vec![
        PermissionGate {
            profile: "execution".to_string(),
            resource: "metadata".to_string(),
            permission_set_id: "kv/data-write".to_string(),
            input_id: "kvEnabled".to_string(),
            enabled_value: "true".to_string(),
        },
        PermissionGate {
            profile: "execution".to_string(),
            resource: "jobs".to_string(),
            permission_set_id: "queue/data-write".to_string(),
            input_id: "queueMode".to_string(),
            enabled_value: "on".to_string(),
        },
    ];

    Stack::new("gated".to_string())
        .permissions(permissions)
        .inputs(vec![
            boolean_input("kvEnabled", false),
            enum_input("queueMode", &["on", "off"], "off"),
        ])
        .add(service_account, ResourceLifecycle::Frozen)
        .add(Kv::new("metadata".to_string()).build(), ResourceLifecycle::Frozen)
        .add(Queue::new("jobs".to_string()).build(), ResourceLifecycle::Frozen)
        .add(Storage::new("assets".to_string()).build(), ResourceLifecycle::Frozen)
        .build()
}

fn statement_actions(policy_document: &Value) -> Vec<String> {
    let mut actions = Vec::new();
    for statement in policy_document["Statement"]
        .as_array()
        .expect("policy document should have a Statement list")
    {
        match &statement["Action"] {
            Value::String(action) => actions.push(action.clone()),
            Value::Array(list) => {
                for action in list {
                    actions.push(action.as_str().expect("action should be a string").to_string());
                }
            }
            other => panic!("unexpected Action shape: {other:?}"),
        }
    }
    actions
}

#[test]
fn gated_grants_render_under_a_condition_and_ungated_grants_do_not() {
    let yaml = render_built_ins(
        &gated_stack(),
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws permission gates",
    );
    let template: Value = serde_yaml::from_str(&yaml).expect("template YAML should parse");

    // Every permission set here is resource-scoped, so each renders as its own
    // standalone AWS::IAM::Policy; the two gated ones carry a Condition and the
    // ungated one does not.

    // Gated kv set: standalone policy under the kvEnabled boolean condition.
    let kv_policy = &template["Resources"]["MetadataExecutionSaRoleKvDataWrite"];
    assert_eq!(kv_policy["Type"], "AWS::IAM::Policy");
    assert_eq!(kv_policy["Condition"], "WhenKvEnabledTrue");
    assert!(
        statement_actions(&kv_policy["Properties"]["PolicyDocument"])
            .iter()
            .any(|a| a == "dynamodb:PutItem"),
        "gated kv policy must carry the dynamodb actions"
    );

    // Gated queue set: standalone policy under the queueMode enum condition.
    let queue_policy = &template["Resources"]["JobsExecutionSaRoleQueueDataWrite"];
    assert_eq!(queue_policy["Type"], "AWS::IAM::Policy");
    assert_eq!(queue_policy["Condition"], "WhenQueueModeOn");
    assert!(
        statement_actions(&queue_policy["Properties"]["PolicyDocument"])
            .iter()
            .any(|a| a == "sqs:SendMessage"),
        "gated queue policy must carry the sqs actions"
    );

    // Ungated storage set: standalone policy with no condition.
    let storage_policy = &template["Resources"]["AssetsExecutionSaRoleStoragePermission00"];
    assert_eq!(storage_policy["Type"], "AWS::IAM::Policy");
    assert!(
        storage_policy.get("Condition").is_none(),
        "ungated storage policy must render unconditionally"
    );

    // Fail-closed: each Condition is true only when the deployer input matches.
    // The parameter defaults (kvEnabled=false, queueMode=off) leave both gated
    // policies out of the deployed stack unless the deployer opts in.
    assert!(evaluate_condition(&template, "WhenKvEnabledTrue", &[("InputKvEnabled", "true")]));
    assert!(!evaluate_condition(&template, "WhenKvEnabledTrue", &[("InputKvEnabled", "false")]));
    assert!(!evaluate_condition(&template, "WhenKvEnabledTrue", &[]));
    assert!(evaluate_condition(&template, "WhenQueueModeOn", &[("InputQueueMode", "on")]));
    assert!(!evaluate_condition(&template, "WhenQueueModeOn", &[("InputQueueMode", "off")]));
    assert!(!evaluate_condition(&template, "WhenQueueModeOn", &[]));
}
