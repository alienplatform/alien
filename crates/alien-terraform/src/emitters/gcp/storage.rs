//! GCP Storage — Cloud Storage bucket with uniform bucket-level access.
//!
//! Defaults closed by design: uniform bucket-level access (no ACLs),
//! public access prevention enforced, soft-delete versioning when
//! requested, lifecycle rules translated to GCP `lifecycle_rule`
//! blocks.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::enabled,
    emitters::gcp::helpers::{
        binding_label_for_role, downcast, emit_custom_roles_for_bindings, labels,
        permission_context, required_label, resource_prefix_template, role_expression_for_binding,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{
    import::EmitContext, LifecycleRule, PermissionProfile, PermissionSetReference,
    RemoteStackManagement, Result, ServiceAccount, Storage,
};
use alien_error::Context;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpStorageEmitter;

impl TfEmitter for GcpStorageEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let storage = downcast::<Storage>(ctx, Storage::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let enabled_when = ctx.resource.enabled_when.as_deref();
        let mut fragment = TfFragment::default();

        fragment
            .resource_blocks
            .push(bucket(label, ctx, storage, enabled_when)?);

        if storage.public_read {
            fragment
                .resource_blocks
                .push(public_iam_binding(label, enabled_when)?);
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
                enabled::attribute(enabled_when, "google_storage_bucket", label, "name"),
            ),
            (
                "bucketSelfLink",
                enabled::attribute(enabled_when, "google_storage_bucket", label, "self_link"),
            ),
            ("projectId", expr::raw("var.gcp_project")),
            (
                "location",
                enabled::attribute(enabled_when, "google_storage_bucket", label, "location"),
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
            ("service", Expression::String("gcs".to_string())),
            (
                "bucketName",
                enabled::attribute(enabled_when, "google_storage_bucket", label, "name"),
            ),
        ])))
    }
}

fn bucket(
    label: &str,
    ctx: &EmitContext<'_>,
    storage: &Storage,
    enabled_when: Option<&str>,
) -> Result<hcl::structure::Block> {
    let mut body: Vec<hcl::structure::Structure> = vec![
        attr("name", resource_prefix_template(storage.id())),
        attr("project", expr::raw("var.gcp_project")),
        attr("location", expr::raw("upper(var.gcp_region)")),
        attr("storage_class", Expression::String("STANDARD".to_string())),
        attr("uniform_bucket_level_access", Expression::Bool(true)),
        attr("force_destroy", Expression::Bool(true)),
        attr(
            "public_access_prevention",
            Expression::String(if storage.public_read {
                "inherited".to_string()
            } else {
                "enforced".to_string()
            }),
        ),
        attr("labels", labels(ctx, "storage")),
    ];

    if storage.versioning {
        body.push(nested(block(
            "versioning",
            [attr("enabled", Expression::Bool(true))],
        )));
    }

    for rule in &storage.lifecycle_rules {
        body.push(nested(lifecycle_rule_block(rule)));
    }

    let mut bucket = resource_block("google_storage_bucket", label, body);
    enabled::gate(&mut bucket, enabled_when)?;
    Ok(bucket)
}

fn lifecycle_rule_block(rule: &LifecycleRule) -> hcl::structure::Block {
    let mut condition_attrs: Vec<hcl::structure::Structure> = vec![attr(
        "age",
        Expression::Number(hcl::Number::from(i64::from(rule.days))),
    )];
    if let Some(prefix) = &rule.prefix {
        condition_attrs.push(attr(
            "matches_prefix",
            Expression::Array(vec![Expression::String(prefix.clone())]),
        ));
    }
    block(
        "lifecycle_rule",
        [
            nested(block(
                "action",
                [attr("type", Expression::String("Delete".to_string()))],
            )),
            nested(block("condition", condition_attrs)),
        ],
    )
}

fn public_iam_binding(label: &str, enabled_when: Option<&str>) -> Result<hcl::structure::Block> {
    let mut binding = resource_block(
        "google_storage_bucket_iam_member",
        &format!("{label}_public_read"),
        [
            attr("bucket", bucket_name(label, enabled_when)),
            attr(
                "role",
                Expression::String("roles/storage.objectViewer".to_string()),
            ),
            attr("member", Expression::String("allUsers".to_string())),
        ],
    );
    enabled::gate(&mut binding, enabled_when)?;
    Ok(binding)
}

fn bucket_name(label: &str, enabled_when: Option<&str>) -> Expression {
    enabled::attribute(enabled_when, "google_storage_bucket", label, "name")
}

fn emit_storage_iam(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    label: &str,
    enabled_when: Option<&str>,
) -> Result<()> {
    for (owner_label, permission_refs) in storage_permission_owners(ctx) {
        let member = service_account_member_for_label(&owner_label);
        let bucket_ref = match enabled_when {
            Some(_) => format!("${{google_storage_bucket.{label}[0].name}}"),
            None => format!("${{google_storage_bucket.{label}.name}}"),
        };
        let context =
            permission_context(&owner_label, ctx.stack.id()).with_resource_name(bucket_ref);
        let generator = GcpRuntimePermissionsGenerator::new();

        for permission_ref in permission_refs {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("storage/") {
                continue;
            }

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, &context)
                .context(alien_core::ErrorData::GenericError {
                    message: format!(
                        "failed to generate GCP storage IAM grant plan for '{}'",
                        permission_set.id
                    ),
                })?;
            let bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
            // Custom roles carry their own `var.gcp_manage_custom_roles` count
            // and are referenced through it, so the deployer's gate stays off
            // them: a second `count` on one block is not renderable. A declined
            // bucket then leaves a role definition that grants nothing, which is
            // harmless.
            let custom_roles = emit_custom_roles_for_bindings(fragment, &grant_plan, &bindings)?;

            for (idx, binding) in bindings.into_iter().enumerate() {
                let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
                let role = role_expression_for_binding(&binding.role, &custom_roles)?;
                let mut body = vec![
                    attr("bucket", bucket_name(label, enabled_when)),
                    attr("role", role),
                    attr("member", member.clone()),
                ];
                if let Some(condition) = binding.condition {
                    body.push(nested(block(
                        "condition",
                        [
                            attr("title", Expression::String(condition.title)),
                            attr("description", Expression::String(condition.description)),
                            attr("expression", expr::template(condition.expression)),
                        ],
                    )));
                }
                let mut member_block = resource_block(
                    "google_storage_bucket_iam_member",
                    &format!("{role_label}_{label}_{owner_label}_storage_{idx}"),
                    body,
                );
                enabled::gate(&mut member_block, enabled_when)?;
                fragment.resource_blocks.push(member_block);
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
