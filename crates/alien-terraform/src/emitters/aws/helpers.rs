//! Shared helpers for AWS Terraform emitters.
//!
//! Anything that touches more than one resource (downcast, label
//! lookup, common tag block, common IAM role helpers, network
//! resolution) lives here so per-resource emitters stay focused on the
//! Terraform their cloud team would write.

use std::collections::HashSet;

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, Network, NetworkSettings, PermissionProfile, PermissionSet,
    PermissionSetReference, ResourceDefinition, ResourceRef, ResourceType, Result, ServiceAccount,
    ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY,
};
use alien_error::{AlienError, Context, IntoAlienError};
pub(crate) use alien_permissions::generators::ensure_unique_statement_sids;
use alien_permissions::{
    aws_resource_policies::{aws_resource_policy_name, without_refs_the_stack_policy_grants},
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use hcl::{
    expr::Expression,
    structure::{Block, Structure},
};

/// Downcast `ctx.resource.config` to the typed resource definition or
/// return a typed `UnexpectedResourceType` error.
pub fn downcast<'a, T: ResourceDefinition>(
    ctx: &'a EmitContext<'_>,
    expected: ResourceType,
) -> Result<&'a T> {
    ctx.resource.config.downcast_ref::<T>().ok_or_else(|| {
        AlienError::new(ErrorData::UnexpectedResourceType {
            resource_id: ctx.resource_id.to_string(),
            expected,
            actual: ctx.resource.config.resource_type(),
        })
    })
}

/// Look up the precomputed Terraform label for the current emitter
/// context.
pub fn required_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.name_for(ctx.resource_id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("missing terraform label for resource '{}'", ctx.resource_id),
        })
    })
}

/// Look up the precomputed label for a referenced resource.
pub fn label_for_ref<'a>(ctx: &'a EmitContext<'_>, reference: &ResourceRef) -> Result<&'a str> {
    ctx.name_for(reference.id()).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!(
                "missing terraform label for referenced resource '{}'",
                reference.id()
            ),
        })
    })
}

/// `${local.resource_prefix}-{suffix}` HCL template expression.
pub fn resource_prefix_template(suffix: &str) -> Expression {
    expr::template(format!("${{local.resource_prefix}}-{suffix}"))
}

/// IAM role names are capped at 64 characters. Keep the normal
/// resource-prefixed name while it fits; otherwise preserve a readable
/// prefix and append a stable hash to avoid collisions.
pub fn iam_role_name_template(suffix: &str) -> Expression {
    const IAM_ROLE_NAME_MAX_LEN: usize = 64;
    const HASH_LEN: usize = 8;
    const HASH_SEPARATOR_LEN: usize = 1;
    const PREFIX_LEN: usize = IAM_ROLE_NAME_MAX_LEN - HASH_SEPARATOR_LEN - HASH_LEN;

    let suffix_literal = serde_json::to_string(suffix).expect("serializing a string cannot fail");
    let full_name = format!("format(\"%s-%s\", local.resource_prefix, {suffix_literal})");
    expr::raw(format!(
        "length({full_name}) <= {IAM_ROLE_NAME_MAX_LEN} ? {full_name} : format(\"%s-%s\", substr({full_name}, 0, {PREFIX_LEN}), substr(sha1({full_name}), 0, {HASH_LEN}))"
    ))
}

/// IAM customer-managed policy names are capped at 128 characters and
/// must be unique in an account. Keep the resource-prefixed name while it
/// fits; otherwise preserve a readable prefix and append a stable hash.
pub fn iam_managed_policy_name_template(suffix: &str) -> Expression {
    const IAM_POLICY_NAME_MAX_LEN: usize = 128;
    const HASH_LEN: usize = 8;
    const HASH_SEPARATOR_LEN: usize = 1;
    const PREFIX_LEN: usize = IAM_POLICY_NAME_MAX_LEN - HASH_SEPARATOR_LEN - HASH_LEN;

    let suffix_literal = serde_json::to_string(suffix).expect("serializing a string cannot fail");
    let full_name = format!("format(\"%s-%s\", local.resource_prefix, {suffix_literal})");
    expr::raw(format!(
        "length({full_name}) <= {IAM_POLICY_NAME_MAX_LEN} ? {full_name} : format(\"%s-%s\", substr({full_name}, 0, {PREFIX_LEN}), substr(sha1({full_name}), 0, {HASH_LEN}))"
    ))
}

