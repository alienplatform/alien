//! Storage service implementations

#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
pub(crate) mod credential_bridge;

#[cfg(feature = "aws")]
pub mod aws_s3;
#[cfg(feature = "azure")]
pub mod azure_blob;
#[cfg(feature = "gcp")]
pub mod gcp_gcs;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub use aws_s3::S3Storage;
#[cfg(feature = "azure")]
pub use azure_blob::BlobStorage;
#[cfg(feature = "gcp")]
pub use gcp_gcs::GcsStorage;
#[cfg(feature = "grpc")]
pub use grpc::GrpcStorage;
#[cfg(feature = "local")]
pub use local::LocalStorage;
