//! Check site registry destinations before transferring image layers.

use std::{collections::BTreeMap, time::Duration};

use alien_cli_common::airgap::{self, BundleImage, RegistryAccess};
use alien_error::{AlienError, Context, IntoAlienError};
use oci_client::{
    client::{ClientConfig, ClientProtocol},
    secrets::RegistryAuth,
    Client, Reference, RegistryOperation,
};
use reqwest::{redirect::Policy, RequestBuilder, StatusCode, Url};

use crate::error::{ErrorData, Result};

pub(super) fn repositories(
    images: &[BundleImage],
    prefix: &str,
) -> Result<BTreeMap<String, Vec<Reference>>> {
    let mut repositories = BTreeMap::<String, Vec<Reference>>::new();
    for image in airgap::site_references(images, prefix).into_values() {
        let reference: Reference = image
            .parse()
            .into_alien_error()
            .context(error(format!("Invalid image destination {image}")))?;
        let name = format!("{}/{}", reference.registry(), reference.repository());
        repositories.entry(name).or_default().push(reference);
    }
    Ok(repositories)
}

fn error(message: impl Into<String>) -> ErrorData {
    ErrorData::RegistryPreflightFailed {
        message: message.into(),
        failures: Vec::new(),
    }
}

fn authenticate(
    request: RequestBuilder,
    token: Option<&str>,
    auth: &RegistryAuth,
) -> RequestBuilder {
    match (token, auth) {
        (Some(token), _) => request.bearer_auth(token),
        (None, RegistryAuth::Basic(user, password)) => request.basic_auth(user, Some(password)),
        (None, RegistryAuth::Anonymous) => request,
        (None, RegistryAuth::Bearer(token)) => request.bearer_auth(token),
    }
}

pub(super) async fn check(
    repositories: &BTreeMap<String, Vec<Reference>>,
    access: &RegistryAccess,
) -> Result<()> {
    let oci = Client::new(ClientConfig {
        protocol: if access.insecure {
            ClientProtocol::Http
        } else {
            ClientProtocol::Https
        },
        read_timeout: Some(Duration::from_secs(30)),
        connect_timeout: Some(Duration::from_secs(10)),
        ..Default::default()
    });
    // Do not forward registry credentials to redirects returned by a proxy.
    let http = reqwest::Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .into_alien_error()
        .context(error("Preparing registry checks"))?;
    let mut failures = Vec::new();
    for (name, images) in repositories {
        if let Err(error) = check_repository(name, images, access, &oci, &http).await {
            failures.push(error.into_generic());
        }
    }
    if !failures.is_empty() {
        let details = failures
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  ");
        return Err(AlienError::new(ErrorData::RegistryPreflightFailed {
            message: format!(
                "These repositories could not be verified for upload:\n  {details}\nCreate missing repositories or correct registry access before running sync again. For Amazon ECR, configure a repository creation template for the site prefix with appliedFor=[\"CREATE_ON_PUSH\"]. No image layers were uploaded."
            ),
            failures,
        }));
    }
    Ok(())
}

