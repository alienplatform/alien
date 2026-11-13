use crate::{
    block::{attr, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, labels, required_label, service_account_member_for_label},
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, Key, RemoteBindings, RemoteStackManagement, Result,
};
use alien_error::AlienError;
use alien_permissions::key_access::{cryptography_service_accounts, key_access_plan};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpKeyEmitter;

impl TfEmitter for GcpKeyEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let key = downcast::<Key>(ctx, Key::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let plan = key_access_plan(ctx.stack, ctx.resource_id).map_err(|error| {
            AlienError::new(ErrorData::GenericError {
                message: format!("Failed to work out the key's access plan: {error}"),
            })
        })?;
        let ring_label = format!("{label}_ring");
        let ring_suffix_label = format!("{label}_ring_suffix");
        let mut fragment = TfFragment::default()
            .with_resource(resource_block(
                "random_id",
                &ring_suffix_label,
                [attr(
                    "byte_length",
                    Expression::Number(hcl::Number::from(3i64)),
                )],
            ))
            .with_resource(resource_block(
                "google_kms_key_ring",
                &ring_label,
                [
                    attr("project", expr::raw("var.gcp_project")),
                    attr(
                        "name",
                        expr::raw(format!(
                            "format(\"%s-%s\", trim(substr(replace(lower(format(\"%s-{}\", local.resource_prefix)), \"_\", \"-\"), 0, 56), \"-\"), random_id.{ring_suffix_label}.hex)",
                            key.id()
                        )),
                    ),
                    attr("location", expr::raw("var.gcp_region")),
                ],
            ))
            .with_resource(resource_block(
                "google_kms_crypto_key",
                label,
                [
                    attr("name", Expression::String("key".to_string())),
                    attr(
                        "key_ring",
                        expr::traversal(["google_kms_key_ring", ring_label.as_str(), "id"]),
                    ),
                    attr("purpose", Expression::String("ENCRYPT_DECRYPT".to_string())),
                    attr(
                        "rotation_period",
                        Expression::String("7776000s".to_string()),
                    ),
                    attr("labels", labels(ctx, "key")),
                    nested(crate::block::block(
                        "lifecycle",
                        [attr("prevent_destroy", Expression::Bool(true))],
                    )),
                ],
            ));

        if let Some(access_label) = remote_bindings_label(ctx) {
            let role = gcp_remote_role(&plan)?;
            fragment.resource_blocks.push(resource_block(
                "google_kms_crypto_key_iam_member",
                &format!("{label}_remote_cryptography"),
                [
                    attr(
                        "crypto_key_id",
                        expr::traversal(["google_kms_crypto_key", label, "id"]),
                    ),
                    attr("role", Expression::String(role)),
                    attr(
                        "member",
                        expr::template(format!(
                            "serviceAccount:${{google_service_account.{access_label}.email}}"
                        )),
                    ),
                ],
            ));
        }

        for account_id in cryptography_service_accounts(ctx.stack, ctx.resource_id) {
            let Some(account_label) = ctx.name_for(&account_id) else {
                continue;
            };
            fragment.resource_blocks.push(resource_block(
                "google_kms_crypto_key_iam_member",
                &format!("{label}_{account_label}_cryptography"),
                [
                    attr(
                        "crypto_key_id",
                        expr::traversal(["google_kms_crypto_key", label, "id"]),
                    ),
                    attr("role", Expression::String(gcp_remote_role(&plan)?)),
                    attr("member", service_account_member_for_label(account_label)),
                ],
            ));
        }

        // A Container volume encrypted with this key is created at run time by Compute Engine,
        // which wraps the disk's data key as its own service agent. Setup grants that agent the
        // key because the runtime never edits key policy.
        if volumes_use_key(ctx) {
            // The agent is `service-<project number>@compute-system.iam.gserviceaccount.com`.
            // `google_project_service_identity` cannot name it: for Compute it returns a null
            // email, which `terraform validate` accepts and only `apply` rejects. It still
            // creates the agent, so the grant waits for it: a project that never used Compute
            // has no agent until then, and Google rejects a grant to a missing account.
            let agent_label = format!("{label}_compute_agent");
            fragment.resource_blocks.push(resource_block(
                "google_project_service_identity",
                &agent_label,
                [
                    // Service agents exist only in the google-beta provider.
                    attr("provider", expr::raw("google-beta")),
                    attr("project", expr::raw("var.gcp_project")),
                    attr(
                        "service",
                        Expression::String("compute.googleapis.com".to_string()),
                    ),
                ],
            ));
            let project_label = format!("{label}_project");
            fragment.data_blocks.push(data_block(
                "google_project",
                &project_label,
                [attr("project_id", expr::raw("var.gcp_project"))],
            ));
            fragment.resource_blocks.push(resource_block(
                "google_kms_crypto_key_iam_member",
                &format!("{label}_compute_disks"),
                [
                    attr(
                        "crypto_key_id",
                        expr::traversal(["google_kms_crypto_key", label, "id"]),
                    ),
                    attr(
                        "role",
                        Expression::String(
                            "roles/cloudkms.cryptoKeyEncrypterDecrypter".to_string(),
                        ),
                    ),
                    attr(
                        "member",
                        expr::template(format!(
                            "serviceAccount:service-${{data.google_project.{project_label}.number}}@compute-system.iam.gserviceaccount.com"
                        )),
                    ),
                    attr(
                        "depends_on",
                        expr::raw(format!("[google_project_service_identity.{agent_label}]")),
                    ),
                ],
            ));
        }

        if let Some(management_label) = management_label(ctx) {
            fragment.resource_blocks.push(resource_block(
                "google_kms_crypto_key_iam_member",
                &format!("{label}_management"),
                [
                    attr(
                        "crypto_key_id",
                        expr::traversal(["google_kms_crypto_key", label, "id"]),
                    ),
                    attr(
                        "role",
                        Expression::String("roles/cloudkms.viewer".to_string()),
                    ),
                    attr("member", service_account_member_for_label(management_label)),
                ],
            ));
        }

        fragment.retain_resources(|kind, _| {
            matches!(kind, "google_kms_crypto_key" | "google_kms_key_ring")
        });
        fragment.retain_state(|kind, _| kind == "random_id");
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "cryptoKeyName",
                expr::traversal(["google_kms_crypto_key", label, "id"]),
            ),
            (
                "primaryVersion",
                expr::raw(format!("google_kms_crypto_key.{label}.primary[0].name")),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("cloud-kms".to_string())),
            (
                "cryptoKeyName",
                expr::traversal(["google_kms_crypto_key", label, "id"]),
            ),
        ])))
    }
}

