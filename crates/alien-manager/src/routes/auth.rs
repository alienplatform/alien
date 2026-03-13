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

/// Require that the caller has admin privileges.
pub fn require_admin(subject: &AuthSubject) -> Result<()> {
    if !subject.is_admin() {
        return Err(ErrorData::forbidden("Admin access required"));
    }
    Ok(())
}

/// Require admin or deployment group token that owns the specified group.
pub fn require_admin_or_group(subject: &AuthSubject, group_id: &str) -> Result<()> {
    if !subject.can_access_group(group_id) {
        return Err(ErrorData::forbidden(
            "Access denied: requires admin or matching deployment group token",
        ));
    }
    Ok(())
}

/// Require that the caller is admin or a deployment group token (any group).
pub fn require_admin_or_any_group(subject: &AuthSubject) -> Result<()> {
    if !subject.is_admin() && !subject.is_deployment_group() {
        return Err(ErrorData::forbidden(
            "Access denied: requires admin or deployment group token",
        ));
    }
    Ok(())
}

/// Require deployment token matching the specified deployment.
#[allow(dead_code)]
pub fn require_deployment_token(subject: &AuthSubject, deployment_id: &str) -> Result<()> {
    if !subject.is_deployment() || !subject.can_access_deployment(deployment_id) {
        return Err(ErrorData::forbidden(
            "Access denied: requires matching deployment token",
        ));
    }
    Ok(())
}
