//! Resolved setup policies and registration must describe the same IAM ceiling.

use super::{
    helpers::{
        custom_resource_registration, gate_input, render_built_ins_template, try_render_built_ins,
    },
    network_inputs_tests::evaluate,
};
use alien_cloudformation::{CloudFormationTarget, RegistrationMode};
use alien_core::{
    import::data::AwsRemoteStackManagementImportData, Ai, Key, ManagementPermissions,
    PermissionProfile, RemoteStackManagement, ResourceLifecycle, Stack, StackSettings,
};
use alien_permissions::{
    generators::{aws_permissions_boundary_covers, aws_stack_permissions_boundary},
    PermissionContext,
};
use serde_json::{json, Value};
use std::collections::HashMap;

fn stack() -> Stack {
    Stack::new("boundary".into())
        .permission(
            "application",
            PermissionProfile::new()
                .resource("first", ["key/remote-cryptography"])
                .resource("second", ["key/remote-cryptography"]),
        )
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global([
                "service-account/provision",
                "artifact-registry/provision",
                "artifact-registry/management",
                "worker/provision",
                "ai/provision",
                "container/provision",
            ]),
        ))
        .add(Key::new("first".into()).build(), ResourceLifecycle::Frozen)
        .add(Key::new("second".into()).build(), ResourceLifecycle::Frozen)
        .add(
            RemoteStackManagement::new("management".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

#[test]
fn boundary_creation_registration_and_runtime_generation_agree() {
    let stack = stack();
    let (template, yaml) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        RegistrationMode::Both {
            lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".into(),
            callback_url: None,
        },
        CloudFormationTarget::Aws,
        "aws",
        "permissions boundary with two physical keys",
    );
    if let Ok(path) = std::env::var("ALIEN_TEST_BOUNDARY_TEMPLATE") {
        std::fs::write(path, yaml).expect("write validation artifact");
    }
    let boundary = &template.resources["ManagementPermissionsBoundary"];
    assert!(
        !boundary.properties.contains_key("Roles"),
        "the ceiling is not an identity grant"
    );
    assert!(
        template.resources["DeploymentRegistration"]
            .depends_on
            .contains(&"ManagementPermissionsBoundary".to_string()),
        "runtime cleanup must finish before setup deletes the ceiling"
    );
    for prefix in ["boundary-short".to_string(), "s".repeat(40)] {
        let account = "123456789012";
        let region = "ap-southeast-7";
        let arn = format!("arn:aws:iam::{account}:policy/{prefix}-permissions-boundary");
        let first = "00000000-0000-0000-0000-000000000001";
        let second = "00000000-0000-0000-0000-000000000002";
        let parameters: HashMap<String, Value> = [
            ("AWS::StackName", json!(prefix)),
            ("AWS::Region", json!(region)),
            ("AWS::AccountId", json!(account)),
            ("AWS::Partition", json!("aws")),
            (
                "ManagingRoleArn",
                json!("arn:aws:iam::210987654321:role/manager"),
            ),
            ("ManagementRole", json!(format!("{prefix}-management"))),
            (
                "ManagementRole.Arn",
                json!(format!("arn:aws:iam::{account}:role/{prefix}-management")),
            ),
            ("ManagementPermissionsBoundary", json!(arn)),
            ("First", json!(first)),
            ("Second", json!(second)),
            (
                "First.Arn",
                json!(format!("arn:aws:kms:{region}:{account}:key/{first}")),
            ),
            (
                "Second.Arn",
                json!(format!("arn:aws:kms:{region}:{account}:key/{second}")),
            ),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let resolve = |expression: &Value| evaluate(expression, &template, &parameters);
        let installed =
            resolve(&serde_json::to_value(&boundary.properties["PolicyDocument"]).unwrap());
        let context = PermissionContext::new()
            .with_stack_prefix(&prefix)
            .with_aws_account_id(account)
            .with_aws_region(region)
            .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
            .with_managing_account_id("210987654321");
        let expected = aws_stack_permissions_boundary(&stack, &context, |_, scope| {
            let id = match scope {
                "first" => first,
                "second" => second,
                _ => panic!("unexpected scope: {scope}"),
            };
            Ok(context
                .clone()
                .with_resource_id(scope)
                .with_resource_name(format!("arn:aws:kms:{region}:{account}:key/{id}")))
        })
        .unwrap();
        assert!(aws_permissions_boundary_covers(&installed, &expected).unwrap());
        assert!(aws_permissions_boundary_covers(&expected, &installed).unwrap());
        assert!(installed.to_string().contains(first) && installed.to_string().contains(second));
        let payload = resolve(
            &serde_json::to_value(
                &template.resources["DeploymentRegistration"].properties["Resources"],
            )
            .unwrap(),
        );
        let import: AwsRemoteStackManagementImportData = serde_json::from_value(
            payload
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["id"] == "management")
                .unwrap()["importData"]
                .clone(),
        )
        .unwrap();
        assert_eq!(
            import.permissions_boundary_arn.as_deref(),
            Some(arn.as_str())
        );
        assert_eq!(
            import.permissions_boundary_document,
            Some(installed.clone())
        );
        let mut output_entries = Vec::new();
        for (name, output) in &template.outputs {
            if name.starts_with("DeploymentResources") {
                let resolved = resolve(&serde_json::to_value(&output.value).unwrap());
                output_entries.extend(
                    serde_json::from_str::<Vec<Value>>(resolved.as_str().unwrap()).unwrap(),
                );
            }
        }
        assert_eq!(
            output_entries
                .iter()
                .find(|entry| entry["id"] == "management")
                .unwrap()["importData"],
            serde_json::to_value(import).unwrap()
        );
        let mut role_writes = 0;
        for resource in template
            .resources
            .values()
            .filter(|resource| resource.properties.contains_key("Roles"))
        {
            let policy =
                resolve(&serde_json::to_value(&resource.properties["PolicyDocument"]).unwrap());
            for statement in policy["Statement"].as_array().unwrap() {
                if statement["Effect"] != "Allow" {
                    continue;
                }
                for action in statement["Action"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_else(|| std::slice::from_ref(&statement["Action"]))
                {
                    if matches!(
                        action.as_str().unwrap(),
                        "iam:CreateRole"
                            | "iam:PutRolePolicy"
                            | "iam:DeleteRolePolicy"
                            | "iam:AttachRolePolicy"
                            | "iam:DetachRolePolicy"
                            | "iam:UpdateAssumeRolePolicy"
                            | "iam:PutRolePermissionsBoundary"
                    ) {
                        role_writes += 1;
                        if action == "iam:DetachRolePolicy"
                            && statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"]
                                .is_null()
                        {
                            // The approved removal-only migration for obsolete DLM roles.
                            assert_eq!(
                                statement["Condition"],
                                json!({"StringEquals": {
                                    "iam:PolicyARN": "arn:aws:iam::aws:policy/service-role/AWSDataLifecycleManagerServiceRole"
                                }})
                            );
                            assert_eq!(
                                statement["Resource"],
                                json!([format!("arn:aws:iam::{account}:role/{prefix}-dlm-*")])
                            );
                            continue;
                        }
                        assert_eq!(
                            statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"], arn,
                            "{statement}"
                        );
                    }
                    assert_ne!(action, "iam:DeleteRolePermissionsBoundary");
                }
            }
        }
        assert!(role_writes >= 7);
    }
}

#[test]
fn conditional_key_grants_fail_before_emitting_an_unresolvable_boundary() {
    let mut stack = stack();
    stack.inputs.push(gate_input(
        "keyEnabled",
        "Enable key",
        "Create the first key",
    ));
    stack.resources.get_mut("first").unwrap().enabled_when = Some("keyEnabled".into());
    let error = try_render_built_ins(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "conditional key boundary",
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("must be unconditional"));
}

#[test]
fn gated_resource_role_writes_keep_the_boundary_condition() {
    let stack = Stack::new("gated-boundary".into())
        .inputs(vec![gate_input(
            "aiEnabled",
            "Enable AI",
            "Create the AI gateway",
        )])
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["ai/provision"]),
        ))
        .add_enabled_when(
            Ai::new("gateway".into()).build(),
            ResourceLifecycle::Frozen,
            "aiEnabled",
        )
        .add(
            RemoteStackManagement::new("management".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let (template, _) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "gated role writes",
    );
    let mut writes = 0;
    for resource in template
        .resources
        .values()
        .filter(|resource| resource.condition.is_some())
    {
        let Some(policy) = resource.properties.get("PolicyDocument") else {
            continue;
        };
        let policy = serde_json::to_value(policy).unwrap();
        for statement in policy["Statement"].as_array().unwrap() {
            if statement["Action"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action == "iam:PutRolePolicy")
            {
                writes += 1;
                assert_eq!(
                    statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"],
                    json!({"Fn::Sub": "arn:${AWS::Partition}:iam::${AWS::AccountId}:policy/${AWS::StackName}-permissions-boundary"})
                );
            }
        }
    }
    assert!(writes > 0);
}

#[test]
fn size_budget_keeps_distinct_physical_keys_distinct() {
    let mut builder = Stack::new("many-keys".into()).add(
        RemoteStackManagement::new("management".into()).build(),
        ResourceLifecycle::Frozen,
    );
    let mut profile = PermissionProfile::new();
    for index in 0..12 {
        let id = format!("key-{index}");
        profile = profile.resource(&id, ["key/remote-cryptography"]);
        builder = builder.add(Key::new(id).build(), ResourceLifecycle::Frozen);
    }
    let error = try_render_built_ins(
        &builder.permission("application", profile).build(),
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "distinct key size budget",
    )
    .unwrap_err();
    assert!(
        format!("{error:?}").contains("PERMISSIONS_BOUNDARY_TOO_LARGE"),
        "{error:?}"
    );
}
