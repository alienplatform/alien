//! The bundle a sandbox's MicroVM image is built from.
//!
//! `CreateMicrovmImage` builds from a zip holding a Dockerfile, read from S3 in the image's own
//! region. The release's image already carries the agent, its contract and the original command,
//! so the Dockerfile is one `FROM` line. The controller on an unprotected install and the
//! verifier on a protected one render it here, so the same image always has the same bytes and
//! the same key.

use std::io::Write;

use alien_core::sandbox_build_role::SANDBOX_BUNDLE_KEY_PREFIX;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// A rendered bundle and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedBundle {
    pub bucket: String,
    /// `sandbox-bundle/<sandbox id>/<sha256 of the zip>/bundle.zip`: a release that changes nothing
    /// renders the same bytes, so the same key, and so no rebuild. The Storage is shared by the
    /// deployment's sandboxes, so each keeps to its own prefix and deletes only there.
    pub key: String,
    pub zip: Vec<u8>,
}

impl RenderedBundle {
    pub fn uri(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.key)
    }
}

/// The key prefix one sandbox's bundles are written under.
pub fn sandbox_prefix(resource_id: &str) -> String {
    format!("{SANDBOX_BUNDLE_KEY_PREFIX}/{resource_id}/")
}

/// Renders the bundle that builds `image`. The error says why the image cannot be built from.
pub fn render(bucket: &str, image: &str, resource_id: &str) -> Result<RenderedBundle, String> {
    // A newline in the reference would write Dockerfile directives of its own.
    if image.is_empty() || image.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!(
            "sandbox image '{image}' is empty or carries whitespace or control characters"
        ));
    }
    let dockerfile = format!("FROM {image}\n");

    let failed = |error: &dyn std::fmt::Display| {
        format!("could not assemble the sandbox image bundle: {error}")
    };
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // A fixed timestamp keeps the bytes, and so the key, a function of the image alone. The
    // archive is flat: `CreateMicrovmImage` looks for the Dockerfile at its root.
    let options = SimpleFileOptions::default()
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    zip.start_file("Dockerfile", options)
        .map_err(|error| failed(&error))?;
    zip.write_all(dockerfile.as_bytes())
        .map_err(|error| failed(&error))?;
    let zip = zip.finish().map_err(|error| failed(&error))?.into_inner();

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
