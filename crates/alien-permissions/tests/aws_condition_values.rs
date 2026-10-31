use alien_core::{AwsBindingSpec, PermissionSet};
use alien_permissions::{
    generators::{
        aws_permissions_boundary, AwsCloudFormationPermissionsGenerator,
        AwsRuntimePermissionsGenerator,
    },
    BindingTarget, PermissionContext,
};
use serde_json::{json, Value};

fn permission_set(condition: Value) -> PermissionSet {
    serde_json::from_value(json!({
        "id": "custom/tagging",
        "description": "Tag runtime resources",
        "platforms": {"aws": [{
            "grant": {"actions": ["ec2:CreateTags"]},
            "binding": {"stack": {"resources": ["*"], "condition": condition}}
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

#[test]
fn runtime_and_cloudformation_preserve_scalar_and_list_conditions() {
    let conditions = json!({
        "StringEquals": {"aws:RequestTag/deployment": "${stackPrefix}"},
        "ForAllValues:StringEquals": {"aws:TagKeys": ["managed-by", "deployment", "resource"]},
        "StringLike": {"aws:RequestTag/resource": ["${stackPrefix}-one", "${stackPrefix}-two", "${aws:username}"]},
        "Null": {"aws:TagKeys": "false"}
    });
    let set = permission_set(conditions);
    let runtime = AwsRuntimePermissionsGenerator::new()
        .generate_policy(&set, BindingTarget::Stack, &context())
        .unwrap();
    let expected = json!({
        "StringEquals": {"aws:RequestTag/deployment": "app"},
        "ForAllValues:StringEquals": {"aws:TagKeys": ["managed-by", "deployment", "resource"]},
        "StringLike": {"aws:RequestTag/resource": ["app-one", "app-two", "${aws:username}"]},
        "Null": {"aws:TagKeys": "false"}
    });
    assert_eq!(json!(runtime.statement[0].condition), expected);
    let cloudformation = AwsCloudFormationPermissionsGenerator::new()
        .generate_policy(&set, BindingTarget::Stack, &context().with_stack_prefix(""))
        .unwrap();
    let mut cf_expected = expected.clone();
    cf_expected["StringEquals"]["aws:RequestTag/deployment"] =
        json!({"Fn::Sub": "${AWS::StackName}"});
    cf_expected["StringLike"]["aws:RequestTag/resource"] = json!([
        {"Fn::Sub": "${AWS::StackName}-one"},
        {"Fn::Sub": "${AWS::StackName}-two"},
        "${aws:username}"
    ]);
    assert_eq!(json!(cloudformation.statement[0].condition), cf_expected);

    let boundary = aws_permissions_boundary(&context(), &runtime.statement).unwrap();
    assert!(boundary["Statement"]
        .as_array()
        .unwrap()
        .iter()
        .any(|statement| {
            statement["Action"] == json!("ec2:CreateTags") && statement["Condition"] == expected
        }));
}

#[test]
fn legacy_generation_does_not_drop_a_custom_boundary_list_condition() {
    let set = permission_set(json!({
        "StringEquals": {"iam:PermissionsBoundary": ["literal", "${awsPermissionsBoundaryArn}"]}
    }));
    assert!(AwsRuntimePermissionsGenerator::new()
        .generate_policy(&set, BindingTarget::Stack, &context())
        .is_err());
    assert!(AwsCloudFormationPermissionsGenerator::new()
        .generate_policy(&set, BindingTarget::Stack, &context())
        .is_err());
}

#[test]
fn condition_values_reject_objects_and_non_string_list_members() {
    for invalid in [
        json!(true),
        json!(42),
        json!(null),
        json!({"key": "value"}),
        json!(["tag", 42]),
        json!([["tag"]]),
    ] {
        assert!(
            serde_json::from_value::<AwsBindingSpec>(json!({
                "resources": ["*"],
                "condition": {"StringEquals": {"aws:TagKeys": invalid}}
            }))
            .is_err(),
            "accepted invalid condition value {invalid}"
        );
    }
    for valid in [json!("tag"), json!(["tag"]), json!(["tag", "other"])] {
        let input =
            json!({"resources": ["*"], "condition": {"StringEquals": {"aws:TagKeys": valid}}});
        let binding: AwsBindingSpec = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(binding).unwrap(), input);
    }
}

#[test]
fn operation_compilation_keeps_list_conditions_when_scoping_resources() {
    let condition = json!({"StringEquals": {"s3:ExistingObjectTag/team": ["one", "two"]}});
    let request = json!({
        "task": "compileAws",
        "statements": [{
            "effect": "Allow", "actions": ["s3:GetObject"],
            "resources": ["arn:aws:s3:::*/*"], "condition": condition,
            "reasons": ["Read tagged objects"], "sources": []
        }],
        "ceilings": {"s3BucketArns": ["arn:aws:s3:::app-bucket"], "sqsQueueArns": []}
    });
    let result: Value = serde_json::from_str(&alien_permissions::operations::request_json(
        &request.to_string(),
    ))
    .unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["value"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["value"][0]["resources"],
        json!(["arn:aws:s3:::app-bucket/*"])
    );
    assert_eq!(result["value"][0]["condition"], condition);
}
