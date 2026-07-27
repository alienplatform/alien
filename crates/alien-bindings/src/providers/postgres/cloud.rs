//! Surface shared by the three cloud Postgres backends (Aurora, Cloud SQL, Flexible
//! Server).
//!
//! Everything here is cloud-only, so the module is gated once at its `mod` declaration
//! rather than attribute-by-attribute: a local-only build has no secret store to read
//! and no cloud handle to hand back.

use crate::error::{ErrorData, Result};
use alien_core::bindings::BindingValue;
use alien_error::Context;

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
