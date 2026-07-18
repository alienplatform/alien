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
        TrackedPermissionRef,
    },
    emitters::gates::permission_gate_count,
    expr,
};
use alien_core::{
    import::EmitContext, LifecycleRule, PermissionProfile, RemoteStackManagement, Result,
    ServiceAccount, Storage,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsStorageEmitter;

impl TfEmitter for AwsStorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let storage = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        fragment.resource_blocks.push(bucket(label, ctx, storage));
        fragment.resource_blocks.push(encryption(label));
        fragment.resource_blocks.push(ownership_controls(label));
        fragment
            .resource_blocks
            .push(public_access_block(label, !storage.public_read));
        fragment.resource_blocks.push(bucket_policy(label, storage));

        if storage.versioning {
            fragment.resource_blocks.push(versioning(label));
        }
        if !storage.lifecycle_rules.is_empty() {
            fragment
                .resource_blocks
                .push(lifecycle(label, &storage.lifecycle_rules));
        }
        emit_storage_iam(ctx, &mut fragment, label)?;

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "bucketName",
                expr::traversal(["aws_s3_bucket", label, "bucket"]),
            ),
            (
                "bucketArn",
                expr::traversal(["aws_s3_bucket", label, "arn"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("s3".to_string())),
            (
                "bucketName",
                expr::traversal(["aws_s3_bucket", label, "bucket"]),
            ),
        ])))
    }
}

fn bucket(label: &str, ctx: &EmitContext<'_>, storage: &Storage) -> hcl::structure::Block {
    resource_block(
        "aws_s3_bucket",
        label,
        [
            attr("bucket", resource_prefix_template(storage.id())),
            attr("force_destroy", Expression::Bool(true)),
            attr("tags", tags(ctx, "storage")),
        ],
    )
}

fn encryption(label: &str) -> hcl::structure::Block {
    let inner = block(
        "apply_server_side_encryption_by_default",
        [attr(
            "sse_algorithm",
            Expression::String("AES256".to_string()),
        )],
    );
    let rule = block("rule", [nested(inner)]);
    resource_block(
        "aws_s3_bucket_server_side_encryption_configuration",
        label,
        [attr("bucket", bucket_id(label)), nested(rule)],
    )
}

fn ownership_controls(label: &str) -> hcl::structure::Block {
    let rule = block(
        "rule",
        [attr(
            "object_ownership",
            Expression::String("BucketOwnerEnforced".to_string()),
        )],
    );
    resource_block(
        "aws_s3_bucket_ownership_controls",
        label,
        [attr("bucket", bucket_id(label)), nested(rule)],
    )
}

fn public_access_block(label: &str, block_public: bool) -> hcl::structure::Block {
    let value = Expression::Bool(block_public);
    resource_block(
        "aws_s3_bucket_public_access_block",
        label,
        [
            attr("bucket", bucket_id(label)),
            attr("block_public_acls", value.clone()),
            attr("block_public_policy", value.clone()),
            attr("ignore_public_acls", value.clone()),
            attr("restrict_public_buckets", value),
        ],
    )
}

fn bucket_policy(label: &str, storage: &Storage) -> hcl::structure::Block {
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
                expr::traversal(["aws_s3_bucket", label, "arn"]),
                expr::raw(format!("\"${{aws_s3_bucket.{label}.arn}}/*\"")),
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
            (
                "Resource",
                expr::raw(format!("\"${{aws_s3_bucket.{label}.arn}}/*\"")),
            ),
        ]));
    }

    let policy = expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        ("Statement", Expression::Array(statements)),
    ]);

    resource_block(
        "aws_s3_bucket_policy",
        label,
        [
            attr("bucket", bucket_id(label)),
            attr("policy", crate::emitters::aws::helpers::jsonencode(policy)),
        ],
    )
}

fn versioning(label: &str) -> hcl::structure::Block {
    let inner = block(
        "versioning_configuration",
        [attr("status", Expression::String("Enabled".to_string()))],
    );
    resource_block(
        "aws_s3_bucket_versioning",
        label,
        [attr("bucket", bucket_id(label)), nested(inner)],
    )
}

fn lifecycle(label: &str, rules: &[LifecycleRule]) -> hcl::structure::Block {
    let mut body: Vec<hcl::structure::Structure> = vec![attr("bucket", bucket_id(label))];
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
    resource_block("aws_s3_bucket_lifecycle_configuration", label, body)
}

fn bucket_id(label: &str) -> Expression {
    expr::traversal(["aws_s3_bucket", label, "id"])
}

fn emit_storage_iam(ctx: &EmitContext<'_>, fragment: &mut TfFragment, label: &str) -> Result<()> {
    for owner in storage_permission_owners(ctx) {
        let context = aws_terraform_permission_context()
            .with_resource_name(format!("${{aws_s3_bucket.{label}.bucket}}"));

        for (idx, tracked_ref) in owner.refs.iter().enumerate() {
            let Some(permission_set) = tracked_ref
                .reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("storage/") {
                continue;
            }

            let gate_count = match &owner.profile {
                Some(profile) => permission_gate_count(
                    ctx,
                    profile,
                    &permission_set.id,
                    &tracked_ref.origin_keys(ctx.resource_id),
                )?,
                None => None,
            };
            let policy_label = format!(
                "{label}_{}_{}_{idx}",
                owner.label,
                iam_policy_name_sanitize(&permission_set.id)
            );
            let policy_name = iam_policy_name_sanitize(&format!(
                "{}-{}-{idx}",
                ctx.resource_id, permission_set.id
            ));

            emit_iam_role_policy_for_target_with_label(
                fragment,
                &owner.label,
                &permission_set,
                &policy_label,
                &policy_name,
                &context,
                BindingTarget::Resource,
                gate_count,
            )?;
        }
    }

    Ok(())
}

struct StorageOwner {
    label: String,
    profile: Option<String>,
    refs: Vec<TrackedPermissionRef>,
}

fn storage_permission_owners(ctx: &EmitContext<'_>) -> Vec<StorageOwner> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = storage_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some((label, _service_account)) = service_account_for_id(ctx, &service_account_id) {
            owners.push(StorageOwner {
                label: label.to_string(),
                profile: Some(profile_name.clone()),
                refs,
            });
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = storage_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some(label) = remote_stack_management_label(ctx) {
                owners.push(StorageOwner {
                    label: label.to_string(),
                    profile: None,
                    refs,
                });
            }
        }
    }

    owners
}

fn storage_permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
) -> Vec<TrackedPermissionRef> {
    let mut refs: Vec<TrackedPermissionRef> = Vec::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if !refs
                .iter()
                .any(|tracked| tracked.reference.id() == permission_ref.id())
            {
                refs.push(TrackedPermissionRef {
                    reference: permission_ref.clone(),
                    in_resource: true,
                    in_wildcard: false,
                });
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("storage/"))
        {
            if let Some(tracked) = refs
                .iter_mut()
                .find(|tracked| tracked.reference.id() == permission_ref.id())
            {
                tracked.in_wildcard = true;
            } else {
                refs.push(TrackedPermissionRef {
                    reference: permission_ref.clone(),
                    in_resource: false,
                    in_wildcard: true,
                });
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
