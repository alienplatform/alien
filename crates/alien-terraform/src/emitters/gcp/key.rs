use crate::{
    block::{attr, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gcp::helpers::{downcast, labels, required_label, service_account_member_for_label},
    expr,
};
use alien_core::{
    import::EmitContext, ErrorData, Key, RemoteBindings, RemoteStackManagement, Result,
};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpKeyEmitter;

impl TfEmitter for GcpKeyEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let key = downcast::<Key>(ctx, Key::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
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
            let role = gcp_remote_role()?;
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

fn gcp_remote_role() -> Result<String> {
    let permission_set = alien_permissions::get_permission_set("key/remote-cryptography")
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "key/remote-cryptography permission set is not registered".to_string(),
            })
        })?;
    let roles = permission_set
        .platforms
        .gcp
        .as_ref()
        .and_then(|entries| entries.first())
        .and_then(|entry| entry.grant.predefined_roles.as_ref())
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "key/remote-cryptography has no GCP predefined role".to_string(),
            })
        })?;
    roles.first().cloned().ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: "key/remote-cryptography has an empty GCP role list".to_string(),
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
