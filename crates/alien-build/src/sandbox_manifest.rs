//! OCI manifests must describe inherited Docker layers with OCI media types too.

use dockdash::Image;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

fn document(path: &Path, name: &str) -> io::Result<Value> {
    let mut archive = tar::Archive::new(File::open(path)?);
    for entry in archive.entries_with_seek()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new(name) {
            let mut bytes = Vec::new();
            entry.by_ref().take(1 << 20).read_to_end(&mut bytes)?;
            return serde_json::from_slice(&bytes).map_err(io::Error::other);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("image archive is missing {name}"),
    ))
}

/// Normalize only the assembled image's descriptors. Layer/config bytes and their
/// identities remain unchanged. Some runtimes reject mixed Docker/OCI media types.
pub(crate) fn normalize(image: &Image) -> io::Result<()> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "expected one assembled OCI image manifest",
        )
    };
    let mut index = document(image.path(), "index.json")?;
    let manifests = index["manifests"].as_array_mut().ok_or_else(invalid)?;
    if manifests.len() != 1 {
        return Err(invalid());
    }
    let descriptor = &mut manifests[0];
    let digest = descriptor["digest"].as_str().ok_or_else(invalid)?;
    let old_path = format!("blobs/{}", digest.replace(':', "/"));
    let mut manifest = document(image.path(), &old_path)?;
    if manifest["mediaType"] != "application/vnd.oci.image.manifest.v1+json" {
        return Err(invalid());
    }
    let mut changed = false;
    for layer in manifest["layers"].as_array_mut().ok_or_else(invalid)? {
        let normalized = match layer["mediaType"].as_str().ok_or_else(invalid)? {
            "application/vnd.docker.image.rootfs.diff.tar.gzip" => {
                "application/vnd.oci.image.layer.v1.tar+gzip"
            }
            "application/vnd.docker.image.rootfs.diff.tar" => {
                "application/vnd.oci.image.layer.v1.tar"
            }
            "application/vnd.docker.image.rootfs.foreign.diff.tar.gzip" => {
                "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip"
            }
            _ => continue,
        };
        layer["mediaType"] = normalized.into();
        changed = true;
    }
    if !changed {
        return Ok(());
    }
    let bytes = serde_json::to_vec(&manifest).map_err(io::Error::other)?;
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    descriptor["digest"] = digest.clone().into();
    descriptor["size"] = bytes.len().into();
    let index = serde_json::to_vec(&index).map_err(io::Error::other)?;
    let parent = image.path().parent().ok_or_else(invalid)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut output = tar::Builder::new(temporary.as_file_mut());
        let mut archive = tar::Archive::new(File::open(image.path())?);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;
            if path.as_ref() != Path::new("index.json") && path.as_ref() != Path::new(&old_path) {
                output.append(&entry.header().clone(), &mut entry)?;
            }
        }
        for (name, data) in [
            (format!("blobs/{}", digest.replace(':', "/")), bytes),
            ("index.json".into(), index),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            output.append_data(&mut header, name, data.as_slice())?;
        }
        output.finish()?;
    }
    temporary.persist(image.path()).map_err(io::Error::other)?;
    Ok(())
}
