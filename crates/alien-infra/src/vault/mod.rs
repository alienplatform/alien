mod aws;
pub use aws::*;

mod gcp;
pub use gcp::*;

mod azure;
pub use azure::*;

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;

#[cfg(feature = "kubernetes")]
mod kubernetes;
#[cfg(feature = "kubernetes")]
pub use kubernetes::*;

#[cfg(feature = "test")]
mod test;
#[cfg(feature = "test")]
pub use test::*;

#[cfg(feature = "aws")]
mod templates;
#[cfg(feature = "aws")]
pub use templates::*;
