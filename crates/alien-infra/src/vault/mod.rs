use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{PermissionSet, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};

/// Hash the resolved grants owned by this vault, including wildcard grants.
/// GCP consumer grants belong to service accounts; its vault owns management grants.
fn permissions_revision(ctx: &ResourceControllerContext<'_>) -> Result<String> {
    let resource_id = ctx.desired_config.id();
    let mut profiles = Vec::new();
    if ctx.platform != Platform::Gcp {
        profiles.extend(
            ctx.desired_stack
                .permissions
                .profiles
                .iter()
                .map(|(name, profile)| (Some(name.as_str()), profile)),
        );
    }
    if let Some(profile) = ctx.desired_stack.management().profile() {
        profiles.push((None, profile));
    }
    let mut resolved: Vec<(Option<&str>, Vec<PermissionSet>)> = Vec::new();
    for (name, profile) in profiles {
        let references = profile.0.iter().flat_map(|(scope, refs)| {
            refs.iter().filter(move |reference| {
                scope == resource_id || (scope == "*" && reference.id().starts_with("vault/"))
            })
        });
        let mut sets = Vec::new();
        for reference in references {
            sets.push(
                reference
                    .resolve(|id| alien_permissions::get_permission_set(id).cloned())
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!("Vault permission set '{}' not found", reference.id()),
                            resource_id: Some(resource_id.to_string()),
                        })
                    })?,
            );
        }
        if !sets.is_empty() {
            resolved.push((name, sets));
        }
    }
    let bytes = serde_json::to_vec(&resolved).into_alien_error().context(
        ErrorData::InfrastructureError {
            message: "Failed to serialize vault permissions".to_string(),
            operation: Some("vault_permissions_revision".to_string()),
            resource_id: Some(resource_id.to_string()),
        },
    )?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

mod aws;
pub use aws::*;

mod aws_import;
pub use aws_import::AwsVaultImporter;

mod gcp;
pub use gcp::*;

mod gcp_import;
pub use gcp_import::GcpVaultImporter;

mod azure;
pub use azure::*;

mod azure_import;
pub use azure_import::AzureVaultImporter;

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;

#[cfg(feature = "kubernetes")]
mod kubernetes;
#[cfg(feature = "kubernetes")]
pub use kubernetes::*;

#[cfg(feature = "test")]
mod test;
#[cfg(feature = "test")]
pub use test::*;
