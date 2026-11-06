//! Digest-preserving OCI closure transfer between an owned registry's regional endpoints.
use std::{
    collections::{HashMap, HashSet},
    io::SeekFrom,
    time::Duration,
};

use alien_error::{AlienError, Context, IntoAlienError};
use futures::{stream, TryStreamExt};
use oci_client::{
    manifest::{
        OciImageIndex, OciImageManifest, IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE,
        OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE,
    },
    secrets::RegistryAuth,
    Reference,
};
use reqwest::{header, Client, RequestBuilder, Response, StatusCode, Url};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::error::{ErrorData, Result};

const TYPES: [&str; 4] = [
    OCI_IMAGE_INDEX_MEDIA_TYPE,
    IMAGE_MANIFEST_LIST_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE,
    IMAGE_MANIFEST_MEDIA_TYPE,
];
const MAX_MANIFESTS: usize = 128;
const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_MANIFEST_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_DESCRIPTORS: usize = 4096;
const MAX_DEPTH: usize = 16;
const MAX_BLOB_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const MAX_TOTAL_BLOB_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const IO_BUFFER_BYTES: usize = 64 * 1024;

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::RegionalImageInvalid {
        reason: reason.into(),
    })
}

fn transfer_error(operation: &str) -> ErrorData {
    ErrorData::RegionalImageTransferFailed {
        operation: operation.into(),
    }
}

fn authenticate(request: RequestBuilder, auth: &RegistryAuth) -> RequestBuilder {
    match auth {
        RegistryAuth::Basic(user, password) => request.basic_auth(user, Some(password)),
        RegistryAuth::Anonymous => request,
        RegistryAuth::Bearer(token) => request.bearer_auth(token),
    }
}

async fn send(request: RequestBuilder, operation: &str) -> Result<Response> {
    request
        .send()
        .await
        .into_alien_error()
        .context(transfer_error(operation))
}

fn successful(response: Response, operation: &str) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(AlienError::new(ErrorData::RegionalImageAccessDenied {
            operation: operation.into(),
        }));
    }
    if status.is_server_error()
        || matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::REQUEST_TIMEOUT
        )
    {
        return Err(AlienError::new(transfer_error(operation)));
    }
    Err(invalid(format!(
        "{operation} returned HTTP {}",
        status.as_u16()
    )))
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn endpoint(reference: &Reference, scheme: &str, kind: &str, identity: &str) -> Result<Url> {
    Url::parse(&format!(
        "{scheme}://{}/v2/{}/{kind}/{identity}",
        reference.registry(),
        reference.repository()
    ))
    .into_alien_error()
    .context(ErrorData::RegionalImageInvalid {
        reason: "invalid registry endpoint".into(),
    })
}

