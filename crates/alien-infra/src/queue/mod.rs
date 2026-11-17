#[cfg(feature = "aws")]
pub mod aws;

#[cfg(feature = "gcp")]
pub mod gcp;
#[cfg(feature = "gcp")]
pub(crate) mod gcp_dead_letter;

#[cfg(feature = "azure")]
pub mod azure;

#[cfg(feature = "aws")]
mod aws_import;
#[cfg(feature = "aws")]
pub use aws_import::AwsQueueImporter;

#[cfg(feature = "gcp")]
mod gcp_import;
#[cfg(feature = "gcp")]
pub use gcp_import::GcpQueueImporter;

#[cfg(feature = "azure")]
mod azure_import;
#[cfg(feature = "azure")]
pub use azure_import::AzureQueueImporter;

#[cfg(feature = "local")]
pub mod local;

/// Whether this run may change a queue's settings after it exists. A setup-owned (Frozen) queue
/// is changed only by a direct setup run; anything else asks for setup to be rerun.
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
pub(crate) fn may_change_queue_settings(
    ctx: &crate::core::ResourceControllerContext<'_>,
    id: &str,
) -> bool {
    use alien_core::{InitialSetupAuthority, ResourceLifecycle};
    ctx.initial_setup_authority == InitialSetupAuthority::DirectSetup
        || !ctx
            .desired_stack
            .resources
            .get(id)
            .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen)
}
