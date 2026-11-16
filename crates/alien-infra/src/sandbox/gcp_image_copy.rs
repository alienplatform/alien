//! Copies a sandbox image, by digest, from the manager's `/v2/` registry proxy into an Artifact
//! Registry repository in the deployment's own project.
//!
//! Agent Platform checks that the caller of the template create can read the image, and a
//! Google-owned agent pulls it later, so the image has to be somewhere the deployment's project
//! owns. Nothing is granted on the manager's side: the pull uses the deployment token the
//! controller already holds, the push uses the project's own credentials.
//!
//! Blobs are streamed, never held in memory or written to disk. Artifact Registry verifies every
//! blob's and the manifest's digest on upload, so a corrupted or substituted byte is refused by the
//! destination; the manifest's digest is also checked here against the one asked for. Blobs go in
//! one request each: Artifact Registry refuses the second chunk of a chunked upload.

use std::future::Future;

use alien_error::{AlienError, Context, IntoAlienError};
use reqwest::{header, Client, Response, StatusCode, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};

const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, \
    application/vnd.oci.image.index.v1+json, \
    application/vnd.docker.distribution.manifest.v2+json, \
    application/vnd.docker.distribution.manifest.list.v2+json";

/// Where the image is read from: the manager's registry proxy.
pub(crate) struct CopySource<'a> {
    /// The manager's base URL. The token is sent to this host only.
    pub base_url: &'a str,
    /// Repository path on the proxy, as the release names it (no host).
    pub repository: &'a str,
    /// The deployment token.
    pub token: &'a str,
}

/// Where the image is written: an Artifact Registry repository the caller owns.
pub(crate) struct CopyDestination<'a> {
    /// `https://<region>-docker.pkg.dev`.
    pub base_url: &'a str,
    /// `<project>/<repository>/<image>`.
    pub repository: &'a str,
    /// Google access token of the identity that may write to the repository.
    pub access_token: &'a str,
}

fn failed(resource_id: &str, message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::CloudPlatformError {
        message: format!("Copying the sandbox image failed: {}", message.into()),
        resource_id: Some(resource_id.to_string()),
    })
}

#[derive(Deserialize)]
struct Descriptor {
    digest: String,
}

