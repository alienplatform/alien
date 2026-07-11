//! Container resource controllers.

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;

#[cfg(feature = "kubernetes")]
pub(crate) mod kubernetes;
#[cfg(feature = "kubernetes")]
pub use kubernetes::*;

#[cfg(feature = "local")]
mod local_utils;
