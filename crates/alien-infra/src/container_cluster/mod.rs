//! ContainerCluster resource controllers.

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;
