//! The verifier: on a protected install, the only principal that changes
//! what runs.
//!
//! A worker controller hands it the cloud write it built, exactly as an
//! unprotected install sends it, with the signed statements. It finds the
//! install's signing settings and ledger itself, derives the install id from
//! its own identity, verifies the statements, checks every field of the write
//! against the cloud's table, sends it unchanged and reads back what decides
//! what runs. A field no row names refuses the write, so a new cloud field
//! can't slip through. On machine clusters it writes the launch template and
//! bounds (AWS) or resizes within the signed bounds (GCP, Azure).
//!
//! - [`trust`]: finding the settings and ledger, and accepting statements;
//! - [`plan`]: the request, where the verifier runs, and signed invoke grants;
//! - [`fields`] and [`cloud_write`]: the field tables and each cloud's writes;
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
pub mod azure_machines;
pub mod cloud_run;
pub mod cloud_write;
pub mod container_apps;
pub mod error;
pub mod fields;
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
pub use plan::{Request, SECRETS_ENV};
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
