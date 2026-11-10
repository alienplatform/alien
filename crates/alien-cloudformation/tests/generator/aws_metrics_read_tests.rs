//! A new AWS install with a worker and telemetry gets `observe/metrics-read`. The setup template
//! must allow `cloudwatch:GetMetricData` in the management role's own policy, and nothing may cap
//! that role below it, or the grant sits in the profile and is denied at runtime. The permissions
//! boundary the template creates caps the roles management writes for the app, not the management
//! role itself.

use super::helpers::{custom_resource_registration, render_built_ins_template};
use alien_cloudformation::{CfTemplate, CloudFormationTarget};
use alien_core::{
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, PermissionProfile, PermissionsConfig, Platform, ResourceLifecycle, Stack,
    StackSettings, StackState, TelemetryMode, Worker, WorkerCode,
};
use alien_preflights::runner::PreflightRunner;
use serde_json::{json, Value};

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

/// The stack as setup prepares it: template preflights, then mutations.
async fn prepared(stack: Stack, settings: &StackSettings) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Aws)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(settings.clone())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: String::new(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    runner
        .apply_mutations(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("template mutations apply")
}

fn render(stack: &Stack, settings: StackSettings) -> CfTemplate {
    render_built_ins_template(
        stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "management role reads Lambda metrics",
    )
    .0
}

fn action_list(statement: &Value) -> Vec<&str> {
    match &statement["Action"] {
        Value::String(action) => vec![action.as_str()],
        Value::Array(actions) => actions.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    }
}

fn allows_metric_data_on_everything(document: &Value) -> bool {
    document["Statement"].as_array().is_some_and(|statements| {
        statements.iter().any(|statement| {
            statement["Effect"] == "Allow"
                && action_list(statement).contains(&ACTION)
                && (statement["Resource"] == "*" || statement["Resource"] == json!(["*"]))
        })
    })
}

fn management_policy_documents(template: &CfTemplate) -> Vec<Value> {
    template
        .resources
        .values()
        .filter(|resource| {
            resource.resource_type == "AWS::IAM::ManagedPolicy"
                && resource
                    .logical_id
                    .starts_with("ManagementRoleManagementPolicy")
        })
        .map(|policy| serde_json::to_value(&policy.properties["PolicyDocument"]).unwrap())
        .collect()
}

fn management_role_boundary(template: &CfTemplate) -> Option<&alien_cloudformation::CfExpression> {
    template.resources["ManagementRole"]
        .properties
        .get("PermissionsBoundary")
}

#[tokio::test]
async fn setup_allows_metrics_read_in_the_management_policy_with_no_cap_on_the_role() {
    let settings = StackSettings {
        telemetry: TelemetryMode::Auto,
        ..Default::default()
    };
    let stack = prepared(release(), &settings).await;
    let template = render(&stack, settings);

    let policies = management_policy_documents(&template);
    assert!(
        policies.iter().any(allows_metric_data_on_everything),
        "the management role's policy must allow {ACTION}: {policies:#?}"
    );
    assert!(
        management_role_boundary(&template).is_none(),
        "the management role must not be capped by a permissions boundary"
    );
}

#[tokio::test]
async fn setup_without_telemetry_leaves_metrics_read_out_of_the_policy() {
    let settings = StackSettings {
        telemetry: TelemetryMode::Off,
        ..Default::default()
    };
    let stack = prepared(release(), &settings).await;
    let template = render(&stack, settings);

    assert!(!management_policy_documents(&template)
        .iter()
        .any(allows_metric_data_on_everything));
}
