//! Surface shared by the three cloud Postgres backends (Aurora, Cloud SQL, Flexible
//! Server).
//!
//! Everything here is cloud-only, so the module is gated once at its `mod` declaration
//! rather than attribute-by-attribute: a local-only build has no secret store to read
//! and no cloud handle to hand back.

use crate::error::{ErrorData, Result};
use crate::traits::PostgresConnectionParams;
use alien_core::bindings::BindingValue;
use alien_error::Context;

/// A resolved cloud Postgres binding (Aurora, Cloud SQL, or Flexible Server).
///
/// The three backends differ only in *how* they reach their password — which secret
/// store holds it and how its payload is decoded — never in what they hand back. Each
/// module therefore exposes a `resolve` function returning the connection parameters,
/// and they all share this one handle.
#[derive(Debug)]
pub(crate) struct CloudPostgres {
    params: PostgresConnectionParams,
}

impl CloudPostgres {
    pub(crate) fn new(params: PostgresConnectionParams) -> Self {
        Self { params }
    }
}

impl crate::traits::Binding for CloudPostgres {}

impl crate::traits::Postgres for CloudPostgres {
    fn connection_params(&self) -> PostgresConnectionParams {
        self.params.clone()
    }
}

/// Extracts a cloud variant's secret locator (ARN / name / URI) as a concrete string.
///
/// `field` is the camelCase binding field name, so the error names the key the user
/// would actually look for in `ALIEN_<NAME>_BINDING`.
pub(crate) fn resolve_secret_locator(
    binding_name: &str,
    field: &str,
    locator: &BindingValue<String>,
) -> Result<String> {
    locator
        .clone()
        .into_value(binding_name, field)
        .context(ErrorData::config_invalid(
            binding_name,
            format!("Failed to extract '{field}' from Postgres binding"),
        ))
}
