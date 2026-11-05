use crate::error::{ErrorData, Result};
use alien_error::AlienError;
use dockdash::PushOptions;
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig};
use oci_client::errors::{OciDistributionError, OciErrorCode};
use oci_client::manifest::{
    IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE,
};
use oci_client::Reference;
use sha2::{Digest, Sha256};

const MANIFEST_MEDIA_TYPES: [&str; 4] = [
    OCI_IMAGE_INDEX_MEDIA_TYPE,
    IMAGE_MANIFEST_LIST_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE,
    IMAGE_MANIFEST_MEDIA_TYPE,
];

/// Digest of the manifest `image` names, or `None` when the registry has no such manifest.
/// Any other failure, such as a refused credential, is an error.
pub async fn manifest_digest(image: &str, options: &PushOptions) -> Result<Option<String>> {
    let reference = parse(image)?;
    match client(options)
        .fetch_manifest_digest(&reference, &options.auth)
        .await
    {
        Ok(digest) => Ok(Some(digest)),
        Err(error) if is_missing_manifest(&error) => Ok(None),
        Err(error) => Err(lookup_error(image, &error)),
    }
}

/// Points the tag in `target` at the manifest `source` names, in the same repository, and
/// returns the manifest digest. The manifest bytes are copied verbatim, so an image index keeps
/// its digest and its per-platform entries.
pub async fn tag_manifest(source: &str, target: &str, options: &PushOptions) -> Result<String> {
    let source_reference = parse(source)?;
    let target_reference = parse(target)?;
    let client = client(options);
    let (manifest, digest) = client
        .pull_manifest_raw(&source_reference, &options.auth, &MANIFEST_MEDIA_TYPES)
        .await
        .map_err(|error| lookup_error(source, &error))?;
    let media_type = crate::manifest_media_type(&manifest).unwrap_or_else(|| {
        if has_manifests_field(&manifest) {
            OCI_IMAGE_INDEX_MEDIA_TYPE.to_string()
        } else {
            OCI_IMAGE_MEDIA_TYPE.to_string()
        }
    });
    let content_type = media_type.parse().map_err(|_| {
        AlienError::new(ErrorData::ImageLookupRejected {
            image: source.to_string(),
            reason: format!("Manifest media type '{media_type}' is not a valid header"),
        })
    })?;

    client
        .store_auth_if_needed(target_reference.resolve_registry(), &options.auth)
        .await;
    client
        .push_manifest_raw(&target_reference, manifest, content_type)
        .await
        .map_err(|error| match registry_failure(&error) {
            RegistryFailure::Transient(reason) => AlienError::new(ErrorData::ImagePushFailed {
                image: target.to_string(),
                reason,
            }),
            RegistryFailure::Rejected(reason) => AlienError::new(ErrorData::ImagePushRejected {
                image: target.to_string(),
                reason,
            }),
        })?;
    Ok(digest)
}

/// The digest of the image pushed at `image`, checked against the OCI archives it was pushed
/// from. The registry's manifest, or each manifest of its index, must name exactly the config
/// and layers of one of `archives`, and the digest is computed from the bytes fetched. A
/// registry can serialize the manifest as it likes, but it can't name another image.
pub async fn verified_digest(
    image: &str,
    archives: &[std::path::PathBuf],
    options: &PushOptions,
) -> Result<String> {
    let built = archives
        .iter()
        .map(|archive| archive_content(archive))
        .collect::<Result<Vec<_>>>()?;
    let rejected = |reason: String| {
        AlienError::new(ErrorData::ImageLookupRejected {
            image: image.to_string(),
            reason,
        })
    };
    let reference = parse(image)?;
    let client = client(options);
    let (bytes, _) = client
        .pull_manifest_raw(&reference, &options.auth, &MANIFEST_MEDIA_TYPES)
        .await
        .map_err(|error| lookup_error(image, &error))?;
    let digest = sha256_digest(&bytes);
    if reference.digest().is_some_and(|named| named != digest) {
        return Err(rejected(format!(
            "the registry returned a manifest with digest {digest}"
        )));
    }
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| rejected(format!("the manifest isn't JSON: {error}")))?;
    let children = match manifest.get("manifests") {
        None => vec![bytes],
        Some(entries) => {
            let entries = entries
                .as_array()
                .filter(|entries| !entries.is_empty())
                .ok_or_else(|| rejected("the image index lists no manifests".to_string()))?;
            let mut children = Vec::new();
            for entry in entries {
                let child = entry["digest"]
                    .as_str()
                    .ok_or_else(|| rejected("an index entry has no digest".to_string()))?;
                let child_reference = parse(&format!(
                    "{}@{child}",
                    image.split_once('@').map_or(image, |(name, _)| name)
                ))?;
                let (child_bytes, _) = client
                    .pull_manifest_raw(&child_reference, &options.auth, &MANIFEST_MEDIA_TYPES)
                    .await
                    .map_err(|error| lookup_error(image, &error))?;
                if sha256_digest(&child_bytes) != child {
                    return Err(rejected(format!(
                        "the registry returned other bytes for {child}"
                    )));
                }
                children.push(child_bytes);
            }
            children
        }
    };
    for child in children {
        let content = manifest_content(&child)
            .ok_or_else(|| rejected("a manifest names no config or layers".to_string()))?;
        if !built.contains(&content) {
            return Err(rejected(
                "the registry's image isn't the one built locally".to_string(),
            ));
        }
    }
    Ok(digest)
}

