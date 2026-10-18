//! Setup-owned capability for retiring exact runtime gateway revisions.

use super::helpers::tags;
use crate::template::{CfExpression as E, CfResource};
use alien_core::{
    aws_worker_gateway::{
        CLEANUP_FUNCTION_PREFIX, CLEANUP_HANDLER_SOURCE, CLEANUP_RUNTIME, cleanup_revision,
    },
    import::EmitContext,
};

pub(super) fn version_id(management_role: &str) -> String {
    format!(
        "{management_role}GatewayCleanupVersion{}",
        cleanup_revision()
    )
}

pub(super) fn emit(ctx: &EmitContext<'_>, management_role: &str) -> Vec<CfResource> {
    let role_id = format!("{management_role}GatewayCleanupRole");
    let function_id = format!("{management_role}GatewayCleanupFunction");
    let name = E::sub(format!("{CLEANUP_FUNCTION_PREFIX}${{AWS::StackName}}"));
    let tasks = E::sub(
        "arn:${AWS::Partition}:ecs:${AWS::Region}:${AWS::AccountId}:task-definition/${AWS::StackName}-*-gateway-*:*",
    );
    let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".to_string());
    role.properties.extend([
        ("RoleName".to_string(), name.clone()),
        ("Path".to_string(), E::from("/setup/")),
        (
            "AssumeRolePolicyDocument".to_string(),
            policy(vec![E::object([
                ("Effect", E::from("Allow")),
                (
                    "Principal",
                    E::object([("Service", E::from("lambda.amazonaws.com"))]),
                ),
                ("Action", E::from("sts:AssumeRole")),
            ])]),
        ),
        (
            "Policies".to_string(),
            E::list([E::object([
                ("PolicyName", E::from("gateway-retirement")),
                (
                    "PolicyDocument",
                    policy(vec![
                        statement(
                            "ReadGatewayOwnership",
                            "ecs:ListTagsForResource",
                            tasks.clone(),
                        ),
                        // ECS deregistration has no resource-level authorization. Only
                        // this immutable handler can use it after native ownership checks.
                        statement(
                            "RetireValidatedGateway",
                            "ecs:DeregisterTaskDefinition",
                            E::from("*"),
                        ),
                        statement("DeleteGatewayRevision", "ecs:DeleteTaskDefinitions", tasks),
                    ]),
                ),
            ])]),
        ),
        ("Tags".to_string(), tags(ctx)),
    ]);
    let mut function = CfResource::new(function_id.clone(), "AWS::Lambda::Function".to_string());
    function.properties.extend([
        ("FunctionName".to_string(), name),
        (
            "Description".to_string(),
            E::from("Retire owned private gateway task revisions"),
        ),
        ("Role".to_string(), E::get_att(&role_id, "Arn")),
        ("Runtime".to_string(), E::from(CLEANUP_RUNTIME)),
        ("Handler".to_string(), E::from("index.handler")),
        ("Timeout".to_string(), E::from(60u32)),
        (
            "Code".to_string(),
            E::object([("ZipFile", E::from(CLEANUP_HANDLER_SOURCE))]),
        ),
        (
            "Environment".to_string(),
            E::object([(
                "Variables",
                E::object([("ALIEN_STACK_PREFIX", E::ref_("AWS::StackName"))]),
            )]),
        ),
        ("Tags".to_string(), tags(ctx)),
    ]);
    // Description changes replace the version after code/configuration updates.
    // Keep a stable template identity and register the resulting immutable ARN.
    let mut version = CfResource::new(
        version_id(management_role),
        "AWS::Lambda::Version".to_string(),
    );
    version
        .properties
        .insert("FunctionName".to_string(), E::ref_(&function_id));
    version
        .properties
        .insert("Description".to_string(), E::from(cleanup_revision()));
    version.depends_on.push(function_id);
    let mut invoke = CfResource::new(
        format!("{management_role}GatewayCleanupInvoke"),
        "AWS::IAM::Policy".to_string(),
    );
    invoke.properties.extend([
        (
            "PolicyName".to_string(),
            E::from("gateway-retirement-invoke"),
        ),
        ("Roles".to_string(), E::list([E::ref_(management_role)])),
        (
            "PolicyDocument".to_string(),
            policy(vec![statement(
                "InvokeGatewayRetirement",
                "lambda:InvokeFunction",
                E::ref_(version_id(management_role)),
            )]),
        ),
    ]);
    vec![role, function, version, invoke]
}

fn statement(sid: &str, action: &str, resource: E) -> E {
    E::object([
        ("Sid", E::from(sid)),
        ("Effect", E::from("Allow")),
        ("Action", E::list([E::from(action)])),
        ("Resource", resource),
    ])
}

fn policy(statements: Vec<E>) -> E {
    E::object([
        ("Version", E::from("2012-10-17")),
        ("Statement", E::list(statements)),
    ])
}
