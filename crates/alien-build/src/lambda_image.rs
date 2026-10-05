use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use serde_json::Value;
use std::{collections::HashMap, fs::File, io::Read, path::Path};

/// Validate the completed image, including inherited layers, before Lambda sees it.
/// Large layer blobs are skipped with seeks; only bounded JSON metadata is buffered.
pub(crate) fn validate(path: &Path, resource_name: &str) -> Result<()> {
    let invalid = |reason: &str| {
        ErrorData::InvalidResourceConfig {
            resource_id: resource_name.to_string(),
            reason: format!(
                "AWS Worker image '{}' is not Lambda-compatible: {reason}. Rebuild with the current CLI using `alien build`, then release again. Custom base images must use Linux ARM64 and gzip layers.",
                path.display()
            ),
        }
    };
    let file = File::open(path)
        .into_alien_error()
        .context(invalid("could not open the ARM64 OCI archive"))?;
    let mut archive = tar::Archive::new(file);
    const MAX_METADATA_BYTES: u64 = 1 << 20;
    let mut metadata = HashMap::<String, Value>::new();
    for entry in archive
        .entries_with_seek()
        .into_alien_error()
        .context(invalid("could not read OCI archive entries"))?
    {
        let mut entry = entry
            .into_alien_error()
            .context(invalid("could not read OCI archive entry"))?;
        let name = entry
            .path()
            .into_alien_error()
            .context(invalid("could not read OCI archive entry path"))?
            .to_string_lossy()
            .into_owned();
        if (name == "index.json" || name.starts_with("blobs/"))
            && entry.size() <= MAX_METADATA_BYTES
        {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .into_alien_error()
                .context(invalid("could not read OCI metadata"))?;
            if let Ok(value) = serde_json::from_slice(&bytes) {
                metadata.insert(name, value);
            }
        }
    }
    let get_blob = |descriptor: &Value| -> Result<&Value> {
        let digest = descriptor["digest"]
            .as_str()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or_else(|| AlienError::new(invalid("missing SHA256 image descriptor")))?;
        metadata
            .get(&format!("blobs/sha256/{digest}"))
            .ok_or_else(|| AlienError::new(invalid("missing or oversized image metadata blob")))
    };
    let manifests = metadata
        .get("index.json")
        .and_then(|index| index["manifests"].as_array())
        .ok_or_else(|| AlienError::new(invalid("missing image index")))?;
    let [descriptor] = manifests.as_slice() else {
        return Err(AlienError::new(invalid(
            "expected exactly one image manifest",
        )));
    };
    let manifest = get_blob(descriptor)?;
    let config = get_blob(&manifest["config"])?;
    if config["architecture"] != "arm64" || config["os"] != "linux" {
        return Err(AlienError::new(invalid(
            "expected Linux ARM64 image configuration",
        )));
    }
    let layers = manifest["layers"]
        .as_array()
        .ok_or_else(|| AlienError::new(invalid("missing image layers")))?;
    for layer in layers {
        match layer["mediaType"].as_str() {
            Some("application/vnd.oci.image.layer.v1.tar+gzip")
            | Some("application/vnd.docker.image.rootfs.diff.tar.gzip") => {}
            _ => {
                return Err(AlienError::new(invalid(&format!(
                    "unsupported layer media type {}",
                    layer["mediaType"]
                ))));
            }
        }
    }
    Ok(())
}
