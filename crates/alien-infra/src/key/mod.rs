mod aws;
mod azure;
mod gcp;

pub use aws::{AwsKeyController, AwsKeyImporter};
pub use azure::{AzureKeyController, AzureKeyImporter};
pub use gcp::{GcpKeyController, GcpKeyImporter};

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

/// The HTTP status and provider error code of the response a failed key call got.
///
/// `None` when no response came back (a transport failure or timeout).
fn cloud_response(error: &alien_client_core::Error) -> Option<(u16, String)> {
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
        Some((status, code))
    })
}

/// Whether AWS KMS answered that the key is gone or that its policy denies us.
///
/// A BYOK customer revokes access by removing our principal from the key policy
/// (`AccessDeniedException`) or by deleting the key (`NotFoundException`).
/// Throttling, `KMSInternalException`, `DependencyTimeoutException`, credential
/// errors and transport failures say nothing about the key.
fn aws_key_revoked(error: &alien_client_core::Error) -> bool {
    cloud_response(error).is_some_and(|(_, code)| {
        matches!(code.as_str(), "AccessDeniedException" | "NotFoundException")
    })
}

/// Whether Cloud KMS or Key Vault answered that the key is gone (404) or that
/// access to it is denied (403: an IAM, RBAC, access policy or vault firewall
/// decision made by the key's owner).
///
/// 401, 429, 5xx, timeouts and transport failures say nothing about the key.
fn key_revoked_by_status(error: &alien_client_core::Error) -> bool {
    cloud_response(error).is_some_and(|(status, _)| matches!(status, 403 | 404))
}

#[cfg(all(test, feature = "test"))]
mod tests;
