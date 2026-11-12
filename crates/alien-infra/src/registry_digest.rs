//! Reads the digest a registry holds for a tag.

use std::time::Duration;

use alien_error::AlienError;

use crate::error::{ErrorData, Result};

const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.index.v1+json, \
application/vnd.oci.image.manifest.v1+json, \
application/vnd.docker.distribution.manifest.list.v2+json, \
application/vnd.docker.distribution.manifest.v2+json";

fn unavailable(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: None,
    })
}

/// The `sha256:` digest of `repository:reference` at `registry_origin`, from a
/// manifest HEAD request that carries the runtime pull `token`. The token and
/// the response body never appear in an error.
pub(crate) async fn fetch_manifest_digest(
    registry_origin: &str,
    repository: &str,
    reference: &str,
    token: &str,
) -> Result<String> {
    let url = format!(
        "{}/v2/{repository}/manifests/{reference}",
        registry_origin.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| unavailable("Cannot build the registry client"))?;
    let response = client
        .head(&url)
        .basic_auth("deployment", Some(token))
        .header(reqwest::header::ACCEPT, MANIFEST_ACCEPT)
        .send()
        .await
        .map_err(|_| unavailable("Cannot reach the registry to resolve the image digest"))?;
    if !response.status().is_success() {
        return Err(unavailable(&format!(
            "The registry refused the image digest lookup ({})",
            response.status()
        )));
    }
    let digest = response
        .headers()
        .get("docker-content-digest")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| unavailable("The registry did not return an image digest"))?;
    match digest.strip_prefix("sha256:") {
        Some(hex) if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(digest.to_string())
        }
        _ => Err(unavailable("The registry returned an invalid image digest")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[tokio::test]
    async fn reads_the_digest_header_with_the_pull_token() {
        let server = MockServer::start_async().await;
        let head = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::HEAD)
                    .path("/v2/runtime/app/manifests/v1")
                    .header_exists("authorization");
                then.status(200).header("Docker-Content-Digest", DIGEST);
            })
            .await;
        let digest = fetch_manifest_digest(&server.base_url(), "runtime/app", "v1", "pull-token")
            .await
            .unwrap();
        assert_eq!(digest, DIGEST);
        head.assert_async().await;
    }

    #[tokio::test]
    async fn refuses_a_missing_or_malformed_digest_and_a_refused_lookup() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::HEAD)
                    .path("/v2/a/manifests/none");
                then.status(200);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::HEAD)
                    .path("/v2/a/manifests/bad");
                then.status(200)
                    .header("Docker-Content-Digest", "sha256:short");
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::HEAD)
                    .path("/v2/a/manifests/gone");
                then.status(404);
            })
            .await;
        for reference in ["none", "bad", "gone"] {
            let error = fetch_manifest_digest(&server.base_url(), "a", reference, "secret-token")
                .await
                .unwrap_err();
            assert!(!error.to_string().contains("secret-token"), "{error}");
        }
    }
}
