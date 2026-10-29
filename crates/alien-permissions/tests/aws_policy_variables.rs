use alien_core::{PermissionProfile, PermissionSet, PermissionSetReference, Stack};
use alien_permissions::{
    generators::{
        aws_permissions_boundary_covers, aws_stack_permissions_boundary,
        AwsCloudFormationPermissionsGenerator, AwsRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use serde_json::{json, Value};

fn permission_set(resource: &str, condition: &str) -> PermissionSet {
    serde_json::from_value(json!({
        "id": "custom/read",
        "description": "Read objects selected by request context",
        "platforms": {"aws": [{
            "grant": {"actions": ["s3:GetObject"]},
            "binding": {"stack": {
                "resources": [resource],
                "condition": {"StringLike": {"s3:prefix": condition}}
            }}
        }]}
    }))
    .unwrap()
}

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("app")
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1")
        .with_managing_account_id("210987654321")
}

// Evaluate the emitted intrinsics once, as CloudFormation does. Substituted values
// and escaped literals are not recursively interpreted as template variables.
fn resolve(value: &Value) -> Value {
    if let Some(reference) = value.get("Ref") {
        return match reference.as_str().unwrap() {
            "ManagingRoleArn" => json!("arn:aws:iam::210987654321:role/manager"),
            other => panic!("unexpected Ref {other}"),
        };
    }
    if let Some(split) = value.get("Fn::Split") {
        let source = resolve(&split[1]);
        return json!(source
            .as_str()
            .unwrap()
            .split(split[0].as_str().unwrap())
            .collect::<Vec<_>>());
    }
    if let Some(select) = value.get("Fn::Select") {
        return resolve(&select[1])[select[0].as_u64().unwrap() as usize].clone();
    }
    if let Some(sub) = value.get("Fn::Sub") {
        let (template, variables) = if let Some(template) = sub.as_str() {
            (template, None)
        } else {
            (sub[0].as_str().unwrap(), Some(sub[1].as_object().unwrap()))
        };
        let mut output = String::new();
        let mut remaining = template;
        while let Some((literal, rest)) = remaining.split_once("${") {
            output.push_str(literal);
            let (variable, suffix) = rest.split_once('}').unwrap();
            if let Some(escaped) = variable.strip_prefix('!') {
                output.push_str(&format!("${{{escaped}}}"));
            } else if let Some(value) = variables.and_then(|values| values.get(variable)) {
                output.push_str(resolve(value).as_str().unwrap());
            } else {
                output.push_str(match variable {
                    "AWS::Partition" => "aws",
                    "ManagingRoleArn" => "arn:aws:iam::210987654321:role/manager",
                    "AWS::StackName" => "app",
                    "AWS::AccountId" => "123456789012",
                    "AWS::Region" => "us-east-1",
                    "BoundaryPolicy" => "arn:aws:iam::123456789012:policy/app-permissions-boundary",
                    other => panic!("unresolved CloudFormation variable {other}"),
                });
            }
            remaining = suffix;
        }
        output.push_str(remaining);
        return json!(output);
    }
    match value {
        Value::Array(values) => values.iter().map(resolve).collect(),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| (key.clone(), resolve(value)))
            .collect(),
        _ => value.clone(),
    }
}

