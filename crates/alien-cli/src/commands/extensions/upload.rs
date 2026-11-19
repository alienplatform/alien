//! Uploads the objects of the extension bundles a release pins, before the release is created:
//! each controller binary, then the core zip, so a stored core zip means its whole bundle is
//! stored. The manager or the platform checks each object against its sha256 and refuses one
//! that fails, naming the extension, and the release is then not created.

use alien_error::{AlienError, Context, ContextError, GenericError, IntoAlienError};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::release::Upload;
use crate::error::{ErrorData, Result};

/// What wraps every failed upload of `upload`, naming the extension and the object. It keeps the
/// cause's retryability and status.
fn not_uploaded(upload: &Upload) -> ErrorData {
    ErrorData::ApiRequestFailed {
        message: format!(
            "Extension '{}': its {} was not uploaded",
            upload.extension,
            object_name(upload)
        ),
        url: None,
    }
}

fn object_name(upload: &Upload) -> String {
    if upload.object == "core" {
        "core zip".to_string()
    } else {
        format!("controller for {}", upload.object)
    }
}

/// The objects of each extension, its controllers before its core zip.
fn in_upload_order(uploads: &[Upload]) -> Vec<&Upload> {
    let mut ordered: Vec<&Upload> = uploads.iter().collect();
    ordered.sort_by_key(|upload| upload.object == "core");
    ordered
}

/// The object's bytes, checked against the sha256 and size the release recorded for it.
fn read(upload: &Upload) -> Result<Vec<u8>> {
    let bytes =
        std::fs::read(&upload.path)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: upload.path.display().to_string(),
                reason: format!(
                    "the {} of extension '{}'",
                    object_name(upload),
                    upload.extension
                ),
            })?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    if sha256 != upload.sha256 || bytes.len() as u64 != upload.size {
        return Err(AlienError::new(ErrorData::ExtensionBundleRefused {
            extension: upload.extension.clone(),
            message: format!(
                "its {} at {} changed after the build: sha256 {sha256}, not {}; build it again",
                object_name(upload),
                upload.path.display(),
                upload.sha256
            ),
        }));
    }
    Ok(bytes)
}

/// A request that got no answer: another try may pass. Its URL, which may be presigned, is left
/// out.
fn unsent(upload: &Upload, error: reqwest::Error) -> AlienError<ErrorData> {
    Err::<(), _>(error.without_url())
        .into_alien_error()
        .context(ErrorData::HttpRequestFailed {
            message: format!(
                "Extension '{}': its {} was not uploaded",
                upload.extension,
                object_name(upload)
            ),
            url: None,
        })
        .unwrap_err()
}

/// A response that refused `upload`, with the API's message when its body has one.
async fn refused(upload: &Upload, response: reqwest::Response) -> AlienError<ErrorData> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    #[derive(Deserialize)]
    struct ApiError {
        message: String,
    }
    let message = match serde_json::from_str::<ApiError>(&body) {
        Ok(error) => format!("{status}: {}", error.message),
        Err(_) => format!("{status}: {body}"),
    };
    refused_with(upload, status, message)
}

/// `upload` refused with `status`, which the error carries: a server error, a throttled request
/// or a request the server timed out may pass on another try.
fn refused_with(
    upload: &Upload,
    status: reqwest::StatusCode,
    message: String,
) -> AlienError<ErrorData> {
    let mut cause = AlienError::new(GenericError { message });
    cause.http_status_code = Some(status.as_u16());
    cause.retryable = status.is_server_error()
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::REQUEST_TIMEOUT;
    ContextError::context(cause, not_uploaded(upload))
}

