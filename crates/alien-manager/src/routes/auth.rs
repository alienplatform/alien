//! Auth extraction helpers for route handlers.

use axum::http::HeaderMap;

use super::AppState;
use crate::error::{ErrorData, Result};
use crate::traits::AuthSubject;

/// Extract and validate the auth subject from request headers.
/// Returns `Ok(Some(subject))` if a valid token was provided,
/// `Ok(None)` if no token was provided (for optional-auth endpoints),
/// or `Err` if the token was invalid.
pub async fn extract_auth(state: &AppState, headers: &HeaderMap) -> Result<Option<AuthSubject>> {
    state
        .auth_validator
        .validate(headers)
        .await
        .map_err(|e| ErrorData::unauthorized(e.message))
}

/// Extract auth and require a valid token. Returns error if no token is provided.
pub async fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<AuthSubject> {
    match extract_auth(state, headers).await? {
        Some(subject) => Ok(subject),
        None => Err(ErrorData::unauthorized("Authentication required")),
    }
}

/// Require that the caller has full access (admin or workspace-wide).
/// Use this for operations that any workspace owner should be able to do.
pub fn require_full_access(subject: &AuthSubject) -> Result<()> {
    if !subject.has_full_access() {
        return Err(ErrorData::forbidden("Full access required"));
    }
    Ok(())
}

/// Require strict Admin token (OSS operator only).
/// Use this for manager-level operations like token management that
/// workspace users should NOT have access to in platform mode.
pub fn require_admin(subject: &AuthSubject) -> Result<()> {
    if !subject.is_admin() {
        return Err(ErrorData::forbidden("Admin access required"));
    }
    Ok(())
}

/// Require full access or deployment group token that owns the specified group.
pub fn require_admin_or_group(subject: &AuthSubject, group_id: &str) -> Result<()> {
    if !subject.can_access_group(group_id) {
        return Err(ErrorData::forbidden(
            "Access denied: requires admin or matching deployment group token",
        ));
    }
    Ok(())
}

/// Require that the caller is admin/workspace-wide or a deployment group token.
pub fn require_admin_or_any_group(subject: &AuthSubject) -> Result<()> {
    if !subject.has_full_access() && !subject.is_deployment_group() {
        return Err(ErrorData::forbidden(
            "Access denied: requires admin or deployment group token",
        ));
    }
    Ok(())
}
