//! The gate: the only principal that writes the serverless functions it owns.
//!
//! The control plane calls the gate with statements, never with a function
//! spec. The gate verifies the statements against the anchor its setup
//! wrote, builds the whole function from the signed workload, applies it and
//! reads it back. Any field it doesn't know, in the request or in what the
//! cloud returns, refuses the write, so a new cloud field can't slip through.
//!
//! - [`plan`]: from a request and the trusted state to the function to write;
//! - [`function`]: apply a planned function and check it reads back the same;
//! - [`cloud_run`] and [`gcp`]: the same for Cloud Run services on GCP;
//! - [`container_apps`] and [`azure`]: the same for Container Apps on Azure;
//! - [`state`]: the signer chain and floors the gate keeps between calls.

#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

pub mod azure;
pub mod cloud_run;
pub mod container_apps;
pub mod error;
pub mod function;
pub mod gcp;
pub mod plan;
pub mod state;

pub use error::{ErrorData, Result};
pub use function::{apply, check_read_back, FunctionStore};
pub use plan::{plan, Action, FunctionSpec, GateConfig, GateRequest, Planned};
pub use state::GateState;
