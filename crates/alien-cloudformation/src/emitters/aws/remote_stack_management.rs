//! AWS RemoteStackManagement — cross-account IAM role.
//!
//! The role's trust policy allows the manager's IAM identity (passed via
//! `ManagingRoleArn` parameter) to assume it. The inline policy is auto-derived
//! from the stack's management permission profile.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, chunk_managed_policy_statements, managing_role_trust_statement,
            required_logical_id, resource_config, tags, uniquify_iam_statement_sids,
            CONDITION_HAS_MANAGEMENT_EXTERNAL_ID, PARAM_MANAGEMENT_EXTERNAL_ID,
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
    generators::{aws_stack_permissions_boundary, AwsCloudFormationPermissionsGenerator},
    BindingTarget, PermissionContext,
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
            remote_management_trust_policy(ctx),
        );
        role.properties.insert("Tags".to_string(), tags(ctx));

        let boundary_id = boundary_logical_id(logical_id);
        let mut boundary = CfResource::new(boundary_id, "AWS::IAM::ManagedPolicy".to_string());
        boundary.properties.insert(
            "ManagedPolicyName".to_string(),
            CfExpression::sub("${AWS::StackName}-deployment-management-boundary"),
        );
        boundary
            .properties
            .insert("PolicyDocument".to_string(), boundary_document(ctx)?);
        let policy_documents = remote_management_policy_documents(ctx)?;
        let mut resources = vec![role, boundary];
        resources.extend(management_policy_resources(&role_id, policy_documents));

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        resource_config::<RemoteStackManagement>(ctx, RemoteStackManagement::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let role_id = role_logical_id(logical_id);
        let fields = vec![
            ("roleName", CfExpression::ref_(&role_id)),
            ("roleArn", CfExpression::get_att(&role_id, "Arn")),
            ("managementPermissionsApplied", CfExpression::from(true)),
            (
                "permissionsBoundaryArn",
                CfExpression::ref_(boundary_logical_id(logical_id)),
            ),
            ("permissionsBoundaryDocument", boundary_document(ctx)?),
        ];
        Ok(CfExpression::object(fields))
    }
}

fn boundary_logical_id(resource_logical_id: &str) -> String {
    format!("{resource_logical_id}PermissionsBoundary")
}

fn boundary_document(ctx: &EmitContext<'_>) -> Result<CfExpression> {
    // Validate the resolved document at the supported maximum input lengths too.
    // Short template tokens must not hide an oversized installed IAM policy.
    let generate = |context: &PermissionContext| {
        aws_stack_permissions_boundary(ctx.stack, context, |_, scope| {
            boundary_grant_context(ctx, context, scope)
        })
        .context(ErrorData::GenericError {
            message: "failed to generate AWS permissions boundary".to_string(),
        })
    };
    generate(
        &PermissionContext::new()
            .with_stack_prefix("s".repeat(40))
            .with_aws_region("r".repeat(16))
            .with_aws_account_id("123456789012")
            .with_managing_role_arn(format!(
                "arn:aws:iam::210987654321:role/{}",
                "r".repeat(2019)
            ))
            .with_managing_account_id("210987654321")
            .with_aws_permissions_boundary_arn(format!(
                "arn:aws:iam::123456789012:policy/{}-deployment-management-boundary",
                "s".repeat(40)
            )),
    )?;
    let document = generate(
        &PermissionContext::new()
            .with_stack_prefix("${AWS::StackName}")
            .with_aws_region("${AWS::Region}")
            .with_aws_account_id("${AWS::AccountId}")
            .with_managing_role_arn("${ManagingRoleArn}")
            .with_managing_account_id("${managingAccountId}")
            .with_aws_permissions_boundary_arn(
                "arn:aws:iam::${AWS::AccountId}:policy/${AWS::StackName}-deployment-management-boundary",
            ),
    )?;
    let rendered = AwsCloudFormationPermissionsGenerator::new()
        .render_document(&document)
        .context(ErrorData::GenericError {
            message: "failed to render AWS permissions boundary".to_string(),
        })?;
    let mut rendered = cf_from_json(rendered)?;
    for (id, entry) in ctx.stack.resources() {
        if entry.config.resource_type() != alien_core::Key::RESOURCE_TYPE {
            continue;
        }
        if let Some(logical_id) = ctx.name_for(id) {
            let key = format!("${{{logical_id}}}");
            if entry.enabled_when.is_some()
                && document["Statement"].as_array().is_some_and(|statements| {
                    statements.iter().any(|statement| {
                        statement["NotAction"] != "iam:*" && statement.to_string().contains(&key)
                    })
                })
            {
                return Err(AlienError::new(ErrorData::GenericError {
                    message: format!("AWS permissions boundary key '{id}' used by inline grants must be unconditional"),
                }));
            }
            replace_expression(
                &mut rendered,
                &CfExpression::sub(format!("arn:${{AWS::Partition}}:kms:${{AWS::Region}}:${{AWS::AccountId}}:key/${{{logical_id}}}")),
                &crate::emitters::enabled::when_enabled(
                    entry.enabled_when.as_deref(),
                    CfExpression::get_att(logical_id, "Arn"),
                    CfExpression::no_value(),
                ),
            );
        }
    }
    // Resolve conditional ARN elements before JSON registration serialization:
    // ToJsonString would otherwise preserve NoValue as a literal null.
    if ctx.stack.resources.values().any(|entry| {
        entry.config.resource_type() == alien_core::Key::RESOURCE_TYPE
            && entry.enabled_when.is_some()
    }) {
        if let CfExpression::Object(fields) = &mut rendered {
            if let Some(CfExpression::List(statements)) = fields.get_mut("Statement") {
                for statement in statements {
                    let CfExpression::Object(fields) = statement else {
                        continue;
                    };
                    if fields.get("NotAction") != Some(&CfExpression::from("iam:*")) {
                        continue;
                    }
                    if let Some(resources @ CfExpression::List(_)) = fields.get_mut("Resource") {
                        let list = std::mem::replace(resources, CfExpression::Null);
                        *resources = CfExpression::object([(
                            "Fn::Split",
                            CfExpression::list([
                                CfExpression::from(","),
                                CfExpression::join(",", list),
                            ]),
                        )]);
                    }
                }
            }
        }
    }
    Ok(rendered)
}

