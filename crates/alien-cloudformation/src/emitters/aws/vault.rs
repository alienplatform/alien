//! AWS Vault — Parameter Store namespace.
//!
//! AWS Systems Manager Parameter Store is account-and-region-scoped, not
//! a CloudFormation resource. The vault is realized as a name prefix
//! (`${AWS::StackName}-{vault.id}`) that the controller uses for
//! `ssm:PutParameter`. ImportData carries the prefix so importers can
//! reconstruct the vault namespace without a cloud lookup.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, service_account_role_id,
            uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionProfile, PermissionSetReference,
    RemoteStackManagement, Result, ServiceAccount, Vault,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsVaultEmitter;

impl CfEmitter for AwsVaultEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let vault = resource_config::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        let mut resources = vault_iam_policies(ctx, vault)?;
        let Some(management_logical_id) = remote_stack_management_logical_id(ctx) else {
            return Ok(resources);
        };
        let Some(policy_document) = management_vault_policy_document(ctx, vault)? else {
            return Ok(resources);
        };

        let logical_id = required_logical_id(ctx)?;
        let mut policy = CfResource::new(
            format!("{logical_id}ManagementVaultPolicy"),
            "AWS::IAM::Policy".to_string(),
        );
        policy.properties.insert(
            "PolicyName".to_string(),
            CfExpression::sub(format!(
                "alien-mgmt-{}-vault-policy",
                sanitize_policy_name_segment(vault.id())
            )),
        );
        policy
            .properties
            .insert("PolicyDocument".to_string(), policy_document);
        policy.properties.insert(
            "Roles".to_string(),
            CfExpression::list([CfExpression::ref_(management_role_logical_id(
                management_logical_id,
            ))]),
        );

        resources.push(policy);
        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let vault = resource_config::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(CfExpression::object([
            ("accountId", CfExpression::ref_("AWS::AccountId")),
            ("region", CfExpression::ref_("AWS::Region")),
            (
                "parameterPrefix",
                CfExpression::sub(format!("${{AWS::StackName}}-{}", vault.id())),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let vault = resource_config::<Vault>(ctx, Vault::RESOURCE_TYPE)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("parameter-store")),
            (
                "vaultPrefix",
                CfExpression::sub(format!("${{AWS::StackName}}-{}", vault.id())),
            ),
        ])))
    }
}

fn vault_iam_policies(ctx: &EmitContext<'_>, vault: &Vault) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context =
        permission_context().with_resource_name(format!("${{AWS::StackName}}-{}", vault.id()));

    for (owner_index, (role_id, permission_refs)) in vault_permission_owners(ctx) {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("vault/") {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate AWS CloudFormation vault IAM policy for '{}'",
                        vault.id()
                    ),
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
            let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            else {
                continue;
            };

            let policy_id = format!("{role_id}VaultPermission{owner_index}{permission_index}");
            let mut policy_resource = CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-vault-{owner_index}-{permission_index}",
                    vault.id()
                )),
            );
            policy_resource.properties.insert(
                "PolicyDocument".to_string(),
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list(uniquify_iam_statement_sids(policy_statements)),
                    ),
                ]),
            );
            policy_resource.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(&role_id)]),
            );
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

fn management_vault_policy_document(
    ctx: &EmitContext<'_>,
    vault: &Vault,
) -> Result<Option<CfExpression>> {
    let mut statements = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context =
        permission_context().with_resource_name(format!("${{AWS::StackName}}-{}", vault.id()));

    for permission_set_ref in management_permission_refs(ctx) {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if permission_set.platforms.aws.is_none() {
            continue;
        }

        let policy = generator
            .generate_policy(&permission_set, BindingTarget::Resource, &context)
            .context(ErrorData::GenericError {
                message: "failed to generate AWS vault management IAM policy".to_string(),
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
        if let Some(CfExpression::List(policy_statements)) = policy_object.shift_remove("Statement")
        {
            statements.extend(policy_statements);
        }
    }

    if statements.is_empty() {
        return Ok(None);
    }

    Ok(Some(CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list(uniquify_iam_statement_sids(statements)),
        ),
    ])))
}

fn management_permission_refs<'a>(ctx: &'a EmitContext<'_>) -> Vec<&'a PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };
    resource_permission_refs(profile, ctx.resource_id)
}

fn resource_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("vault/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    refs
}

fn vault_permission_owners(
    ctx: &EmitContext<'_>,
) -> Vec<(usize, (String, Vec<PermissionSetReference>))> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if service_account_for_id(ctx, &service_account_id).is_some() {
            if let Some(role_id) = service_account_role_id(ctx, profile_name) {
                owners.push((role_id, refs));
            }
        }
    }

    owners.into_iter().enumerate().collect()
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a ServiceAccount> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()
}

fn remote_stack_management_logical_id<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}

fn management_role_logical_id(resource_logical_id: &str) -> String {
    if resource_logical_id == "Management" {
        "ManagementRole".to_string()
    } else {
        format!("{resource_logical_id}Role")
    }
}

fn sanitize_policy_name_segment(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}
