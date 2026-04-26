mod aws;
pub use aws::*;

mod gcp;
pub use gcp::*;

mod azure;
pub use azure::*;

#[cfg(feature = "kubernetes")]
mod kubernetes;
#[cfg(feature = "kubernetes")]
pub use kubernetes::*;

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "local")]
pub use local::*;

#[cfg(feature = "test")]
mod test;
#[cfg(feature = "test")]
pub use test::*;

mod templates;
pub use templates::*;

/// Re-export from alien-core (single source of truth).
pub use alien_core::crontab_to_eventbridge;

mod readiness_probe;
pub use readiness_probe::*;

#[cfg(test)]
mod fixtures;
