//! Customer-managed encryption keys for GCP data stores (Cloud SQL, Memorystore).
//!
//! The stack's `Key` is a Cloud KMS crypto key created by setup in the deployment's project.
//! A store encrypts its data with it once the store's Google-managed service agent may use the
//! key. Disabling the key version then stops the store, which is the kill switch.

use alien_core::{import::EmitContext, ErrorData, Key, ResourceRef, Result};
use alien_error::AlienError;
use crate::block::{attr, resource_block};
use crate::{expr, TfFragment};
use hcl::expr::Expression;

/// The role a service agent needs to wrap and unwrap data keys with a Cloud KMS key.
const ENCRYPTER_DECRYPTER: &str = "roles/cloudkms.cryptoKeyEncrypterDecrypter";

/// What a store needs from its customer key.
pub(crate) struct StoreKey {
    /// `google_kms_crypto_key.<label>.id`, the full crypto key resource name.
    pub name: Expression,
    /// The IAM grant the store must wait for before it is created.
    pub grant_dependency: String,
}

/// Resolves `key` to its crypto key and grants `service` (for example `sqladmin.googleapis.com`)
/// access to it. A project has one service agent per service and a non-authoritative grant is
/// removed with whichever copy is destroyed, so the agent and the grant exist once per key and
/// service. Every store using that pair emits the identical shared blocks (the generator
/// keeps one) and depends on the same grant. The grant outlives the store on uninstall; the
/// caller keeps it with [`retain_store_key`].
pub(crate) fn grant_store_key(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    key: &ResourceRef,
    owner_label: &str,
    service: &str,
) -> Result<StoreKey> {
    let key_label = ctx.name_for(key.id()).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("missing encryption Key '{}' for '{owner_label}'", key.id()),
        })
    })?;
    debug_assert_eq!(key.resource_type, Key::RESOURCE_TYPE);
    let service_short = service.split('.').next().unwrap_or(service);
    let identity_label = format!("{key_label}_{service_short}_service_agent");
    // Creates the service agent when the service has not been used in the project yet.
    fragment.push_shared_resource(resource_block(
        "google_project_service_identity",
        &identity_label,
        [
            // Service agents exist only in the google-beta provider.
            attr("provider", expr::raw("google-beta")),
            attr("project", expr::raw("var.gcp_project")),
            attr("service", Expression::String(service.to_string())),
        ],
    ));
    let grant_label = format!("{key_label}_{service_short}_key_access");
    fragment.push_shared_resource(resource_block(
        "google_kms_crypto_key_iam_member",
        &grant_label,
        [
            attr(
                "crypto_key_id",
                expr::traversal(["google_kms_crypto_key", key_label, "id"]),
            ),
            attr("role", Expression::String(ENCRYPTER_DECRYPTER.to_string())),
            attr(
                "member",
                expr::template(format!(
                    "serviceAccount:${{google_project_service_identity.{identity_label}.email}}"
                )),
            ),
        ],
    ));
    Ok(StoreKey {
        name: expr::traversal(["google_kms_crypto_key", key_label, "id"]),
        grant_dependency: format!("google_kms_crypto_key_iam_member.{grant_label}"),
    })
}

/// Whether `kind` is the key grant [`grant_store_key`] emits. Uninstall leaves the stores, so it
/// must leave the grants they need to stay readable with the key.
pub(crate) fn is_store_key_grant(kind: &str) -> bool {
    kind == "google_kms_crypto_key_iam_member"
}
