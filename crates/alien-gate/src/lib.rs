//! The verifier: on a protected install, the only principal that changes
//! what runs.
//!
//! The control plane calls it with signed statements, never with a spec. It
//! finds the install's signing settings and ledger itself, derives the
//! install id from its own identity, verifies the statements, builds each
//! function from the signed workload, applies it and reads it back. Any
//! field it doesn't know, in the request or in what the cloud returns,
//! refuses the write, so a new cloud field can't slip through. On machine
//! clusters it writes the launch template and bounds (AWS) or resizes within
//! the signed bounds (GCP, Azure).
//!
//! - [`trust`]: finding the settings and ledger, and accepting statements;
//! - [`plan`]: from an accepted release to the functions to write;
//! - [`function`] and [`aws`]: Lambda and the install's SSM parameters;
//! - [`cloud_run`] and [`gcp`]: Cloud Run and Parameter Manager on GCP;
//! - [`container_apps`] and [`azure`]: Container Apps and Key Vault on Azure;
//! - [`machines`]: launch templates, Auto Scaling groups and resizes;
//! - [`verifier`]: one call, from request to writes.

#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

pub mod aws;
pub mod azure;
pub mod cloud_run;
pub mod container_apps;
pub mod error;
#[cfg(test)]
pub mod fixtures;
pub mod function;
pub mod gcp;
pub mod gcp_machines;
pub mod machines;
pub mod plan;
pub mod trust;
pub mod verifier;

pub use error::{ErrorData, Result};
pub use plan::{FunctionSpec, ReleaseRequest, Request, SECRETS_ENV};
pub use verifier::{Backend, Verifier};

/// The outer reason and what caused it, as one line.
pub fn describe<T: alien_error::AlienErrorData + Clone + std::fmt::Debug + serde::Serialize>(
    error: &alien_error::AlienError<T>,
) -> String {
    let causes = std::iter::successors(error.source.as_deref(), |cause| cause.source.as_deref())
        .map(|cause| format!("{}: {}", cause.code, cause.message));
    std::iter::once(error.message.clone())
        .chain(causes)
        .collect::<Vec<_>>()
        .join(": ")
}
