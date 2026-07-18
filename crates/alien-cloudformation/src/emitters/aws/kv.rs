//! AWS KV — DynamoDB on-demand table with composite key, TTL, and SSE,
//! plus `AWS::IAM::Policy` resources for every permission profile that
//! references a `kv/` permission set on this resource.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            iam_policy_resource, iam_policy_statements, permission_gate_condition,
            remote_stack_management_role_id, required_logical_id, resource_config,
            service_account_role_id, tags,
        },
        service_account::permission_context,
    },
    generator::sanitize_logical_id,
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Kv, PermissionProfile, PermissionSetReference, Result};
use alien_permissions::BindingTarget;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKvEmitter;

impl CfEmitter for AwsKvEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
        let table_id = required_logical_id(ctx)?;
        let mut table = CfResource::new(table_id.to_string(), "AWS::DynamoDB::Table".to_string());

        table.properties.insert(
            "BillingMode".to_string(),
            CfExpression::from("PAY_PER_REQUEST"),
        );
        table.properties.insert(
            "AttributeDefinitions".to_string(),
            CfExpression::list([
                CfExpression::object([
                    ("AttributeName", CfExpression::from("pk")),
                    ("AttributeType", CfExpression::from("S")),
                ]),
                CfExpression::object([
                    ("AttributeName", CfExpression::from("sk")),
                    ("AttributeType", CfExpression::from("S")),
                ]),
            ]),
        );
        table.properties.insert(
            "KeySchema".to_string(),
            CfExpression::list([
                CfExpression::object([
                    ("AttributeName", CfExpression::from("pk")),
                    ("KeyType", CfExpression::from("HASH")),
                ]),
                CfExpression::object([
                    ("AttributeName", CfExpression::from("sk")),
                    ("KeyType", CfExpression::from("RANGE")),
                ]),
            ]),
        );
        table.properties.insert(
            "SSESpecification".to_string(),
            CfExpression::object([("SSEEnabled", CfExpression::from(true))]),
        );
        table.properties.insert(
            "TimeToLiveSpecification".to_string(),
            CfExpression::object([
                ("AttributeName", CfExpression::from("ttl")),
                ("Enabled", CfExpression::from(true)),
            ]),
        );
        table.properties.insert(
            "PointInTimeRecoverySpecification".to_string(),
            CfExpression::object([("PointInTimeRecoveryEnabled", CfExpression::from(true))]),
        );
        table.properties.insert("Tags".to_string(), tags(ctx));
        table.deletion_policy = Some("Retain".to_string());
        table.update_replace_policy = Some("Retain".to_string());

        let mut resources = vec![table];
        resources.extend(kv_iam_policies(ctx, table_id)?);

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let table_id = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("tableName", CfExpression::ref_(table_id)),
            ("tableArn", CfExpression::get_att(table_id, "Arn")),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let table_id = required_logical_id(ctx)?;
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("dynamodb")),
            ("tableName", CfExpression::ref_(table_id)),
            ("region", CfExpression::ref_("AWS::Region")),
        ])))
    }
}

fn kv_iam_policies(ctx: &EmitContext<'_>, table_id: &str) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    // `${TableLogicalId}` rides the permissions generator's Fn::Sub auto-wrap —
    // CloudFormation generates the physical table name.
    let context = permission_context().with_resource_name(format!("${{{table_id}}}"));

    for (profile_name, role_id, permission_refs) in kv_permission_owners(ctx) {
        for permission_ref in permission_refs {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            let Some(statements) = iam_policy_statements(
                &permission_set,
                BindingTarget::Resource,
                &context,
                &format!("kv '{}'", ctx.resource_id),
            )?
            else {
                continue;
            };

            let condition = profile_name.as_deref().and_then(|profile| {
                permission_gate_condition(ctx, profile, &permission_set.id, &[ctx.resource_id])
            });
            let policy_id = format!(
                "{table_id}{role_id}{}",
                sanitize_logical_id(&permission_set.id)
            );
            let policy_name = CfExpression::sub(format!(
                "${{AWS::StackName}}-{}-{}",
                ctx.resource_id,
                permission_set.id.replace('/', "-")
            ));
            let mut policy =
                iam_policy_resource(policy_id, policy_name, statements, &role_id, condition);
            policy.depends_on.push(table_id.to_string());
            resources.push(policy);
        }
    }

    Ok(resources)
}

fn kv_permission_owners(
    ctx: &EmitContext<'_>,
) -> Vec<(Option<String>, String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = kv_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        if let Some(role_id) = service_account_role_id(ctx, profile_name) {
            owners.push((Some(profile_name.clone()), role_id, refs));
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = kv_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some(role_id) = remote_stack_management_role_id(ctx) {
                owners.push((None, role_id, refs));
            }
        }
    }

    owners
}

fn kv_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = HashSet::new();
    let Some(resource_refs) = profile.0.get(resource_id) else {
        return refs;
    };

    for permission_ref in resource_refs {
        let id = permission_ref.id();
        if !id.starts_with("kv/") || id.ends_with("/provision") {
            continue;
        }
        if seen_ids.insert(id.to_string()) {
            refs.push(permission_ref.clone());
        }
    }

    refs
}
