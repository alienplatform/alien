//! Auth extraction helpers for route handlers.
//!
//! Handlers fetch a unified [`crate::auth::Subject`] via [`require_auth`] and
//! pass it directly to [`crate::auth::Authz`] / store calls. The validator
//! places the raw bearer into `Subject.bearer_token`, so handlers do not pull
//! the `Authorization` header themselves.

use axum::http::HeaderMap;

use super::AppState;
use crate::auth::Subject;
use crate::error::{ErrorData, Result};

/// Extract and validate the auth subject from request headers.
/// Returns `Ok(Some(subject))` if a valid token was provided,
/// `Ok(None)` if no token was provided (for optional-auth endpoints),
/// or `Err` if the token was invalid.
pub async fn extract_auth(state: &AppState, headers: &HeaderMap) -> Result<Option<Subject>> {
    state
        .auth_validator
        .validate(headers)
        .await
        .map_err(|e| ErrorData::unauthorized(e.message))
}

/// Extract auth and require a valid token. Returns error if no token is provided.
pub async fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<Subject> {
    match extract_auth(state, headers).await? {
        Some(subject) => Ok(subject),
        None => Err(ErrorData::unauthorized("Authentication required")),
    }
}