/// Uploads to a manager that stores bundles itself: `HEAD` each object and `POST` the ones it
/// lacks. `client` carries the manager's token.
pub(crate) async fn upload_to_manager(
    client: &reqwest::Client,
    manager_url: &str,
    project_id: &str,
    uploads: &[Upload],
) -> Result<()> {
    for upload in in_upload_order(uploads) {
        let url = alien_infra::extension_bundle::manager_object_url(
            manager_url,
            &format!("sha256:{}", upload.sha256),
        );
        let stored = client
            .head(&url)
            .query(&[("project", project_id)])
            .send()
            .await
            .map_err(|error| unsent(upload, error))?;
        if stored.status().is_success() {
            continue;
        }
        if stored.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(refused(upload, stored).await);
        }
        let response = client
            .post(&url)
            .query(&[
                ("extension", upload.extension.as_str()),
                ("object", upload.object.as_str()),
                ("project", project_id),
            ])
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(read(upload)?)
            .send()
            .await
            .map_err(|error| unsent(upload, error))?;
        if !response.status().is_success() {
            return Err(refused(upload, response).await);
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresignedUpload {
    sha256: String,
    upload_id: String,
    upload_url: String,
    content_type: String,
}

#[derive(Deserialize)]
struct PresignedUploads {
    uploads: Vec<PresignedUpload>,
}

/// Uploads to the platform API: it presigns an upload for each object the project lacks, the
/// objects go straight to storage, and publishing checks the bundle and stores it. `client`
/// carries the platform token; the presigned uploads carry their own.
pub(crate) async fn upload_to_platform(
    client: &reqwest::Client,
    api_url: &str,
    workspace: Option<&str>,
    project_id: &str,
    uploads: &[Upload],
) -> Result<()> {
    let api_url = api_url.trim_end_matches('/');
    let mut query = vec![("project", project_id.to_string())];
    if let Some(workspace) = workspace {
        query.push(("workspace", workspace.to_string()));
    }
    let storage = reqwest::Client::new();
    let mut extensions: Vec<&str> = uploads.iter().map(|u| u.extension.as_str()).collect();
    extensions.dedup();
    for extension in extensions {
        let objects: Vec<&Upload> = in_upload_order(uploads)
            .into_iter()
            .filter(|upload| upload.extension == extension)
            .collect();
        let Some(core) = objects.iter().find(|upload| upload.object == "core") else {
            return Err(AlienError::new(ErrorData::ExtensionBundleRefused {
                extension: extension.to_string(),
                message: "the release lists no core zip for it; build it again".to_string(),
            }));
        };
        let response = client
            .post(format!("{api_url}/v1/extensions/bundles/upload-urls"))
            .query(&query)
            .json(&serde_json::json!({
                "extension": extension,
                "objects": objects
                    .iter()
                    .map(|u| serde_json::json!({"object": u.object, "sha256": u.sha256, "size": u.size}))
                    .collect::<Vec<_>>(),
            }))
            .send()
            .await
            .map_err(|error| unsent(core, error))?;
        if !response.status().is_success() {
            return Err(refused(core, response).await);
        }
        let presigned: PresignedUploads = response
            .json()
            .await
            .map_err(reqwest::Error::without_url)
            .into_alien_error()
            .context(not_uploaded(core))?;
        let mut uploaded = Vec::new();
        for upload in &objects {
            let Some(presigned) = presigned.uploads.iter().find(|p| p.sha256 == upload.sha256)
            else {
                continue;
            };
            // The URL is presigned: its query is a credential, so no error carries it.
            let put = storage
                .put(&presigned.upload_url)
                .header(reqwest::header::CONTENT_TYPE, &presigned.content_type)
                .body(read(upload)?)
                .send()
                .await
                .map_err(|error| unsent(upload, error))?;
            // Storage's answer is not read: it may echo the presigned request.
            if !put.status().is_success() {
                return Err(refused_with(
                    upload,
                    put.status(),
                    format!("storage answered {}", put.status()),
                ));
            }
            uploaded.push(serde_json::json!({
                "sha256": upload.sha256,
                "uploadId": presigned.upload_id,
            }));
        }
        let response = client
            .post(format!("{api_url}/v1/extensions/bundles"))
            .query(&query)
            .json(&serde_json::json!({
                "extension": extension,
                "digest": format!("sha256:{}", core.sha256),
                "uploads": uploaded,
            }))
            .send()
            .await
            .map_err(|error| unsent(core, error))?;
        if !response.status().is_success() {
            return Err(refused(core, response).await);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{Method, MockServer};

    /// A bundle of `acme` written to `dir`: its core zip and one controller.
    fn bundle(dir: &std::path::Path) -> Vec<Upload> {
        [
            ("core", &b"core zip"[..]),
            ("linux-x64", &b"controller"[..]),
        ]
        .into_iter()
        .map(|(object, bytes)| {
            let path = dir.join(object);
            std::fs::write(&path, bytes).unwrap();
            Upload {
                extension: "acme".to_string(),
                object: object.to_string(),
                path,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                size: bytes.len() as u64,
            }
        })
        .collect()
    }

    /// An answer that asks the client to wait and send again may pass on another try; one that
    /// refuses the request does not.
    #[test]
    fn a_throttled_or_timed_out_upload_may_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = bundle(dir.path());
        for (status, retryable) in [
            (reqwest::StatusCode::TOO_MANY_REQUESTS, true),
            (reqwest::StatusCode::REQUEST_TIMEOUT, true),
            (reqwest::StatusCode::SERVICE_UNAVAILABLE, true),
            (reqwest::StatusCode::FORBIDDEN, false),
        ] {
            let error = refused_with(&uploads[0], status, format!("{status}"));
            assert_eq!(error.retryable, retryable, "{status}: {error}");
            assert_eq!(error.http_status_code, Some(status.as_u16()), "{error}");
        }
    }

    fn object_path(upload: &Upload) -> String {
        format!("/v1/extensions/bundles/sha256:{}", upload.sha256)
    }

    #[tokio::test]
    async fn a_manager_gets_the_objects_it_lacks_controllers_first() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = bundle(dir.path());
        let (core, controller) = (&uploads[0], &uploads[1]);
        let server = MockServer::start_async().await;
        let missing = server
            .mock_async(|when, then| {
                when.method(Method::HEAD);
                then.status(404);
            })
            .await;
        let posted_controller = server
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path(object_path(controller))
                    .query_param("object", "linux-x64")
                    .query_param("project", "prj_1")
                    .body("controller");
                then.status(201);
            })
            .await;
        let posted_core = server
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path(object_path(core))
                    .query_param("object", "core")
                    .body("core zip");
                then.status(201);
            })
            .await;

        upload_to_manager(
            &reqwest::Client::new(),
            &server.base_url(),
            "prj_1",
            &uploads,
        )
        .await
        .unwrap();

        missing.assert_hits_async(2).await;
        posted_controller.assert_hits_async(1).await;
        posted_core.assert_hits_async(1).await;
        // A stored core zip means its whole bundle is stored: it goes last.
        let order: Vec<&str> = in_upload_order(&uploads)
            .iter()
            .map(|upload| upload.object.as_str())
            .collect();
        assert_eq!(order, ["linux-x64", "core"]);
    }

    #[tokio::test]
    async fn a_manager_that_refuses_an_object_stops_the_release_naming_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = bundle(dir.path());
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(Method::HEAD);
                then.status(404);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(Method::POST);
                then.status(400).json_body(serde_json::json!({
                    "code": "BAD_REQUEST",
                    "message": "Extension 'acme': its bundle's controller for linux-x64 was refused"
                }));
            })
            .await;

        let error = upload_to_manager(
            &reqwest::Client::new(),
            &server.base_url(),
            "prj_1",
            &uploads,
        )
        .await
        .unwrap_err();

        let report = error.to_string();
        assert!(
            report.contains("Extension 'acme'") && report.contains("400"),
            "{report}"
        );
        assert_eq!(error.http_status_code, Some(400));
        assert!(!error.retryable);
    }

    /// A manager that fails with a server error leaves the status on the error, and marks it as
    /// one another try may pass.
    #[tokio::test]
    async fn a_server_error_keeps_its_status_and_may_be_retried() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = bundle(dir.path());
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(Method::HEAD);
                then.status(503);
            })
            .await;

        let error = upload_to_manager(
            &reqwest::Client::new(),
            &server.base_url(),
            "prj_1",
            &uploads,
        )
        .await
        .unwrap_err();

        assert_eq!(error.http_status_code, Some(503), "{error}");
        assert!(error.retryable, "{error}");
        assert!(error.to_string().contains("Extension 'acme'"), "{error}");
    }

    /// A release whose uploads list an extension's controllers and no core zip is refused,
    /// naming the extension, before anything is sent.
    #[tokio::test]
    async fn an_extension_without_its_core_zip_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let uploads: Vec<Upload> = bundle(dir.path())
            .into_iter()
            .filter(|upload| upload.object != "core")
            .collect();
        let server = MockServer::start_async().await;
        let presign = server
            .mock_async(|when, then| {
                when.method(Method::POST);
                then.status(200)
                    .json_body(serde_json::json!({"uploads": []}));
            })
            .await;

        let error = upload_to_platform(
            &reqwest::Client::new(),
            &server.base_url(),
            None,
            "prj_1",
            &uploads,
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
        assert!(error.message.contains("'acme'"), "{}", error.message);
        presign.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn a_changed_file_is_refused_before_anything_is_sent() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = bundle(dir.path());
        std::fs::write(&uploads[1].path, b"tampered!!").unwrap();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(Method::HEAD);
                then.status(404);
            })
            .await;
        let posted = server
            .mock_async(|when, then| {
                when.method(Method::POST);
                then.status(201);
            })
            .await;

        let error = upload_to_manager(
            &reqwest::Client::new(),
            &server.base_url(),
            "prj_1",
            &uploads,
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
        assert!(
            error.message.contains("changed after the build"),
            "{}",
            error.message
        );
        posted.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn the_platform_presigns_what_is_missing_and_publishes_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = bundle(dir.path());
        let (core, controller) = (&uploads[0], &uploads[1]);
        let server = MockServer::start_async().await;
        let storage = format!("{}/storage", server.base_url());
        let presign = server
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/extensions/bundles/upload-urls")
                    .query_param("project", "prj_1")
                    .query_param("workspace", "ws");
                then.status(200).json_body(serde_json::json!({"uploads": [
                    {"object": "linux-x64", "sha256": controller.sha256, "uploadId": "up-ctl",
                     "uploadUrl": format!("{storage}/up-ctl"), "contentType": "application/octet-stream"},
                    {"object": "core", "sha256": core.sha256, "uploadId": "up-core",
                     "uploadUrl": format!("{storage}/up-core"), "contentType": "application/octet-stream"}
                ]}));
            })
            .await;
        let put = server
            .mock_async(|when, then| {
                when.method(Method::PUT).path_contains("/storage/");
                then.status(200);
            })
            .await;
        let publish = server
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/extensions/bundles")
                    .json_body(serde_json::json!({
                        "extension": "acme",
                        "digest": format!("sha256:{}", core.sha256),
                        "uploads": [
                            {"sha256": controller.sha256, "uploadId": "up-ctl"},
                            {"sha256": core.sha256, "uploadId": "up-core"}
                        ]
                    }));
                then.status(201);
            })
            .await;

        upload_to_platform(
            &reqwest::Client::new(),
            &server.base_url(),
            Some("ws"),
            "prj_1",
            &uploads,
        )
        .await
        .unwrap();

        presign.assert_hits_async(1).await;
        put.assert_hits_async(2).await;
        publish.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn a_platform_that_refuses_the_bundle_stops_the_release_naming_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let uploads = bundle(dir.path());
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(Method::POST)
                    .path("/v1/extensions/bundles/upload-urls");
                then.status(200)
                    .json_body(serde_json::json!({"uploads": []}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(Method::POST).path("/v1/extensions/bundles");
                then.status(400).json_body(serde_json::json!({
                    "code": "INVALID_EXTENSION_BUNDLE",
                    "message": "Extension 'acme': its bundle is invalid: the manifest names a controller for linux-x64 that was not uploaded."
                }));
            })
            .await;

        let error = upload_to_platform(
            &reqwest::Client::new(),
            &server.base_url(),
            None,
            "prj_1",
            &uploads,
        )
        .await
        .unwrap_err();

        let report = error.to_string();
        assert!(
            report.contains("Extension 'acme'") && report.contains("not uploaded"),
            "{report}"
        );
        assert_eq!(error.http_status_code, Some(400));
    }
}
