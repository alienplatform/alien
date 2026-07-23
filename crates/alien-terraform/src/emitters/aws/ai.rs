//! AWS AI — Bedrock inference gateway.
//!
//! AWS Bedrock is a regional, account-scoped service with no per-stack
//! cloud resource to provision. The emitter returns an empty fragment for
//! the resource itself and emits any resource-scoped `aws_iam_role_policy`
//! blocks for permission profiles that reference an `ai/*` set (e.g.
//! `ai/invoke`, or `ai/finetune` when the resource declares a fine-tuning
//! job) on this resource. The region is carried in the import ref so the
//! controller can reconstruct the Bedrock endpoint without a cloud round-trip.
//!
//! Stack-level permissions flow through `AwsServiceAccountEmitter` via
//! `stack_permission_sets`; resource-scoped grants are emitted here.

use crate::{
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
        iam_policy_name_sanitize, required_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, Ai, PermissionProfile, PermissionSetReference, Result, ServiceAccount,
};
use alien_permissions::BindingTarget;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsAiEmitter;

impl TfEmitter for AwsAiEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let ai = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let mut fragment = TfFragment::empty();

        let context = aws_terraform_permission_context()
            .with_resource_name(format!("${{local.resource_prefix}}-{}", ai.id()));

        for (owner_label, permission_set_refs) in ai_permission_owners(ctx) {
            for (idx, permission_set_ref) in permission_set_refs.iter().enumerate() {
                if let Some(permission_set) = permission_set_ref
                    .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                {
                    let ai_label_segment = sanitize_label_segment(ai.id());
                    emit_iam_role_policy_for_target_with_label(
                        &mut fragment,
                        &owner_label,
                        &permission_set,
                        &format!("{owner_label}_ai_{ai_label_segment}_set_{idx}"),
                        &format!(
                            "access-{}-{}",
                            ai.id(),
                            iam_policy_name_sanitize(&permission_set.id)
                        ),
                        &context,
                        BindingTarget::Resource,
                    )?;
                }
            }
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let _ = downcast::<Ai>(ctx, Ai::RESOURCE_TYPE)?;
        let _ = required_label(ctx)?;
        Ok(expr::object([("region", expr::raw("data.aws_region.current.region"))]))
    }
}

fn ai_permission_owners(ctx: &EmitContext<'_>) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = ai_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        let service_account_id = format!("{profile_name}-sa");
        if let Some(label) = service_account_label(ctx, &service_account_id) {
            owners.push((label.to_string(), refs));
        }
    }
    owners
}

fn ai_permission_refs(profile: &PermissionProfile, resource_id: &str) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }
    refs
}

fn service_account_label<'a>(ctx: &'a EmitContext<'_>, service_account_id: &str) -> Option<&'a str> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    ctx.name_for(service_account_id)
}

fn sanitize_label_segment(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
