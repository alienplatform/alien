//! The key policy a direct setup sends to KMS must equal the one the CloudFormation package
//! carries for the same stack. Both come from one access plan; this test is the net for drift.

use super::helpers::render_built_ins_template;
use alien_cloudformation::RegistrationMode;
use alien_core::{
    Key, PermissionProfile, PermissionSetReference, PermissionsConfig, RemoteBindings,
    RemoteStackManagement, ResourceLifecycle, ResourceRef, ServiceAccount, Stack, StackSettings,
    Storage,
};
use alien_permissions::{
    aws_key_policy::{aws_key_policy_for_stack, LiteralAwsPrincipals},
    key_access::key_access_plan,
};
use serde_json::Value;

const PARTITION: &str = "aws";
const ACCOUNT: &str = "123456789012";
const PREFIX: &str = "diffstack";

/// Replaces the CloudFormation intrinsics with the values the controller uses.
fn substitute(value: Value, roles: &[(&str, String)]) -> Value {
    match value {
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::Sub") => {
            let text = map["Fn::Sub"].as_str().expect("Fn::Sub text");
            Value::String(
                text.replace("${AWS::Partition}", PARTITION)
                    .replace("${AWS::AccountId}", ACCOUNT)
                    .replace("${AWS::StackName}", PREFIX),
            )
        }
        Value::Object(map) if map.len() == 1 && map.contains_key("Fn::GetAtt") => {
            let [logical, attribute] = map["Fn::GetAtt"]
                .as_array()
                .and_then(|items| <&[Value; 2]>::try_from(items.as_slice()).ok())
                .cloned()
                .expect("GetAtt pair");
            assert_eq!(attribute, "Arn");
            let logical = logical.as_str().unwrap();
            let name = roles
                .iter()
                .find(|(id, _)| *id == logical)
                .unwrap_or_else(|| panic!("no controller role for {logical}"))
                .1
                .clone();
            Value::String(format!("arn:{PARTITION}:iam::{ACCOUNT}:role/{name}"))
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, substitute(value, roles)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|value| substitute(value, roles))
                .collect(),
        ),
        other => other,
    }
}

/// Orders arrays of strings: the order of a principal list carries no meaning.
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect(),
        ),
        Value::Array(items) => {
            let mut items: Vec<Value> = items.into_iter().map(canonical).collect();
            if items.iter().all(Value::is_string) {
                items.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            }
            Value::Array(items)
        }
        other => other,
    }
}

fn assert_same_policy(stack: &Stack, key_id: &str, logical_key: &str) {
    let (template, _) = render_built_ins_template(
        stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        alien_cloudformation::CloudFormationTarget::Aws,
        "aws",
        "key policy differential",
    );
    let cf_policy = serde_json::to_value(
        &template
            .resources
            .get(logical_key)
            .expect("key resource")
            .properties["KeyPolicy"],
    )
    .unwrap();

    let principals = LiteralAwsPrincipals {
        partition: PARTITION,
        account_id: ACCOUNT,
        prefix: PREFIX,
    };
    let roles = [
        ("ManagementRole", principals.management_role_name()),
        ("RemoteBindingsRole", principals.access_role_name()),
    ];
    let plan = key_access_plan(stack, key_id).unwrap();
    let controller_policy = aws_key_policy_for_stack(&plan, &principals);
    assert_eq!(
        canonical(substitute(cf_policy, &roles)),
        canonical(controller_policy),
        "the direct-setup key policy drifted from the CloudFormation one for '{key_id}'"
    );
}

fn with_service_accounts(accounts: &[&str], key_scope: &str) -> Stack {
    let mut permissions = PermissionsConfig::new();
    let mut stack = Stack::new("diff".to_string())
        .add(
            Key::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    for account in accounts {
        permissions = permissions.with_profile(
            account.trim_end_matches("-sa"),
            PermissionProfile::new().resource(key_scope, ["key/remote-cryptography"]),
        );
        stack = stack.add(
            ServiceAccount::new(account.to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    let mut stack = stack.permissions(permissions).build();
    stack.permissions.management =
        alien_core::ManagementPermissions::Extend(PermissionProfile::new().resource(
            "secrets",
            [
                PermissionSetReference::from_name("key/management"),
                PermissionSetReference::from_name("key/availability"),
            ],
        ));
    stack
}

#[test]
fn runtime_only_key_with_two_granted_accounts_and_management() {
    let stack = with_service_accounts(&["manager-sa", "worker-sa"], "secrets");
    assert_same_policy(&stack, "secrets", "Secrets");
}

#[test]
fn key_used_by_storage_has_no_allowlist() {
    let mut stack = Stack::new("diff".to_string())
        .add(
            Key::new("data-key".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("data".to_string())
                .encryption_key(ResourceRef::new(Key::RESOURCE_TYPE, "data-key"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    stack.permissions.management =
        alien_core::ManagementPermissions::Extend(PermissionProfile::new().resource(
            "data-key",
            [PermissionSetReference::from_name("key/management")],
        ));
    assert_same_policy(&stack, "data-key", "DataKey");
}

#[test]
fn key_published_through_remote_bindings() {
    let stack = Stack::new("diff".to_string())
        .add_with_remote_access(
            Key::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("remote-bindings".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    assert_same_policy(&stack, "secrets", "Secrets");
}