/// Standard tags block. `alien_resource_type` is the resource-type slug
/// ("storage", "queue", ...).
pub fn tags(ctx: &EmitContext<'_>, alien_resource_type: &'static str) -> Expression {
    expr::object([
        (
            ALIEN_MANAGED_BY_TAG_KEY,
            Expression::String("setup".to_string()),
        ),
        (ALIEN_STACK_TAG_KEY, expr::raw("local.resource_prefix")),
        (
            ALIEN_RESOURCE_TAG_KEY,
            Expression::String(ctx.resource_id.to_string()),
        ),
        (
            "resource-type",
            Expression::String(alien_resource_type.to_string()),
        ),
    ])
}

/// First [`Network`] resource in the stack with its label. Used by
/// compute emitters that need a VPC reference.
pub fn default_network<'a>(ctx: &EmitContext<'a>) -> Option<(&'a str, &'a Network)> {
    ctx.stack.resources().find_map(|(id, entry)| {
        let network = entry.config.downcast_ref::<Network>()?;
        let label = ctx.name_for(id)?;
        Some((label, network))
    })
}

/// Look up the IAM role's Terraform reference for a service account by
/// permissions profile name (the `<profile>-sa` convention used across
/// AWS resources).
pub fn service_account_role_arn(ctx: &EmitContext<'_>, profile_name: &str) -> Option<Expression> {
    let service_account_id = format!("{profile_name}-sa");
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(&service_account_id)?;
    Some(expr::traversal(["aws_iam_role", label, "arn"]))
}

/// Build an `assume_role_policy` JSON expression for service principals
/// (e.g. `lambda.amazonaws.com`).
pub fn service_assume_role_policy(services: &[&str]) -> Expression {
    let principal = if services.len() == 1 {
        Expression::String(services[0].to_string())
    } else {
        Expression::Array(
            services
                .iter()
                .map(|s| Expression::String((*s).to_string()))
                .collect(),
        )
    };
    let policy = expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        (
            "Statement",
            Expression::Array(vec![expr::object([
                ("Effect", Expression::String("Allow".to_string())),
                (
                    "Principal",
                    expr::object([("Service", principal)].into_iter()),
                ),
                ("Action", Expression::String("sts:AssumeRole".to_string())),
            ])]),
        ),
    ]);
    jsonencode(policy)
}

/// `jsonencode(...)` HCL function call.
/// The trust statement that lets the managing role assume a setup-created role.
///
/// With `var.management_external_id` set, the caller must also send it as
/// `sts:ExternalId`, so the managing role, which many customers trust, cannot
/// be pointed at this customer's role on behalf of someone else. With it empty
/// the statement is exactly the trust modules had before the variable existed.
pub fn managing_role_trust_statement() -> Expression {
    expr::object([
        ("Sid", Expression::String("AllowManagingRole".to_string())),
        ("Effect", Expression::String("Allow".to_string())),
        (
            "Principal",
            expr::object([("AWS", expr::raw("var.managing_role_arn"))]),
        ),
        ("Action", Expression::String("sts:AssumeRole".to_string())),
        (
            "Condition",
            expr::object([(
                "StringEquals",
                expr::raw(
                    r#"merge({ "aws:PrincipalArn" = var.managing_role_arn }, var.management_external_id == "" ? {} : { "sts:ExternalId" = var.management_external_id })"#,
                ),
            )]),
        ),
    ])
}

pub fn jsonencode(value: Expression) -> Expression {
    Expression::FuncCall(Box::new(
        hcl::expr::FuncCall::builder(hcl::Identifier::sanitized("jsonencode"))
            .arg(value)
            .build(),
    ))
}

/// Convert a JSON policy into an HCL `Expression` that renders cleanly
/// under `jsonencode`. String values containing `${...}` are wrapped as
/// quoted templates so the rendered HCL keeps the interpolation;
/// everything else maps 1:1.
///
/// `AwsRuntimePermissionsGenerator` produces fully-interpolated policy
/// strings whose contents may already embed Terraform templates like
/// `${local.resource_prefix}` (when called with an HCL-flavored
/// [`PermissionContext`]). This helper preserves that interpolation
/// when translating to HCL.
pub fn json_value_to_expression(value: serde_json::Value) -> Expression {
    json_value_to_expression_impl(value, None)
}

