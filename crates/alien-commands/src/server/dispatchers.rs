//! Re-export dispatchers from the top-level module for backward compatibility.
//!
//! The dispatcher implementations live in `crate::dispatchers` so they can be
//! used with the lightweight `dispatchers` feature (without the full `server`
//! feature and its heavy dependencies like alien-bindings/axum/object_store).

pub use crate::dispatchers::*;
