//! AWS KV — DynamoDB on-demand table with composite key, TTL, and SSE.

use crate::{
    emitter::CfEmitter,
    emitters::aws::{
        helpers::{
            cf_from_json, required_logical_id, resource_config, resource_permission_owners, tags,
            uniquify_iam_statement_sids,
        },
        service_account::permission_context,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, ErrorData, Kv, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{generators::AwsCloudFormationPermissionsGenerator, BindingTarget};

/// Permission-set id prefix for this resource type.
const PERMISSION_SET_PREFIX: &str = "kv/";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKvEmitter;

impl CfEmitter for AwsKvEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let kv = resource_config::<Kv>(ctx, Kv::RESOURCE_TYPE)?;
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
        resources.extend(kv_iam_policies(ctx, kv, table_id)?);
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

/// IAM policies attaching granted `kv/*` permission sets to the owning
/// service-account roles, scoped to this table's ARN.
fn kv_iam_policies(ctx: &EmitContext<'_>, kv: &Kv, table_id: &str) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context =
        permission_context().with_resource_name(format!("${{AWS::StackName}}-{}", kv.id()));

    for (owner_index, (role_id, permission_refs)) in
        resource_permission_owners(ctx, PERMISSION_SET_PREFIX)
            .into_iter()
            .enumerate()
    {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with(PERMISSION_SET_PREFIX) {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::GenericError {
                    message: format!(
                        "failed to generate AWS CloudFormation kv IAM policy for '{}'",
                        kv.id()
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
            // The physical table name carries a CloudFormation-generated
            // suffix, so name-pattern resource bindings cannot match it; pin
            // every statement to this table's ARN.
            let policy_statements = policy_statements
                .into_iter()
                .map(|statement| pin_statement_to_table(statement, table_id))
                .collect::<Vec<_>>();

            let policy_id =
                format!("{table_id}{role_id}KvPermission{owner_index}{permission_index}");
            let mut policy_resource = CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-kv-{owner_index}-{permission_index}",
                    kv.id()
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
            policy_resource.depends_on.push(table_id.to_string());
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

fn pin_statement_to_table(statement: CfExpression, table_id: &str) -> CfExpression {
    let CfExpression::Object(mut statement_object) = statement else {
        return statement;
    };
    statement_object.insert(
        "Resource".to_string(),
        CfExpression::get_att(table_id, "Arn"),
    );
    CfExpression::Object(statement_object)
}
