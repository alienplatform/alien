//! Build service implementations
pub(crate) mod script;

#[cfg(feature = "azure")]
pub mod aca;
#[cfg(feature = "gcp")]
pub mod cloudbuild;
#[cfg(feature = "aws")]
pub mod codebuild;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "kubernetes")]
pub mod kubernetes;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "azure")]
pub use aca::AcaBuild;
#[cfg(feature = "gcp")]
pub use cloudbuild::CloudbuildBuild;
#[cfg(feature = "aws")]
pub use codebuild::CodebuildBuild;
#[cfg(feature = "grpc")]
pub use grpc::GrpcBuild;
#[cfg(feature = "kubernetes")]
pub use kubernetes::KubernetesBuild;
#[cfg(feature = "local")]
pub use local::LocalBuild;
