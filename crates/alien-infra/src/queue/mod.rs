#[cfg(feature = "aws")]
pub mod aws;

#[cfg(feature = "gcp")]
pub mod gcp;

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
