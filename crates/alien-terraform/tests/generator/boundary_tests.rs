//! Setup must install and register the same ceiling the runtime generates.
#[path = "../../../alien-permissions/tests/iam_policy/mod.rs"]
mod iam_policy;
use super::helpers::{assert_terraform_valid, gate_input, render, try_render};
use alien_core::{
    import::{
        data::{AwsAiImportData, AwsRemoteStackManagementImportData},
        ImportedResource,
    },
    Ai, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Key,
    ManagementPermissions, PermissionProfile, Platform, RemoteStackManagement, ResourceLifecycle,
    SetupManagement, Stack, StackSettings, StackState, ALIEN_MANAGED_BY_TAG_KEY,
    ALIEN_STACK_TAG_KEY,
};
use alien_permissions::{
    generators::{
        aws_permissions_boundary_covers, aws_stack_permissions_boundary, AwsIamPolicy,
        AwsIamStatement,
    },
    PermissionContext,
};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use alien_terraform::TerraformTarget;
use hcl::{
    eval::{Context, Evaluate, FuncDef, ParamType},
    Expression,
};
use iam_policy::{evaluate, Decision, Request};
use serde_json::{json, Value};

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

fn attribute<'a>(block: &'a hcl::Block, key: &str) -> &'a Expression {
    block
        .body
        .attributes()
        .find(|a| a.key() == key)
        .unwrap()
        .expr()
}

fn document(expression: &Expression, context: &Context) -> Value {
    let Expression::FuncCall(call) = expression else {
        panic!("expected jsonencode")
    };
    assert_eq!(call.name.name.as_str(), "jsonencode");
    serde_json::to_value(call.args[0].evaluate(context).unwrap()).unwrap()
}

fn find_import(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Object(fields) => {
            if fields
                .keys()
                .any(|k| k.to_string() == "permissionsBoundaryArn")
            {
                return Some(expression);
            }
            fields.values().find_map(find_import)
        }
        Expression::Array(items) => items.iter().find_map(find_import),
        Expression::FuncCall(call) => call.args.iter().find_map(find_import),
        _ => None,
    }
}