fn json_value_to_expression_impl(
    value: serde_json::Value,
    key_order: Option<fn(&str) -> u8>,
) -> Expression {
    match value {
        serde_json::Value::Null => Expression::Null,
        serde_json::Value::Bool(b) => Expression::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Expression::Number(hcl::Number::from(i))
            } else if let Some(f) = n.as_f64() {
                Expression::Number(hcl::Number::from_f64(f).unwrap_or_else(|| hcl::Number::from(0)))
            } else {
                Expression::Null
            }
        }
        serde_json::Value::String(s) => {
            if s.contains("${") {
                expr::template(s)
            } else {
                Expression::String(s)
            }
        }
        serde_json::Value::Array(items) => Expression::Array(
            items
                .into_iter()
                .map(|item| json_value_to_expression_impl(item, key_order))
                .collect(),
        ),
        serde_json::Value::Object(map) => {
            let mut pairs: Vec<(String, Expression)> = map
                .into_iter()
                .map(|(key, value)| (key, json_value_to_expression_impl(value, key_order)))
                .collect();
            if let Some(key_order) = key_order {
                pairs.sort_by(|(left, _), (right, _)| {
                    key_order(left)
                        .cmp(&key_order(right))
                        .then_with(|| left.cmp(right))
                });
            }
            expr::object(pairs.iter().map(|(k, v)| (k.as_str(), v.clone())))
        }
    }
}

fn iam_json_key_order(key: &str) -> u8 {
    match key {
        "Version" => 0,
        "Statement" => 1,
        "Sid" => 2,
        "Effect" => 3,
        "Principal" => 4,
        "Action" => 5,
        "Resource" | "NotResource" => 6,
        "Condition" => 7,
        _ => u8::MAX,
    }
}

/// `jsonencode(json_value_to_expression(value))` — wrap a JSON IAM
/// policy document (typically the serialized
/// [`alien_permissions::generators::AwsIamPolicy`]) as an HCL
/// `jsonencode(...)` expression that preserves any embedded
/// `${var.…}` / `${data.…}` templates.
pub fn jsonencode_policy_value(value: serde_json::Value) -> Expression {
    jsonencode(json_value_to_expression_impl(
        value,
        Some(iam_json_key_order),
    ))
}

/// Build a `policy = jsonencode({Version, Statement})` attribute.
pub fn policy_attr(statements: Vec<Expression>) -> Structure {
    attr(
        "policy",
        jsonencode(expr::object([
            ("Version", Expression::String("2012-10-17".to_string())),
            ("Statement", Expression::Array(statements)),
        ])),
    )
}

/// IAM names allow `[\w+=,.@-]`. Replace any disallowed character with
/// `-` so generated policy / role names always validate.
pub fn iam_policy_name_sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Terraform-flavored [`PermissionContext`] for AWS emitters.
///
/// Embeds HCL templates (`${var.…}`, `${data.…}`) as the variable
/// values so [`AwsRuntimePermissionsGenerator`] interpolates them
/// straight into the policy strings — no CloudFormation intrinsics
/// involved. Mirrors the GCP pattern in
/// [`crate::emitters::gcp::helpers::permission_context`].
pub fn aws_terraform_permission_context() -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("${local.resource_prefix}")
        .with_aws_region("${data.aws_region.current.region}")
        .with_aws_account_id("${data.aws_caller_identity.current.account_id}")
        .with_managing_account_id("${var.managing_account_id}")
        .with_managing_role_arn("${var.managing_role_arn}")
}

/// Permission context for a setup that creates its management identity.
pub fn stack_permission_context(ctx: &EmitContext<'_>) -> PermissionContext {
    let context = aws_terraform_permission_context();
    match ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == alien_core::RemoteStackManagement::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    }) {
        Some(label) => context.with_aws_permissions_boundary_arn(format!(
            "${{aws_iam_policy.{label}_permissions_boundary.arn}}"
        )),
        None => context,
    }
}

