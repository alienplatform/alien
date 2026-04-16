#[cfg(feature = "aws")]
pub mod aws;

#[cfg(feature = "gcp")]
pub mod gcp;

#[cfg(feature = "azure")]
pub mod azure;

#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub mod templates;