fn boundary_grant_context(
    ctx: &EmitContext<'_>,
    base: &PermissionContext,
    scope: &str,
) -> alien_permissions::Result<PermissionContext> {
    alien_permissions::generators::aws_boundary_scope_context(ctx.stack, base, scope, |scope| {
        if ctx.stack.resources[scope].config.resource_type() != alien_core::Key::RESOURCE_TYPE {
            return Ok(None);
        }
        let logical_id = ctx.name_for(scope).ok_or_else(|| {
            AlienError::new(alien_permissions::ErrorData::InvalidPermissionSet {
                message: format!("AWS permissions boundary key '{scope}' has no setup resource"),
            })
        })?;
        // Ref returns the generated key ID. Use that exact ID, never the stack prefix.
        let key_id = if base.stack_prefix.as_deref() == Some("${AWS::StackName}") {
            format!("${{{logical_id}}}")
        } else {
            // Keep keys distinct while budgeting their provider-assigned UUIDs.
            let index = ctx.stack.resources.get_index_of(scope).ok_or_else(|| {
                AlienError::new(alien_permissions::ErrorData::InvalidPermissionSet {
                    message: format!("AWS permissions boundary key '{scope}' is missing"),
                })
            })?;
            format!("00000000-0000-0000-0000-{index:012x}")
        };
        Ok(Some(format!(
            "arn:aws:kms:{}:{}:key/{key_id}",
            base.aws_region.as_deref().unwrap_or_default(),
            base.aws_account_id.as_deref().unwrap_or_default(),
        )))
    })
}

fn role_logical_id(resource_logical_id: &str) -> String {
    if resource_logical_id == "Management" {
        "ManagementRole".to_string()
    } else {
        format!("{resource_logical_id}Role")
    }
}

fn remote_management_trust_policy(ctx: &EmitContext<'_>) -> CfExpression {
    if let Some(statement) = super::kubernetes_cluster::eks_pod_trust_statement(ctx, "manager") {
        // The pod statement is JSON text built with `Fn::Sub`, so the whole
        // document is text and the ExternalId choice selects between two
        // documents. Only the managing-role statement takes the ExternalId; the
        // in-cluster manager authenticates with its service-account token.
        let document = |condition: &str| {
            CfExpression::sub_with(
                format!(
                    r#"{{"Version":"2012-10-17","Statement":[{{"Sid":"AllowManagingRole","Effect":"Allow","Principal":{{"AWS":"${{ManagingRole}}"}},"Action":"sts:AssumeRole","Condition":{{"StringEquals":{{{condition}}}}}}},${{PodTrust}}]}}"#
                ),
                [
                    ("ManagingRole", CfExpression::ref_(PARAM_MANAGING_ROLE_ARN)),
                    ("PodTrust", statement.clone()),
                ],
            )
        };
        return CfExpression::if_(
            CONDITION_HAS_MANAGEMENT_EXTERNAL_ID,
            document(&format!(
                r#""aws:PrincipalArn":"${{ManagingRole}}","sts:ExternalId":"${{{PARAM_MANAGEMENT_EXTERNAL_ID}}}""#
            )),
            document(r#""aws:PrincipalArn":"${ManagingRole}""#),
        );
    }
    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([managing_role_trust_statement()]),
        ),
    ])
}

fn remote_management_policy_documents(ctx: &EmitContext<'_>) -> Result<Vec<CfExpression>> {
    let mut statements = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let boundary_arn = "arn:${AWS::Partition}:iam::${AWS::AccountId}:policy/${AWS::StackName}-deployment-management-boundary";
    let context = permission_context().with_aws_permissions_boundary_arn(boundary_arn);

    if let Some(profile) = ctx.stack.management().profile() {
        for permission_set_ref in global_permission_refs(ctx, profile) {
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

    // Chunk against the complete expanded ARN before replacing it with a short Ref.
    let mut documents = chunk_managed_policy_statements(uniquify_iam_statement_sids(statements))?;
    let boundary_id = boundary_logical_id(required_logical_id(ctx)?);
    for document in &mut documents {
        replace_expression(
            document,
            &CfExpression::sub(boundary_arn),
            &CfExpression::ref_(&boundary_id),
        );
    }
    Ok(documents)
}

fn replace_expression(value: &mut CfExpression, from: &CfExpression, to: &CfExpression) {
    if value == from {
        *value = to.clone();
        return;
    }
    match value {
        CfExpression::List(values) => {
            for value in values {
                replace_expression(value, from, to);
            }
        }
        CfExpression::Object(values) => {
            for value in values.values_mut() {
                replace_expression(value, from, to);
            }
        }
        _ => {}
    }
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

fn global_permission_refs<'a>(
    ctx: &EmitContext<'_>,
    profile: &'a PermissionProfile,
) -> Vec<&'a PermissionSetReference> {
    alien_permissions::management_identity_global_refs(ctx.stack.resources.values(), profile)
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
            display_name: None,
            description: None,
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
}
