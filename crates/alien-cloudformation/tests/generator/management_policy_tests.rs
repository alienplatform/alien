//! The management role's policy when the permission sets it is built from overlap.
//!
//! The stack review before launch names each statement by its Sid, so a statement granted
//! twice shows up twice ("List event source mappings" and "List event source mappings2").

use super::helpers::{custom_resource_registration, render_built_ins_template};
use alien_cloudformation::{CfExpression, CloudFormationTarget};
use alien_core::{
    ManagementPermissions, PermissionProfile, RemoteStackManagement, ResourceLifecycle,
    ServiceAccount, Stack, StackSettings, Worker, WorkerCode,
};

fn statements(document: &CfExpression) -> Vec<&CfExpression> {
    let CfExpression::Object(document) = document else {
        panic!("policy document should be an object: {document:?}");
    };
    match document.get("Statement") {
        Some(CfExpression::List(statements)) => statements.iter().collect(),
        other => panic!("policy document should list statements: {other:?}"),
    }
}

fn grants(statement: &CfExpression, action: &str) -> bool {
    let CfExpression::Object(statement) = statement else {
        return false;
    };
    match statement.get("Action") {
        Some(CfExpression::String(value)) => value == action,
        Some(CfExpression::List(values)) => values
            .iter()
            .any(|value| matches!(value, CfExpression::String(value) if value == action)),
        _ => false,
    }
}

/// With heartbeats on, management gets `worker/heartbeat` next to `worker/management`, and
/// both grant `lambda:ListEventSourceMappings` on `*`.
#[test]
fn management_policy_grants_an_overlapping_statement_once() {
    let stack = Stack::new("overlap".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global(["worker/management", "worker/heartbeat"]),
        ))
        .add(
            ServiceAccount::new("execution-sa".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api:1.0.0".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();

    let (template, _) = render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "overlapping management permission sets",
    );

    let management_statements: Vec<&CfExpression> = template
        .resources
        .values()
        .filter(|resource| {
            resource.resource_type == "AWS::IAM::ManagedPolicy"
                && resource.logical_id.starts_with("ManagementRoleManagementPolicy")
        })
        .flat_map(|policy| statements(&policy.properties["PolicyDocument"]))
        .collect();
    assert!(!management_statements.is_empty(), "the management role should have a policy");

    let listing: Vec<_> = management_statements
        .iter()
        .filter(|statement| grants(statement, "lambda:ListEventSourceMappings"))
        .collect();
    assert_eq!(listing.len(), 1, "{listing:#?}");
}
