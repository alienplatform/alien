//! AWS RemoteStackManagement — cross-account IAM role.
//!
//! The role's trust policy allows the manager's IAM identity (passed via
//! `ManagingRoleArn` parameter) to assume it. The inline policy is auto-derived
//! from the stack's management permission profile.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, tags, uniquify_iam_statement_sids,
            PARAM_MANAGING_ROLE_ARN,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ErrorData, KubernetesCluster, PermissionProfile, PermissionSetReference,
    RemoteStackManagement, ResourceLifecycle, Result, Worker,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{
    generators::AwsCloudFormationPermissionsGenerator, BindingTarget, PermissionContext,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsRemoteStackManagementEmitter;

impl CfEmitter for AwsRemoteStackManagementEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = role_logical_id(logical_id);

        let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".to_string());
        role.properties.insert(
            "RoleName".to_string(),
            CfExpression::sub("${AWS::StackName}-management"),
        );
        role.properties.insert(
            "AssumeRolePolicyDocument".to_string(),
            remote_management_trust_policy(),
        );
        role.properties.insert("Tags".to_string(), tags(ctx));

        let policy_documents = remote_management_policy_documents(ctx)?;
        let mut resources = vec![role];
        resources.extend(management_policy_resources(&role_id, policy_documents));

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        resource_config::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = role_logical_id(logical_id);
        Ok(CfExpression::object([
            ("roleName", CfExpression::ref_(&role_id)),
            ("roleArn", CfExpression::get_att(&role_id, "Arn")),
            ("managementPermissionsApplied", CfExpression::from(true)),
        ]))
    }
}

fn role_logical_id(resource_logical_id: &str) -> String {
    if resource_logical_id == "Management" {
        "ManagementRole".to_string()
    } else {
        format!("{resource_logical_id}Role")
    }
}

fn remote_management_trust_policy() -> CfExpression {
    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([CfExpression::object([
                ("Sid", CfExpression::from("AllowManagingRole")),
                ("Effect", CfExpression::from("Allow")),
                (
                    "Principal",
                    CfExpression::object([("AWS", CfExpression::ref_(PARAM_MANAGING_ROLE_ARN))]),
                ),
                ("Action", CfExpression::from("sts:AssumeRole")),
                (
                    "Condition",
                    CfExpression::object([(
                        "StringEquals",
                        CfExpression::object([(
                            "aws:PrincipalArn",
                            CfExpression::ref_(PARAM_MANAGING_ROLE_ARN),
                        )]),
                    )]),
                ),
            ])]),
        ),
    ])
}

fn remote_management_policy_documents(ctx: &EmitContext<'_>) -> Result<Vec<CfExpression>> {
    let mut statements = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = permission_context();

    if let Some(profile) = ctx.stack.management().profile() {
        for permission_set_ref in global_permission_refs(profile) {
            if let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            {
                if permission_set.platforms.aws.is_none() {
                    continue;
                }
                let policy = generator
                    .generate_policy(&permission_set, BindingTarget::Stack, &context)
                    .context(ErrorData::GenericError {
                        message: "failed to generate AWS management IAM policy".to_string(),
                    })?;
                let policy_value = serde_json::to_value(policy).into_alien_error().context(
                    ErrorData::TemplateSerializationFailed {
                        format: "CloudFormation IAM policy".to_string(),
                        reason: "Failed to serialize IAM policy".to_string(),
                    },
                )?;
                let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                    return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                        format: "CloudFormation IAM policy".to_string(),
                        reason: "policy did not serialize to a JSON object".to_string(),
                    }));
                };
                if let Some(CfExpression::List(policy_statements)) =
                    policy_object.shift_remove("Statement")
                {
                    statements.extend(policy_statements);
                }
            }
        }

        for (resource_id, permission_set_ref) in resource_scoped_permission_refs(profile) {
            if permission_set_ref.id().ends_with("/provision") {
                continue;
            }
            let Some(resource_entry) = ctx.stack.resources.get(resource_id) else {
                continue;
            };
            if resource_entry.lifecycle != ResourceLifecycle::Live {
                continue;
            }
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if permission_set.platforms.aws.is_none() {
                continue;
            }
            let resource_context =
                resource_scoped_aws_permission_context(resource_id, resource_entry, &context);
            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &resource_context)
                .context(ErrorData::GenericError {
                    message: "failed to generate AWS resource-scoped management IAM policy"
                        .to_string(),
                })?;
            let policy_value = serde_json::to_value(policy).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "Failed to serialize IAM policy".to_string(),
                },
            )?;
            let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "policy did not serialize to a JSON object".to_string(),
                }));
            };
            if let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            {
                statements.extend(policy_statements);
            }
        }
    }

    statements.push(CfExpression::object([
        ("Sid", CfExpression::from("ReadOwnManagementRole")),
        ("Effect", CfExpression::from("Allow")),
        ("Action", CfExpression::from("iam:GetRole")),
        (
            "Resource",
            CfExpression::sub(
                "arn:${AWS::Partition}:iam::${AWS::AccountId}:role/${AWS::StackName}-management",
            ),
        ),
    ]));

    statements.extend(deployment_management_self_service_statements());

    chunk_policy_statements(uniquify_iam_statement_sids(statements))
}