/// Emit one `aws_iam_role_policy` block for `permission_set`, attached
/// to the IAM role identified by `role_label`.
///
/// Mirrors the runtime path:
/// [`AwsRuntimePermissionsGenerator::generate_policy`] produces the
/// policy document; the emitter wraps it in `jsonencode(...)` and
/// pushes a sibling `aws_iam_role_policy` resource. Symmetric with the
/// GCP custom-role binding helper
/// helper.
///
/// `policy_label_index` is appended to both the Terraform resource
/// label and the policy `name` so multiple permission sets attached to
/// the same role don't collide.
pub fn emit_iam_role_policy(
    fragment: &mut TfFragment,
    role_label: &str,
    permission_set: &PermissionSet,
    policy_label_index: usize,
    context: &PermissionContext,
) -> Result<()> {
    emit_iam_role_policy_for_target(
        fragment,
        role_label,
        permission_set,
        policy_label_index,
        context,
        BindingTarget::Stack,
    )
}

pub fn emit_iam_role_policy_for_target(
    fragment: &mut TfFragment,
    role_label: &str,
    permission_set: &PermissionSet,
    policy_label_index: usize,
    context: &PermissionContext,
    target: BindingTarget,
) -> Result<()> {
    emit_iam_role_policy_for_target_with_label(
        fragment,
        role_label,
        permission_set,
        &format!("{role_label}_set_{policy_label_index}"),
        &format!(
            "{}-{}",
            iam_policy_name_sanitize(&permission_set.id),
            policy_label_index
        ),
        context,
        target,
    )
}

/// Attaches the `<prefix>` sets each permission profile grants on this resource, by id or under
/// `"*"`, to that profile's service-account role, scoped to the resource's
/// `${local.resource_prefix}-<id>` name.
///
/// Setup owns these grants: after a Terraform handoff the runtime refuses to write them, so a
/// set left out here is never granted. Policy names and the omission of `"*"` sets the
/// stack-level policy already grants follow the runtime writer.
pub fn emit_workload_resource_grants(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    permission_set_prefix: &str,
) -> Result<()> {
    let resource_label = required_label(ctx)?;
    let context = stack_permission_context(ctx)
        .with_resource_name(format!("${{local.resource_prefix}}-{}", ctx.resource_id));
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let service_account_id = format!("{profile_name}-sa");
        let Some(role_label) = ctx.stack.resources().find_map(|(id, entry)| {
            (id.as_str() == service_account_id
                && entry.config.downcast_ref::<ServiceAccount>().is_some())
            .then(|| ctx.name_for(id))
            .flatten()
        }) else {
            continue;
        };
        let mut seen = HashSet::new();
        let granted: Vec<PermissionSetReference> = [ctx.resource_id, "*"]
            .into_iter()
            .filter_map(|key| profile.0.get(key))
            .flatten()
            .filter(|reference| reference.id().starts_with(permission_set_prefix))
            .filter(|reference| seen.insert(reference.id().to_string()))
            .cloned()
            .collect();
        let granted = without_stack_policy_grants(ctx, profile_name, profile, granted, &context)?;
        for reference in granted {
            let permission_set = reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!(
                            "permission set '{}' granted on '{}' is not registered",
                            reference.id(),
                            ctx.resource_id
                        ),
                    })
                })?;
            emit_iam_role_policy_for_target_with_label(
                fragment,
                role_label,
                &permission_set,
                &format!(
                    "{role_label}_{resource_label}_{}",
                    permission_set.id.replace(['/', '-'], "_")
                ),
                &aws_resource_policy_name(ctx.resource_id, &permission_set.id),
                &context,
                BindingTarget::Resource,
            )?;
        }
    }
    Ok(())
}

/// Drops the sets of `refs` that the stack-level policy of `profile_name`'s service account
/// already grants on the resource being emitted, resolved with `context`. Sets the resource
/// names itself stay. The runtime writer selects a resource's policies with the same rule.
pub fn without_stack_policy_grants(
    ctx: &EmitContext<'_>,
    profile_name: &str,
    profile: &PermissionProfile,
    refs: Vec<PermissionSetReference>,
    context: &PermissionContext,
) -> Result<Vec<PermissionSetReference>> {
    without_refs_the_stack_policy_grants(
        ctx.stack,
        profile_name,
        profile,
        ctx.resource_id,
        refs,
        &AwsRuntimePermissionsGenerator::new(),
        context,
    )
    .context(ErrorData::GenericError {
        message: format!(
            "failed to select profile '{profile_name}' policies for '{}'",
            ctx.resource_id
        ),
    })
}

