//! AWS Storage — S3 bucket emitter.
//!
//! Uses the modern AWS provider idiom: one `aws_s3_bucket` plus separate
//! configuration sub-resources (encryption, ownership controls, public
//! access block, optional versioning, optional lifecycle).

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, required_label, resource_prefix_template, tags,
    },
    emitters::enabled,
    expr,
};
use alien_core::{
    import::EmitContext, LifecycleRule, PermissionProfile, PermissionSetReference,
    RemoteStackManagement, Result, ServiceAccount, Storage,
};
use alien_permissions::BindingTarget;
use hcl::{
    expr::Expression,
    structure::{Block, Structure},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsStorageEmitter;

impl TfEmitter for AwsStorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let storage = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
        let mut fragment = TfFragment::default();

        // The bucket's configuration siblings only describe a bucket that
        // exists, so they carry the same gate rather than outliving it.
        fragment
            .resource_blocks
            .push(bucket(label, ctx, storage, enabled_when)?);
        fragment
            .resource_blocks
            .push(encryption(label, enabled_when)?);
        fragment
            .resource_blocks
            .push(ownership_controls(label, enabled_when)?);
        fragment.resource_blocks.push(public_access_block(
            label,
            !storage.public_read,
            enabled_when,
        )?);
        fragment
            .resource_blocks
            .push(bucket_policy(label, storage, enabled_when)?);

        if storage.versioning {
            fragment
                .resource_blocks
                .push(versioning(label, enabled_when)?);
        }
        if !storage.lifecycle_rules.is_empty() {
            fragment.resource_blocks.push(lifecycle(
                label,
                &storage.lifecycle_rules,
                enabled_when,
            )?);
        }
        emit_storage_iam(ctx, &mut fragment, label, enabled_when)?;

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
        Ok(expr::object([
            (
                "bucketName",
                enabled::attribute(enabled_when, "aws_s3_bucket", label, "bucket"),
            ),
            (
                "bucketArn",
                enabled::attribute(enabled_when, "aws_s3_bucket", label, "arn"),
            ),
        ]))
    }

    fn supports_enabled_when(&self) -> bool {
        true
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
        Ok(Some(expr::object([
            ("service", Expression::String("s3".to_string())),
            (
                "bucketName",
                enabled::attribute(enabled_when, "aws_s3_bucket", label, "bucket"),
            ),
        ])))
    }
}

fn bucket(
    label: &str,
    ctx: &EmitContext<'_>,
    storage: &Storage,
    enabled_when: Option<&str>,
) -> Result<Block> {
    let mut bucket = resource_block(
        "aws_s3_bucket",
        label,
        [
            attr("bucket", resource_prefix_template(storage.id())),
            attr("force_destroy", Expression::Bool(true)),
            attr("tags", tags(ctx, "storage")),
        ],
    );
    enabled::gate(&mut bucket, enabled_when)?;
    Ok(bucket)
}

fn encryption(label: &str, enabled_when: Option<&str>) -> Result<Block> {
    let inner = block(
        "apply_server_side_encryption_by_default",
        [attr(
            "sse_algorithm",
            Expression::String("AES256".to_string()),
        )],
    );
    let rule = block("rule", [nested(inner)]);
    let mut encryption = resource_block(
        "aws_s3_bucket_server_side_encryption_configuration",
        label,
        [attr("bucket", bucket_id(label, enabled_when)), nested(rule)],
    );
    enabled::gate(&mut encryption, enabled_when)?;
    Ok(encryption)
}

fn ownership_controls(label: &str, enabled_when: Option<&str>) -> Result<Block> {
    let rule = block(
        "rule",
        [attr(
            "object_ownership",
            Expression::String("BucketOwnerEnforced".to_string()),
        )],
    );
    let mut controls = resource_block(
        "aws_s3_bucket_ownership_controls",
        label,
        [attr("bucket", bucket_id(label, enabled_when)), nested(rule)],
    );
    enabled::gate(&mut controls, enabled_when)?;
    Ok(controls)
}

fn public_access_block(
    label: &str,
    block_public: bool,
    enabled_when: Option<&str>,
) -> Result<Block> {
    let value = Expression::Bool(block_public);
    let mut access_block = resource_block(
        "aws_s3_bucket_public_access_block",
        label,
        [
            attr("bucket", bucket_id(label, enabled_when)),
            attr("block_public_acls", value.clone()),
            attr("block_public_policy", value.clone()),
            attr("ignore_public_acls", value.clone()),
            attr("restrict_public_buckets", value),
        ],
    );
    enabled::gate(&mut access_block, enabled_when)?;
    Ok(access_block)
}