#[derive(Deserialize)]
struct ManifestBody {
    #[serde(default, rename = "mediaType")]
    media_type: Option<String>,
    config: Option<Descriptor>,
    #[serde(default)]
    layers: Vec<Descriptor>,
    #[serde(default)]
    manifests: Vec<Descriptor>,
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

/// Resolve `reference` (a tag or a digest) on the source to the manifest's digest, without
/// copying anything.
pub(crate) async fn resolve_digest(
    http: &Client,
    source: &CopySource<'_>,
    reference: &str,
    resource_id: &str,
) -> Result<String> {
    let (_, _, digest) = get_manifest(http, source, reference, resource_id).await?;
    Ok(digest)
}

async fn get_manifest(
    http: &Client,
    source: &CopySource<'_>,
    reference: &str,
    resource_id: &str,
) -> Result<(String, Vec<u8>, String)> {
    let url = format!(
        "{}/v2/{}/manifests/{reference}",
        source.base_url.trim_end_matches('/'),
        source.repository
    );
    let response = http
        .get(&url)
        .bearer_auth(source.token)
        .header(header::ACCEPT, MANIFEST_ACCEPT)
        .send()
        .await
        .map_err(|e| {
            failed(
                resource_id,
                format!("the manager's registry is unreachable: {}", e.without_url()),
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(failed(
            resource_id,
            format!("the manager's registry refused the manifest of '{reference}' ({status})"),
        ));
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = response
        .bytes()
        .await
        .map_err(|e| {
            failed(
                resource_id,
                format!("reading the manifest: {}", e.without_url()),
            )
        })?
        .to_vec();
    let digest = sha256_digest(&bytes);
    if reference.starts_with("sha256:") && reference != digest {
        return Err(failed(
            resource_id,
            format!("the manager's registry answered a manifest whose digest is {digest}, not {reference}"),
        ));
    }
    let media_type = serde_json::from_slice::<ManifestBody>(&bytes)
        .ok()
        .and_then(|m| m.media_type)
        .or(content_type)
        .unwrap_or_else(|| "application/vnd.oci.image.manifest.v1+json".to_string());
    Ok((media_type, bytes, digest))
}

async fn head_ok(http: &Client, url: &str, token: &str, resource_id: &str) -> Result<bool> {
    let response = http
        .head(url)
        .bearer_auth(token)
        .header(header::ACCEPT, MANIFEST_ACCEPT)
        .send()
        .await
        .map_err(|e| {
            failed(
                resource_id,
                format!(
                    "the destination registry is unreachable: {}",
                    e.without_url()
                ),
            )
        })?;
    match response.status() {
        s if s.is_success() => Ok(true),
        StatusCode::NOT_FOUND => Ok(false),
        // A missing repository can be hidden behind a 403 by some registries: the push that
        // follows reports a real permission failure.
        StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => Ok(false),
        s => Err(failed(
            resource_id,
            format!("the destination registry answered {s}"),
        )),
    }
}

async fn push_blob(
    http: &Client,
    source: &CopySource<'_>,
    destination: &CopyDestination<'_>,
    digest: &str,
    resource_id: &str,
) -> Result<()> {
    let dest_base = destination.base_url.trim_end_matches('/');
    let exists_url = format!("{dest_base}/v2/{}/blobs/{digest}", destination.repository);
    if head_ok(http, &exists_url, destination.access_token, resource_id).await? {
        return Ok(());
    }
    let pull = http
        .get(format!(
            "{}/v2/{}/blobs/{digest}",
            source.base_url.trim_end_matches('/'),
            source.repository
        ))
        .bearer_auth(source.token)
        .send()
        .await
        .map_err(|e| failed(resource_id, format!("pulling a layer: {}", e.without_url())))?;
    if !pull.status().is_success() {
        return Err(failed(
            resource_id,
            format!(
                "the manager's registry refused layer {digest} ({})",
                pull.status()
            ),
        ));
    }
    let length = pull.content_length();
    let start = http
        .post(format!(
            "{dest_base}/v2/{}/blobs/uploads/",
            destination.repository
        ))
        .bearer_auth(destination.access_token)
        .header(header::CONTENT_LENGTH, 0)
        .send()
        .await
        .map_err(|e| {
            failed(
                resource_id,
                format!("starting an upload: {}", e.without_url()),
            )
        })?;
    if !start.status().is_success() {
        return Err(failed(
            resource_id,
            format!(
                "the destination repository refused an upload ({})",
                start.status()
            ),
        ));
    }
    let location = start
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| failed(resource_id, "the destination gave no upload location"))?;
    let mut upload = Url::parse(dest_base)
        .and_then(|base| base.join(location))
        .map_err(|_| {
            failed(
                resource_id,
                "the destination gave an invalid upload location",
            )
        })?;
    upload.query_pairs_mut().append_pair("digest", digest);
    let stream = pull.bytes_stream();
    let mut put = http
        .put(upload)
        .bearer_auth(destination.access_token)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(reqwest::Body::wrap_stream(stream));
    if let Some(length) = length {
        put = put.header(header::CONTENT_LENGTH, length);
    }
    let done = put.send().await.map_err(|e| {
        failed(
            resource_id,
            format!("uploading layer {digest}: {}", e.without_url()),
        )
    })?;
    if !done.status().is_success() {
        return Err(failed(
            resource_id,
            format!("the destination refused layer {digest} ({})", done.status()),
        ));
    }
    Ok(())
}

async fn push_manifest(
    http: &Client,
    destination: &CopyDestination<'_>,
    reference: &str,
    media_type: &str,
    bytes: &[u8],
    resource_id: &str,
) -> Result<()> {
    let response: Response = http
        .put(format!(
            "{}/v2/{}/manifests/{reference}",
            destination.base_url.trim_end_matches('/'),
            destination.repository
        ))
        .bearer_auth(destination.access_token)
        .header(header::CONTENT_TYPE, media_type)
        .body(bytes.to_vec())
        .send()
        .await
        .map_err(|e| {
            failed(
                resource_id,
                format!("pushing the manifest: {}", e.without_url()),
            )
        })?;
    if !response.status().is_success() {
        return Err(failed(
            resource_id,
            format!(
                "the destination refused the manifest ({})",
                response.status()
            ),
        ));
    }
    Ok(())
}

/// Copy the image at `reference` and everything it names to `destination`, then give it `keep_tag`
/// so a cleanup of untagged versions never takes it. Returns the manifest digest, which is the
/// reference the template must use. Every step is idempotent by digest.
pub(crate) fn copy_image<'a>(
    http: &'a Client,
    source: &'a CopySource<'a>,
    destination: &'a CopyDestination<'a>,
    reference: &'a str,
    keep_tag: Option<&'a str>,
    resource_id: &'a str,
) -> std::pin::Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let (media_type, bytes, digest) =
            get_manifest(http, source, reference, resource_id).await?;
        let body: ManifestBody = serde_json::from_slice(&bytes).into_alien_error().context(
            ErrorData::CloudPlatformError {
                message: "Copying the sandbox image failed: the manifest is not JSON".to_string(),
                resource_id: Some(resource_id.to_string()),
            },
        )?;
        let manifest_url = format!(
            "{}/v2/{}/manifests/{digest}",
            destination.base_url.trim_end_matches('/'),
            destination.repository
        );
        // A manifest only exists in a repository once every blob it names does.
        if !head_ok(http, &manifest_url, destination.access_token, resource_id).await? {
            for child in &body.manifests {
                copy_image(http, source, destination, &child.digest, None, resource_id).await?;
            }
            let blobs = body.config.iter().chain(body.layers.iter());
            for blob in blobs {
                push_blob(http, source, destination, &blob.digest, resource_id).await?;
            }
            push_manifest(http, destination, &digest, &media_type, &bytes, resource_id).await?;
        }
        if let Some(tag) = keep_tag {
            push_manifest(http, destination, tag, &media_type, &bytes, resource_id).await?;
        }
        Ok(digest)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    const LAYER: &[u8] = b"layer bytes";
    const CONFIG: &[u8] = b"{}";

    fn manifest() -> (String, String) {
        let body = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"digest": sha256_digest(CONFIG)},
            "layers": [{"digest": sha256_digest(LAYER)}],
        })
        .to_string();
        let digest = sha256_digest(body.as_bytes());
        (body, digest)
    }

    #[tokio::test]
    async fn copies_manifest_and_layers_by_digest_and_tags_it() {
        let (body, digest) = manifest();
        let source = MockServer::start();
        let dest = MockServer::start();
        source.mock(|w, t| {
            w.method(GET)
                .path("/v2/repo/img/manifests/v1")
                .header("authorization", "Bearer dep-token");
            t.status(200).body(&body);
        });
        for blob in [CONFIG, LAYER] {
            let d = sha256_digest(blob);
            source.mock(|w, t| {
                w.method(GET)
                    .path(format!("/v2/repo/img/blobs/{d}"))
                    .header("authorization", "Bearer dep-token");
                t.status(200).body(blob);
            });
            dest.mock(|w, t| {
                w.method(httpmock::Method::HEAD)
                    .path(format!("/v2/p/r/img/blobs/{d}"));
                t.status(404);
            });
            dest.mock(|w, t| {
                w.method(PUT)
                    .path("/upload")
                    .query_param("digest", &d)
                    .header("authorization", "Bearer g-token")
                    .body(std::str::from_utf8(blob).unwrap());
                t.status(201);
            });
        }
        dest.mock(|w, t| {
            w.method(httpmock::Method::HEAD)
                .path(format!("/v2/p/r/img/manifests/{digest}"));
            t.status(404);
        });
        dest.mock(|w, t| {
            w.method(POST).path("/v2/p/r/img/blobs/uploads/");
            t.status(202).header("location", "/upload");
        });
        let by_digest = dest.mock(|w, t| {
            w.method(PUT)
                .path(format!("/v2/p/r/img/manifests/{digest}"))
                .body(&body);
            t.status(201);
        });
        let by_tag = dest.mock(|w, t| {
            w.method(PUT)
                .path("/v2/p/r/img/manifests/d-keep")
                .body(&body);
            t.status(201);
        });
        let got = copy_image(
            &Client::new(),
            &CopySource {
                base_url: &source.base_url(),
                repository: "repo/img",
                token: "dep-token",
            },
            &CopyDestination {
                base_url: &dest.base_url(),
                repository: "p/r/img",
                access_token: "g-token",
            },
            "v1",
            Some("d-keep"),
            "sbx",
        )
        .await
        .expect("copy");
        assert_eq!(got, digest);
        by_digest.assert();
        by_tag.assert();
    }

    #[tokio::test]
    async fn an_existing_digest_skips_the_blobs_but_keeps_the_tag() {
        let (body, digest) = manifest();
        let source = MockServer::start();
        let dest = MockServer::start();
        source.mock(|w, t| {
            w.method(GET)
                .path(format!("/v2/repo/img/manifests/{digest}"));
            t.status(200).body(&body);
        });
        dest.mock(|w, t| {
            w.method(httpmock::Method::HEAD)
                .path(format!("/v2/p/r/img/manifests/{digest}"));
            t.status(200);
        });
        let by_tag = dest.mock(|w, t| {
            w.method(PUT).path("/v2/p/r/img/manifests/d-keep");
            t.status(201);
        });
        copy_image(
            &Client::new(),
            &CopySource {
                base_url: &source.base_url(),
                repository: "repo/img",
                token: "t",
            },
            &CopyDestination {
                base_url: &dest.base_url(),
                repository: "p/r/img",
                access_token: "g",
            },
            &digest,
            Some("d-keep"),
            "sbx",
        )
        .await
        .expect("copy");
        by_tag.assert();
    }

    #[tokio::test]
    async fn a_manifest_that_is_not_the_digest_asked_for_is_refused_and_nothing_is_written() {
        let (body, _) = manifest();
        let other = sha256_digest(b"something else");
        let source = MockServer::start();
        let dest = MockServer::start();
        source.mock(|w, t| {
            w.method(GET)
                .path(format!("/v2/repo/img/manifests/{other}"));
            t.status(200).body(&body);
        });
        let any = dest.mock(|w, t| {
            w.path_matches(regex::Regex::new(".*").unwrap());
            t.status(200);
        });
        let err = copy_image(
            &Client::new(),
            &CopySource {
                base_url: &source.base_url(),
                repository: "repo/img",
                token: "t",
            },
            &CopyDestination {
                base_url: &dest.base_url(),
                repository: "p/r/img",
                access_token: "g",
            },
            &other,
            None,
            "sbx",
        )
        .await
        .expect_err("digest mismatch");
        assert!(err.to_string().contains("whose digest is"), "{err}");
        any.assert_hits(0);
    }

    #[tokio::test]
    async fn a_refusal_from_the_manager_names_the_status_and_not_the_token() {
        let source = MockServer::start();
        let dest = MockServer::start();
        source.mock(|w, t| {
            w.method(GET);
            t.status(403);
        });
        let err = copy_image(
            &Client::new(),
            &CopySource {
                base_url: &source.base_url(),
                repository: "repo/img",
                token: "secret-deployment-token",
            },
            &CopyDestination {
                base_url: &dest.base_url(),
                repository: "p/r/img",
                access_token: "g",
            },
            "v1",
            None,
            "sbx",
        )
        .await
        .expect_err("refused");
        let text = format!("{err:?} {err}");
        assert!(text.contains("403"), "{text}");
        assert!(!text.contains("secret-deployment-token"), "{text}");
    }

    /// The manager's registry may redirect a blob to a storage URL on another host. The deployment
    /// token must not follow the redirect.
    #[tokio::test]
    async fn the_deployment_token_is_not_sent_to_another_host_on_a_redirect() {
        let (body, digest) = manifest();
        let source = MockServer::start();
        let storage = MockServer::start();
        let dest = MockServer::start();
        source.mock(|w, t| {
            w.method(GET).path("/v2/repo/img/manifests/v1");
            t.status(200).body(&body);
        });
        let layer = sha256_digest(LAYER);
        let config = sha256_digest(CONFIG);
        source.mock(|w, t| {
            w.method(GET).path(format!("/v2/repo/img/blobs/{layer}"));
            t.status(307).header("location", storage.url("/blob"));
        });
        source.mock(|w, t| {
            w.method(GET).path(format!("/v2/repo/img/blobs/{config}"));
            t.status(200).body(CONFIG);
        });
        let leaked = storage.mock(|w, t| {
            w.method(GET).path("/blob").header_exists("authorization");
            t.status(200).body(LAYER);
        });
        let clean = storage.mock(|w, t| {
            w.method(GET).path("/blob");
            t.status(200).body(LAYER);
        });
        dest.mock(|w, t| {
            w.method(httpmock::Method::HEAD);
            t.status(404);
        });
        dest.mock(|w, t| {
            w.method(POST);
            t.status(202).header("location", "/upload");
        });
        dest.mock(|w, t| {
            w.method(PUT);
            t.status(201);
        });
        let got = copy_image(
            &Client::new(),
            &CopySource {
                base_url: &source.base_url(),
                repository: "repo/img",
                token: "dep-token",
            },
            &CopyDestination {
                base_url: &dest.base_url(),
                repository: "p/r/img",
                access_token: "g",
            },
            "v1",
            None,
            "sbx",
        )
        .await
        .expect("copy");
        assert_eq!(got, digest);
        leaked.assert_hits(0);
        assert!(clean.hits() >= 1);
    }
}
