//! The workload role's grants must not depend on whether the stack is set up with
//! CloudFormation or Terraform.
//!
//! Both setups hand the stack over to the runtime, which then refuses to write a Frozen
//! resource's grants itself. A grant one generator emits and the other omits is therefore a
//! permission the application only has under one of them.

use super::helpers::{assert_terraform_valid, render};
use alien_cloudformation::{
    generate_cloudformation_template, CfExpression, CfRegistry, CfTemplate, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    Ai, Kv, PermissionProfile, Queue, ResourceLifecycle, ServiceAccount, Stack, StackSettings,
    Storage, Vault, Worker, WorkerCode,
};
use alien_terraform::{ModuleFiles, TerraformTarget};
use std::collections::BTreeSet;

/// One Frozen resource of each AWS type a workload profile can be granted on by id, a `"*"`
/// grant the stack-level policy covers, and a Live Worker running as the profile.
fn workload_stack() -> Stack {
    let profile = PermissionProfile::new()
        .global(["queue/data-read"])
        .resource("jobs", ["queue/data-write"])
        .resource("metadata", ["kv/data-write"])
        .resource("assets", ["storage/data-write"])
        .resource("secrets", ["vault/data-read"])
        .resource("llm", ["ai/invoke"]);
    let service_account =
        ServiceAccount::from_permission_profile("execution-sa".to_string(), &profile, |name| {
            alien_permissions::get_permission_set(name).cloned()
        })
        .expect("profile resolves");
    let worker = Worker::new("api".to_string())
        .code(WorkerCode::Image {
            image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api:1.0.0".to_string(),
        })
        .permissions("execution".to_string())
        .build();

    Stack::new("grants".to_string())
        .permission("execution", profile)
        .add(service_account, ResourceLifecycle::Frozen)
        .add(
            Queue::new("jobs".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("metadata".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Ai::new("llm".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(worker, ResourceLifecycle::Live)
        .build()
}

#[test]
fn terraform_grants_the_workload_role_what_cloudformation_grants() {
    let stack = workload_stack();
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &CfRegistry::built_in(),
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .expect("template renders");
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());

    let cloudformation = cloudformation_role_actions(&template, "execution-sa");
    let terraform = terraform_role_actions(&module, "execution_sa");

    // Each resource type appears once, so an action's service names the resource it is for.
    for action in [
        "sqs:SendMessage",
        "sqs:ReceiveMessage",
        "dynamodb:PutItem",
        "s3:PutObject",
        "ssm:GetParameter",
        "bedrock:InvokeModel",
    ] {
        assert!(
            cloudformation.contains(action),
            "the fixture must exercise {action}: {cloudformation:?}"
        );
    }
    assert_eq!(
        terraform,
        cloudformation,
        "\nonly in CloudFormation: {:?}\nonly in Terraform: {:?}",
        cloudformation.difference(&terraform).collect::<Vec<_>>(),
        terraform.difference(&cloudformation).collect::<Vec<_>>(),
    );
}

#[test]
fn a_frozen_queue_and_table_grant_the_workload_role_by_name() {
    let module = render(
        &workload_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );

    for (label, name, resource) in [
        (
            "execution_sa_jobs_queue_data_write",
            "deployment-jobs-queue-data-write",
            "arn:aws:sqs:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:${local.resource_prefix}-jobs",
        ),
        (
            "execution_sa_metadata_kv_data_write",
            "deployment-metadata-kv-data-write",
            "arn:aws:dynamodb:${data.aws_region.current.region}:${data.aws_caller_identity.current.account_id}:table/${local.resource_prefix}-metadata",
        ),
    ] {
        let policy = terraform_role_policies(&module)
            .find(|policy| policy.label == label)
            .unwrap_or_else(|| panic!("aws_iam_role_policy.{label} must render"));
        assert_eq!(policy.role, "execution_sa");
        assert_eq!(policy.name, name);
        assert_eq!(
            policy.resources,
            [resource],
            "{label} must be scoped to {resource} alone"
        );
    }
    // The stack-level policy already grants the "*" set on every prefixed queue.
    assert!(
        terraform_role_policies(&module)
            .all(|policy| policy.name != "deployment-jobs-queue-data-read"),
        "a set the stack-level policy grants is not repeated per queue"
    );
    assert_terraform_valid(&module, "aws workload grants");
}

/// Allowed actions of every policy attached to the CloudFormation role for `service_account_id`,
/// inline or as a separate `AWS::IAM::Policy`.
fn cloudformation_role_actions(
    template: &CfTemplate,
    service_account_id: &str,
) -> BTreeSet<String> {
    let role_name = format!("${{AWS::StackName}}-{service_account_id}");
    let (role_id, role) = template
        .resources
        .iter()
        .find(|(_, resource)| {
            resource.resource_type == "AWS::IAM::Role"
                && resource.properties.get("RoleName").is_some_and(|name| {
                    serde_json::to_string(name)
                        .expect("serializes")
                        .contains(&role_name)
                })
        })
        .unwrap_or_else(|| panic!("the template must declare the role {role_name}"));

    let mut documents = Vec::new();
    if let Some(CfExpression::List(policies)) = role.properties.get("Policies") {
        for policy in policies {
            let CfExpression::Object(policy) = policy else {
                panic!("an inline policy is an object: {policy:?}");
            };
            documents.push(policy["PolicyDocument"].clone());
        }
    }
    let reference = CfExpression::object([("Ref", CfExpression::from(role_id.as_str()))]);
    for resource in template.resources.values() {
        if !matches!(
            resource.resource_type.as_str(),
            "AWS::IAM::Policy" | "AWS::IAM::ManagedPolicy"
        ) {
            continue;
        }
        let Some(CfExpression::List(roles)) = resource.properties.get("Roles") else {
            continue;
        };
        if roles.contains(&reference) {
            documents.push(resource.properties["PolicyDocument"].clone());
        }
    }

    let mut actions = BTreeSet::new();
    for document in documents {
        let document = serde_json::to_value(&document).expect("serializes");
        collect_allowed_actions(&document, &mut actions);
    }
    actions
}

/// Allowed actions of every `aws_iam_role_policy` on `aws_iam_role.<role_label>`.
fn terraform_role_actions(module: &ModuleFiles, role_label: &str) -> BTreeSet<String> {
    let attached: Vec<_> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(_, contents)| {
            let body: hcl::Body = hcl::from_str(contents).expect("rendered HCL parses");
            body.blocks()
                .filter(|block| {
                    block.identifier() == "resource"
                        && block.labels().first().map(|label| label.as_str())
                            == Some("aws_iam_role_policy_attachment")
                        && block.body().attributes().any(|attribute| {
                            attribute.key() == "role"
                                && attribute
                                    .expr()
                                    .to_string()
                                    .contains(&format!("aws_iam_role.{role_label}."))
                        })
                })
                .map(|block| block.labels()[1].as_str().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        attached.is_empty(),
        "managed policy attachments are not compared yet: {attached:?}"
    );

    let mut actions = BTreeSet::new();
    for policy in terraform_role_policies(module).filter(|policy| policy.role == role_label) {
        collect_allowed_actions(&policy.document, &mut actions);
    }
    actions
}

struct TerraformRolePolicy {
    label: String,
    role: String,
    name: String,
    document: serde_json::Value,
    resources: Vec<String>,
}

fn terraform_role_policies(module: &ModuleFiles) -> impl Iterator<Item = TerraformRolePolicy> {
    let mut policies = Vec::new();
    for (path, contents) in module.iter() {
        if !path.ends_with(".tf") {
            continue;
        }
        let body: hcl::Body = hcl::from_str(contents).expect("rendered HCL parses");
        for block in body.blocks().filter(|block| {
            block.identifier() == "resource"
                && block.labels().first().map(|label| label.as_str()) == Some("aws_iam_role_policy")
        }) {
            let attribute = |key: &str| {
                block
                    .body()
                    .attributes()
                    .find(|attribute| attribute.key() == key)
                    .unwrap_or_else(|| panic!("aws_iam_role_policy must set {key}"))
                    .expr()
                    .clone()
            };
            let role = attribute("role").to_string();
            let role = role
                .strip_prefix("aws_iam_role.")
                .and_then(|rest| rest.strip_suffix(".id"))
                .unwrap_or_else(|| panic!("role must reference an aws_iam_role id: {role}"))
                .to_string();
            let hcl::Expression::String(name) = attribute("name") else {
                panic!("policy name must be a literal");
            };
            let document = match attribute("policy") {
                hcl::Expression::FuncCall(call) if call.name.name.as_str() == "jsonencode" => {
                    hcl_to_json(&call.args[0])
                }
                other => panic!("policy must be a jsonencode call: {other}"),
            };
            let mut resources = Vec::new();
            for statement in document["Statement"].as_array().expect("statements") {
                match &statement["Resource"] {
                    serde_json::Value::String(value) => resources.push(value.clone()),
                    serde_json::Value::Array(values) => resources.extend(
                        values
                            .iter()
                            .map(|value| value.as_str().expect("string").to_string()),
                    ),
                    _ => {}
                }
            }
            policies.push(TerraformRolePolicy {
                label: block.labels()[1].as_str().to_string(),
                role,
                name,
                document,
                resources,
            });
        }
    }
    policies.into_iter()
}

/// The literal JSON a `jsonencode` argument produces. Interpolations stay as their template
/// text; only actions are compared across generators.
fn hcl_to_json(expression: &hcl::Expression) -> serde_json::Value {
    match expression {
        hcl::Expression::String(text) => serde_json::Value::String(text.clone()),
        hcl::Expression::TemplateExpr(template) => serde_json::Value::String(template.to_string()),
        hcl::Expression::Bool(value) => serde_json::Value::Bool(*value),
        hcl::Expression::Array(items) => {
            serde_json::Value::Array(items.iter().map(hcl_to_json).collect())
        }
        hcl::Expression::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let key = match key {
                        hcl::ObjectKey::Identifier(identifier) => identifier.to_string(),
                        hcl::ObjectKey::Expression(hcl::Expression::String(text)) => text.clone(),
                        other => panic!("unexpected object key {other:?}"),
                    };
                    (key, hcl_to_json(value))
                })
                .collect(),
        ),
        other => serde_json::Value::String(other.to_string()),
    }
}

fn collect_allowed_actions(document: &serde_json::Value, actions: &mut BTreeSet<String>) {
    for statement in document["Statement"]
        .as_array()
        .unwrap_or_else(|| panic!("a policy document has statements: {document}"))
    {
        assert_eq!(
            statement["Effect"], "Allow",
            "workload policies only allow: {statement}"
        );
        match &statement["Action"] {
            serde_json::Value::String(action) => {
                actions.insert(action.clone());
            }
            serde_json::Value::Array(values) => {
                actions.extend(
                    values
                        .iter()
                        .map(|value| value.as_str().expect("action string").to_string()),
                );
            }
            other => panic!("statement without actions: {other}"),
        }
    }
}