/// IAM statements letting the `<stack>-management` role manage its own
/// deployment-management policies after setup.
///
/// The runtime remote-stack-management controller (see
/// `alien-infra/src/remote_stack_management/aws.rs`) creates, versions, and
/// attaches managed policies named `<stack>-deployment-management-<idx>` onto the
/// `<stack>-management` role — this is how a Frozen, remote-access resource
/// (e.g. a BYOB Storage bucket, whose concrete ARN is only known once the
/// customer stack provisions it) grants the management identity access at
/// runtime. Setup only compiles static grants for `Live` resources, so without
/// these the runtime create/attach calls fail with `403`.
///
/// Scope is confined to this stack's own `-deployment-management-*` policy
/// namespace, and attach/detach is further restricted by an `iam:PolicyARN`
/// condition so this role can never attach an arbitrary managed policy.
fn deployment_management_self_service_statements() -> Vec<CfExpression> {
    let policy_arn_pattern =
        "arn:${AWS::Partition}:iam::${AWS::AccountId}:policy/${AWS::StackName}-deployment-management-*";
    let role_arn = "arn:${AWS::Partition}:iam::${AWS::AccountId}:role/${AWS::StackName}-management";

    vec![
        CfExpression::object([
            (
                "Sid",
                CfExpression::from("ManageDeploymentManagementPolicies"),
            ),
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("iam:CreatePolicy"),
                    CfExpression::from("iam:CreatePolicyVersion"),
                    CfExpression::from("iam:DeletePolicyVersion"),
                    CfExpression::from("iam:ListPolicyVersions"),
                    CfExpression::from("iam:GetPolicy"),
                    CfExpression::from("iam:GetPolicyVersion"),
                    CfExpression::from("iam:DeletePolicy"),
                ]),
            ),
            ("Resource", CfExpression::sub(policy_arn_pattern)),
        ]),
        CfExpression::object([
            (
                "Sid",
                CfExpression::from("AttachDeploymentManagementPolicies"),
            ),
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("iam:AttachRolePolicy"),
                    CfExpression::from("iam:DetachRolePolicy"),
                ]),
            ),
            ("Resource", CfExpression::sub(role_arn)),
            (
                "Condition",
                CfExpression::object([(
                    "ArnLike",
                    CfExpression::object([(
                        "iam:PolicyARN",
                        CfExpression::sub(policy_arn_pattern),
                    )]),
                )]),
            ),
        ]),
        // `iam:ListAttachedRolePolicies` and the legacy inline
        // `iam:DeleteRolePolicy` cleanup carry no `iam:PolicyARN`, so they get
        // their own statement scoped to the management role (placing them under
        // the ArnLike condition above would deny them).
        CfExpression::object([
            (
                "Sid",
                CfExpression::from("ManageOwnManagementRoleInlinePolicies"),
            ),
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("iam:ListAttachedRolePolicies"),
                    CfExpression::from("iam:DeleteRolePolicy"),
                ]),
            ),
            ("Resource", CfExpression::sub(role_arn)),
        ]),
    ]
}

fn policy_document(statements: Vec<CfExpression>) -> CfExpression {
    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ])
}

fn management_policy_resources(
    role_id: &str,
    policy_documents: Vec<CfExpression>,
) -> Vec<CfResource> {
    policy_documents
        .into_iter()
        .enumerate()
        .map(|(index, policy_document)| {
            let mut policy = CfResource::new(
                format!("{role_id}ManagementPolicy{}", index + 1),
                "AWS::IAM::ManagedPolicy".to_string(),
            );
            policy.properties.insert(
                "Description".to_string(),
                CfExpression::from("Application management permissions"),
            );
            policy
                .properties
                .insert("PolicyDocument".to_string(), policy_document);
            policy.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(role_id)]),
            );
            policy.depends_on.push(role_id.to_string());
            policy
        })
        .collect()
}

fn chunk_policy_statements(statements: Vec<CfExpression>) -> Result<Vec<CfExpression>> {
    const MAX_MANAGED_POLICY_BYTES: usize = 5_500;

    let mut chunks = Vec::new();
    let mut current = Vec::new();

    for statement in statements {
        let mut candidate = current.clone();
        candidate.push(statement.clone());
        if policy_document_size(&candidate)? <= MAX_MANAGED_POLICY_BYTES {
            current = candidate;
            continue;
        }

        if current.is_empty() {
            return Err(AlienError::new(ErrorData::GenericError {
                message: "AWS management IAM statement is too large for a managed policy"
                    .to_string(),
            }));
        }

        chunks.push(policy_document(current));
        current = vec![statement];
    }

    if !current.is_empty() {
        chunks.push(policy_document(current));
    }

    Ok(chunks)
}