fn bucket_policy(label: &str, storage: &Storage, enabled_when: Option<&str>) -> Result<Block> {
    let objects_arn = || {
        expr::raw(format!(
            "\"${{{}}}/*\"",
            bucket_path(label, "arn", enabled_when)
        ))
    };

    let mut statements = vec![expr::object([
        (
            "Sid",
            Expression::String("DenyInsecureTransport".to_string()),
        ),
        ("Effect", Expression::String("Deny".to_string())),
        ("Principal", Expression::String("*".to_string())),
        ("Action", Expression::String("s3:*".to_string())),
        (
            "Resource",
            Expression::Array(vec![
                enabled::attribute(enabled_when, "aws_s3_bucket", label, "arn"),
                objects_arn(),
            ]),
        ),
        (
            "Condition",
            expr::object([(
                "Bool",
                expr::object([(
                    "aws:SecureTransport",
                    Expression::String("false".to_string()),
                )]),
            )]),
        ),
    ])];

    if storage.public_read {
        statements.push(expr::object([
            ("Sid", Expression::String("AllowPublicRead".to_string())),
            ("Effect", Expression::String("Allow".to_string())),
            ("Principal", Expression::String("*".to_string())),
            ("Action", Expression::String("s3:GetObject".to_string())),
            ("Resource", objects_arn()),
        ]));
    }

    let policy = expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        ("Statement", Expression::Array(statements)),
    ]);

    let mut bucket_policy = resource_block(
        "aws_s3_bucket_policy",
        label,
        [
            attr("bucket", bucket_id(label, enabled_when)),
            attr("policy", crate::emitters::aws::helpers::jsonencode(policy)),
        ],
    );
    enabled::gate(&mut bucket_policy, enabled_when)?;
    Ok(bucket_policy)
}

fn versioning(label: &str, enabled_when: Option<&str>) -> Result<Block> {
    let inner = block(
        "versioning_configuration",
        [attr("status", Expression::String("Enabled".to_string()))],
    );
    let mut versioning = resource_block(
        "aws_s3_bucket_versioning",
        label,
        [
            attr("bucket", bucket_id(label, enabled_when)),
            nested(inner),
        ],
    );
    enabled::gate(&mut versioning, enabled_when)?;
    Ok(versioning)
}

fn lifecycle(label: &str, rules: &[LifecycleRule], enabled_when: Option<&str>) -> Result<Block> {
    let mut body: Vec<Structure> = vec![attr("bucket", bucket_id(label, enabled_when))];
    for (index, rule) in rules.iter().enumerate() {
        let prefix = rule.prefix.clone().unwrap_or_default();
        let filter = block("filter", [attr("prefix", Expression::String(prefix))]);
        let expiration = block(
            "expiration",
            [attr(
                "days",
                Expression::Number(hcl::Number::from(i64::from(rule.days))),
            )],
        );
        let rule_block = block(
            "rule",
            [
                attr("id", Expression::String(format!("rule-{}", index + 1))),
                attr("status", Expression::String("Enabled".to_string())),
                nested(filter),
                nested(expiration),
            ],
        );
        body.push(nested(rule_block));
    }
    let mut lifecycle = resource_block("aws_s3_bucket_lifecycle_configuration", label, body);
    enabled::gate(&mut lifecycle, enabled_when)?;
    Ok(lifecycle)
}

fn bucket_id(label: &str, enabled_when: Option<&str>) -> Expression {
    enabled::attribute(enabled_when, "aws_s3_bucket", label, "id")
}

/// Reference path to one of the bucket's own attributes, for the places that
/// interpolate it into a string rather than emitting an expression.
fn bucket_path(label: &str, attribute: &str, enabled_when: Option<&str>) -> String {
    match enabled_when {
        Some(_) => format!("aws_s3_bucket.{label}[0].{attribute}"),
        None => format!("aws_s3_bucket.{label}.{attribute}"),
    }
}

fn emit_storage_iam(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    enabled_when: Option<&str>,
) -> Result<()> {
    for (owner_label, permission_refs) in storage_permission_owners(ctx) {
        let context = aws_terraform_permission_context().with_resource_name(format!(
            "${{{}}}",
            bucket_path(label, "bucket", enabled_when)
        ));

        for (idx, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("storage/") {
                continue;
            }

            let policy_label = format!(
                "{label}_{owner_label}_{}_{idx}",
                iam_policy_name_sanitize(&permission_set.id)
            );
            let policy_name = iam_policy_name_sanitize(&format!(
                "{}-{}-{idx}",
                ctx.resource_id, permission_set.id
            ));

            // The policy document names the bucket, so it cannot outlive it.
            // The shared emitter is used by ungated resources too, so the gate
            // goes on the blocks it just appended rather than into its
            // signature.
            let appended_from = fragment.resource_blocks.len();
            emit_iam_role_policy_for_target_with_label(
                fragment,
                &owner_label,
                &permission_set,
                &policy_label,
                &policy_name,
                &context,
                BindingTarget::Resource,
            )?;
            for block in &mut fragment.resource_blocks[appended_from..] {
                enabled::gate(block, enabled_when)?;
            }
        }
    }

    Ok(())
}

fn storage_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = storage_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some((label, _service_account)) = service_account_for_id(ctx, &service_account_id) {
            owners.push((label.to_string(), refs));
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = storage_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some(label) = remote_stack_management_label(ctx) {
                owners.push((label.to_string(), refs));
            }
        }
    }

    owners
}

fn storage_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("storage/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<(&'a str, &'a ServiceAccount)> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    let service_account = entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(service_account_id)?;
    Some((label, service_account))
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}
