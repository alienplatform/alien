//! Artifact registry service implementations

#[cfg(feature = "azure")]
pub mod acr;
#[cfg(feature = "aws")]
pub mod ecr;
#[cfg(feature = "gcp")]
pub mod gar;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "azure")]
pub use acr::AcrArtifactRegistry;
#[cfg(feature = "aws")]
pub use ecr::EcrArtifactRegistry;
#[cfg(feature = "gcp")]
pub use gar::GarArtifactRegistry;
#[cfg(feature = "grpc")]
pub use grpc::GrpcArtifactRegistry;
#[cfg(feature = "local")]
pub use local::LocalArtifactRegistry;