/// [`without_stack_policy_grants`], also dropping the sets that render for this resource
/// exactly as the stack-level policy holds them: a set bound to the same resources at both
/// targets, such as the deployer-secrets protection Deny, written again changes nothing,
/// whatever its effect. Sets the resource names itself stay.
pub fn without_stack_policy_copies(
    ctx: &EmitContext<'_>,
    profile_name: &str,
    profile: &PermissionProfile,
    refs: Vec<PermissionSetReference>,
    context: &PermissionContext,
) -> Result<Vec<PermissionSetReference>> {
    let stack_sets = ctx
        .stack
        .resources()
        .find(|(id, _)| id.as_str() == format!("{profile_name}-sa"))
        .and_then(|(_, entry)| entry.config.downcast_ref::<ServiceAccount>())
        .map(|account| account.stack_permission_sets.as_slice())
        .unwrap_or_default();
    let named = profile
        .0
        .get(ctx.resource_id)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut kept = Vec::new();
    for reference in without_stack_policy_grants(ctx, profile_name, profile, refs, context)? {
        let held = reference
            .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            .filter(|set| set.platforms.aws.is_some() && stack_sets.contains(set));
        let copy = match held {
            Some(set) if !named.contains(&reference) => {
                let render = |target| {
                    generator
                        .generate_policy(&set, target, context)
                        .context(ErrorData::GenericError {
                            message: format!(
                                "failed to compare permission set '{}' on '{}' with profile '{profile_name}'s stack-level policy",
                                set.id, ctx.resource_id
                            ),
                        })
                };
                render(BindingTarget::Stack)? == render(BindingTarget::Resource)?
            }
            _ => false,
        };
        if !copy {
            kept.push(reference);
        }
    }
    Ok(kept)
}

pub fn emit_iam_role_policy_for_target_with_label(
    fragment: &mut TfFragment,
    role_label: &str,
    permission_set: &PermissionSet,
    policy_label: &str,
    policy_name: &str,
    context: &PermissionContext,
    target: BindingTarget,
) -> Result<()> {
    if permission_set.platforms.aws.is_none() {
        return Ok(());
    }

    let generator = AwsRuntimePermissionsGenerator::new();
    let policy = generator
        .generate_policy(permission_set, target, context)
        .context(ErrorData::GenericError {
            message: format!(
                "failed to generate AWS Terraform policy for permission set '{}'",
                permission_set.id
            ),
        })?;
    let policy_value = serde_json::to_value(policy).into_alien_error().context(
        ErrorData::TemplateSerializationFailed {
            format: "Terraform IAM policy".to_string(),
            reason: "Failed to serialize IAM policy".to_string(),
        },
    )?;
    let policy_expr = jsonencode_policy_value(policy_value);
    fragment.resource_blocks.push(resource_block(
        "aws_iam_role_policy",
        policy_label,
        [
            attr("name", Expression::String(policy_name.to_string())),
            attr("role", expr::traversal(["aws_iam_role", role_label, "id"])),
            attr("policy", policy_expr),
        ],
    ));
    Ok(())
}

/// Keep the address of a grant that is no longer written, as an inert stub on `role_label`.
///
/// Installed deployments hold the grant inline under this address. Terraform cannot order
/// deleting it after the grant that replaces it exists, but it orders an in-place update, so
/// the address stays and the generator orders the stub after the role's managed policies.
pub fn emit_iam_role_policy_stub(
    fragment: &mut TfFragment,
    role_label: &str,
    policy_label: &str,
    policy_name: &str,
) {
    fragment.resource_blocks.push(resource_block(
        "aws_iam_role_policy",
        policy_label,
        [
            attr("name", Expression::String(policy_name.to_string())),
            attr("role", expr::traversal(["aws_iam_role", role_label, "id"])),
            attr(
                "policy",
                Expression::String(crate::aws_role_policies::STUB_POLICY.to_string()),
            ),
        ],
    ));
}