#[test]
fn terraform_installs_and_registers_the_runtime_boundary_with_distinct_keys() {
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    assert_terraform_valid(&module, "permissions boundary with distinct selected keys");
    let bodies: Vec<hcl::Body> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(_, contents)| hcl::from_str(contents).unwrap())
        .collect();
    let blocks: Vec<_> = bodies.iter().flat_map(|body| body.blocks()).collect();
    let boundary = blocks
        .iter()
        .find(|b| {
            b.labels()
                .iter()
                .any(|l| l.as_str() == "management_permissions_boundary")
        })
        .unwrap();
    assert!(boundary
        .body
        .attributes()
        .all(|attribute| attribute.key() != "tags"));
    for prefix in ["boundary-short".to_string(), "s".repeat(40)] {
        let region = "ap-southeast-7";
        let account = "123456789012";
        let arn = format!("arn:aws:iam::{account}:policy/{prefix}-deployment-management-boundary");
        let key1 =
            format!("arn:aws:kms:{region}:{account}:key/00000000-0000-0000-0000-000000000001");
        let key2 =
            format!("arn:aws:kms:{region}:{account}:key/00000000-0000-0000-0000-000000000002");
        let mut context = Context::new();
        for (name, value) in [
            (
                "local",
                json!({"resource_prefix": prefix, "first_selected_key": {"arn": key1}, "second_selected_key": {"arn": key2}}),
            ),
            (
                "data",
                json!({"aws_region": {"current": {"region": region}}, "aws_caller_identity": {"current": {"account_id": account}}}),
            ),
            (
                "var",
                json!({"managing_account_id": "210987654321", "managing_role_arn": "arn:aws:iam::210987654321:role/manager"}),
            ),
            (
                "aws_iam_policy",
                json!({"management_permissions_boundary": {"arn": arn}}),
            ),
            (
                "aws_iam_role",
                json!({"management": {"name": format!("{prefix}-management"), "arn": format!("arn:aws:iam::{account}:role/{prefix}-management")}}),
            ),
        ] {
            context.declare_var(name, hcl::to_value(value).unwrap());
        }
        if let Some(enabled) = enabled {
            context.declare_var(
                "var",
                hcl::to_value(json!({
                    "managing_account_id": "210987654321",
                    "managing_role_arn": "arn:aws:iam::210987654321:role/manager",
                    "input_key_enabled": enabled,
                }))
                .unwrap(),
            );
            if !enabled {
                context.declare_var(
                    "local",
                    hcl::to_value(json!({
                        "resource_prefix": prefix,
                        "first_selected_key": null,
                        "second_selected_key": null,
                    }))
                    .unwrap(),
                );
            }
            context.declare_func(
                "compact",
                FuncDef::builder().param(ParamType::Any).build(|args| {
                    Ok(hcl::Value::Array(
                        args[0]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|value| !value.is_null() && value.as_str() != Some(""))
                            .cloned()
                            .collect(),
                    ))
                }),
            );
        }
        let installed = document(attribute(boundary, "policy"), &context);
        assert_eq!(
            serde_json::to_value(attribute(boundary, "name").evaluate(&context).unwrap()).unwrap(),
            json!(format!("{prefix}-deployment-management-boundary"))
        );
        let runtime = PermissionContext::new()
            .with_stack_prefix(&prefix)
            .with_aws_region(region)
            .with_aws_account_id(account)
            .with_managing_account_id("210987654321")
            .with_managing_role_arn("arn:aws:iam::210987654321:role/manager");
        let mut resolved_stack = stack.clone();
        if enabled == Some(false) {
            resolved_stack.resources.shift_remove("first");
            resolved_stack.resources.shift_remove("second");
            resolved_stack.resources.shift_remove("db");
            resolved_stack
                .permissions
                .profiles
                .get_mut("application")
                .unwrap()
                .0
                .shift_remove("first");
        }
        let expected = aws_stack_permissions_boundary(&resolved_stack, &runtime, |_, scope| {
            Ok(runtime
                .clone()
                .with_resource_id(scope)
                .with_resource_name(match scope {
                    "first" => &key1,
                    "second" => &key2,
                    _ => panic!("unexpected scope"),
                }))
        })
        .unwrap();
        assert!(aws_permissions_boundary_covers(&installed, &expected)
            .unwrap_or_else(|error| panic!("{error}: {installed}")));
        assert!(aws_permissions_boundary_covers(&expected, &installed).unwrap());
        assert_eq!(
            installed.to_string().contains(&key1),
            enabled != Some(false)
        );
        assert_eq!(
            installed.to_string().contains(&key2),
            enabled != Some(false)
        );
        context.declare_var("aws_iam_policy", hcl::to_value(json!({"management_permissions_boundary": {"arn": arn, "policy": installed.to_string()}})).unwrap());
        context.declare_func(
            "jsondecode",
            FuncDef::builder().param(ParamType::String).build(|args| {
                serde_json::from_str(args[0].as_str().unwrap()).map_err(|e| e.to_string())
            }),
        );
        let import_expr = blocks
            .iter()
            .flat_map(|b| b.body.attributes())
            .find_map(|a| find_import(a.expr()))
            .expect("management registration");
        let imported: AwsRemoteStackManagementImportData = serde_json::from_value(
            serde_json::to_value(import_expr.evaluate(&context).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(imported.permissions_boundary_arn, Some(arn.clone()));
        assert_eq!(imported.permissions_boundary_document, Some(installed));
        let mut writes = 0;
        for block in &blocks {
            if !block
                .labels()
                .first()
                .is_some_and(|l| ["aws_iam_policy", "aws_iam_role_policy"].contains(&l.as_str()))
                || std::ptr::eq(*block, *boundary)
            {
                continue;
            }
            let policy = document(attribute(block, "policy"), &context);
            assert!(policy.to_string().len() <= 6144);
            for statement in policy["Statement"].as_array().unwrap() {
                if statement["Effect"] != "Allow" {
                    continue;
                }
                let actions = statement["Action"]
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![statement["Action"].clone()]);
                for action in actions {
                    if [
                        "iam:CreateRole",
                        "iam:PutRolePolicy",
                        "iam:AttachRolePolicy",
                        "iam:UpdateAssumeRolePolicy",
                        "iam:PutRolePermissionsBoundary",
                        "iam:DeleteRolePolicy",
                    ]
                    .contains(&action.as_str().unwrap())
                    {
                        writes += 1;
                        assert_eq!(
                            statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"],
                            json!(arn)
                        );
                    }
                }
            }
        }
        assert!(writes >= 6);
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
    let error = try_render(&stack, TerraformTarget::Aws, StackSettings::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("inline grants must be unconditional"),
        "{error}"
    );
}