async fn manifest(
    client: &Client,
    reference: &Reference,
    auth: &RegistryAuth,
    scheme: &str,
    identity: &str,
) -> Result<Vec<u8>> {
    let response = successful(
        send(
            authenticate(
                client.get(endpoint(reference, scheme, "manifests", identity)?),
                auth,
            )
            .header(header::ACCEPT, TYPES.join(", ")),
            "download manifest",
        )
        .await?,
        "download manifest",
    )?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_MANIFEST_BYTES as u64)
    {
        return Err(invalid("manifest exceeds 4 MiB"));
    }
    let mut body = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = body
        .try_next()
        .await
        .into_alien_error()
        .context(transfer_error("read manifest"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_MANIFEST_BYTES {
            return Err(invalid("manifest exceeds 4 MiB"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if digest(&bytes) != identity {
        return Err(invalid("manifest digest mismatch"));
    }
    Ok(bytes)
}

async fn blob_exists(
    client: &Client,
    reference: &Reference,
    auth: &RegistryAuth,
    scheme: &str,
    identity: &str,
    size: u64,
) -> Result<bool> {
    let response = send(
        authenticate(
            client.head(endpoint(reference, scheme, "blobs", identity)?),
            auth,
        ),
        "check regional blob",
    )
    .await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(false);
    }
    let response = successful(response, "check regional blob")?;
    let actual = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if actual != Some(size) {
        return Err(invalid("regional blob size differs from its descriptor"));
    }
    // The authenticated registry addresses the blob by the immutable digest in the HEAD URL.
    if response
        .headers()
        .get("Docker-Content-Digest")
        .is_some_and(|v| v.as_bytes() != identity.as_bytes())
    {
        return Err(invalid("regional blob digest differs from its descriptor"));
    }
    Ok(true)
}

async fn copy_blob(
    client: &Client,
    source: &Reference,
    target: &Reference,
    source_auth: &RegistryAuth,
    target_auth: &RegistryAuth,
    scheme: &str,
    identity: &str,
    size: u64,
) -> Result<()> {
    if blob_exists(client, target, target_auth, scheme, identity, size).await? {
        return Ok(());
    }
    // Verify before publishing any bytes. Only one layer is spooled at a time, and the
    // anonymous temporary file is removed on cancellation, failure, and process exit.
    let file = tempfile::tempfile()
        .into_alien_error()
        .context(transfer_error("create image spool"))?;
    let mut file = tokio::fs::File::from_std(file);
    let download_client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .into_alien_error()
        .context(transfer_error("create download client"))?;
    let response = successful(
        send(
            authenticate(
                download_client.get(endpoint(source, scheme, "blobs", identity)?),
                source_auth,
            ),
            "download image blob",
        )
        .await?,
        "download image blob",
    )?;
    if response
        .content_length()
        .is_some_and(|length| length != size)
    {
        return Err(invalid("source blob size differs from its descriptor"));
    }
    let mut body = response.bytes_stream();
    let mut downloaded = 0_u64;
    let mut hash = Sha256::new();
    while let Some(chunk) = body
        .try_next()
        .await
        .into_alien_error()
        .context(transfer_error("read image blob"))?
    {
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| invalid("image blob size overflow"))?;
        if downloaded > size {
            return Err(invalid("image blob exceeds its declared size"));
        }
        hash.update(&chunk);
        file.write_all(&chunk)
            .await
            .into_alien_error()
            .context(transfer_error("write image spool"))?;
    }
    if downloaded != size || format!("sha256:{:x}", hash.finalize()) != identity {
        return Err(invalid("source image blob size or digest mismatch"));
    }
    file.seek(SeekFrom::Start(0))
        .await
        .into_alien_error()
        .context(transfer_error("rewind image spool"))?;
    let start = endpoint(target, scheme, "blobs", "uploads/")?;
    let response = successful(
        send(
            authenticate(client.post(start.clone()), target_auth).header(header::CONTENT_LENGTH, 0),
            "start blob upload",
        )
        .await?,
        "start blob upload",
    )?;
    let location = response
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| invalid("registry omitted upload location"))?;
    let mut upload =
        start
            .join(location)
            .into_alien_error()
            .context(ErrorData::RegionalImageInvalid {
                reason: "invalid upload location".into(),
            })?;
    // Never send registry credentials or image bytes to a provider-supplied foreign URL.
    if upload.origin() != start.origin()
        || !upload
            .path()
            .starts_with(&format!("/v2/{}/blobs/uploads/", target.repository()))
    {
        return Err(invalid(
            "registry upload location escaped the selected repository",
        ));
    }
    upload.query_pairs_mut().append_pair("digest", identity);
    let body = stream::try_unfold(file, |mut file| async move {
        let mut chunk = vec![0_u8; IO_BUFFER_BYTES];
        let size = file.read(&mut chunk).await?;
        if size == 0 {
            return Ok::<_, std::io::Error>(None);
        }
        chunk.truncate(size);
        Ok(Some((chunk, file)))
    });
    let response = send(
        authenticate(client.put(upload), target_auth)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(body)),
        "upload image blob",
    )
    .await?;
    // An interrupted response or concurrent writer may leave a completed blob. Only a
    // provider HEAD proving the expected immutable object makes a failed PUT successful.
    if !response.status().is_success() {
        let failure = successful(response, "upload image blob");
        if blob_exists(client, target, target_auth, scheme, identity, size).await? {
            return Ok(());
        }
        failure?;
    }
    if !blob_exists(client, target, target_auth, scheme, identity, size).await? {
        return Err(AlienError::new(transfer_error(
            "verify uploaded image blob",
        )));
    }
    Ok(())
}

async fn publish(
    client: &Client,
    target: &Reference,
    auth: &RegistryAuth,
    scheme: &str,
    identity: &str,
    raw: Vec<u8>,
    media_type: &str,
) -> Result<()> {
    let result = send(
        authenticate(
            client.put(endpoint(target, scheme, "manifests", identity)?),
            auth,
        )
        .header(header::CONTENT_TYPE, media_type)
        .body(raw),
        "publish manifest",
    )
    .await
    .and_then(|response| successful(response, "publish manifest"));
    // ECR can report ImageAlreadyExists after an earlier attempt committed the same digest.
    // Readback proves that exact content instead of treating every conflict as success.
    let readback = manifest(client, target, auth, scheme, identity).await;
    match readback {
        Ok(_) => Ok(()),
        Err(error) => {
            result?;
            Err(error)
        }
    }
}

