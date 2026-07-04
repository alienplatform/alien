//! Worker service implementations

#[cfg(feature = "aws")]
pub mod aws_lambda;
#[cfg(feature = "azure")]
pub mod azure_container_app;
#[cfg(feature = "gcp")]
pub mod gcp_cloudrun;
#[cfg(feature = "kubernetes")]
pub mod kubernetes;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub use aws_lambda::LambdaWorker;
#[cfg(feature = "azure")]
pub use azure_container_app::ContainerAppWorker;
#[cfg(feature = "gcp")]
pub use gcp_cloudrun::CloudRunWorker;
#[cfg(feature = "kubernetes")]
pub use kubernetes::KubernetesWorker;
#[cfg(feature = "local")]
pub use local::LocalWorker;
