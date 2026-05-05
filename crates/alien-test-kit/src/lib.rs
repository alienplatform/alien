//! Shared test helpers for distribution generators and import transports.
//!
//! The crate stays lightweight so generator crates can depend on it from tests
//! without pulling cloud SDKs into their compile path.

pub mod error;
pub mod fixtures;
pub mod linters;
pub mod mock_manager;

pub use error::{ErrorData, Result};
pub use fixtures::*;
pub use linters::*;
pub use mock_manager::*;
