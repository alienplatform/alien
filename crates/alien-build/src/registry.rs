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

enum RegistryFailure {
    Transient(String),
    Rejected(String),
}

/// Registry errors can carry signed URLs, so the source error is never attached or formatted:
/// only its status and error codes reach the reason.
fn registry_failure(error: &OciDistributionError) -> RegistryFailure {
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
}
