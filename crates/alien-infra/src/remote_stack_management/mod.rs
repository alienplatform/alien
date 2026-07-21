mod aws;
pub use aws::*;
mod aws_remote_storage;

mod aws_import;
pub use aws_import::AwsRemoteStackManagementImporter;

mod gcp;
pub use gcp::*;

mod gcp_remote_storage;

mod gcp_import;
pub use gcp_import::GcpRemoteStackManagementImporter;

mod azure;
pub use azure::*;
mod azure_remote_storage;

mod azure_import;
pub use azure_import::AzureRemoteStackManagementImporter;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use sha2::{Digest, Sha256};

/// Stable fingerprint of every input that changes the management identity's
/// effective grants. Controllers persist this only after a successful cloud
/// reconciliation and use it to schedule later updates for deployment-level
/// changes that are not part of `RemoteStackManagement`'s resource config.
fn desired_management_grant_fingerprint(
    ctx: &ResourceControllerContext<'_>,
    remote_storage_scopes: &[String],
) -> Result<String> {
    let mut remote_storage_scopes = remote_storage_scopes.to_vec();
    remote_storage_scopes.sort_unstable();
    remote_storage_scopes.dedup();

    let projection = (
        &ctx.desired_stack.permissions.management,
        remote_storage_scopes,
    );
    let encoded = serde_json::to_vec(&projection).into_alien_error().context(
        ErrorData::ResourceStateSerializationFailed {
            resource_id: "remote-stack-management".to_string(),
            message: "Failed to fingerprint desired management grants".to_string(),
        },
    )?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

/// Remote Bindings v0 is deliberately limited to Storage created by setup.
/// An external binding only imports a caller-supplied resource reference; it
/// does not prove that Alien setup owns that resource or may grant the
/// deployment management identity access to its contents.
fn ensure_setup_owned_remote_storage(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<()> {
    if ctx.deployment_config.external_bindings.has(resource_id) {
        return Err(alien_error::AlienError::new(
            ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Remote Storage resource '{resource_id}' cannot use an external binding; remote access is limited to resources created by setup"
                ),
                resource_id: Some(resource_id.to_string()),
            },
        ));
    }
    Ok(())
}

#[cfg(feature = "test")]
mod test;
#[cfg(feature = "test")]
pub use test::*;