async fn check_repository(
    name: &str,
    images: &[Reference],
    access: &RegistryAccess,
    oci: &Client,
    http: &reqwest::Client,
) -> Result<()> {
    let Some(image) = images.first() else {
        return Ok(());
    };
    let mut exists = false;
    for image in images {
        if oci.fetch_manifest_digest(image, &access.auth).await.is_ok() {
            exists = true;
            break;
        }
    }
    // An existing manifest proves that the repository exists. This also
    // preserves read-only access when every required image is already there.
    if exists {
        return Ok(());
    }
    let token = oci
        .auth(image, &access.auth, RegistryOperation::Push)
        .await
        .into_alien_error()
        .context(error(format!("Authenticating for {name}")))?;
    let scheme = if access.insecure { "http" } else { "https" };
    let url: Url = format!(
        "{scheme}://{}/v2/{}/blobs/uploads/",
        image.resolve_registry(),
        image.repository()
    )
    .parse()
    .into_alien_error()
    .context(error(format!("Invalid registry URL for {name}")))?;
    // Starting an empty upload also works with registries that create a
    // repository on push. Checking tags alone would reject those installs.
    let response = authenticate(http.post(url.clone()), token.as_deref(), &access.auth)
        .header("Content-Length", 0)
        .send()
        .await
        .into_alien_error()
        .context(error(format!("Checking uploads to {name}")))?;
    if response.status() != StatusCode::ACCEPTED {
        return Err(AlienError::new(error(format!(
            "{name} (HTTP {})",
            response.status().as_u16()
        ))));
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|header| header.to_str().ok())
        .ok_or_else(|| {
            AlienError::new(error(format!(
                "{name} accepted an upload without a Location header"
            )))
        })?;
    let location = url
        .join(location)
        .into_alien_error()
        .context(error(format!("Invalid upload location from {name}")))?;
    if location.origin() != url.origin() {
        tracing::debug!(
            repository = name,
            "Skipping empty upload cancellation on another origin"
        );
        return Ok(());
    }
    let cancelled = authenticate(http.delete(location), token.as_deref(), &access.auth)
        .header("Content-Length", 0)
        .send()
        .await;
    // Cancellation is optional on some registries. No image bytes were sent;
    // inability to cancel an empty session does not invalidate push access.
    match cancelled {
        Ok(response)
            if response.status().is_success() || response.status() == StatusCode::NOT_FOUND => {}
        Ok(response) => {
            tracing::debug!(repository = name, status = %response.status(), "Registry did not cancel the empty preflight upload")
        }
        Err(error) => {
            tracing::debug!(repository = name, error = %error.without_url(), "Could not cancel the empty preflight upload")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use httpmock::{
        prelude::*,
        Method::{HEAD, PATCH},
    };

    use super::*;

    fn image(repository: &str) -> BundleImage {
        BundleImage {
            source: format!("source.example.com/{repository}:1"),
            repository: repository.into(),
            tag: Some("1".into()),
            digest: format!("sha256:{}", "a".repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        }
    }

    #[tokio::test]
    async fn preflight_allows_create_on_push_and_cancels_without_sending_layers() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/");
                then.status(200);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(HEAD);
                then.status(404);
            })
            .await;
        let upload = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/site/app/blobs/uploads/")
                    .header("authorization", "Basic dXNlcjpwYXNzd29yZA==")
                    .body("");
                then.status(202)
                    .header("Location", "/v2/site/app/blobs/uploads/probe?session=one");
            })
            .await;
        let cancel = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v2/site/app/blobs/uploads/probe")
                    .query_param("session", "one");
                then.status(204);
            })
            .await;
        let layers = server
            .mock_async(|when, then| {
                when.method(PUT);
                then.status(500);
            })
            .await;
        let parts = server
            .mock_async(|when, then| {
                when.method(PATCH);
                then.status(500);
            })
            .await;
        let destinations = repositories(
            &[image("app"), image("app")],
            &format!("{}/site", server.address()),
        )
        .unwrap();
        check(
            &destinations,
            &RegistryAccess {
                auth: RegistryAuth::Basic("user".into(), "password".into()),
                insecure: true,
            },
        )
        .await
        .unwrap();
        upload.assert_hits_async(1).await;
        cancel.assert_hits_async(1).await;
        layers.assert_hits_async(0).await;
        parts.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn preflight_reports_every_rejected_repository_before_uploading_images() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/");
                then.status(200);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(HEAD);
                then.status(404);
            })
            .await;
        let missing = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/site/app/blobs/uploads/")
                    .body("");
                then.status(404);
            })
            .await;
        let denied = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/site/tools/operator/blobs/uploads/")
                    .body("");
                then.status(403);
            })
            .await;
        let destinations = repositories(
            &[image("app"), image("tools/operator")],
            &format!("{}/site/", server.address()),
        )
        .unwrap();
        let error = check(
            &destinations,
            &RegistryAccess {
                auth: RegistryAuth::Anonymous,
                insecure: true,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "REGISTRY_PREFLIGHT_FAILED");
        let message = error.to_string();
        for expected in [
            "site/app (HTTP 404)",
            "site/tools/operator (HTTP 403)",
            "CREATE_ON_PUSH",
            "No image layers were uploaded",
        ] {
            assert!(message.contains(expected), "{message}");
        }
        missing.assert_hits_async(1).await;
        denied.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn preflight_keeps_repository_failures_when_later_authentication_fails() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/");
                then.status(401).header(
                    "www-authenticate",
                    format!(
                        "Bearer realm=\"{}\",service=\"registry\"",
                        server.url("/token")
                    ),
                );
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(HEAD);
                then.status(404);
            })
            .await;
        for repository in ["app", "last"] {
            server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path("/token")
                        .query_param("scope", format!("repository:site/{repository}:pull,push"));
                    then.status(200)
                        .json_body(serde_json::json!({"token": "push-token"}));
                })
                .await;
        }
        let denied = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/token")
                    .query_param("scope", "repository:site/denied:pull,push");
                then.status(403).body("push access denied");
            })
            .await;
        let missing = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/site/app/blobs/uploads/")
                    .header("authorization", "Bearer push-token")
                    .body("");
                then.status(404);
            })
            .await;
        let last = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v2/site/last/blobs/uploads/")
                    .body("");
                then.status(202)
                    .header("location", "/v2/site/last/blobs/uploads/probe");
            })
            .await;
        let cancel = server
            .mock_async(|when, then| {
                when.method(DELETE)
                    .path("/v2/site/last/blobs/uploads/probe");
                then.status(204);
            })
            .await;
        let destinations = repositories(
            &[image("app"), image("denied"), image("last")],
            &format!("{}/site", server.address()),
        )
        .unwrap();
        let error = check(
            &destinations,
            &RegistryAccess {
                auth: RegistryAuth::Anonymous,
                insecure: true,
            },
        )
        .await
        .unwrap_err();
        let message = error.to_string();
        for expected in [
            "site/app (HTTP 404)",
            "site/denied",
            "CREATE_ON_PUSH",
            "push access denied",
        ] {
            assert!(message.contains(expected), "{message}");
        }
        let failures = error.context.as_ref().unwrap()["failures"]
            .as_array()
            .unwrap();
        assert_eq!(failures.len(), 2);
        assert!(failures[1]["source"]
            .to_string()
            .contains("push access denied"));
        missing.assert_hits_async(1).await;
        denied.assert_hits_async(1).await;
        last.assert_hits_async(1).await;
        cancel.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn existing_repository_needs_no_write_probe() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v2/");
                then.status(200);
            })
            .await;
        let manifest = server
            .mock_async(|when, then| {
                when.method(HEAD);
                then.status(200).header(
                    "Docker-Content-Digest",
                    format!("sha256:{}", "a".repeat(64)),
                );
            })
            .await;
        let upload = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(403);
            })
            .await;
        let destinations = repositories(&[image("app")], &server.address().to_string()).unwrap();
        check(
            &destinations,
            &RegistryAccess {
                auth: RegistryAuth::Anonymous,
                insecure: true,
            },
        )
        .await
        .unwrap();
        manifest.assert_hits_async(1).await;
        upload.assert_hits_async(0).await;
    }
}
