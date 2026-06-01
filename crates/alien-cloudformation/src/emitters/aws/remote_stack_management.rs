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
    import::EmitContext, DeploymentModel, ErrorData, KubernetesCluster, PermissionProfile,
    PermissionSetReference, RemoteStackManagement, Result,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

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
        if let Some(irsa) = eks_pull_irsa_context(ctx) {
            role.properties.insert(
                "AssumeRolePolicyDocument".to_string(),
                irsa_trust_policy(
                    &irsa.oidc_provider_id,
                    &irsa.cluster_id,
                    &irsa.namespace,
                    &irsa.service_account_name,
                ),
            );
            role.depends_on.push(irsa.oidc_provider_id);
        } else {
            role.properties.insert(
                "AssumeRolePolicyDocument".to_string(),
                remote_management_trust_policy(),
            );
        }
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

struct EksPullIrsaContext {
    oidc_provider_id: String,
    cluster_id: String,
    namespace: String,
    service_account_name: String,
}

fn eks_pull_irsa_context(ctx: &EmitContext<'_>) -> Option<EksPullIrsaContext> {
    if ctx.stack_settings.deployment_model != DeploymentModel::Pull {
        return None;
    }

    ctx.stack.resources().find_map(|(resource_id, entry)| {
        let cluster = entry.config.downcast_ref::<KubernetesCluster>()?;
        let prefix = ctx.name_for(resource_id)?;
        Some(EksPullIrsaContext {
            oidc_provider_id: format!("{prefix}OidcProvider"),
            cluster_id: format!("{prefix}Cluster"),
            namespace: cluster.namespace.clone(),
            service_account_name: "${AWS::StackName}-manager-sa".to_string(),
        })
    })
}

fn irsa_trust_policy(
    oidc_provider_id: &str,
    cluster_id: &str,
    namespace: &str,
    service_account_name: &str,
) -> CfExpression {
    CfExpression::object([(
        "Fn::Sub",
        CfExpression::list([
            CfExpression::from(format!(
                r#"{{
  "Version": "2012-10-17",
  "Statement": [{{
    "Effect": "Allow",
    "Principal": {{"Federated": "${{OidcProviderArn}}"}},
    "Action": "sts:AssumeRoleWithWebIdentity",
    "Condition": {{
      "StringEquals": {{
        "${{OidcIssuerHostPath}}:aud": "sts.amazonaws.com",
        "${{OidcIssuerHostPath}}:sub": "system:serviceaccount:{namespace}:{service_account_name}"
      }}
    }}
  }}]
}}"#
            )),
            CfExpression::object([
                ("OidcProviderArn", CfExpression::ref_(oidc_provider_id)),
                ("OidcIssuerHostPath", oidc_issuer_host_path(cluster_id)),
            ]),
        ]),
    )])
}

fn oidc_issuer_host_path(cluster_id: &str) -> CfExpression {
    CfExpression::object([(
        "Fn::Select",
        CfExpression::list([
            CfExpression::from(1u8),
            CfExpression::object([(
                "Fn::Split",
                CfExpression::list([
                    CfExpression::from("https://"),
                    CfExpression::get_att(cluster_id, "OpenIdConnectIssuerUrl"),
                ]),
            )]),
        ]),
    )])
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
            let Some(resource_entry) = ctx.stack.resources.get(resource_id) else {
                continue;
            };
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if permission_set.platforms.aws.is_none()
                || !resource_scoped_aws_permission_applies(&permission_set.id, resource_entry)
            {
                continue;
            }

            let resource_context = context.clone().with_resource_name(resource_id.to_string());
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

    chunk_policy_statements(uniquify_iam_statement_sids(statements))
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

fn resource_scoped_aws_permission_applies(
    permission_set_id: &str,
    _resource_entry: &alien_core::ResourceEntry,
) -> bool {
    permission_set_id == "kubernetes-public-endpoint/management"
}