fn policy_document_size(statements: &[CfExpression]) -> Result<usize> {
    serde_json::to_string(&policy_document(statements.to_vec()))
        .into_alien_error()
        .context(ErrorData::TemplateSerializationFailed {
            format: "CloudFormation IAM policy".to_string(),
            reason: "Failed to serialize IAM policy for size validation".to_string(),
        })
        .map(|policy| policy.len())
}

fn global_permission_refs(profile: &PermissionProfile) -> Vec<&PermissionSetReference> {
    profile
        .0
        .get("*")
        .map(|refs| refs.iter().collect())
        .unwrap_or_default()
}

fn resource_scoped_permission_refs(
    profile: &PermissionProfile,
) -> Vec<(&str, &PermissionSetReference)> {
    profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(resource_id, refs)| {
            refs.iter()
                .map(move |permission_set_ref| (resource_id.as_str(), permission_set_ref))
        })
        .collect()
}

fn resource_scoped_aws_permission_context(
    resource_id: &str,
    resource_entry: &alien_core::ResourceEntry,
    base_context: &PermissionContext,
) -> PermissionContext {
    let mut context = base_context
        .clone()
        .with_resource_id(resource_id.to_string());
    context.resource_name = None;

    if resource_entry.config.downcast_ref::<Worker>().is_some() {
        return context.with_resource_name(format!("${{AWS::StackName}}-{resource_id}"));
    }

    if resource_entry
        .config
        .downcast_ref::<KubernetesCluster>()
        .is_some()
    {
        return context.with_resource_name(resource_id.to_string());
    }

    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Resource, ResourceEntry, ResourceLifecycle, Worker, WorkerCode};

    fn live_worker_entry(id: &str) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Worker::new(id.to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("default".to_string())
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Live,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        }
    }

    #[test]
    fn aws_remote_management_resource_scope_names_future_worker_lambda() {
        let entry = live_worker_entry("jobs");
        let context = resource_scoped_aws_permission_context("jobs", &entry, &permission_context());

        assert_eq!(context.resource_id.as_deref(), Some("jobs"));
        assert_eq!(
            context.resource_name.as_deref(),
            Some("${AWS::StackName}-jobs")
        );
    }

    #[test]
    fn aws_remote_management_resource_scope_does_not_filter_by_permission_id() {
        let entry = live_worker_entry("jobs");
        let context = resource_scoped_aws_permission_context("jobs", &entry, &permission_context());

        assert_eq!(
            context.resource_name.as_deref(),
            Some("${AWS::StackName}-jobs")
        );
    }

    // Regression: the management role must be able to create/version/attach its
    // own `<stack>-deployment-management-*` managed policies at runtime (the path
    // a Frozen, remote-access BYOB Storage resource needs), scoped to this
    // stack's namespace only.
    #[test]
    fn management_role_can_self_service_deployment_management_policies() {
        let statements = deployment_management_self_service_statements();
        let json = serde_json::to_value(&statements).expect("serialize statements");
        let arr = json.as_array().expect("statements array");

        let by_sid = |sid: &str| -> &serde_json::Value {
            arr.iter()
                .find(|s| s.get("Sid").and_then(|v| v.as_str()) == Some(sid))
                .unwrap_or_else(|| panic!("missing statement {sid}"))
        };

        let manage = by_sid("ManageDeploymentManagementPolicies");
        let manage_str = serde_json::to_string(manage).unwrap();
        for action in [
            "iam:CreatePolicy",
            "iam:CreatePolicyVersion",
            "iam:DeletePolicyVersion",
            "iam:ListPolicyVersions",
            "iam:DeletePolicy",
        ] {
            assert!(manage_str.contains(action), "policy stmt missing {action}");
        }
        assert!(
            manage_str.contains("policy/${AWS::StackName}-deployment-management-*"),
            "policy stmt not scoped to the deployment-management namespace"
        );
        assert!(
            manage.get("Condition").is_none(),
            "policy create/version must not be gated by a PolicyARN condition"
        );

        let attach = by_sid("AttachDeploymentManagementPolicies");
        let attach_str = serde_json::to_string(attach).unwrap();
        assert!(attach_str.contains("iam:AttachRolePolicy"));
        assert!(attach_str.contains("iam:DetachRolePolicy"));
        assert!(attach_str.contains("role/${AWS::StackName}-management"));
        assert!(
            attach_str.contains("iam:PolicyARN")
                && attach_str.contains("policy/${AWS::StackName}-deployment-management-*"),
            "attach must be restricted to the deployment-management namespace"
        );

        let inline = by_sid("ManageOwnManagementRoleInlinePolicies");
        let inline_str = serde_json::to_string(inline).unwrap();
        assert!(inline_str.contains("iam:ListAttachedRolePolicies"));
        assert!(inline_str.contains("iam:DeleteRolePolicy"));
        assert!(
            inline.get("Condition").is_none(),
            "list/delete-inline must not carry a PolicyARN condition"
        );
    }
}
