mod aws;
mod azure;
mod gcp;

pub use aws::{AwsKeyController, AwsKeyImporter};
pub use azure::{AzureKeyController, AzureKeyImporter};
pub use gcp::{GcpKeyController, GcpKeyImporter};

/// Who made the cloud key a controller tracks.
///
/// Every state saved before controllers could create keys is an import, so `Imported` is the
/// default. A controller writes to a key only when it created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum KeyOrigin {
    /// The setup template (CloudFormation or Terraform) made the key and owns it.
    #[default]
    Imported,
    /// Alien made the key during a direct setup run.
    Created,
}

/// Whether this step may write to the cloud key: only a direct setup run, which has the admin's
/// credentials. After setup the manager steps key controllers with management credentials, and
/// the management identity never creates, changes the policy of, disables or deletes a key.
pub(crate) fn may_write_keys(ctx: &crate::core::ResourceControllerContext<'_>) -> bool {
    ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
        && ctx.deployment_config.setup_run
}

/// The refusal for a key that was not made in this deployment's setup run.
pub(crate) fn key_created_by_setup(
    key_id: &str,
) -> alien_error::AlienError<crate::error::ErrorData> {
    alien_error::AlienError::new(crate::error::ErrorData::ResourceConfigInvalid {
        resource_id: Some(key_id.to_string()),
        message: format!(
            "Key '{key_id}' is created by initial setup. Rerun setup (`alien deploy`, CloudFormation or Terraform) to add it."
        ),
    })
}

/// Hex digest of what a created key's access depends on, to see when a rerun must rewrite it.
pub(crate) fn access_revision(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn merge_key_reimport(
    existing: alien_core::StackResourceState,
    imported: alien_core::StackResourceState,
    ctx: &alien_core::import::ImportContext<'_>,
) -> alien_core::Result<alien_core::StackResourceState> {
    if let Some(previous) = existing
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<alien_core::KeyOutputs>())
    {
        let current = imported
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<alien_core::KeyOutputs>());
        if current.map(|outputs| &outputs.fingerprint) != Some(&previous.fingerprint) {
            return Err(alien_error::AlienError::new(alien_core::ErrorData::InvalidResourceUpdate {
                resource_id: ctx.resource_id.to_string(),
                reason: "The encryption key identity is immutable; create a new deployment to use a different key".to_string(),
            }));
        }
    }
    Ok(imported)
}

/// The HTTP status, provider error code and URL of the response a failed key call got.
///
/// `None` when no response came back (a transport failure or timeout).
fn cloud_response(error: &alien_client_core::Error) -> Option<(u16, String, String)> {
    let contexts = std::iter::once(error.context.as_ref()).chain(
        std::iter::successors(error.source.as_deref(), |source| source.source.as_deref())
            .map(|source| source.context.as_ref()),
    );
    contexts.flatten().find_map(|context| {
        let status = u16::try_from(context.get("http_status")?.as_u64()?).ok()?;
        let code = context
            .get("http_response_text")
            .and_then(serde_json::Value::as_str)
            .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
            .and_then(|body| {
                body.get("__type")
                    .or_else(|| body.get("code"))
                    .and_then(serde_json::Value::as_str)
                    .map(|code| code.rsplit('#').next().unwrap_or(code).to_string())
            })
            .unwrap_or_default();
        let url = context
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        Some((status, code, url))
    })
}

/// Whether AWS KMS answered that the key is gone or that its policy denies us.
///
/// A BYOK customer revokes access by removing our principal from the key policy
/// (`AccessDeniedException`) or by deleting the key (`NotFoundException`).
/// Throttling, `KMSInternalException`, `DependencyTimeoutException`, credential
/// errors (`UnrecognizedClientException`, or STS's XML `AccessDenied` when a
/// role assumption fails) and transport failures say nothing about the key.
fn aws_key_revoked(error: &alien_client_core::Error) -> bool {
    cloud_response(error).is_some_and(|(_, code, _)| {
        matches!(code.as_str(), "AccessDeniedException" | "NotFoundException")
    })
}

/// Whether Cloud KMS or Key Vault answered the request for `key` that the key
/// is gone (404) or that access to it is denied (403: an IAM, RBAC, access
/// policy or vault firewall decision made by the key's owner).
///
/// The response must come from the key's own URL: a 403 from fetching our
/// token (service account impersonation, Entra ID) is about our credentials.
/// 401, 429, 5xx, timeouts and transport failures say nothing about the key.
fn key_revoked_by_status(error: &alien_client_core::Error, key: &str) -> bool {
    cloud_response(error)
        .is_some_and(|(status, _, url)| matches!(status, 403 | 404) && url.contains(key))
}

#[cfg(all(test, feature = "test"))]
mod tests;
