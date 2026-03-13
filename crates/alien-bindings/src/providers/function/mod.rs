//! Function service implementations

#[cfg(feature = "aws")]
pub mod aws_lambda;
#[cfg(feature = "azure")]
pub mod azure_container_app;
#[cfg(feature = "gcp")]
pub mod gcp_cloudrun;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "kubernetes")]
pub mod kubernetes;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub use aws_lambda::LambdaFunction;
#[cfg(feature = "azure")]
pub use azure_container_app::ContainerAppFunction;
#[cfg(feature = "gcp")]
pub use gcp_cloudrun::CloudRunFunction;
#[cfg(feature = "grpc")]
pub use grpc::GrpcFunction;
#[cfg(feature = "kubernetes")]
pub use kubernetes::KubernetesFunction;
#[cfg(feature = "local")]
pub use local::LocalFunction;
