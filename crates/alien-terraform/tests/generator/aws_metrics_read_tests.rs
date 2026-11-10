//! A new AWS install with a worker and telemetry gets `observe/metrics-read`. The module must
//! allow `cloudwatch:GetMetricData` in the management role's own policies and nothing may cap that
//! role below it. The permissions boundary the module creates caps the roles management writes
//! for the app, not the management role itself.

use super::helpers::{prepare_for_render, render, resource_attributes, role_policy_documents};
use alien_core::{
    PermissionProfile, PermissionsConfig, ResourceLifecycle, Stack, StackSettings, TelemetryMode,
    Worker, WorkerCode,
};
use alien_terraform::TerraformTarget;

const ACTION: &str = "cloudwatch:GetMetricData";

fn release() -> Stack {
    Stack::new("metrics".to_string())
        .permissions(PermissionsConfig::new().with_profile(
            "execution",
            PermissionProfile::new().global(["storage/data-read"]),
        ))
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api:1.0.0".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build()
}

fn field<'a>(object: &'a hcl::Expression, key: &str) -> Option<&'a hcl::Expression> {
    let hcl::Expression::Object(object) = object else {
        return None;
    };
    object.iter().find_map(|(name, value)| {
        let name = match name {
            hcl::ObjectKey::Identifier(identifier) => identifier.to_string(),
            hcl::ObjectKey::Expression(hcl::Expression::String(text)) => text.clone(),
            _ => return None,
        };
        (name == key).then_some(value)
    })
}

fn is_text(expression: &hcl::Expression, expected: &str) -> bool {
    match expression {
        hcl::Expression::String(text) => text == expected,
        hcl::Expression::Array(items) => items.iter().any(|item| is_text(item, expected)),
        _ => false,
    }
}

/// Whether some statement of a management policy allows `ACTION` on every resource.
fn management_allows_metric_data(module: &alien_terraform::ModuleFiles) -> bool {
    role_policy_documents(module, "management")
        .iter()
        .any(|(_, document)| {
            let Some(hcl::Expression::Array(statements)) = field(document, "Statement") else {
                return false;
            };
            statements.iter().any(|statement| {
                field(statement, "Effect").is_some_and(|effect| is_text(effect, "Allow"))
                    && field(statement, "Action").is_some_and(|action| is_text(action, ACTION))
                    && field(statement, "Resource").is_some_and(|resource| is_text(resource, "*"))
            })
        })
}

#[tokio::test]
async fn setup_allows_metrics_read_in_the_management_policy_with_no_cap_on_the_role() {
    let settings = StackSettings {
        telemetry: TelemetryMode::Auto,
        ..Default::default()
    };
    let stack = prepare_for_render(release()).await;
    let module = render(&stack, TerraformTarget::Aws, settings);

    assert!(
        management_allows_metric_data(&module),
        "the management role's policy must allow {ACTION}"
    );
    let roles = resource_attributes(&module, "aws_iam_role");
    let (_, management) = roles
        .iter()
        .find(|(label, _)| label == "management")
        .expect("the module creates the management role");
    assert!(
        !management.contains_key("permissions_boundary"),
        "the management role must not be capped by a permissions boundary: {management:?}"
    );
}

#[tokio::test]
async fn setup_without_a_worker_leaves_metrics_read_out_of_the_policy() {
    let without_worker = Stack::new("metrics".to_string())
        .permissions(PermissionsConfig::new().with_profile(
            "execution",
            PermissionProfile::new().global(["storage/data-read"]),
        ))
        .build();
    let stack = prepare_for_render(without_worker).await;
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());

    assert!(!management_allows_metric_data(&module));
}
