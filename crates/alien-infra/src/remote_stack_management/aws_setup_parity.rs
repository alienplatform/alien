//! Compare policies actually written by direct setup with both generated packages.

use std::collections::{BTreeSet, HashMap};

use alien_cloudformation::{
    generate_cloudformation_template, CfRegistry, CfTemplate, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::import::data::AwsRemoteStackManagementImportData;
use alien_permissions::generators::aws_permissions_boundary_covers;
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};
use hcl::{
    eval::{Context, Evaluate, FuncDef, ParamType},
    Expression,
};
use serde_json::{json, Value};

use crate::core::ResourceControllerContext;

pub(super) fn assert_setup_policy_parity(
    ctx: &ResourceControllerContext<'_>,
    installed: &Value,
    controller_statements: &[Value],
) {
    let aws = ctx.get_aws_config().unwrap();
    let prefix = ctx.resource_prefix;
    let account = &aws.account_id;
    let region = &aws.region;
    let manager = &ctx
        .get_aws_management_config()
        .unwrap()
        .unwrap()
        .managing_role_arn;
    let arn = format!("arn:aws:iam::{account}:policy/{prefix}-permissions-boundary");
    let role = format!("{prefix}-management");
    let role_arn = format!("arn:aws:iam::{account}:role/{role}");
    let template = generate_cloudformation_template(
        ctx.desired_stack,
        CloudFormationOptions {
            registry: &CfRegistry::built_in(),
            target: CloudFormationTarget::Aws,
            stack_settings: ctx.deployment_config.stack_settings.clone(),
            setup_target: "aws".into(),
            setup_fingerprint: "test".into(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::CustomResource {
                lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".into(),
                callback_url: None,
            },
            description: None,
            management_trust: Default::default(),
        },
    )
    .unwrap();
    let parameters: HashMap<String, Value> = [
        ("AWS::StackName", json!(prefix)),
        ("AWS::Region", json!(region)),
        ("AWS::AccountId", json!(account)),
        ("AWS::Partition", json!("aws")),
        ("ManagingRoleArn", json!(manager)),
        ("ManagementRole", json!(role)),
        ("ManagementRole.Arn", json!(role_arn)),
        ("ManagementPermissionsBoundary", json!(arn)),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    let resolve = |value: &Value| evaluate(value, &template, &parameters);
    let boundary = resolve(
        &serde_json::to_value(
            &template.resources["ManagementPermissionsBoundary"].properties["PolicyDocument"],
        )
        .unwrap(),
    );
    assert_equivalent(installed, &boundary);
    let cfn_statements: Vec<Value> = template
        .resources
        .values()
        .filter(|r| r.properties.contains_key("Roles"))
        .flat_map(|r| {
            resolve(&serde_json::to_value(&r.properties["PolicyDocument"]).unwrap())["Statement"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect();
    assert_eq!(
        security_grants(controller_statements, &arn),
        security_grants(&cfn_statements, &arn),
        "CloudFormation role-write grants and denies"
    );
    // Resolve only the management entry; other resources have unrelated provider outputs.
    let payload =
        serde_json::to_value(&template.resources["DeploymentRegistration"].properties["Resources"])
            .unwrap();
    let entry = payload
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "management")
        .unwrap();
    let import: AwsRemoteStackManagementImportData =
        serde_json::from_value(resolve(&entry["importData"])).unwrap();
    assert_eq!(
        import.permissions_boundary_arn.as_deref(),
        Some(arn.as_str())
    );
    assert_eq!(
        import.permissions_boundary_document.as_ref(),
        Some(&boundary)
    );

    let module = generate_terraform_module(
        ctx.desired_stack,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: None,
            registry: &TfRegistry::built_in(),
            stack_settings: ctx.deployment_config.stack_settings.clone(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .unwrap();
    let bodies: Vec<hcl::Body> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(_, text)| hcl::from_str(text).unwrap())
        .collect();
    let blocks: Vec<_> = bodies.iter().flat_map(|b| b.blocks()).collect();
    let boundary_block = blocks
        .iter()
        .find(|b| {
            b.labels()
                .iter()
                .any(|l| l.as_str() == "management_permissions_boundary")
        })
        .unwrap();
    let mut context = Context::new();
    for (name, value) in [
        ("local", json!({"resource_prefix": prefix})),
        (
            "data",
            json!({"aws_region": {"current": {"region": region}}, "aws_caller_identity": {"current": {"account_id": account}}}),
        ),
        (
            "var",
            json!({"managing_account_id": manager.split(':').nth(4).unwrap(), "managing_role_arn": manager}),
        ),
        (
            "aws_iam_policy",
            json!({"management_permissions_boundary": {"arn": arn}}),
        ),
        (
            "aws_iam_role",
            json!({"management": {"name": role, "arn": role_arn}}),
        ),
    ] {
        context.declare_var(name, hcl::to_value(value).unwrap());
    }
    let terraform_boundary = document(attribute(boundary_block, "policy"), &context);
    assert_equivalent(installed, &terraform_boundary);
    assert_equivalent(&boundary, &terraform_boundary);
    let tf_statements: Vec<Value> = blocks
        .iter()
        .filter(|b| {
            b.labels()
                .first()
                .is_some_and(|l| ["aws_iam_policy", "aws_iam_role_policy"].contains(&l.as_str()))
                && !std::ptr::eq(**b, *boundary_block)
                && b.labels()
                    .get(1)
                    .is_some_and(|l| l.as_str().starts_with("management"))
        })
        .flat_map(|b| {
            document(attribute(b, "policy"), &context)["Statement"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect();
    assert_eq!(
        security_grants(controller_statements, &arn),
        security_grants(&tf_statements, &arn),
        "Terraform role-write grants and denies"
    );
    context.declare_var("aws_iam_policy", hcl::to_value(json!({"management_permissions_boundary": {"arn": arn, "policy": terraform_boundary.to_string()}})).unwrap());
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
        .unwrap();
    let import: AwsRemoteStackManagementImportData = serde_json::from_value(
        serde_json::to_value(import_expr.evaluate(&context).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        import.permissions_boundary_arn.as_deref(),
        Some(arn.as_str())
    );
    assert_eq!(
        import.permissions_boundary_document.as_ref(),
        Some(&terraform_boundary)
    );
}

fn assert_equivalent(left: &Value, right: &Value) {
    assert!(aws_permissions_boundary_covers(left, right).unwrap());
    assert!(aws_permissions_boundary_covers(right, left).unwrap());
}

// Compare individual grants, ignoring statement IDs, grouping and chunk boundaries.
fn security_grants(statements: &[Value], boundary: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut denies = 0;
    let mut writes = 0;
    for statement in statements {
        let deny = statement["Effect"] == "Deny";
        denies += usize::from(deny && is_role_guard(statement));
        for action in values(&statement["Action"]) {
            let write = matches!(
                action.as_str(),
                Some(
                    "iam:CreateRole"
                        | "iam:PutRolePolicy"
                        | "iam:AttachRolePolicy"
                        | "iam:UpdateAssumeRolePolicy"
                        | "iam:PutRolePermissionsBoundary"
                        | "iam:DeleteRolePolicy"
                        | "iam:DetachRolePolicy"
                )
            );
            if !deny && write {
                writes += 1;
                assert_eq!(
                    statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"],
                    boundary
                );
            }
            if !deny {
                assert_ne!(action, "iam:DeleteRolePermissionsBoundary");
            }
            if deny || write {
                for resource in values(&statement["Resource"]) {
                    let mut grant = json!([
                        statement["Effect"],
                        action,
                        resource,
                        statement["Condition"]
                    ]);
                    grant.sort_all_objects();
                    result.insert(grant.to_string());
                }
            }
        }
    }
    assert_eq!(
        denies, 2,
        "self-protection and setup-owned sandbox role guards"
    );
    assert!(writes > 0);
    result
}

pub(super) fn is_role_guard(statement: &Value) -> bool {
    statement["Effect"] == "Deny"
        && values(&statement["Action"])
            .iter()
            .any(|action| action == "iam:PutRolePolicy")
}

fn values(value: &Value) -> &[Value] {
    value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(value))
}

fn attribute<'a>(block: &'a hcl::Block, name: &str) -> &'a Expression {
    block
        .body
        .attributes()
        .find(|a| a.key() == name)
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
        _ => None,
    }
}

// Fail on unsupported intrinsics so new generator behavior cannot silently bypass
// the assertions. Conditions come from the generated template, not test answers.
fn evaluate(value: &Value, template: &CfTemplate, parameters: &HashMap<String, Value>) -> Value {
    let eval = |v| evaluate(v, template, parameters);
    match value {
        Value::Object(fields) => {
            if let Some(name) = fields.get("Ref") {
                if name == "AWS::NoValue" {
                    return Value::Null;
                }
                return parameters[name.as_str().unwrap()].clone();
            }
            if let Some(args) = fields.get("Fn::GetAtt") {
                return parameters[&format!(
                    "{}.{}",
                    args[0].as_str().unwrap(),
                    args[1].as_str().unwrap()
                )]
                    .clone();
            }
            if let Some(args) = fields.get("Fn::Split") {
                return json!(eval(&args[1])
                    .as_str()
                    .unwrap()
                    .split(args[0].as_str().unwrap())
                    .collect::<Vec<_>>());
            }
            if let Some(args) = fields.get("Fn::Sub") {
                let (text, locals) = match args {
                    Value::String(text) => (text.as_str(), None),
                    Value::Array(args) => (args[0].as_str().unwrap(), args[1].as_object()),
                    _ => panic!("invalid substitution: {args}"),
                };
                let mut result = String::new();
                let mut rest = text;
                while let Some((before, after)) = rest.split_once("${") {
                    result.push_str(before);
                    let (name, tail) = after.split_once('}').unwrap();
                    if let Some(literal) = name.strip_prefix('!') {
                        result.push_str(&format!("${{{literal}}}"));
                    } else {
                        let replacement = locals
                            .and_then(|locals| locals.get(name))
                            .map(eval)
                            .unwrap_or_else(|| parameters[name].clone());
                        result.push_str(replacement.as_str().unwrap());
                    }
                    rest = tail;
                }
                result.push_str(rest);
                return json!(result);
            }
            if let Some(args) = fields.get("Fn::If") {
                let condition =
                    serde_json::to_value(&template.conditions[args[0].as_str().unwrap()]).unwrap();
                return eval(
                    &args[if eval(&condition).as_bool().unwrap() {
                        1
                    } else {
                        2
                    }],
                );
            }
            if let Some(value) = fields.get("Fn::ToJsonString") {
                return json!(serde_json::to_string(&eval(value)).unwrap());
            }
            if let Some(args) = fields.get("Fn::Equals") {
                return json!(eval(&args[0]) == eval(&args[1]));
            }
            if let Some(args) = fields.get("Fn::Not") {
                return json!(!eval(&args[0]).as_bool().unwrap());
            }
            if let Some(args) = fields.get("Fn::Join") {
                let values = eval(&args[1]);
                return json!(values
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(args[0].as_str().unwrap()));
            }
            if let Some(args) = fields.get("Fn::Select") {
                return eval(&args[1])[args[0].as_u64().unwrap() as usize].clone();
            }
            assert!(
                !fields.keys().any(|key| key.starts_with("Fn::")),
                "unsupported expression: {value}"
            );
            Value::Object(fields.iter().map(|(k, v)| (k.clone(), eval(v))).collect())
        }
        Value::Array(values) => Value::Array(values.iter().map(eval).collect()),
        other => other.clone(),
    }
}