pub fn emit_iam_managed_policy_chunks(
    fragment: &mut TfFragment,
    role_label: &str,
    base_label: &str,
    base_name: &str,
    statements: Vec<AwsIamStatement>,
) -> Result<()> {
    const MAX_MANAGED_POLICY_BYTES: usize = 5_500;

    let mut statements = statements;
    ensure_unique_statement_sids(&mut statements);
    let chunks = chunk_iam_statements(statements, MAX_MANAGED_POLICY_BYTES)?;
    for (idx, chunk) in chunks.into_iter().enumerate() {
        let policy_label = format!("{base_label}_{idx}");
        let attachment_label = format!("{policy_label}_attachment");
        let policy_name = format!("{base_name}-{idx}");
        let policy_expr = policy_document_expr(chunk)?;

        fragment.resource_blocks.push(resource_block(
            "aws_iam_policy",
            &policy_label,
            [
                attr("name", iam_managed_policy_name_template(&policy_name)),
                attr("policy", policy_expr),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "aws_iam_role_policy_attachment",
            &attachment_label,
            [
                attr(
                    "role",
                    expr::traversal(["aws_iam_role", role_label, "name"]),
                ),
                attr(
                    "policy_arn",
                    expr::traversal(["aws_iam_policy", policy_label.as_str(), "arn"]),
                ),
            ],
        ));
    }

    Ok(())
}

fn chunk_iam_statements(
    statements: Vec<AwsIamStatement>,
    max_policy_bytes: usize,
) -> Result<Vec<Vec<AwsIamStatement>>> {
    let mut chunks = Vec::new();
    let mut current = Vec::new();

    for statement in statements {
        let mut candidate = current.clone();
        candidate.push(statement.clone());
        if policy_document_size(&candidate)? <= max_policy_bytes {
            current = candidate;
            continue;
        }

        if current.is_empty()
            || policy_document_size(std::slice::from_ref(&statement))? > max_policy_bytes
        {
            return Err(AlienError::new(ErrorData::GenericError {
                message: format!(
                    "AWS IAM statement '{}' is too large for a managed policy",
                    statement.sid
                ),
            }));
        }

        chunks.push(current);
        current = vec![statement];
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    Ok(chunks)
}

fn policy_document_size(statements: &[AwsIamStatement]) -> Result<usize> {
    serde_json::to_string(&policy_document_json(statements)?)
        .into_alien_error()
        .context(ErrorData::TemplateSerializationFailed {
            format: "Terraform IAM policy".to_string(),
            reason: "Failed to serialize IAM policy".to_string(),
        })
        .map(|policy| {
            // Prefix validation permits 40 characters. Keep template overhead
            // as headroom, but account for growth from shorter placeholders.
            let token = "${local.resource_prefix}";
            let mut size = policy.len() + policy.matches(token).count() * (40 - token.len());
            // Boundary attributes resolve to an ARN containing the maximum prefix.
            let boundary_arn_len = "arn:aws:iam::123456789012:policy/".len()
                + 40
                + "-deployment-management-boundary".len();
            for reference in policy.split("${aws_iam_policy.").skip(1) {
                if let Some((reference, _)) = reference.split_once('}') {
                    if reference.ends_with("_permissions_boundary.arn") {
                        let token_len = "${aws_iam_policy.".len() + reference.len() + 1;
                        size += boundary_arn_len.saturating_sub(token_len);
                    }
                }
            }
            size
        })
}

pub(crate) fn policy_document_expr(statements: Vec<AwsIamStatement>) -> Result<Expression> {
    Ok(jsonencode_policy_value(policy_document_json(&statements)?))
}

fn policy_document_json(statements: &[AwsIamStatement]) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "Version": "2012-10-17",
        "Statement": statements,
    }))
}

/// VPC-config private subnet ids expression.
pub fn private_subnet_ids_expr(ctx: &EmitContext<'_>) -> Expression {
    let Some((label, network)) = default_network(ctx) else {
        return expr::raw("[]");
    };
    match &network.settings {
        NetworkSettings::Create { .. } if stack_has_setup_postgres(ctx) => expr::raw(format!(
            "var.network_mode == \"create-new\" ? aws_subnet.{label}_private[*].id : var.network_mode == \"use-existing\" ? compact([for id in var.private_subnet_ids : trimspace(id)]) : data.aws_subnets.{label}_default[0].ids"
        )),
        NetworkSettings::Create { .. } => expr::raw(format!(
            "var.network_mode == \"create-new\" ? aws_subnet.{label}_private[*].id : var.network_mode == \"use-existing\" ? compact([for id in var.private_subnet_ids : trimspace(id)]) : []"
        )),
        NetworkSettings::UseDefault => {
            expr::raw(format!("data.aws_subnets.{label}_default.ids"))
        }
        NetworkSettings::ByoVpcAws { .. } => expr::raw(format!("compact([for id in var.{label}_private_subnet_ids : trimspace(id)])")),
        _ => expr::raw("[]"),
    }
}