#[test]
fn request_variables_survive_runtime_cloudformation_and_boundary_compilation() {
    for variable in [
        "${aws:username}",
        "${s3:prefix}",
        "${aws:PrincipalTag/team name, 'unassigned'}",
        "${cognito-identity.amazonaws.com:sub}",
        "${*}/${?}/${$}",
    ] {
        for resource in [
            format!("arn:aws:s3:::${{stackPrefix}}-data/{variable}"),
            format!("arn:aws:s3:::fixed/{variable}"),
            format!("arn:aws:s3:::${{managingAccountId}}-data/{variable}"),
        ] {
            for condition in [variable.to_string(), format!("${{stackPrefix}}/{variable}")] {
                let set = permission_set(&resource, &condition);
                let runtime = AwsRuntimePermissionsGenerator::new()
                    .generate_policy(&set, BindingTarget::Stack, &context())
                    .unwrap();
                let cf = AwsCloudFormationPermissionsGenerator::new()
                    .generate_policy(&set, BindingTarget::Stack, &context().with_stack_prefix(""))
                    .unwrap();
                let expected_resource = resource
                    .replace("${stackPrefix}", "app")
                    .replace("${managingAccountId}", "210987654321");
                let expected_condition = condition.replace("${stackPrefix}", "app");
                assert_eq!(runtime.statement[0].resource, [expected_resource]);
                assert_eq!(
                    runtime.statement[0].condition.as_ref().unwrap()["StringLike"]["s3:prefix"],
                    expected_condition
                );
                let document = serde_json::to_value(&runtime).unwrap();
                assert_eq!(resolve(&serde_json::to_value(cf).unwrap()), document);

                let profile =
                    PermissionProfile::new().global([PermissionSetReference::Inline(set)]);
                let stack = Stack::new("app".to_string())
                    .permission("reader", profile)
                    .build();
                let boundary =
                    aws_stack_permissions_boundary(&stack, &context(), |_, _| Ok(context()))
                        .unwrap();
                assert!(aws_permissions_boundary_covers(&boundary, &document).unwrap());
                assert!(boundary["Statement"].as_array().unwrap().iter().any(
                    |statement| statement.get("Resource")
                        == document["Statement"][0].get("Resource")
                        && statement.get("Condition") == document["Statement"][0].get("Condition")
                ));
            }
        }
    }
}

#[test]
fn cloudformation_preserves_explicit_escapes_and_boundary_references() {
    let set = permission_set(
        "arn:aws:s3:::${stackPrefix}/${!aws:username}",
        "${awsPermissionsBoundaryArn}/${aws:username}",
    );
    let cf = AwsCloudFormationPermissionsGenerator::new()
        .generate_policy(
            &set,
            BindingTarget::Stack,
            &context()
                .with_stack_prefix("")
                .with_aws_permissions_boundary_arn("${BoundaryPolicy}"),
        )
        .unwrap();
    let document = resolve(&serde_json::to_value(cf).unwrap());
    assert_eq!(
        document["Statement"][0]["Resource"],
        json!(["arn:aws:s3:::app/${aws:username}"])
    );
    assert_eq!(
        document["Statement"][0]["Condition"]["StringLike"]["s3:prefix"],
        "arn:aws:iam::123456789012:policy/app-permissions-boundary/${aws:username}"
    );
}

#[test]
fn iam_variable_support_does_not_accept_missing_setup_inputs() {
    for variable in [
        "${unknown}",
        "${AWS::AccountId}",
        "${local.resource_prefix}",
    ] {
        for set in [
            permission_set(variable, "literal"),
            permission_set("*", variable),
        ] {
            let error = AwsRuntimePermissionsGenerator::new()
                .generate_policy(&set, BindingTarget::Stack, &context())
                .unwrap_err();
            assert_eq!(error.code, "VARIABLE_NOT_FOUND");
        }
    }
    for condition in [
        json!({"${aws:username}": {"s3:prefix": "literal"}}),
        json!({"StringEquals": {"${aws:username}": "literal"}}),
    ] {
        let mut value = serde_json::to_value(permission_set("*", "literal")).unwrap();
        value["platforms"]["aws"][0]["binding"]["stack"]["condition"] = condition;
        let set = serde_json::from_value(value).unwrap();
        let error = AwsRuntimePermissionsGenerator::new()
            .generate_policy(&set, BindingTarget::Stack, &context())
            .unwrap_err();
        assert_eq!(error.code, "VARIABLE_NOT_FOUND");
    }
    let error = AwsRuntimePermissionsGenerator::new()
        .generate_policy(
            &permission_set("arn:aws:s3:::bucket/${aws:username", "literal"),
            BindingTarget::Stack,
            &context(),
        )
        .unwrap_err();
    assert_eq!(error.code, "INVALID_PERMISSION_SET");
}

