//! Shared fail-closed checks until a backend can enforce per-sandbox policies.

use alien_core::SandboxEgress;
use alien_error::{AlienError, ContextError};

use crate::error::{ErrorData, Result};

pub(crate) fn unsupported(operation: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationNotSupported {
        operation: operation.to_string(),
        reason: "this sandbox backend does not yet support per-sandbox egress enforcement"
            .to_string(),
    })
}

pub(crate) fn policy_error(error: AlienError<alien_core::ErrorData>) -> AlienError<ErrorData> {
    let context = match &error.error {
        Some(alien_core::ErrorData::SandboxEgressPolicyTooLarge { limit }) => {
            ErrorData::SandboxEgressPolicyTooLarge {
                limit: limit.clone(),
            }
        }
        Some(alien_core::ErrorData::SandboxEgressWidens { target }) => {
            ErrorData::SandboxEgressWidens {
                target: target.clone(),
            }
        }
        _ => ErrorData::SandboxEgressInvalid {
            reason: error.message.clone(),
        },
    };
    error.context(context)
}

pub(crate) fn check_bound(requested: &SandboxEgress, bound: Option<&SandboxEgress>) -> Result<()> {
    requested.validate().map_err(policy_error)?;
    let bound =
        bound.ok_or_else(|| unsupported("egress: legacy binding has no declaration bound"))?;
    requested.ensure_subset_of(bound).map_err(policy_error)
}

pub(crate) fn require_default(
    requested: Option<&SandboxEgress>,
    bound: Option<&SandboxEgress>,
) -> Result<()> {
    let Some(requested) = requested else {
        return Ok(());
    };
    check_bound(requested, bound)?;
    // Checking both directions accepts equivalent spelling/order without pretending that a
    // backend can enforce a restriction it never received.
    if bound.is_some_and(|bound| bound.ensure_subset_of(requested).is_ok()) {
        Ok(())
    } else {
        Err(unsupported("egress"))
    }
}
