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
