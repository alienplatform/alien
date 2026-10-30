//! Resolved setup policies and registration must describe the same IAM ceiling.

use super::{
    helpers::{
        custom_resource_registration, gate_input, render_built_ins_template, try_render_built_ins,
    },
    network_inputs_tests::evaluate,
};
use alien_cloudformation::{CloudFormationTarget, RegistrationMode};
use alien_core::{
    import::{
        data::{AwsAiImportData, AwsRemoteStackManagementImportData},
        ImportedResource,
    },
    Ai, Key, ManagementPermissions, PermissionProfile, RemoteStackManagement, ResourceLifecycle,
    Stack, StackSettings,
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
            PermissionProfile::new().resource("first", ["key/remote-cryptography"]),
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
            alien_core::Container::new("db".into())
                .cluster("compute".into())
                .permissions("execution".into())
                .code(alien_core::ContainerCode::Image {
                    image: "postgres:17".into(),
                })
                .cpu(alien_core::ResourceSpec {
                    min: "1".into(),
                    desired: "1".into(),
                })
                .memory(alien_core::ResourceSpec {
                    min: "1Gi".into(),
                    desired: "1Gi".into(),
                })
                .persistent_storage(alien_core::PersistentStorage {
                    size: "10Gi".into(),
                    mount_path: "/data".into(),
                    encryption_key: Some(alien_core::ResourceRef::new("key".into(), "second")),
                    backups: alien_core::VolumeBackups::default(),
                })
                .build(),
            ResourceLifecycle::Live,
        )
        .add(
            RemoteStackManagement::new("management".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

#[test]
fn boundary_creation_registration_and_runtime_generation_agree() {
    for enabled in [None, Some(true), Some(false)] {
        check_boundary(enabled);
    }
}

fn check_boundary(enabled: Option<bool>) {
    let mut stack = stack();
    if enabled.is_some() {
        stack.inputs.push(gate_input(
            "keyEnabled",
            "Enable keys",
            "Create encryption keys",
        ));
        for id in ["first", "second"] {
            stack.resources.get_mut(id).unwrap().enabled_when = Some("keyEnabled".into());
        }
    }
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
        let arn = format!("arn:aws:iam::{account}:policy/{prefix}-deployment-management-boundary");
        let first = "00000000-0000-0000-0000-000000000001";
        let second = "00000000-0000-0000-0000-000000000002";
        let mut parameters: HashMap<String, Value> = [
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
        if let Some(enabled) = enabled {
            parameters.insert("InputKeyEnabled".into(), json!(enabled.to_string()));
            if !enabled {
                for name in ["First", "Second", "First.Arn", "Second.Arn"] {
                    parameters.remove(name);
                }
            }
        }
        let resolve = |expression: &Value| evaluate(expression, &template, &parameters);
        let installed =
            resolve(&serde_json::to_value(&boundary.properties["PolicyDocument"]).unwrap());
        let context = PermissionContext::new()
            .with_stack_prefix(&prefix)
            .with_aws_account_id(account)
            .with_aws_region(region)
            .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
            .with_managing_account_id("210987654321");
        let mut expected = aws_stack_permissions_boundary(&stack, &context, |_, scope| {
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
        if enabled == Some(false) {
            for statement in expected["Statement"].as_array_mut().unwrap() {
                if let Some(resources) = statement["Resource"].as_array_mut() {
                    resources.retain(|resource| {
                        resource != &json!(format!("arn:aws:kms:{region}:{account}:key/{first}"))
                            && resource
                                != &json!(format!("arn:aws:kms:{region}:{account}:key/{second}"))
                    });
                }
            }
        }
        assert!(aws_permissions_boundary_covers(&installed, &expected).unwrap());
        assert!(aws_permissions_boundary_covers(&expected, &installed).unwrap());
        assert_eq!(
            installed.to_string().contains(first),
            enabled != Some(false)
        );
        assert_eq!(
            installed.to_string().contains(second),
            enabled != Some(false)
        );
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
fn conditional_inline_key_grants_remain_rejected() {
    let mut stack = stack();
    stack
        .inputs
        .push(gate_input("keyEnabled", "Enable key", "Create key"));
    stack.resources.get_mut("first").unwrap().enabled_when = Some("keyEnabled".into());
    stack
        .permissions
        .profiles
        .get_mut("application")
        .unwrap()
        .0
        .insert(
            "first".into(),
            vec![alien_core::PermissionSetReference::Inline(
                alien_permissions::get_permission_set("key/remote-cryptography")
                    .unwrap()
                    .clone(),
            )],
        );
    let error = try_render_built_ins(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "conditional inline key",
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("inline grants must be unconditional"),
        "{error}"
    );
}

#[test]
fn gated_ai_registration_and_role_writes_preserve_the_boundary() {
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
        RegistrationMode::Both {
            lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".into(),
            callback_url: None,
        },
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
                    json!({"Fn::Sub": "arn:${AWS::Partition}:iam::${AWS::AccountId}:policy/${AWS::StackName}-deployment-management-boundary"})
                );
            }
        }
    }
    assert!(writes > 0);
    for enabled in [true, false] {
        let arn = "arn:aws:iam::123456789012:policy/bounded-deployment-management-boundary";
        let parameters = [
            ("AWS::StackName", json!("bounded")),
            ("AWS::Region", json!("us-east-1")),
            ("AWS::AccountId", json!("123456789012")),
            ("AWS::Partition", json!("aws")),
            (
                "ManagingRoleArn",
                json!("arn:aws:iam::210987654321:role/manager"),
            ),
            ("ManagementRole", json!("bounded-management")),
            (
                "ManagementRole.Arn",
                json!("arn:aws:iam::123456789012:role/bounded-management"),
            ),
            ("ManagementPermissionsBoundary", json!(arn)),
            ("InputAiEnabled", json!(enabled.to_string())),
            ("ManagementExternalId", json!("")),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        let payload = serde_json::to_value(
            &template.resources["DeploymentRegistration"].properties["Resources"],
        )
        .unwrap();
        // CloudFormation removes NoValue entries from resource-property lists.
        // Ordinary null entries must fail typed deserialization.
        let entries: Vec<ImportedResource> = payload
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| {
                let selected = if let Some(branches) = entry.get("Fn::If") {
                    let condition = &template.conditions[branches[0].as_str().unwrap()];
                    let accepted = evaluate(
                        &serde_json::to_value(condition).unwrap(),
                        &template,
                        &parameters,
                    )
                    .as_bool()
                    .unwrap();
                    &branches[if accepted { 1 } else { 2 }]
                } else {
                    entry
                };
                if selected == &json!({"Ref": "AWS::NoValue"}) {
                    return None;
                }
                Some(serde_json::from_value(evaluate(selected, &template, &parameters)).unwrap())
            })
            .collect();
        let expected_ids = if enabled {
            vec!["gateway", "management"]
        } else {
            vec!["management"]
        };
        let mut ids: Vec<_> = entries.iter().map(|entry| entry.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, expected_ids);
        for entry in &entries {
            if entry.id == "gateway" {
                assert_eq!(entry.resource_type.0, "ai");
                let data: AwsAiImportData =
                    serde_json::from_value(entry.import_data.clone()).unwrap();
                assert_eq!(data.region, "us-east-1");
            } else {
                assert_eq!(entry.resource_type.0, "remote-stack-management");
                let data: AwsRemoteStackManagementImportData =
                    serde_json::from_value(entry.import_data.clone()).unwrap();
                assert_eq!(data.permissions_boundary_arn.as_deref(), Some(arn));
                let installed = evaluate(
                    &serde_json::to_value(
                        &template.resources["ManagementPermissionsBoundary"].properties
                            ["PolicyDocument"],
                    )
                    .unwrap(),
                    &template,
                    &parameters,
                );
                assert_eq!(data.permissions_boundary_document, Some(installed));
            }
        }
        let mut output_entries = Vec::<ImportedResource>::new();
        for (name, output) in &template.outputs {
            if name.starts_with("DeploymentResources") {
                let text = evaluate(
                    &serde_json::to_value(&output.value).unwrap(),
                    &template,
                    &parameters,
                );
                output_entries.extend(
                    serde_json::from_str::<Vec<ImportedResource>>(text.as_str().unwrap()).unwrap(),
                );
            }
        }
        output_entries.sort_by(|left, right| left.id.cmp(&right.id));
        let mut entries = entries;
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        assert_eq!(
            output_entries, entries,
            "both registration transports must agree"
        );
    }
}

#[test]
fn size_budget_keeps_distinct_physical_keys_distinct() {
    let mut builder = Stack::new("many-keys".into()).add(
        RemoteStackManagement::new("management".into()).build(),
        ResourceLifecycle::Frozen,
    );
    let mut profile = PermissionProfile::new();
    for index in 0..40 {
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
