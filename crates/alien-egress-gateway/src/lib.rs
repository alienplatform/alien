//! Sandbox egress authorization and proxy enforcement.
#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]
pub mod admin;
pub mod config;
pub mod credentials;
pub mod error;
pub mod http_proxy;
pub mod io;
pub mod policy;
pub mod protocol;
pub mod runtime;
pub mod server;
pub mod store;
pub mod token;