#[test]
fn compiled_boundary_rendering_matches_runtime_with_custom_request_variables() {
    let set = permission_set(
        "arn:aws:s3:::${managingAccountId}-data/${aws:PrincipalTag/team, 'unassigned'}",
        "${stackPrefix}/${aws:username}",
    );
    let stack = Stack::new("sample".to_string())
        .permission(
            "reader",
            PermissionProfile::new().global([PermissionSetReference::Inline(set)]),
        )
        .build();
    let runtime_context = context();
    let symbolic_context = PermissionContext::new()
        .with_stack_prefix("${AWS::StackName}")
        .with_aws_account_id("${AWS::AccountId}")
        .with_aws_region("${AWS::Region}")
        .with_managing_account_id("${managingAccountId}");
    let runtime =
        aws_stack_permissions_boundary(
            &stack,
            &runtime_context,
            |_, _| Ok(runtime_context.clone()),
        )
        .unwrap();
    let symbolic = aws_stack_permissions_boundary(&stack, &symbolic_context, |_, _| {
        Ok(symbolic_context.clone())
    })
    .unwrap();
    let rendered = AwsCloudFormationPermissionsGenerator::new()
        .render_document(&symbolic)
        .unwrap();
    assert_eq!(resolve(&rendered), runtime);
    assert!(rendered["Statement"]
        .as_array()
        .unwrap()
        .iter()
        .any(|statement| statement["NotAction"] == "iam:*"));
}

#[test]
fn compiled_document_rendering_preserves_selectors_and_condition_value_types() {
    let symbolic = json!({
        "Version": "2012-10-17",
        "Statement": [
            {"Effect": "Allow", "NotAction": "iam:*", "Resource": [
                "arn:aws:s3:::${AWS::StackName}-*",
                "arn:aws:logs:${AWS::Region}:${AWS::AccountId}:log-group:${AWS::StackName}-*"
            ]},
            {"Effect": "Deny", "Action": ["s3:GetObject"], "NotResource": [
                "arn:aws:s3:::${AWS::StackName}-data/${aws:username}"
            ], "Condition": {
                "ArnEquals": {"aws:PrincipalArn": "${ManagingRoleArn}"},
                "StringLike": {"s3:prefix": ["${AWS::StackName}/${aws:username}", "${aws:PrincipalTag/team, 'unassigned'}", "${*}/${?}/${$}"]},
                "Bool": {"aws:SecureTransport": false},
                "NumericLessThan": {"s3:TlsVersion": 1.2}
            }}
        ]
    });
    let rendered = AwsCloudFormationPermissionsGenerator::new()
        .render_document(&symbolic)
        .unwrap();
    let expected = json!({
        "Version": "2012-10-17",
        "Statement": [
            {"Effect": "Allow", "NotAction": "iam:*", "Resource": [
                "arn:aws:s3:::app-*",
                "arn:aws:logs:us-east-1:123456789012:log-group:app-*"
            ]},
            {"Effect": "Deny", "Action": ["s3:GetObject"], "NotResource": [
                "arn:aws:s3:::app-data/${aws:username}"
            ], "Condition": {
                "ArnEquals": {"aws:PrincipalArn": "arn:aws:iam::210987654321:role/manager"},
                "StringLike": {"s3:prefix": ["app/${aws:username}", "${aws:PrincipalTag/team, 'unassigned'}", "${*}/${?}/${$}"]},
                "Bool": {"aws:SecureTransport": false},
                "NumericLessThan": {"s3:TlsVersion": 1.2}
            }}
        ]
    });
    assert_eq!(resolve(&rendered), expected);
    assert!(aws_permissions_boundary_covers(&expected, &expected).unwrap());
}
