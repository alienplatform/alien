//! Digest-preserving OCI closure transfer between an owned registry's regional endpoints.
use std::{collections::HashSet, time::Duration};

use futures::TryStreamExt;
use oci_client::{
    client::ClientConfig,
    manifest::{
        OciImageIndex, OciImageManifest, IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE,
        OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE,
    },
    secrets::RegistryAuth,
    Client, Reference,
};
use sha2::{Digest, Sha256};

const TYPES: [&str; 4] = [
    OCI_IMAGE_INDEX_MEDIA_TYPE,
    IMAGE_MANIFEST_LIST_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE,
    IMAGE_MANIFEST_MEDIA_TYPE,
];
const MAX_MANIFESTS: usize = 128;
const MAX_BLOB_BYTES: usize = 512 * 1024 * 1024;

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// The post-order traversal publishes children before their index, preserving every raw byte.
/// No tag is resolved and no external descriptor URL is followed.
pub(super) async fn copy_closure(
    source: Reference,
    target: Reference,
    source_auth: RegistryAuth,
    target_auth: RegistryAuth,
) -> Result<(), String> {
    let client = Client::new(ClientConfig {
        connect_timeout: Some(Duration::from_secs(10)),
        read_timeout: Some(Duration::from_secs(30)),
        use_monolithic_push: true,
        ..Default::default()
    });
    transfer(&client, source, target, source_auth, target_auth).await
}

async fn transfer(
    client: &Client,
    source: Reference,
    target: Reference,
    source_auth: RegistryAuth,
    target_auth: RegistryAuth,
) -> Result<(), String> {
    let expected = source
        .digest()
        .ok_or("regional images must be digest pinned")?;
    if target.digest() != Some(expected) {
        return Err("source and target digests must match".into());
    }
    client
        .store_auth_if_needed(source.resolve_registry(), &source_auth)
        .await;
    client
        .store_auth_if_needed(target.resolve_registry(), &target_auth)
        .await;
    let mut pending = vec![(expected.to_string(), false)];
    let mut visited = HashSet::new();
    let mut manifests = std::collections::HashMap::new();
    while let Some((current, publish)) = pending.pop() {
        let from = source.clone_with_digest(current.clone());
        let to = target.clone_with_digest(current.clone());
        if publish {
            let (raw, media_type): (Vec<u8>, String) = manifests
                .remove(&current)
                .ok_or("missing index publication state")?;
            client
                .push_manifest_raw(
                    &to,
                    raw,
                    media_type.parse().map_err(|_| "invalid media type")?,
                )
                .await
                .map_err(|_| "regional index upload failed")?;
        } else {
            if !visited.insert(current.clone()) {
                continue;
            }
            if visited.len() > MAX_MANIFESTS {
                return Err("image manifest closure exceeds 128 entries".into());
            }
            let (raw, _) = client
                .pull_manifest_raw(&from, &source_auth, &TYPES)
                .await
                .map_err(|_| "source manifest download failed")?;
            if digest(&raw) != current {
                return Err("source manifest digest mismatch".into());
            }
            let document: serde_json::Value =
                serde_json::from_slice(&raw).map_err(|_| "invalid image manifest")?;
            let media_type = document
                .get("mediaType")
                .and_then(serde_json::Value::as_str)
                .ok_or("manifest mediaType is required")?
                .to_string();
            if [OCI_IMAGE_INDEX_MEDIA_TYPE, IMAGE_MANIFEST_LIST_MEDIA_TYPE]
                .contains(&media_type.as_str())
            {
                let index: OciImageIndex =
                    serde_json::from_slice(&raw).map_err(|_| "invalid image index")?;
                if index.manifests.is_empty() {
                    return Err("empty image index".into());
                }
                pending.push((current.clone(), true));
                for child in index.manifests.iter().rev() {
                    if visited.contains(&child.digest) && manifests.contains_key(&child.digest) {
                        return Err("cyclic image index".into());
                    }
                    pending.push((child.digest.clone(), false));
                }
                manifests.insert(current, (raw, media_type));
                continue;
            }
            if ![OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE].contains(&media_type.as_str()) {
                return Err("unsupported image manifest media type".into());
            }
            let image: OciImageManifest =
                serde_json::from_slice(&raw).map_err(|_| "invalid image manifest")?;
            for descriptor in std::iter::once(&image.config).chain(image.layers.iter()) {
                if descriptor
                    .urls
                    .as_ref()
                    .is_some_and(|urls| !urls.is_empty())
                {
                    return Err(
                        "external image blob URLs are unsupported for regional copies".into(),
                    );
                }
                let size =
                    usize::try_from(descriptor.size).map_err(|_| "invalid image blob size")?;
                if size > MAX_BLOB_BYTES {
                    return Err("regional image blob exceeds 512 MiB".into());
                }
                let mut stream = client
                    .pull_blob_stream(&from, descriptor)
                    .await
                    .map_err(|_| "source image blob download failed")?;
                let mut bytes = Vec::new();
                while let Some(chunk) = stream
                    .try_next()
                    .await
                    .map_err(|_| "source image blob stream failed")?
                {
                    if bytes.len().saturating_add(chunk.len()) > size {
                        return Err("image blob exceeds declared size".into());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                if bytes.len() != size || digest(&bytes) != descriptor.digest {
                    return Err("source image blob size or digest mismatch".into());
                }
                client
                    .push_blob(&to, &bytes, &descriptor.digest)
                    .await
                    .map_err(|_| "regional image blob upload failed")?;
            }
            client
                .push_manifest_raw(
                    &to,
                    raw,
                    media_type.parse().map_err(|_| "invalid media type")?,
                )
                .await
                .map_err(|_| "regional manifest upload failed")?;
        }
        let (readback, _) = client
            .pull_manifest_raw(&to, &target_auth, &TYPES)
            .await
            .map_err(|_| "regional manifest readback failed")?;
        if digest(&readback) != current {
            return Err("regional manifest digest mismatch".into());
        }
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
    use oci_client::client::ClientProtocol;
    use serde_json::json;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct Registry {
        bytes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        published: Arc<Mutex<Vec<String>>>,
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
                    .body(Body::from(bytes))
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
    async fn copies_index_closure_exactly_and_refuses_corrupt_source() {
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
        let client = Client::new(ClientConfig {
            protocol: ClientProtocol::Http,
            use_monolithic_push: true,
            ..Default::default()
        });
        let from: Reference = format!("{source_host}/images@{}", digest(&index))
            .parse()
            .unwrap();
        let to: Reference = format!("{target_host}/images@{}", digest(&index))
            .parse()
            .unwrap();
        transfer(
            &client,
            from.clone(),
            to.clone(),
            RegistryAuth::Anonymous,
            RegistryAuth::Anonymous,
        )
        .await
        .unwrap();
        assert_eq!(*target.bytes.lock().unwrap(), *source.bytes.lock().unwrap());
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
            RegistryAuth::Anonymous
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