#[test]
fn gated_ai_registration_and_role_writes_preserve_the_boundary() {
    let stack = Stack::new("gated".into())
        .inputs(vec![gate_input("aiEnabled", "Enable AI", "Create gateway")])
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
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    assert_terraform_valid(&module, "gated bounded AI role writes");
    let mut writes = 0;
    for (_, contents) in module.iter().filter(|(path, _)| path.ends_with(".tf")) {
        let body: hcl::Body = hcl::from_str(contents).unwrap();
        for block in body.blocks().filter(|b| {
            b.labels()
                .first()
                .is_some_and(|l| l.as_str() == "aws_iam_role_policy")
                && b.body.attributes().any(|a| a.key() == "count")
        }) {
            let mut context = Context::new();
            context.declare_var(
                "local",
                hcl::to_value(json!({"resource_prefix": "bounded"})).unwrap(),
            );
            context.declare_var("data", hcl::to_value(json!({"aws_region": {"current": {"region": "us-east-1"}}, "aws_caller_identity": {"current": {"account_id": "123456789012"}}})).unwrap());
            context.declare_var(
                "aws_iam_policy",
                hcl::to_value(json!({"management_permissions_boundary": {"arn": "boundary-arn"}}))
                    .unwrap(),
            );
            let policy = document(attribute(block, "policy"), &context);
            for statement in policy["Statement"].as_array().unwrap() {
                if statement["Action"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a == "iam:PutRolePolicy")
                {
                    writes += 1;
                    assert_eq!(
                        statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"],
                        json!("boundary-arn")
                    );
                }
            }
        }
    }
    assert!(writes > 0);
    let bodies: Vec<hcl::Body> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(_, text)| hcl::from_str(text).unwrap())
        .collect();
    let blocks: Vec<_> = bodies.iter().flat_map(|body| body.blocks()).collect();
    let boundary = blocks
        .iter()
        .find(|block| {
            block
                .labels()
                .iter()
                .any(|label| label.as_str() == "management_permissions_boundary")
        })
        .unwrap();
    let registration = blocks
        .iter()
        .filter(|block| block.identifier() == "locals")
        .flat_map(|block| block.body.attributes())
        .find(|attribute| attribute.key() == "deployment_resources")
        .unwrap();
    for enabled in [true, false] {
        let arn = "arn:aws:iam::123456789012:policy/bounded-deployment-management-boundary";
        let mut context = Context::new();
        for (name, value) in [
            ("local", json!({"resource_prefix": "bounded"})),
            (
                "data",
                json!({"aws_region": {"current": {"region": "us-east-1"}}, "aws_caller_identity": {"current": {"account_id": "123456789012"}}}),
            ),
            (
                "var",
                json!({"input_ai_enabled": enabled, "managing_account_id": "210987654321", "managing_role_arn": "arn:aws:iam::210987654321:role/manager"}),
            ),
            (
                "aws_iam_role",
                json!({"management": {"name": "bounded-management", "arn": "arn:aws:iam::123456789012:role/bounded-management"}}),
            ),
        ] {
            context.declare_var(name, hcl::to_value(value).unwrap());
        }
        let installed = document(attribute(boundary, "policy"), &context);
        context.declare_var("aws_iam_policy", hcl::to_value(json!({"management_permissions_boundary": {"arn": arn, "policy": installed.to_string()}})).unwrap());
        context.declare_func(
            "jsondecode",
            FuncDef::builder().param(ParamType::String).build(|args| {
                serde_json::from_str(args[0].as_str().unwrap()).map_err(|error| error.to_string())
            }),
        );
        context.declare_func(
            "concat",
            FuncDef::builder()
                .variadic_param(ParamType::Any)
                .build(|args| {
                    let mut items = Vec::new();
                    for arg in args.iter() {
                        items.extend(
                            arg.as_array()
                                .ok_or("concat requires lists")?
                                .iter()
                                .cloned(),
                        );
                    }
                    Ok(hcl::Value::Array(items))
                }),
        );
        // Evaluate the complete list so a disabled entry or dangling indexed
        // provider reference fails instead of being hidden by entry selection.
        let entries: Vec<ImportedResource> = serde_json::from_value(
            serde_json::to_value(registration.expr().evaluate(&context).unwrap()).unwrap(),
        )
        .unwrap();
        let mut ids: Vec<_> = entries.iter().map(|entry| entry.id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            if enabled {
                vec!["gateway", "management"]
            } else {
                vec!["management"]
            }
        );
        for entry in entries {
            if entry.id == "gateway" {
                assert_eq!(entry.resource_type.0, "ai");
                let data: AwsAiImportData = serde_json::from_value(entry.import_data).unwrap();
                assert_eq!(data.region, "us-east-1");
            } else {
                assert_eq!(entry.resource_type.0, "remote-stack-management");
                let data: AwsRemoteStackManagementImportData =
                    serde_json::from_value(entry.import_data).unwrap();
                assert_eq!(data.permissions_boundary_arn.as_deref(), Some(arn));
                assert_eq!(data.permissions_boundary_document, Some(installed.clone()));
            }
        }
    }
}

