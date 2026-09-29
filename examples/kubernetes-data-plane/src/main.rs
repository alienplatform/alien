//! Object service that runs in a customer's Kubernetes cluster.
//!
//! Stores objects in the customer's own S3-compatible bucket (the `objects`
//! binding) and serves them over HTTP. Your control plane calls it through the
//! manager's tunnel with its own bearer token; nothing listens on the
//! customer's network for you.
//!
//! - `PUT /objects/{key}` streams the body into the bucket. `If-None-Match: *`
//!   only creates; `If-Match: <etag>` only replaces that exact version.
//! - `GET /objects/{key}` streams the object back.
//! - `GET /objects?prefix=` lists objects.
//! - `DELETE /objects/{key}` removes an object.

use std::{net::SocketAddr, sync::Arc};

use alien_sdk::{Bindings, Storage};
use axum::{
    body::{Body, Bytes},
    extract::{Path as UrlPath, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures::{StreamExt, TryStreamExt};
use object_store::{path::Path, PutMode, PutOptions, PutPayload, UpdateVersion, WriteMultipart};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Objects up to this size are written in one request; larger ones stream as
/// a multipart upload so memory use stays flat.
const SINGLE_PUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    storage: Arc<dyn Storage>,
    access_token: Arc<str>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let access_token = std::env::var("ACCESS_TOKEN").expect("ACCESS_TOKEN must be set");
    let bindings = Bindings::from_env().expect("failed to load bindings");
    let storage = bindings
        .storage("objects")
        .await
        .expect("failed to load the 'objects' storage binding");
    info!(bucket = %storage.get_url(), "object storage ready");

    let state = AppState {
        storage,
        access_token: access_token.into(),
    };
    let app = Router::new()
        .route("/objects", get(list))
        .route("/objects/{*key}", get(download).put(upload).delete(remove))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize))
        .route("/health", get(|| async { "ok" }))
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "listening");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server failed");
}

/// Every object route requires `Authorization: Bearer <ACCESS_TOKEN>`.
async fn authorize(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if presented != Some(&*state.access_token) {
        warn!(path = %request.uri().path(), "rejected request without a valid access token");
        return error(StatusCode::UNAUTHORIZED, "missing or invalid access token");
    }
    next.run(request).await
}

#[derive(Serialize)]
struct Stored {
    key: String,
    size: usize,
    etag: Option<String>,
}

async fn upload(
    State(state): State<AppState>,
    UrlPath(key): UrlPath<String>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let path = Path::from(key.as_str());
    let mode = match put_mode(&headers) {
        Ok(mode) => mode,
        Err(response) => return response,
    };

    // Read up to the single-put limit; switch to multipart beyond it.
    let mut stream = body.into_data_stream().map_err(std::io::Error::other);
    let mut head = Vec::new();
    let mut size = 0usize;
    while head.len() <= SINGLE_PUT_LIMIT {
        match stream.next().await {
            Some(Ok(chunk)) => head.push(chunk),
            Some(Err(e)) => return error(StatusCode::BAD_REQUEST, &format!("upload failed: {e}")),
            None => break,
        }
        size = head.iter().map(Bytes::len).sum();
        if size > SINGLE_PUT_LIMIT {
            break;
        }
    }

    let result = if size <= SINGLE_PUT_LIMIT {
        let payload: PutPayload = head.into_iter().collect();
        state
            .storage
            .put_opts(&path, payload, PutOptions::from(mode))
            .await
            .map(|r| r.e_tag)
    } else {
        if !matches!(mode, PutMode::Overwrite) {
            return error(
                StatusCode::PRECONDITION_FAILED,
                "conditional writes are limited to objects up to 8 MiB",
            );
        }
        let upload = match state.storage.put_multipart(&path).await {
            Ok(upload) => upload,
            Err(e) => return storage_error(e),
        };
        let mut writer = WriteMultipart::new(upload);
        for chunk in head {
            writer.write(&chunk);
        }
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(chunk) => {
                    size += chunk.len();
                    if let Err(e) = writer.wait_for_capacity(8).await {
                        return storage_error(e);
                    }
                    writer.write(&chunk);
                }
                Err(e) => {
                    let _ = writer.abort().await;
                    return error(StatusCode::BAD_REQUEST, &format!("upload failed: {e}"));
                }
            }
        }
        writer.finish().await.map(|r| r.e_tag)
    };

    match result {
        Ok(etag) => {
            info!(key = %key, size, "stored object");
            (StatusCode::CREATED, Json(Stored { key, size, etag })).into_response()
        }
        Err(e) => storage_error(e),
    }
}

/// `If-None-Match: *` creates only; `If-Match: <etag>` replaces only that version.
fn put_mode(headers: &HeaderMap) -> Result<PutMode, Response> {
    let header_value = |name| {
        headers
            .get(name)
            .map(|v| v.to_str().map(|s| s.trim_matches('"').to_string()))
            .transpose()
            .map_err(|_| error(StatusCode::BAD_REQUEST, "invalid precondition header"))
    };
    if header_value(header::IF_NONE_MATCH)?.as_deref() == Some("*") {
        return Ok(PutMode::Create);
    }
    if let Some(etag) = header_value(header::IF_MATCH)? {
        return Ok(PutMode::Update(UpdateVersion {
            e_tag: Some(format!("\"{etag}\"")),
            version: None,
        }));
    }
    Ok(PutMode::Overwrite)
}

async fn download(State(state): State<AppState>, UrlPath(key): UrlPath<String>) -> Response {
    match state.storage.get(&Path::from(key.as_str())).await {
        Ok(result) => {
            let mut response = Response::builder()
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, result.meta.size);
            if let Some(etag) = &result.meta.e_tag {
                response = response.header(header::ETAG, etag);
            }
            response
                .body(Body::from_stream(result.into_stream()))
                .expect("valid response")
        }
        Err(e) => storage_error(e),
    }
}

#[derive(Deserialize)]
struct ListQuery {
    prefix: Option<String>,
}

#[derive(Serialize)]
struct Listed {
    key: String,
    size: u64,
    etag: Option<String>,
}

async fn list(State(state): State<AppState>, Query(query): Query<ListQuery>) -> Response {
    let prefix = query.prefix.map(|p| Path::from(p.as_str()));
    match state
        .storage
        .list(prefix.as_ref())
        .map_ok(|meta| Listed {
            key: meta.location.to_string(),
            size: meta.size,
            etag: meta.e_tag,
        })
        .try_collect::<Vec<_>>()
        .await
    {
        Ok(objects) => Json(objects).into_response(),
        Err(e) => storage_error(e),
    }
}

async fn remove(State(state): State<AppState>, UrlPath(key): UrlPath<String>) -> Response {
    match state.storage.delete(&Path::from(key.as_str())).await {
        Ok(()) => {
            info!(key = %key, "deleted object");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => storage_error(e),
    }
}

fn storage_error(e: object_store::Error) -> Response {
    match e {
        object_store::Error::NotFound { .. } => error(StatusCode::NOT_FOUND, "object not found"),
        object_store::Error::AlreadyExists { .. } => {
            error(StatusCode::PRECONDITION_FAILED, "object already exists")
        }
        object_store::Error::Precondition { .. } => error(
            StatusCode::PRECONDITION_FAILED,
            "object changed since that version",
        ),
        other => {
            warn!(error = %other, "storage request failed");
            error(StatusCode::BAD_GATEWAY, "object storage request failed")
        }
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}
