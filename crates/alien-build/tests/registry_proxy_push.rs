//! Pushing an AWS image through a manager's registry proxy, whose upstream is Amazon ECR.
//!
//! The proxy forwards upload requests unchanged, so the fake registry here answers the way ECR
//! does. Each behaviour was measured against ECR:
//!
//! - a chunk PATCH gets 201 Created (the OCI spec says 202 Accepted);
//! - completing an upload whose parts, other than the last, are under 5 MB is refused;
//! - a large blob sent in one PUT is refused with 413. ECR accepted 180 MiB and refused
//!   300 MiB; the fake refuses anything over 12 MiB so the test stays fast.
//!
//! To check the real thing, push an image with a layer over 300 MiB to an ECR repository with
//! these push options and confirm it completes (it fails with 413 under monolithic uploads).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alien_build::settings::registry_proxy_push_policy;
use alien_core::Platform;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use dockdash::{Arch, ClientProtocol, Image, Layer, PushOptions, RegistryAuth};
use sha2::{Digest, Sha256};

const MIN_PART_BYTES: usize = 5 * 1024 * 1024;
const MONOLITHIC_LIMIT_BYTES: usize = 12 * 1024 * 1024;
const LAYER_FILE_BYTES: usize = 20 * 1024 * 1024;

#[derive(Default)]
struct FakeEcr {
    next_upload: u64,
    uploads: HashMap<String, Vec<Vec<u8>>>,
    blobs: HashMap<String, usize>,
    manifests: Vec<String>,
    /// Body size of every chunk PATCH, in order.
    chunks: Vec<usize>,
    /// Body size of every PUT that carried the blob itself (a monolithic upload).
    monolithic_puts: Vec<usize>,
}

type Shared = Arc<Mutex<FakeEcr>>;

fn ecr_error(status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({ "errors": [{ "code": code, "message": message }] });
    (status, body.to_string()).into_response()
}

async fn handle(
    State(registry): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path();
    if path == "/v2/" {
        return StatusCode::OK.into_response();
    }
    let Some(rest) = path.strip_prefix("/v2/") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut registry = registry.lock().unwrap();

    if let Some((name, upload)) = rest.split_once("/blobs/uploads/") {
        let location = format!("/v2/{name}/blobs/uploads/{upload}");
        return match method {
            // A new upload, and also the answer to a cross-repository mount request: ECR
            // doesn't mount here, it starts an upload instead.
            Method::POST => {
                registry.next_upload += 1;
                let id = format!("upload-{}", registry.next_upload);
                registry.uploads.insert(id.clone(), Vec::new());
                (
                    StatusCode::ACCEPTED,
                    [(header::LOCATION, format!("/v2/{name}/blobs/uploads/{id}"))],
                )
                    .into_response()
            }
            Method::PATCH => {
                let received: usize = registry.uploads[upload].iter().map(Vec::len).sum();
                let start = headers
                    .get(header::CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|range| range.split_once('-'))
                    .and_then(|(start, _)| start.parse::<usize>().ok());
                if start != Some(received) {
                    return ecr_error(
                        StatusCode::RANGE_NOT_SATISFIABLE,
                        "BLOB_UPLOAD_INVALID",
                        "chunk does not continue the upload",
                    );
                }
                registry.chunks.push(body.len());
                let parts = registry.uploads.get_mut(upload).unwrap();
                parts.push(body.to_vec());
                let end = received + body.len() - 1;
                (
                    StatusCode::CREATED,
                    [
                        (header::LOCATION, location),
                        (header::RANGE, format!("0-{end}")),
                    ],
                )
                    .into_response()
            }
            Method::PUT => {
                if !body.is_empty() {
                    registry.monolithic_puts.push(body.len());
                    if body.len() > MONOLITHIC_LIMIT_BYTES {
                        return (StatusCode::PAYLOAD_TOO_LARGE, "Request Entity Too Large")
                            .into_response();
                    }
                    registry
                        .uploads
                        .get_mut(upload)
                        .unwrap()
                        .push(body.to_vec());
                }
                let parts = registry.uploads.remove(upload).unwrap();
                if parts
                    .split_last()
                    .is_some_and(|(_, earlier)| earlier.iter().any(|p| p.len() < MIN_PART_BYTES))
                {
                    return ecr_error(
                        StatusCode::BAD_REQUEST,
                        "BLOB_UPLOAD_INVALID",
                        "All layer parts except the last part should be at least 5MB",
                    );
                }
                let blob = parts.concat();
                let digest = format!("sha256:{:x}", Sha256::digest(&blob));
                let requested = uri
                    .query()
                    .unwrap_or_default()
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("digest="))
                    .map(|value| value.replace("%3A", ":"));
                if requested.as_deref() != Some(digest.as_str()) {
                    return ecr_error(StatusCode::BAD_REQUEST, "DIGEST_INVALID", "digest mismatch");
                }
                registry.blobs.insert(digest.clone(), blob.len());
                (
                    StatusCode::CREATED,
                    [(header::LOCATION, format!("/v2/{name}/blobs/{digest}"))],
                )
                    .into_response()
            }
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        };
    }

    if let Some((_, digest)) = rest.split_once("/blobs/") {
        return match registry.blobs.get(digest) {
            Some(size) => {
                (StatusCode::OK, [(header::CONTENT_LENGTH, size.to_string())]).into_response()
            }
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }

    if let Some((name, reference)) = rest.split_once("/manifests/") {
        if method != Method::PUT {
            return StatusCode::NOT_FOUND.into_response();
        }
        registry.manifests.push(format!("{name}:{reference}"));
        let digest = format!("sha256:{:x}", Sha256::digest(&body));
        return (
            StatusCode::CREATED,
            [
                (header::LOCATION, format!("/v2/{name}/manifests/{digest}")),
                (
                    header::HeaderName::from_static("docker-content-digest"),
                    digest,
                ),
            ],
        )
            .into_response();
    }

    StatusCode::NOT_FOUND.into_response()
}