#[test]
fn size_budget_keeps_distinct_physical_keys_distinct() {
    let mut builder = Stack::new("many-keys".into()).add(
        RemoteStackManagement::new("management".into()).build(),
        ResourceLifecycle::Frozen,
    );
    let mut profile = PermissionProfile::new();
    for index in 0..24 {
        let id = format!("key-{index}");
        profile = profile.resource(&id, ["key/remote-cryptography"]);
        builder = builder.add(Key::new(id).build(), ResourceLifecycle::Frozen);
    }
    let error = try_render(
        &builder.permission("application", profile).build(),
        TerraformTarget::Aws,
        StackSettings::default(),
    )
    .unwrap_err();
    assert!(
        format!("{error:?}").contains("PERMISSIONS_BOUNDARY_TOO_LARGE"),
        "{error:?}"
    );
}

#[test]
fn registration_depends_on_the_boundary_for_teardown_ordering() {
    let module = alien_terraform::generate_terraform_module(
        &stack(),
        TerraformTarget::Aws,
        alien_terraform::TerraformOptions {
            display_name: None,
            registry: &alien_terraform::TfRegistry::built_in(),
            stack_settings: StackSettings::default(),
            registration: Some(alien_terraform::TerraformRegistration {
                provider_name: "alien".into(),
                provider_source: "alienplatform/alien".into(),
                provider_version: ">= 1.0.0".into(),
                resource_type: "deployment".into(),
                release_id: None,
                setup_target: "aws".into(),
                setup_fingerprint: "test".into(),
                setup_fingerprint_version: 1,
            }),
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .unwrap();
    let registration = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(_, contents)| hcl::from_str::<hcl::Body>(contents).unwrap().into_blocks())
        .find(|block| {
            block
                .labels()
                .first()
                .is_some_and(|label| label.as_str() == "alien_deployment")
        })
        .expect("registered deployment");
    let Expression::Array(dependencies) = attribute(&registration, "depends_on") else {
        panic!("static dependencies")
    };
    assert!(dependencies.iter().any(
        |dependency| dependency.to_string() == "aws_iam_policy.management_permissions_boundary"
    ));
}

async fn rendered_management_statements(automatic: bool, grants: &[&str]) -> Vec<AwsIamStatement> {
    let stack = Stack::new("management-boundary".into())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(grants.iter().copied()),
        ))
        .add(
            RemoteStackManagement::new("management".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let settings = StackSettings {
        setup_management: if automatic {
            SetupManagement::Automatic
        } else {
            SetupManagement::Manual
        },
        ..Default::default()
    };
    let config = DeploymentConfig::builder()
        .stack_settings(settings.clone())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let stack = ManagementPermissionProfileMutation
        .mutate(stack, &StackState::new(Platform::Aws), &config)
        .await
        .unwrap();
    let module = render(&stack, TerraformTarget::Aws, settings);
    assert_terraform_valid(&module, "management authorization");
    let mut context = Context::new();
    for (name, value) in [
        ("local", json!({"resource_prefix": "stack-a"})),
        (
            "var",
            json!({"managing_account_id": "210987654321", "managing_role_arn": "arn:aws:iam::210987654321:role/manager"}),
        ),
        (
            "data",
            json!({
                "aws_partition": {"current": {"partition": "aws"}},
                "aws_region": {"current": {"region": "us-west-2"}},
                "aws_caller_identity": {"current": {"account_id": "123456789012"}}
            }),
        ),
        (
            "aws_iam_policy",
            json!({"management_permissions_boundary": {
                "arn": "arn:aws:iam::123456789012:policy/stack-a-deployment-management-boundary"
            }}),
        ),
        (
            "aws_iam_role",
            json!({"management": {
                "arn": "arn:aws:iam::123456789012:role/stack-a-management"
            }}),
        ),
    ] {
        context.declare_var(name, hcl::to_value(value).unwrap());
    }
    let mut statements = Vec::new();
    for (file, contents) in module.iter().filter(|(file, _)| file.ends_with(".tf")) {
        let body: hcl::Body = hcl::parse(contents).unwrap();
        for block in body.blocks() {
            let labels = block.labels();
            if block.identifier() != "resource" || labels.len() != 2 {
                continue;
            }
            let kind = labels[0].as_str();
            let name = labels[1].as_str();
            if (kind == "aws_iam_policy" && name.starts_with("management_managed_policy"))
                || (kind == "aws_iam_role_policy" && name.starts_with("management_"))
            {
                let mut policy = document(attribute(block, "policy"), &context);
                // IAM accepts a scalar for a single action or resource;
                // the evaluator's statement type stores both as lists.
                for statement in policy["Statement"].as_array_mut().unwrap() {
                    for key in ["Action", "Resource"] {
                        if statement[key].is_string() {
                            statement[key] = json!([statement[key].take()]);
                        }
                    }
                }
                let policy: AwsIamPolicy = serde_json::from_value(policy)
                    .unwrap_or_else(|error| panic!("{file}/{name}: {error}"));
                statements.extend(policy.statement);
            }
        }
    }
    assert!(!statements.is_empty());
    statements
}

#[tokio::test]
async fn terraform_management_policies_require_numeric_asg_versions() {
    // Automatic setup includes every provision set, even for absent resource
    // types. Evaluate the complete emitted policy so no broader Allow bypasses
    // the numeric-version conditions.
    for automatic in [false, true] {
        let statements =
            rendered_management_statements(automatic, &["compute-cluster/management"]).await;
        let arn = "arn:aws:autoscaling:us-west-2:123456789012:autoScalingGroup:example:autoScalingGroupName/stack-a-compute";
        let tags = [
            (ALIEN_STACK_TAG_KEY, "stack-a"),
            (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ];
        for action in [
            "autoscaling:CreateAutoScalingGroup",
            "autoscaling:UpdateAutoScalingGroup",
        ] {
            for version in [Some("true"), Some("false"), None] {
                for launch_configuration in [false, true] {
                    let mut request = Request::new(action, arn)
                        .resource_tags(&tags)
                        .request_tags(&tags);
                    if let Some(version) = version {
                        request =
                            request.with("autoscaling:LaunchTemplateVersionSpecified", version);
                    }
                    if launch_configuration {
                        request = request.with("autoscaling:LaunchConfigurationName", "legacy");
                    }
                    let allowed = !launch_configuration
                        && (version == Some("true")
                            || (version.is_none()
                                && action == "autoscaling:UpdateAutoScalingGroup"));
                    assert_eq!(
                        evaluate(&statements, &request) == Decision::Allowed,
                        allowed,
                        "automatic={automatic}/{action}/{version:?}/{launch_configuration}"
                    );
                }
            }
        }
        for action in [
            "autoscaling:DeleteAutoScalingGroup",
            "autoscaling:SetDesiredCapacity",
            "autoscaling:TerminateInstanceInAutoScalingGroup",
            "autoscaling:StartInstanceRefresh",
        ] {
            assert_eq!(
                evaluate(&statements, &Request::new(action, arn).resource_tags(&tags))
                    == Decision::Allowed,
                action != "autoscaling:StartInstanceRefresh",
                "automatic={automatic}/{action}"
            );
        }
        // Numeric templates do not waive ownership checks, including on create.
        for action in [
            "autoscaling:CreateAutoScalingGroup",
            "autoscaling:UpdateAutoScalingGroup",
        ] {
            for tags in [
                vec![],
                vec![
                    (ALIEN_STACK_TAG_KEY, "stack-b"),
                    (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
                ],
                vec![
                    (ALIEN_STACK_TAG_KEY, "stack-a"),
                    (ALIEN_MANAGED_BY_TAG_KEY, "setup"),
                ],
            ] {
                let request = Request::new(action, arn)
                    .resource_tags(&tags)
                    .request_tags(&tags)
                    .with("autoscaling:LaunchTemplateVersionSpecified", "true");
                assert_eq!(
                    evaluate(&statements, &request),
                    Decision::NotAllowed,
                    "automatic={automatic}/{action}/{tags:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn terraform_management_passes_roles_only_to_their_intended_services() {
    for automatic in [false, true] {
        let statements = rendered_management_statements(
            automatic,
            &[
                "compute-cluster/management",
                "container/provision",
                "worker/provision",
                "build/provision",
            ],
        )
        .await;
        for (role, intended_service) in [
            ("stack-a-volume-backups-compute", "dlm.amazonaws.com"),
            ("stack-a-compute-node", "ec2.amazonaws.com"),
            ("stack-a-execution-sa", "lambda.amazonaws.com"),
            ("stack-a-build-sa", "codebuild.amazonaws.com"),
        ] {
            for prefix in ["stack-a", "stack-b"] {
                let role = role.replacen("stack-a", prefix, 1);
                for service in [
                    Some(intended_service),
                    Some("unrelated.amazonaws.com"),
                    None,
                ] {
                    // PassRole does not supply resource tags to IAM conditions.
                    let mut request = Request::new(
                        "iam:PassRole",
                        format!("arn:aws:iam::123456789012:role/{role}"),
                    );
                    if let Some(service) = service {
                        request = request.with("iam:PassedToService", service);
                    }
                    assert_eq!(
                        evaluate(&statements, &request) == Decision::Allowed,
                        prefix == "stack-a" && service == Some(intended_service),
                        "automatic={automatic}/{role}/{service:?}"
                    );
                }
            }
        }
        // Other provision grants must not let Lambda or CodeBuild use the
        // setup-owned backup role, or revive a legacy backup role.
        for role in ["stack-a-volume-backups-compute", "stack-a-dlm-compute"] {
            for service in ["lambda.amazonaws.com", "codebuild.amazonaws.com"] {
                let request = Request::new(
                    "iam:PassRole",
                    format!("arn:aws:iam::123456789012:role/{role}"),
                )
                .with("iam:PassedToService", service);
                assert_eq!(
                    evaluate(&statements, &request),
                    if role == "stack-a-dlm-compute" {
                        Decision::ExplicitlyDenied
                    } else {
                        Decision::NotAllowed
                    },
                    "automatic={automatic}/{role}/{service}"
                );
            }
        }
    }
}
