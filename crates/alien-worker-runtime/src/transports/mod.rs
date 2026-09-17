//! Transports for receiving work from different platforms.
//!
//! Each transport normalizes platform-specific inputs into standard types:
//! - HTTP requests
//! - StorageEvent
//! - QueueMessage
//! - CronEvent
//! - ArcCommand (platform push)

#[cfg(feature = "aws")]
pub mod lambda;

#[cfg(feature = "gcp")]
pub mod cloudrun;
#[cfg(feature = "azure")]
pub mod containerapp;
pub mod local;
pub mod shared;
