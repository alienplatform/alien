//! The deployment boundary direct AWS setup installs, evaluated against the roles it bounds.
//!
//! The boundary denies every role under it access to the sandbox gateway's configuration
//! parameter. The gateway reads that parameter itself, so its role must stay outside the
//! boundary, as the CloudFormation and Terraform setups create it. Every other role's own
//! grants must stay inside the boundary. Both are checked on a stack prepared by the real
//! preflight mutations, with the boundary that stack generates.

#[path = "../../alien-permissions/tests/iam_policy/mod.rs"]
mod iam_policy;

use alien_core::{
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID, AwsManagementConfig, Container, ContainerCode,
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig,
    PermissionProfile, Platform, ResourceLifecycle, ResourceSpec, Sandbox, SandboxCode,
    SandboxEgress, SandboxLifecyclePolicy, Stack, StackSettings, StackState, SECRETS_VAULT_ID,
};
use alien_infra::aws_permissions_boundary_for_stack;
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use alien_preflights::runner::PreflightRunner;
use iam_policy::{evaluate, wildcard_match, Decision, Request};
use serde_json::{json, Value};

const PREFIX: &str = "gwtest";
const ACCOUNT: &str = "123456789012";
const REGION: &str = "us-east-1";
const MANAGER: &str = "arn:aws:iam::210987654321:role/manager";

fn context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix(PREFIX)
        .with_aws_account_id(ACCOUNT)
        .with_aws_region(REGION)
        .with_managing_account_id("210987654321")
        .with_managing_role_arn(MANAGER)
}

fn deployment_config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: MANAGER.to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build()
}

/// An internet sandbox (which brings the managed gateway) next to a Live container with its
/// own profile: the stack that left the gateway unable to read its configuration.
async fn prepared_stack() -> Stack {
    let stack = Stack::new("gateway-boundary".to_string())
        .permission(
            "app",
            PermissionProfile::new().resource(SECRETS_VAULT_ID, ["vault/data-read"]),
        )
        .add(
            Container::new("api".to_string())
                .code(ContainerCode::Image {
                    image: "registry.example.com/api:1".to_string(),
                })
                .cpu(ResourceSpec {
                    min: "0.25".into(),
                    desired: "0.25".into(),
                })
                .memory(ResourceSpec {
                    min: "512Mi".into(),
                    desired: "512Mi".into(),
                })
                .replicas(1)
                .permissions("app".to_string())
                .port(8080)
                .build(),
            ResourceLifecycle::Live,
        )
        .add(
            Sandbox::new("agent".to_string())
                .code(SandboxCode::Image {
                    image: "ubuntu".to_string(),
                    bundled_from: None,
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy::default())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    PreflightRunner::new()
        .apply_mutations(stack, &state(), &deployment_config())
        .await
        .expect("the stack prepares")
}

fn state() -> StackState {
    let mut state = StackState::new(Platform::Aws);
    state.resource_prefix = PREFIX.to_string();
    state
}

/// A rendered boundary: the statements the shared evaluator reads, and the `NotAction` Allow
/// statements it does not model (a boundary renders no `NotAction` Deny).
struct Boundary {
    statements: Vec<AwsIamStatement>,
    /// Each statement's excluded actions, and the statement with `Action: *` in their place.
    not_action_allows: Vec<(Vec<String>, AwsIamStatement)>,
}

fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => vec![value.clone()],
        Value::Array(values) => values
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect(),
        other => panic!("not a string list: {other}"),
    }
}

fn boundary(document: &Value) -> Boundary {
    let mut boundary = Boundary {
        statements: Vec::new(),
        not_action_allows: Vec::new(),
    };
    for statement in document["Statement"].as_array().unwrap() {
        let mut statement = statement.clone();
        let excluded = statement
            .as_object_mut()
            .unwrap()
            .remove("NotAction")
            .map(|excluded| strings(&excluded));
        if excluded.is_some() {
            assert_eq!(statement["Effect"], "Allow", "{statement}");
            statement["Action"] = json!(["*"]);
        }
        if statement.get("Sid").is_none() {
            statement["Sid"] = json!("");
        }
        for key in ["Action", "Resource", "NotResource"] {
            if let Some(value) = statement.get(key).filter(|value| value.is_string()) {
                statement[key] = json!([value]);
            }
        }
        let statement: AwsIamStatement = serde_json::from_value(statement).unwrap();
        match excluded {
            Some(excluded) => boundary.not_action_allows.push((excluded, statement)),
            None => boundary.statements.push(statement),
        }
    }
    boundary
}