pub(super) async fn copy_closure(
    source: Reference,
    target: Reference,
    source_auth: RegistryAuth,
    target_auth: RegistryAuth,
) -> Result<()> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .into_alien_error()
        .context(transfer_error("create registry client"))?;
    transfer(&client, source, target, source_auth, target_auth, "https").await
}

async fn transfer(
    client: &Client,
    source: Reference,
    target: Reference,
    source_auth: RegistryAuth,
    target_auth: RegistryAuth,
    scheme: &str,
) -> Result<()> {
    let expected = source
        .digest()
        .ok_or_else(|| invalid("regional images must be digest pinned"))?;
    if !valid_digest(expected) || target.digest() != Some(expected) {
        return Err(invalid("source and target sha256 digests must match"));
    }
    let mut pending = vec![(expected.to_string(), false, 0_usize)];
    let mut visited = HashSet::new();
    let mut manifests = HashMap::<String, (Vec<u8>, String)>::new();
    let mut blobs = HashMap::new();
    let mut requirements = HashMap::<String, (usize, String)>::new();
    let mut verified = HashMap::<String, (usize, String)>::new();
    let mut manifest_bytes = 0_usize;
    let mut blob_bytes = 0_u64;
    let mut descriptors = 0_usize;
    while let Some((current, publication, depth)) = pending.pop() {
        if publication {
            let (raw, media) = manifests
                .remove(&current)
                .ok_or_else(|| invalid("missing index publication state"))?;
            publish(client, &target, &target_auth, scheme, &current, raw, &media).await?;
            continue;
        }
        if !valid_digest(&current) || depth > MAX_DEPTH {
            return Err(invalid("invalid digest or image index nesting exceeds 16"));
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        if visited.len() > MAX_MANIFESTS {
            return Err(invalid("image manifest closure exceeds 128 entries"));
        }
        let raw = manifest(client, &source, &source_auth, scheme, &current).await?;
        manifest_bytes = manifest_bytes.saturating_add(raw.len());
        if manifest_bytes > MAX_MANIFEST_TOTAL_BYTES {
            return Err(invalid("image manifest closure exceeds 16 MiB"));
        }
        let document: serde_json::Value = serde_json::from_slice(&raw).into_alien_error().context(
            ErrorData::RegionalImageInvalid {
                reason: "invalid image manifest".into(),
            },
        )?;
        let media = document
            .get("mediaType")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("manifest mediaType is required"))?
            .to_string();
        let actual = (raw.len(), media.clone());
        if requirements
            .get(&current)
            .is_some_and(|required| required != &actual)
        {
            return Err(invalid(
                "index descriptor differs from its referenced manifest",
            ));
        }
        verified.insert(current.clone(), actual);
        if [OCI_IMAGE_INDEX_MEDIA_TYPE, IMAGE_MANIFEST_LIST_MEDIA_TYPE].contains(&media.as_str()) {
            let index: OciImageIndex = serde_json::from_value(document)
                .into_alien_error()
                .context(ErrorData::RegionalImageInvalid {
                    reason: "invalid image index".into(),
                })?;
            if index.manifests.is_empty() {
                return Err(invalid("empty image index"));
            }
            descriptors = descriptors.saturating_add(index.manifests.len());
            if descriptors > MAX_DESCRIPTORS {
                return Err(invalid("image closure exceeds 4096 descriptors"));
            }
            pending.push((current.clone(), true, depth));
            for child in index.manifests.iter().rev() {
                if !valid_digest(&child.digest)
                    || child.size < 0
                    || child.size as u64 > MAX_MANIFEST_BYTES as u64
                {
                    return Err(invalid("invalid index descriptor"));
                }
                let required = (child.size as usize, child.media_type.clone());
                if requirements
                    .get(&child.digest)
                    .is_some_and(|previous| previous != &required)
                    || verified
                        .get(&child.digest)
                        .is_some_and(|actual| actual != &required)
                {
                    return Err(invalid(
                        "conflicting index descriptors for the same manifest",
                    ));
                }
                requirements.insert(child.digest.clone(), required);
                if visited.contains(&child.digest) && manifests.contains_key(&child.digest) {
                    return Err(invalid("cyclic image index"));
                }
                pending.push((child.digest.clone(), false, depth + 1));
            }
            manifests.insert(current, (raw, media));
            continue;
        }
        if ![OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE].contains(&media.as_str()) {
            return Err(invalid("unsupported image manifest media type"));
        }
        let image: OciImageManifest = serde_json::from_value(document)
            .into_alien_error()
            .context(ErrorData::RegionalImageInvalid {
                reason: "invalid image manifest".into(),
            })?;
        descriptors = descriptors.saturating_add(1 + image.layers.len());
        if descriptors > MAX_DESCRIPTORS {
            return Err(invalid("image closure exceeds 4096 descriptors"));
        }
        for descriptor in std::iter::once(&image.config).chain(image.layers.iter()) {
            if descriptor
                .urls
                .as_ref()
                .is_some_and(|urls| !urls.is_empty())
            {
                return Err(invalid(
                    "external image blob URLs are unsupported for regional copies",
                ));
            }
            let size =
                u64::try_from(descriptor.size).map_err(|_| invalid("invalid image blob size"))?;
            if !valid_digest(&descriptor.digest) || size > MAX_BLOB_BYTES {
                return Err(invalid("invalid image digest or blob exceeds 10 GiB"));
            }
            if let Some(previous) = blobs.insert(descriptor.digest.clone(), size) {
                if previous != size {
                    return Err(invalid("conflicting sizes for the same image blob"));
                }
                continue;
            }
            blob_bytes = blob_bytes.saturating_add(size);
            if blob_bytes > MAX_TOTAL_BLOB_BYTES {
                return Err(invalid("image closure exceeds 64 GiB of blobs"));
            }
            copy_blob(
                client,
                &source,
                &target,
                &source_auth,
                &target_auth,
                scheme,
                &descriptor.digest,
                size,
            )
            .await?;
        }
        publish(client, &target, &target_auth, scheme, &current, raw, &media).await?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        extract::{Request, State},
        http::{Response, StatusCode},
        routing::any,
        Router,
    };
    use serde_json::json;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct Registry {
        bytes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        published: Arc<Mutex<Vec<String>>>,
        blob_uploads: Arc<std::sync::atomic::AtomicUsize>,
        fail_manifest_once: Arc<std::sync::atomic::AtomicBool>,
    }

    async fn serve(State(registry): State<Registry>, request: Request) -> Response<Body> {
        let path = request.uri().path().to_string();
        let method = request.method().clone();
        if path == "/v2/" {
            return Response::new(Body::empty());
        }
        if method == "POST" && path.ends_with("/blobs/uploads/") {
            return Response::builder()
                .status(202)
                .header("Location", "/v2/images/blobs/uploads/session")
                .body(Body::empty())
                .unwrap();
        }
        if method == "PUT" {
            let named_digest = if path.contains("/manifests/") {
                path.rsplit('/').next().unwrap().to_string()
            } else {
                url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes())
                    .find(|(name, _)| name == "digest")
                    .expect("upload digest")
                    .1
                    .into_owned()
            };
            let bytes = to_bytes(request.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec();
            assert_eq!(digest(&bytes), named_digest);
            if path.contains("/manifests/") {
                if registry
                    .fail_manifest_once
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    return Response::builder().status(503).body(Body::empty()).unwrap();
                }
                if registry.bytes.lock().unwrap().contains_key(&named_digest) {
                    return Response::builder().status(400).body(Body::empty()).unwrap();
                }
                let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                if let Some(children) = value["manifests"].as_array() {
                    for child in children {
                        assert!(
                            registry
                                .bytes
                                .lock()
                                .unwrap()
                                .contains_key(child["digest"].as_str().unwrap()),
                            "index published before child"
                        );
                    }
                }
                registry
                    .published
                    .lock()
                    .unwrap()
                    .push(named_digest.clone());
            }
            if !path.contains("/manifests/") {
                registry
                    .blob_uploads
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            registry
                .bytes
                .lock()
                .unwrap()
                .insert(named_digest.clone(), bytes);
            return Response::builder()
                .status(201)
                .header("Location", format!("/v2/images/blobs/{named_digest}"))
                .header("Docker-Content-Digest", named_digest)
                .body(Body::empty())
                .unwrap();
        }
        let named_digest = path.rsplit('/').next().unwrap();
        let bytes = registry.bytes.lock().unwrap().get(named_digest).cloned();
        match bytes {
            Some(bytes) => {
                let media = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| value["mediaType"].as_str().map(str::to_string))
                    .unwrap_or_else(|| "application/octet-stream".into());
                Response::builder()
                    .header("Content-Type", media)
                    .header("Docker-Content-Digest", named_digest)
                    .header("Content-Length", bytes.len())
                    .body(if method == "HEAD" {
                        Body::empty()
                    } else {
                        Body::from(bytes)
                    })
                    .unwrap()
            }
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap(),
        }
    }

    async fn registry() -> (Registry, String, tokio::task::JoinHandle<()>) {
        let registry = Registry::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .fallback(any(serve))
            .with_state(registry.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (registry, address.to_string(), task)
    }

    #[tokio::test]
    async fn resumes_partial_index_closure_exactly_and_refuses_corrupt_source() {
        let (source, source_host, source_task) = registry().await;
        let (target, target_host, target_task) = registry().await;
        let config = b"{\"architecture\":\"arm64\",\"os\":\"linux\"}".to_vec();
        let layer = b"private image layer fixture".to_vec();
        let manifest = serde_json::to_vec(&json!({"schemaVersion":2,"mediaType":OCI_IMAGE_MEDIA_TYPE,
            "config":{"mediaType":"application/vnd.oci.image.config.v1+json","digest":digest(&config),"size":config.len()},
            "layers":[{"mediaType":"application/vnd.oci.image.layer.v1.tar","digest":digest(&layer),"size":layer.len()}]})).unwrap();
        let index = serde_json::to_vec(&json!({"schemaVersion":2,"mediaType":OCI_IMAGE_INDEX_MEDIA_TYPE,
            "manifests":[{"mediaType":OCI_IMAGE_MEDIA_TYPE,"digest":digest(&manifest),"size":manifest.len(),"platform":{"architecture":"arm64","os":"linux"}}]})).unwrap();
        for bytes in [&config, &layer, &manifest, &index] {
            source
                .bytes
                .lock()
                .unwrap()
                .insert(digest(bytes), bytes.clone());
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let from: Reference = format!("{source_host}/images@{}", digest(&index))
            .parse()
            .unwrap();
        let to: Reference = format!("{target_host}/images@{}", digest(&index))
            .parse()
            .unwrap();
        target
            .fail_manifest_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let interrupted = transfer(
            &client,
            from.clone(),
            to.clone(),
            RegistryAuth::Anonymous,
            RegistryAuth::Anonymous,
            "http",
        )
        .await
        .unwrap_err();
        assert!(interrupted.retryable);
        assert!(target.published.lock().unwrap().is_empty());
        assert_eq!(
            target
                .blob_uploads
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
        transfer(
            &client,
            from.clone(),
            to.clone(),
            RegistryAuth::Anonymous,
            RegistryAuth::Anonymous,
            "http",
        )
        .await
        .unwrap();
        assert_eq!(*target.bytes.lock().unwrap(), *source.bytes.lock().unwrap());
        assert_eq!(
            target
                .blob_uploads
                .load(std::sync::atomic::Ordering::SeqCst),
            2,
            "retry must HEAD and reuse completed layers"
        );
        transfer(
            &client,
            from.clone(),
            to.clone(),
            RegistryAuth::Anonymous,
            RegistryAuth::Anonymous,
            "http",
        )
        .await
        .unwrap();
        assert_eq!(
            target
                .blob_uploads
                .load(std::sync::atomic::Ordering::SeqCst),
            2,
            "already-published manifests must read back exactly"
        );
        assert_eq!(
            *target.published.lock().unwrap(),
            vec![digest(&manifest), digest(&index)]
        );
        source
            .bytes
            .lock()
            .unwrap()
            .insert(digest(&layer), b"tampered".to_vec());
        target.bytes.lock().unwrap().clear();
        target.published.lock().unwrap().clear();
        assert!(transfer(
            &client,
            from,
            to,
            RegistryAuth::Anonymous,
            RegistryAuth::Anonymous,
            "http",
        )
        .await
        .is_err());
        assert!(
            target.published.lock().unwrap().is_empty(),
            "corrupt layer cannot publish an image or index"
        );
        source_task.abort();
        target_task.abort();
    }
}
