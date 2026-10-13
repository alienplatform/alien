//! Auth extraction helpers for route handlers.
//!
//! Handlers fetch a unified [`crate::auth::Subject`] via [`require_auth`] and
//! pass it directly to [`crate::auth::Authz`] / store calls. The validator
//! places the raw bearer into `Subject.bearer_token`, so handlers do not pull
//! the `Authorization` header themselves.

use alien_error::AlienError;
use axum::http::HeaderMap;
use tracing::warn;

use super::AppState;
use crate::auth::Subject;
use crate::error::{ErrorData, Result};

/// Extract and validate the auth subject from request headers.
/// Returns `Ok(Some(subject))` if a valid token was provided,
/// `Ok(None)` if no token was provided (for optional-auth endpoints),
/// a 503 `AUTHENTICATION_UNAVAILABLE` error if the validator could not check
/// the token right now, or a 401 `UNAUTHORIZED` error if the token is invalid.
pub async fn extract_auth(state: &AppState, headers: &HeaderMap) -> Result<Option<Subject>> {
    state
        .auth_validator
        .validate(headers)
        .await
        .map_err(validation_error)
}

/// Map an [`crate::traits::AuthValidator`] error to the response error,
/// following the validator contract: retryable means "could not verify".
pub fn validation_error(error: AlienError) -> AlienError<ErrorData> {
    if !error.retryable {
        return ErrorData::unauthorized(error.message);
    }
    // The validator's error can describe internals (a database, an upstream
    // URL), so it is logged here and not returned to the caller.
    warn!(
        code = %error.code,
        error = %error.message,
        "Could not verify request credentials"
    );
    AlienError::new(ErrorData::AuthenticationUnavailable)
}

/// Extract auth and require a valid token. Returns error if no token is provided.
pub async fn require_auth(state: &AppState, headers: &HeaderMap) -> Result<Subject> {
    match extract_auth(state, headers).await? {
        Some(subject) => Ok(subject),
        None => Err(ErrorData::unauthorized("Authentication required")),
    }
}