fn gcp_remote_role(plan: &alien_permissions::key_access::KeyAccessPlan) -> Result<String> {
    alien_permissions::key_access::gcp_key_roles(plan)
        .map(|roles| roles.cryptography)
        .map_err(|error| {
            AlienError::new(ErrorData::GenericError {
                message: format!("key/remote-cryptography has no GCP role: {error}"),
            })
        })
}

fn remote_bindings_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    if alien_core::remote_bindings::remote_binding_for_entry(ctx.resource).is_none() {
        return None;
    }
    ctx.stack.resources().find_map(|(id, entry)| {
        (entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    })
}

fn management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    let has_permission = ctx
        .stack
        .management()
        .profile()
        .and_then(|profile| profile.0.get(ctx.resource_id))
        .is_some_and(|refs| {
            refs.iter()
                .any(|reference| reference.id() == "key/management")
        });
    has_permission.then(|| {
        ctx.stack.resources().find_map(|(id, entry)| {
            (entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
                .then(|| ctx.name_for(id))
                .flatten()
        })
    })?
}

/// Whether a Container volume in the stack is encrypted with this key.
fn volumes_use_key(ctx: &EmitContext<'_>) -> bool {
    ctx.stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<alien_core::Container>()
            .and_then(|container| container.persistent_storage.as_ref())
            .and_then(|storage| storage.encryption_key.as_ref())
            .is_some_and(|key| key.id() == ctx.resource_id)
    })
}