async fn start_fake_ecr() -> (String, Shared) {
    let registry = Shared::default();
    let app = Router::new()
        .fallback(handle)
        .layer(DefaultBodyLimit::disable())
        .with_state(registry.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (host, registry)
}

/// Bytes that don't compress, so the gzip layer blob stays about as large as the file.
fn incompressible(len: usize) -> Vec<u8> {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

#[tokio::test]
async fn an_aws_push_through_the_registry_proxy_uploads_a_large_layer_in_chunks() {
    let (host, registry) = start_fake_ecr().await;
    let output = tempfile::tempdir().unwrap();

    let layer = Layer::builder()
        .unwrap()
        .data("app/model.bin", &incompressible(LAYER_FILE_BYTES), None)
        .unwrap()
        .build()
        .await
        .unwrap();
    let layer_digest = layer.blob_digest().to_string();
    let layer_size = std::fs::metadata(layer.path()).unwrap().len() as usize;
    assert!(
        layer_size > MONOLITHIC_LIMIT_BYTES,
        "the layer blob ({layer_size} bytes) must be too large for one PUT"
    );
    let (image, _) = Image::builder()
        .platform("linux", &Arch::Amd64)
        .layer(layer)
        .output_to(output.path().join("app.oci.tar"))
        .build()
        .await
        .unwrap();

    let options = PushOptions {
        auth: RegistryAuth::Anonymous,
        protocol: ClientProtocol::Http,
        monolithic_push: registry_proxy_push_policy(Platform::Aws),
        ..Default::default()
    };
    image
        .push(&format!("{host}/artifacts/prj-a/app:v1"), &options)
        .await
        .expect("an AWS push of a large layer through the registry proxy must succeed");

    let registry = registry.lock().unwrap();
    assert_eq!(registry.manifests, ["artifacts/prj-a/app:v1"]);
    assert_eq!(registry.blobs.get(&layer_digest), Some(&layer_size));
    assert!(
        registry.monolithic_puts.is_empty(),
        "no blob may be sent in one PUT, got PUT bodies of {:?} bytes",
        registry.monolithic_puts
    );
    assert!(
        registry.chunks.iter().sum::<usize>() > layer_size,
        "the layer and the config both go up in chunks, got chunks of {:?} bytes",
        registry.chunks
    );
}
