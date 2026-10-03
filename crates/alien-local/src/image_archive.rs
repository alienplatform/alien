//! Docker archive compatibility without changing OCI content or image configuration.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::{json, Value};
use tempfile::NamedTempFile;

pub(crate) struct LoadArchive {
    pub file: NamedTempFile,
    pub image_ids: Vec<String>,
}

fn read_json(path: &Path, name: &Path) -> io::Result<Value> {
    let mut archive = tar::Archive::new(File::open(path)?);
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.path()?.as_ref() == name {
            return serde_json::from_reader(entry.take(1024 * 1024)).map_err(io::Error::other);
        }
    }
    Err(io::Error::other(format!(
        "Missing image metadata: {}",
        name.display()
    )))
}

fn digest_path(value: &Value) -> io::Result<String> {
    let digest = value
        .as_str()
        .ok_or_else(|| io::Error::other("Missing image digest"))?;
    let hex = digest
        .strip_prefix("sha256:")
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| io::Error::other("Expected a sha256 image digest"))?;
    Ok(format!("blobs/sha256/{hex}"))
}

/// Add a classic Docker manifest and uncompressed copies of zstd layers. OCI
/// descriptors and blobs remain byte-for-byte intact for the containerd store.
/// Temporary files bound memory use even for large layers; no archive is extracted.
pub(crate) fn prepare_load_archive(path: &Path) -> io::Result<LoadArchive> {
    let index = read_json(path, Path::new("index.json"))?;
    let manifests = index["manifests"]
        .as_array()
        .ok_or_else(|| io::Error::other("OCI index has no manifests"))?;
    let [descriptor] = manifests.as_slice() else {
        return Err(io::Error::other("Expected a single-image OCI archive"));
    };
    let manifest_path = digest_path(&descriptor["digest"])?;
    let manifest = read_json(path, Path::new(&manifest_path))?;
    let config_path = digest_path(&manifest["config"]["digest"])?;
    let layers = manifest["layers"]
        .as_array()
        .ok_or_else(|| io::Error::other("OCI manifest has no layers"))?;
    let mut uncompressed = HashMap::new();
    let mut docker_layers = Vec::new();
    for layer in layers {
        let source = digest_path(&layer["digest"])?;
        let media = layer["mediaType"]
            .as_str()
            .ok_or_else(|| io::Error::other("Layer has no media type"))?;
        if media == "application/vnd.oci.image.layer.v1.tar+zstd" {
            let dest = format!(
                "docker-layers/{}.tar",
                source.trim_start_matches("blobs/sha256/")
            );
            uncompressed.insert(source, dest.clone());
            docker_layers.push(dest);
        } else if matches!(
            media,
            "application/vnd.oci.image.layer.v1.tar"
                | "application/vnd.oci.image.layer.v1.tar+gzip"
                | "application/vnd.docker.image.rootfs.diff.tar.gzip"
        ) {
            docker_layers.push(source);
        } else {
            return Err(io::Error::other(format!(
                "Unsupported image layer media type: {media}"
            )));
        }
    }
    let file = NamedTempFile::new()?;
    let mut builder = tar::Builder::new(file.reopen()?);
    let mut archive = tar::Archive::new(File::open(path)?);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.into_owned();
        if entry_path == Path::new("manifest.json") {
            continue;
        }
        if let Some(dest) = uncompressed.remove(entry_path.to_string_lossy().as_ref()) {
            // Retain the original compressed blob and stream its decoded copy
            // separately. The config's rootfs diff_ids still describe these bytes.
            let mut compressed = tempfile::tempfile()?;
            io::copy(&mut entry, &mut compressed)?;
            compressed.seek(SeekFrom::Start(0))?;
            builder.append_data(&mut entry.header().clone(), &entry_path, &mut compressed)?;
            compressed.seek(SeekFrom::Start(0))?;
            let mut decoded = tempfile::tempfile()?;
            let size = io::copy(&mut zstd::Decoder::new(compressed)?, &mut decoded)?;
            decoded.seek(SeekFrom::Start(0))?;
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(size);
            builder.append_data(&mut header, dest, decoded)?;
        } else {
            builder.append_data(&mut entry.header().clone(), &entry_path, &mut entry)?;
        }
    }
    if !uncompressed.is_empty() {
        return Err(io::Error::other("OCI archive is missing layer blobs"));
    }
    let docker_manifest = serde_json::to_vec(&json!([{
        "Config": config_path,
        "RepoTags": null,
        "Layers": docker_layers,
    }]))?;
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o644);
    header.set_size(docker_manifest.len() as u64);
    builder.append_data(&mut header, "manifest.json", docker_manifest.as_slice())?;
    builder.finish()?;
    Ok(LoadArchive {
        file,
        image_ids: vec![
            descriptor["digest"]
                .as_str()
                .ok_or_else(|| io::Error::other("Missing manifest digest"))?
                .to_string(),
            manifest["config"]["digest"]
                .as_str()
                .ok_or_else(|| io::Error::other("Missing config digest"))?
                .to_string(),
        ],
    })
}