/// VPC-config security-group ids expression.
pub fn security_group_ids_expr(ctx: &EmitContext<'_>) -> Expression {
    let Some((label, network)) = default_network(ctx) else {
        return expr::raw("[]");
    };
    match &network.settings {
        NetworkSettings::Create { .. } => expr::raw(format!(
            "var.network_mode == \"create-new\" ? [aws_security_group.{label}_workload[0].id] : var.network_mode == \"use-existing\" ? compact([for id in var.security_group_ids : trimspace(id)]) : []"
        )),
        NetworkSettings::ByoVpcAws { .. } => expr::raw(format!("compact([for id in var.{label}_security_group_ids : trimspace(id)])")),
        _ => expr::raw("[]"),
    }
}

/// VPC ID expression: created VPC when this stack creates the VPC, existing
/// variable otherwise.
pub fn vpc_id_expr(ctx: &EmitContext<'_>) -> Expression {
    let Some((label, network)) = default_network(ctx) else {
        return expr::raw("null");
    };
    match &network.settings {
        NetworkSettings::Create { .. } if stack_has_setup_postgres(ctx) => expr::raw(format!(
            "var.network_mode == \"create-new\" ? aws_vpc.{label}[0].id : var.network_mode == \"use-existing\" ? var.vpc_id : data.aws_vpc.{label}_default[0].id"
        )),
        NetworkSettings::Create { .. } => expr::raw(format!(
            "var.network_mode == \"create-new\" ? aws_vpc.{label}[0].id : var.network_mode == \"use-existing\" ? var.vpc_id : null"
        )),
        NetworkSettings::UseDefault => expr::raw(format!("data.aws_vpc.{label}_default.id")),
        NetworkSettings::ByoVpcAws { .. } => expr::raw(format!("var.{label}_vpc_id")),
        _ => expr::raw("null"),
    }
}

fn stack_has_setup_postgres(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.resources().any(|(_id, entry)| {
        entry.lifecycle == alien_core::ResourceLifecycle::Frozen
            && entry
                .config
                .downcast_ref::<alien_core::Postgres>()
                .is_some()
    })
}

/// Build an IAM `aws_iam_role` resource block with an inline assume role
/// policy. Inline policy attached separately via `inline_policy_block`
/// nested block.
pub fn iam_role_block(
    role_label: &str,
    role_name: Expression,
    assume_role_policy: Expression,
    tags: Expression,
) -> Block {
    resource_block(
        "aws_iam_role",
        role_label,
        [
            attr("name", role_name),
            attr("assume_role_policy", assume_role_policy),
            attr("tags", tags),
        ],
    )
}

/// Build an `aws_iam_role_policy` resource block attaching `policy`
/// (typically the result of `policy_attr`) to `role_label`.
pub fn iam_role_policy_block(
    policy_label: &str,
    role_label: &str,
    policy_name: &str,
    statements: Vec<Expression>,
) -> Block {
    resource_block(
        "aws_iam_role_policy",
        policy_label,
        [
            attr("name", Expression::String(policy_name.to_string())),
            attr("role", expr::traversal(["aws_iam_role", role_label, "id"])),
            policy_attr(statements),
        ],
    )
}

/// IAM `aws_iam_role_policy_attachment` for a managed policy.
pub fn iam_role_managed_policy_attachment(
    label: &str,
    role_label: &str,
    policy_arn: Expression,
) -> Block {
    resource_block(
        "aws_iam_role_policy_attachment",
        label,
        [
            attr(
                "role",
                expr::traversal(["aws_iam_role", role_label, "name"]),
            ),
            attr("policy_arn", policy_arn),
        ],
    )
}

/// Helper to build the inner `block { ... }` shape used by sub-blocks
/// like `versioning_configuration`, `rule`, etc. Returns a `Structure`
/// directly so callers can pass into block bodies.
pub fn nested_block(name: &str, body: Vec<Structure>) -> Structure {
    nested(block(name, body))
}

#[cfg(test)]
#[path = "helpers_policy_tests.rs"]
mod policy_tests;