impl Boundary {
    fn evaluate(&self, request: &Request) -> Decision {
        match evaluate(&self.statements, request) {
            Decision::NotAllowed
                if self.not_action_allows.iter().any(|(excluded, statement)| {
                    !excluded.iter().any(|pattern| {
                        wildcard_match(&pattern.to_lowercase(), &request.action.to_lowercase())
                    }) && evaluate(std::slice::from_ref(statement), request) == Decision::Allowed
                }) =>
            {
                Decision::Allowed
            }
            decision => decision,
        }
    }
}

/// A profile's own grants as the service account and resource controllers write them: stack
/// scope on the role's stack policy, a resource scope with that resource's name.
fn own_grants(stack: &Stack, profile: &PermissionProfile) -> Vec<AwsIamStatement> {
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut statements = Vec::new();
    for (scope, references) in &profile.0 {
        let (target, context) = if scope == "*" {
            (BindingTarget::Stack, context())
        } else {
            assert!(
                stack.resources.contains_key(scope),
                "profile scope '{scope}' names no resource"
            );
            (
                BindingTarget::Resource,
                context()
                    .with_resource_id(scope.clone())
                    .with_resource_name(format!("{PREFIX}-{scope}")),
            )
        };
        for reference in references {
            let set = reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .unwrap();
            if !set
                .platforms
                .aws
                .as_ref()
                .is_some_and(|aws| !aws.is_empty())
            {
                continue;
            }
            statements.extend(
                generator
                    .generate_policy(&set, target.clone(), &context)
                    .unwrap()
                    .statement,
            );
        }
    }
    statements
}

/// Each action and resource an Allow statement grants.
fn grants(statements: &[AwsIamStatement]) -> Vec<(&'static str, String)> {
    statements
        .iter()
        .filter(|statement| statement.effect == "Allow")
        .flat_map(|statement| {
            statement.action.iter().flat_map(move |action| {
                let action: &'static str = Box::leak(action.clone().into_boxed_str());
                statement
                    .resource
                    .iter()
                    .map(move |resource| (action, resource.clone()))
            })
        })
        .collect()
}

fn config_parameter() -> String {
    let parameter = alien_core::vault_naming::parameter_store_parameter_name(
        &format!("{PREFIX}-{SECRETS_VAULT_ID}"),
        "sandbox-egress-gateway-config",
    );
    format!("arn:aws:ssm:{REGION}:{ACCOUNT}:parameter/{parameter}")
}

#[tokio::test]
async fn the_gateway_role_must_stay_unbounded_and_every_bounded_role_keeps_its_grants() {
    let stack = prepared_stack().await;
    let boundary = boundary(
        &aws_permissions_boundary_for_stack(&stack, &state(), &context())
            .expect("the boundary renders"),
    );
    let profiles = &stack.permissions.profiles;
    assert!(
        profiles.contains_key("app") && profiles.contains_key(SANDBOX_EGRESS_GATEWAY_ID),
        "{:?}",
        profiles.keys().collect::<Vec<_>>()
    );

    // The gateway's own grants read its configuration, and the boundary denies that read.
    let gateway = own_grants(&stack, &profiles[SANDBOX_EGRESS_GATEWAY_ID]);
    let read = Request::new("ssm:GetParameter", config_parameter());
    assert_eq!(evaluate(&gateway, &read), Decision::Allowed, "{gateway:#?}");
    assert_eq!(
        boundary.evaluate(&read),
        Decision::ExplicitlyDenied,
        "the boundary must keep other roles away from the gateway configuration"
    );

    // Every other profile's role sits under the boundary: each of its own grants must be
    // allowed there, and none of them reads the gateway configuration.
    for (name, profile) in profiles {
        if name == SANDBOX_EGRESS_GATEWAY_ID {
            continue;
        }
        let own = own_grants(&stack, profile);
        assert_ne!(
            evaluate(&own, &read),
            Decision::Allowed,
            "profile '{name}' reads the gateway configuration"
        );
        let grants = grants(&own);
        assert!(!grants.is_empty(), "profile '{name}' renders no grants");
        for (action, resource) in grants {
            assert_eq!(
                boundary.evaluate(&Request::new(action, resource.clone())),
                Decision::Allowed,
                "profile '{name}': the boundary denies its own grant {action} on {resource}"
            );
        }
    }
}
