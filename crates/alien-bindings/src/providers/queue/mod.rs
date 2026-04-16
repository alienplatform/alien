#[cfg(feature = "aws")]
pub mod aws_sqs;

#[cfg(feature = "gcp")]
pub mod gcp_pubsub;

#[cfg(feature = "azure")]
pub mod azure_service_bus;
#[cfg(feature = "grpc")]
pub mod grpc;

#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub use aws_sqs::AwsSqsQueue;
#[cfg(feature = "azure")]
pub use azure_service_bus::AzureServiceBusQueue;
#[cfg(feature = "gcp")]
pub use gcp_pubsub::GcpPubSubQueue;
#[cfg(feature = "grpc")]
pub use grpc::GrpcQueue;
