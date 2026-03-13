//! # Application Events
//!
//! This module contains event types that can be received by Alien applications.
//! These are platform-agnostic event structures that represent triggers from
//! various sources like queues, storage, and scheduled events.
//!
//! Unlike the `events` module which handles alien events (traces-like system events),
//! this module focuses on application-level events that trigger function execution.

mod queue;
pub use queue::*;

mod storage;
pub use storage::*;

mod scheduled;
pub use scheduled::*;