/// What an image manifest runs: its config digest and its layer digests, in order.
type ImageContent = (String, Vec<String>);

fn manifest_content(manifest: &[u8]) -> Option<ImageContent> {
    let manifest: serde_json::Value = serde_json::from_slice(manifest).ok()?;
    let config = manifest["config"]["digest"].as_str()?.to_string();
    let layers = manifest["layers"]
        .as_array()?
        .iter()
        .map(|layer| layer["digest"].as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()?;
    Some((config, layers))
}

/// The config and layers of the one image in an OCI archive (`index.json` and `blobs/`).
fn archive_content(archive: &std::path::Path) -> Result<ImageContent> {
    let unreadable = |reason: &str| {
        AlienError::new(ErrorData::FileOperationFailed {
            operation: "read OCI archive".to_string(),
            file_path: archive.display().to_string(),
            reason: reason.to_string(),
        })
    };
    let read = |name: &str| -> Result<Option<Vec<u8>>> {
        let file = std::fs::File::open(archive).map_err(|_| unreadable("can't open it"))?;
        for entry in tar::Archive::new(file)
            .entries()
            .map_err(|_| unreadable("not a tar archive"))?
        {
            let mut entry = entry.map_err(|_| unreadable("not a tar archive"))?;
            let path = entry.path().map_err(|_| unreadable("unreadable path"))?;
            if path.to_string_lossy().trim_start_matches("./") == name {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut bytes)
                    .map_err(|_| unreadable("unreadable entry"))?;
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    };
    let index: serde_json::Value = read("index.json")?
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| unreadable("no index.json"))?;
    let digest = match index["manifests"].as_array().map(Vec::as_slice) {
        Some([only]) => only["digest"]
            .as_str()
            .ok_or_else(|| unreadable("index.json names a manifest without a digest"))?
            .to_string(),
        _ => return Err(unreadable("index.json must name one manifest")),
    };
    let manifest = read(&format!("blobs/{}", digest.replacen(':', "/", 1)))?
        .filter(|bytes| sha256_digest(bytes) == digest)
        .ok_or_else(|| unreadable("its manifest blob is missing or corrupt"))?;
    manifest_content(&manifest).ok_or_else(|| unreadable("its manifest names no config or layers"))
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn lookup_error(image: &str, error: &OciDistributionError) -> AlienError<ErrorData> {
    match registry_failure(error) {
        RegistryFailure::Transient(reason) => AlienError::new(ErrorData::ImageLookupFailed {
            image: image.to_string(),
            reason,
        }),
        RegistryFailure::Rejected(reason) => AlienError::new(ErrorData::ImageLookupRejected {
            image: image.to_string(),
            reason,
        }),
    }
}

pub(crate) enum RegistryFailure {
    Transient(String),
    Rejected(String),
}

/// Registry errors can carry signed URLs, so the source error is never attached or formatted:
/// only its status and error codes reach the reason.
pub(crate) fn registry_failure(error: &OciDistributionError) -> RegistryFailure {
    match error {
        OciDistributionError::RequestError(_) => {
            RegistryFailure::Transient("The registry connection failed".to_string())
        }
        OciDistributionError::ServerError { code, .. } if *code >= 500 || *code == 429 => {
            RegistryFailure::Transient(format!("Registry returned HTTP {code}"))
        }
        OciDistributionError::ServerError { code, .. } => {
            RegistryFailure::Rejected(format!("Registry returned HTTP {code}"))
        }
        OciDistributionError::UnauthorizedError { .. } => {
            RegistryFailure::Rejected("Registry authentication failed".to_string())
        }
        // Any non-200 from the token endpoint, an outage included; the status is not kept.
        OciDistributionError::AuthenticationFailure(_) => {
            RegistryFailure::Rejected("The registry token request failed".to_string())
        }
        OciDistributionError::RegistryError { envelope, .. } => {
            let codes = envelope
                .errors
                .iter()
                .map(|error| format!("{:?}", error.code))
                .collect::<Vec<_>>()
                .join(", ");
            if envelope
                .errors
                .iter()
                .any(|error| error.code == OciErrorCode::Toomanyrequests)
            {
                RegistryFailure::Transient(format!("Registry answered {codes}"))
            } else {
                RegistryFailure::Rejected(format!("Registry answered {codes}"))
            }
        }
        _ => RegistryFailure::Rejected("The registry answer was not usable".to_string()),
    }
}

fn client(options: &PushOptions) -> OciClient {
    OciClient::new(OciClientConfig {
        protocol: options.protocol.clone(),
        ..Default::default()
    })
}

fn parse(image: &str) -> Result<Reference> {
    Reference::try_from(image).map_err(|error| {
        AlienError::new(ErrorData::InvalidResourceConfig {
            resource_id: image.to_string(),
            reason: format!("Invalid image reference: {error}"),
        })
    })
}

fn is_missing_manifest(error: &OciDistributionError) -> bool {
    match error {
        // Some registries, including the one Alien embeds, answer every 404
        // with BLOB_UNKNOWN; on a manifest lookup it can only mean the
        // manifest isn't there.
        OciDistributionError::RegistryError { envelope, .. } => {
            envelope.errors.iter().any(|error| {
                matches!(
                    error.code,
                    OciErrorCode::ManifestUnknown | OciErrorCode::BlobUnknown
                )
            })
        }
        OciDistributionError::ServerError { code, .. } => *code == 404,
        OciDistributionError::ImageManifestNotFoundError(_) => true,
        _ => false,
    }
}

fn has_manifests_field(manifest: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(manifest)
        .is_ok_and(|value| value.get("manifests").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use container_registry::ContainerRegistry;
    use httpmock::prelude::*;
    use oci_client::client::ClientProtocol;
    use oci_client::secrets::RegistryAuth;
    use sha2::{Digest, Sha256};

    const INDEX: &str = r#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"sha256:1111111111111111111111111111111111111111111111111111111111111111","size":500,"platform":{"architecture":"arm64","os":"linux"}}]}"#;

    fn options() -> PushOptions {
        PushOptions {
            auth: RegistryAuth::Anonymous,
            protocol: ClientProtocol::Http,
            ..Default::default()
        }
    }

    fn digest_of(bytes: &[u8]) -> String {
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    #[tokio::test]
    async fn tagging_copies_the_manifest_bytes_under_the_new_tag() {
        let server = MockServer::start_async().await;
        let digest = digest_of(INDEX.as_bytes());
        let pull = server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/acme/sandbox/manifests/pushed");
                then.status(200)
                    .header("content-type", OCI_IMAGE_INDEX_MEDIA_TYPE)
                    .header("docker-content-digest", digest.as_str())
                    .body(INDEX);
            })
            .await;
        let push = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path("/v2/acme/sandbox/manifests/source-abc")
                    .header("content-type", OCI_IMAGE_INDEX_MEDIA_TYPE)
                    .body(INDEX);
                then.status(201).header(
                    "location",
                    format!("/v2/acme/sandbox/manifests/{digest}").as_str(),
                );
            })
            .await;

        let host = server.address().to_string();
        let tagged = tag_manifest(
            &format!("{host}/acme/sandbox:pushed"),
            &format!("{host}/acme/sandbox:source-abc"),
            &options(),
        )
        .await
        .expect("the manifest should be tagged");

        pull.assert_async().await;
        push.assert_async().await;
        assert_eq!(tagged, digest);
    }

    #[tokio::test]
    async fn a_present_tag_reports_its_digest() {
        let server = MockServer::start_async().await;
        let digest = digest_of(INDEX.as_bytes());
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::HEAD)
                    .path("/v2/acme/sandbox/manifests/source-abc");
                then.status(200)
                    .header("content-type", OCI_IMAGE_INDEX_MEDIA_TYPE)
                    .header("docker-content-digest", digest.as_str());
            })
            .await;

        let found = manifest_digest(
            &format!("{}/acme/sandbox:source-abc", server.address()),
            &options(),
        )
        .await
        .expect("the lookup should succeed");
        assert_eq!(found, Some(digest));
    }

    #[tokio::test]
    async fn an_unknown_tag_is_absent_not_an_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.path("/v2/acme/sandbox/manifests/source-abc");
                then.status(404)
                    .header("content-type", "application/json")
                    .body(
                        r#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"manifest unknown"}]}"#,
                    );
            })
            .await;

        let found = manifest_digest(
            &format!("{}/acme/sandbox:source-abc", server.address()),
            &options(),
        )
        .await
        .expect("a missing tag is an answer, not a failure");
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn a_tag_missing_from_the_embedded_registry_is_absent() {
        // What `alien release` asks before reusing a cached push against a
        // manager whose registry was reset.
        let registry = ContainerRegistry::builder()
            .build_for_testing()
            .run_in_background();
        let found = manifest_digest(
            &format!("{}/artifacts/default:api-gone", registry.bound_addr()),
            &options(),
        )
        .await
        .expect("a missing tag is not a lookup failure");
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn a_refused_lookup_is_an_error() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.path("/v2/acme/sandbox/manifests/source-abc");
                then.status(403)
                    .header("content-type", "application/json")
                    .body(r#"{"errors":[{"code":"DENIED","message":"denied"}]}"#);
            })
            .await;

        let error = manifest_digest(
            &format!("{}/acme/sandbox:source-abc", server.address()),
            &options(),
        )
        .await
        .expect_err("a refused credential must not read as a missing image");
        assert_eq!(error.code, "IMAGE_LOOKUP_REJECTED");
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn an_unavailable_registry_is_a_retryable_lookup_failure() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.path("/v2/acme/sandbox/manifests/source-abc");
                then.status(503).body("unavailable");
            })
            .await;

        let error = manifest_digest(
            &format!("{}/acme/sandbox:source-abc", server.address()),
            &options(),
        )
        .await
        .expect_err("an unavailable registry is not an answer");
        assert_eq!(error.code, "IMAGE_LOOKUP_FAILED");
        assert!(error.retryable);
    }

    /// A real OCI archive with one layer holding `contents`, and the manifest
    /// dockdash will push for it.
    async fn built_archive(dir: &std::path::Path, contents: &[u8]) -> std::path::PathBuf {
        let archive = dir.join(format!("{}.oci.tar", digest_of(contents).replace(':', "-")));
        let layer = dockdash::Layer::builder()
            .unwrap()
            .data("/app/run", contents, Some(0o755))
            .unwrap()
            .build()
            .await
            .unwrap();
        dockdash::Image::builder()
            .platform("linux", &dockdash::Arch::Amd64)
            .layer(layer)
            .output_to(archive.clone())
            .build()
            .await
            .unwrap();
        archive
    }

    /// A manifest naming `archive`'s config and layers, serialized differently
    /// from the archive's own.
    fn manifest_for(archive: &std::path::Path) -> String {
        let (config, layers) = archive_content(archive).unwrap();
        serde_json::json!({
            "schemaVersion": 2,
            "mediaType": OCI_IMAGE_MEDIA_TYPE,
            "layers": layers.iter().map(|digest| serde_json::json!({
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "digest": digest, "size": 1})).collect::<Vec<_>>(),
            "config": {"mediaType": "application/vnd.oci.image.config.v1+json",
                       "digest": config, "size": 1},
        })
        .to_string()
    }

    async fn serve_manifest(server: &MockServer, reference: &str, body: &str, media_type: &str) {
        let body = body.to_string();
        let media_type = media_type.to_string();
        let path = format!("/v2/acme/app/manifests/{reference}");
        server
            .mock_async(move |when, then| {
                when.method(GET).path(path);
                then.status(200)
                    .header("content-type", media_type.as_str())
                    .body(body);
            })
            .await;
    }

    #[tokio::test]
    async fn a_pushed_image_verifies_against_its_archive() {
        let registry = ContainerRegistry::builder()
            .build_for_testing()
            .run_in_background();
        let dir = tempfile::tempdir().unwrap();
        let archive = built_archive(dir.path(), b"app v1").await;
        let image = format!("{}/artifacts/default:web-1", registry.bound_addr());
        dockdash::Image::from_tarball(&archive)
            .unwrap()
            .push(&image, &options())
            .await
            .unwrap();

        let verified = verified_digest(&image, &[archive], &options())
            .await
            .expect("the pushed image is the built one");
        assert_eq!(
            Some(verified),
            manifest_digest(&image, &options()).await.unwrap()
        );
    }

    #[tokio::test]
    async fn a_registry_serving_another_image_is_refused() {
        let server = MockServer::start_async().await;
        let dir = tempfile::tempdir().unwrap();
        let built = built_archive(dir.path(), b"app v1").await;
        let other = built_archive(dir.path(), b"attacker").await;
        serve_manifest(
            &server,
            "web-1",
            &manifest_for(&other),
            OCI_IMAGE_MEDIA_TYPE,
        )
        .await;

        let error = verified_digest(
            &format!("{}/acme/app:web-1", server.address()),
            &[built.clone()],
            &options(),
        )
        .await
        .expect_err("another image's manifest must not be signed");
        assert_eq!(error.code, "IMAGE_LOOKUP_REJECTED");
        assert!(
            error.to_string().contains("isn't the one built locally"),
            "{error}"
        );

        // The same check passes for the honest manifest, however it's serialized.
        let honest = MockServer::start_async().await;
        let manifest = manifest_for(&built);
        serve_manifest(&honest, "web-1", &manifest, OCI_IMAGE_MEDIA_TYPE).await;
        let verified = verified_digest(
            &format!("{}/acme/app:web-1", honest.address()),
            &[built],
            &options(),
        )
        .await
        .unwrap();
        assert_eq!(verified, digest_of(manifest.as_bytes()));
    }

    #[tokio::test]
    async fn other_bytes_for_a_pinned_digest_are_refused() {
        let server = MockServer::start_async().await;
        let dir = tempfile::tempdir().unwrap();
        let built = built_archive(dir.path(), b"app v1").await;
        let named = format!("sha256:{}", "a".repeat(64));
        serve_manifest(&server, &named, &manifest_for(&built), OCI_IMAGE_MEDIA_TYPE).await;

        let error = verified_digest(
            &format!("{}/acme/app@{named}", server.address()),
            &[built],
            &options(),
        )
        .await
        .expect_err("the bytes must hash to the digest asked for");
        // The OCI client refuses these bytes itself; the digest check backs it.
        assert_eq!(error.code, "IMAGE_LOOKUP_REJECTED");
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn an_index_listing_an_image_nobody_built_is_refused() {
        let server = MockServer::start_async().await;
        let dir = tempfile::tempdir().unwrap();
        let built = built_archive(dir.path(), b"app v1").await;
        let other = built_archive(dir.path(), b"attacker").await;
        let ours = manifest_for(&built);
        let theirs = manifest_for(&other);
        for manifest in [&ours, &theirs] {
            serve_manifest(
                &server,
                &digest_of(manifest.as_bytes()),
                manifest,
                OCI_IMAGE_MEDIA_TYPE,
            )
            .await;
        }
        let index = |manifests: &[&String]| {
            serde_json::json!({
                "schemaVersion": 2,
                "mediaType": OCI_IMAGE_INDEX_MEDIA_TYPE,
                "manifests": manifests.iter().map(|m| serde_json::json!({
                    "mediaType": OCI_IMAGE_MEDIA_TYPE,
                    "digest": digest_of(m.as_bytes()), "size": m.len(),
                    "platform": {"architecture": "amd64", "os": "linux"}})).collect::<Vec<_>>(),
            })
            .to_string()
        };
        serve_manifest(
            &server,
            "honest",
            &index(&[&ours]),
            OCI_IMAGE_INDEX_MEDIA_TYPE,
        )
        .await;
        serve_manifest(
            &server,
            "mixed",
            &index(&[&ours, &theirs]),
            OCI_IMAGE_INDEX_MEDIA_TYPE,
        )
        .await;

        let host = server.address();
        verified_digest(
            &format!("{host}/acme/app:honest"),
            &[built.clone()],
            &options(),
        )
        .await
        .expect("an index of the built image verifies");
        let error = verified_digest(&format!("{host}/acme/app:mixed"), &[built], &options())
            .await
            .expect_err("every image an index lists must be one built here");
        assert!(
            error.to_string().contains("isn't the one built locally"),
            "{error}"
        );
    }
}
