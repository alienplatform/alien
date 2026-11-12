//! Extension bundle objects by digest: `alien release` uploads each core zip and controller
//! binary a release pins, and the Operator and the setup CLI download them.
//!
//! - `POST /v1/extensions/bundles/{digest}?extension=<name>&object=<core|target>`: stores the
//!   body under `digest`, `sha256:<hex>`, once it hashes to it. A core zip must be the named
//!   extension's bundle, and every controller binary its manifest pins must be stored already,
//!   so a release uploads the binaries first.
//! - `GET` (and `HEAD`) `/v1/extensions/bundles/{digest}`: the object, or a redirect to where it
//!   is. `404` when none is stored, which is how `alien release` finds what to upload.
//!
//! Both take `?project=` from a token bound to no project, and refuse one that names another
//! project than the token's.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use alien_infra::extension_bundle::{sha256_hex, verify_sha256, ExtensionBundle, MAX_OBJECT_BYTES};

use super::AppState;
use crate::error::ErrorData;
use crate::traits::BundleObject;

/// The object a release names when it uploads: its extension, and `core` or a controller target.
#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    pub extension: String,
    pub object: String,
    /// The project, for a caller whose token is not bound to one.
    pub project: Option<String>,
}

/// The project a download is for, for a caller whose token is not bound to one.
#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    pub project: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/extensions/bundles/{digest}",
            get(download).post(upload),
        )
        .layer(DefaultBodyLimit::max(MAX_OBJECT_BYTES as usize))
}

/// The project whose bundles `subject` uploads or reads: the one its token is bound to, or,
/// for a token bound to none, the one the request names.
fn project_of(subject: &crate::auth::Subject, named: Option<&str>) -> Result<String, Response> {
    match (subject.scope.project_id(), named) {
        (Some(bound), Some(named)) if bound != named => Err(ErrorData::forbidden(format!(
            "This token is bound to project '{bound}', not '{named}'"
        ))
        .into_response()),
        (Some(bound), _) => Ok(bound.to_string()),
        (None, Some(named)) => Ok(named.to_string()),
        (None, None) => Err(ErrorData::bad_request(
            "This token is bound to no project; name one with ?project=",
        )
        .into_response()),
    }
}

fn digest_hex(digest: &str) -> Result<&str, Response> {
    sha256_hex(digest).ok_or_else(|| {
        ErrorData::bad_request(format!(
            "'{digest}' is not a digest of the form sha256:<hex>"
        ))
        .into_response()
    })
}

async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(digest): Path<String>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Response {
    let subject = match super::auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    if !state.authz.can_upload_extension_bundle(&subject) {
        return ErrorData::forbidden("Cannot upload extension bundles").into_response();
    }
    let sha256 = match digest_hex(&digest) {
        Ok(sha256) => sha256,
        Err(refused) => return refused,
    };
    let project = match project_of(&subject, query.project.as_deref()) {
        Ok(project) => project,
        Err(refused) => return refused,
    };
    let object = if query.object == "core" {
        "core zip".to_string()
    } else {
        format!("controller for {}", query.object)
    };
    if let Err(error) = verify_sha256(&query.extension, &object, &body, sha256) {
        return ErrorData::bad_request(error.message).into_response();
    }
    if query.object == "core" {
        if let Err(response) =
            check_core_zip(&state, &subject, &project, &query.extension, &body).await
        {
            return response;
        }
    }
    let Some(store) = state.extension_bundles.as_ref() else {
        return ErrorData::bad_request("This manager stores no extension bundles").into_response();
    };
    if let Err(error) = store.store().put(&subject, sha256, body.to_vec()).await {
        return store_error(error, &digest);
    }
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "digest": digest })),
    )
        .into_response()
}

/// Fails unless `zip` is `extension`'s bundle and every controller binary it pins is stored.
async fn check_core_zip(
    state: &AppState,
    subject: &crate::auth::Subject,
    project: &str,
    extension: &str,
    zip: &[u8],
) -> Result<(), Response> {
    let bundle = ExtensionBundle::read(extension, zip)
        .map_err(|error| ErrorData::bad_request(error.message).into_response())?;
    let Some(bundles) = state.extension_bundles.as_ref() else {
        return Ok(());
    };
    for (target, binary) in &bundle.manifest().controllers {
        let Some(binary) = binary else { continue };
        let stored = bundles
            .store()
            .get(subject, project, &binary.sha256.to_ascii_lowercase())
            .await
            .map_err(|error| store_error(error, &format!("sha256:{}", binary.sha256)))?;
        if stored.is_none() {
            return Err(ErrorData::bad_request(format!(
                "Extension '{extension}': its bundle names a controller for {target} (sha256 {}) that is not uploaded; upload the controllers before the core zip",
                binary.sha256
            ))
            .into_response());
        }
    }
    Ok(())
}

async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(digest): Path<String>,
    Query(query): Query<DownloadQuery>,
) -> Response {
    let subject = match super::auth::require_auth(&state, &headers).await {
        Ok(subject) => subject,
        Err(error) => return error.into_response(),
    };
    if !state.authz.can_read_extension_bundle(&subject) {
        return ErrorData::forbidden("Cannot read extension bundles").into_response();
    }
    let sha256 = match digest_hex(&digest) {
        Ok(sha256) => sha256,
        Err(refused) => return refused,
    };
    let not_found = || {
        alien_error::AlienError::new(ErrorData::ExtensionBundleObjectNotFound {
            digest: digest.clone(),
        })
        .into_response()
    };
    let project = match project_of(&subject, query.project.as_deref()) {
        Ok(project) => project,
        Err(refused) => return refused,
    };
    let Some(bundles) = state.extension_bundles.as_ref() else {
        return not_found();
    };
    match bundles.store().get(&subject, &project, sha256).await {
        Ok(Some(BundleObject::Bytes(bytes))) => {
            ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
        }
        Ok(Some(BundleObject::Url(url))) => Redirect::temporary(&url).into_response(),
        Ok(None) => not_found(),
        Err(error) => store_error(error, &digest),
    }
}

/// A store's refusal as the status it carries: a store backed by an API answers a caller that
/// may not read the bundle, or a write it does not take, with that API's status.
fn store_error(error: alien_error::AlienError, digest: &str) -> Response {
    match error.http_status_code {
        Some(400) => ErrorData::bad_request(error.message).into_response(),
        Some(403) => ErrorData::forbidden(error.message).into_response(),
        Some(404) => alien_error::AlienError::new(ErrorData::ExtensionBundleObjectNotFound {
            digest: digest.to_string(),
        })
        .into_response(),
        _ => ErrorData::internal(error.message).into_response(),
    }
}
