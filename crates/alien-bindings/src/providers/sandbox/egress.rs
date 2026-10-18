//! Shared validation and explicit refusal for backends without a gateway.
use crate::error::{ErrorData, Result};
use alien_core::{EgressPolicy, SandboxEgress};
use alien_error::{AlienError, ContextError};

pub(crate) fn unsupported(operation: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxEgressGatewayUnavailable {
        reason: format!("{operation} requires gateway enforcement"),
    })
}
pub(crate) fn policy_error(error: AlienError<alien_core::ErrorData>) -> AlienError<ErrorData> {
    let context = match &error.error {
        Some(alien_core::ErrorData::SandboxEgressPolicyTooLarge { limit }) => {
            ErrorData::SandboxEgressPolicyTooLarge {
                limit: limit.clone(),
            }
        }
        _ => ErrorData::SandboxEgressInvalid {
            reason: error.message.clone(),
        },
    };
    error.context(context)
}
pub(crate) fn validate(
    requested: &EgressPolicy,
    declaration: Option<&SandboxEgress>,
) -> Result<()> {
    if !matches!(declaration, Some(SandboxEgress::Internet)) {
        return Err(AlienError::new(ErrorData::SandboxEgressRequiresInternet));
    }
    requested.validate().map_err(policy_error)
}
pub(crate) fn require_default(
    requested: Option<&EgressPolicy>,
    declaration: Option<&SandboxEgress>,
) -> Result<()> {
    if let Some(requested) = requested {
        validate(requested, declaration)?;
        return Err(unsupported("runtime policy"));
    }
    Ok(())
}
