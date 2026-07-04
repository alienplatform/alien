#[cfg(feature = "aws")]
pub mod aws_iam;
#[cfg(feature = "azure")]
pub mod azure_managed_identity;
#[cfg(feature = "gcp")]
pub mod gcp_service_account;

#[cfg(feature = "aws")]
pub use aws_iam::AwsIamServiceAccount;
#[cfg(feature = "azure")]
pub use azure_managed_identity::AzureManagedIdentityServiceAccount;
#[cfg(feature = "gcp")]
pub use gcp_service_account::GcpServiceAccount;
