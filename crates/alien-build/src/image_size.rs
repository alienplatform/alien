//! The uncompressed size of a built image, read from its OCI archive.
//!
//! A registry manifest lists compressed layer sizes only, so the size a MicroVM build has to hold
//! on disk is measured here, from the layers of the archive the build already produced.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use serde_json::Value;

/// Largest JSON document (index, manifest) read from an archive.
const MAX_DOCUMENT_BYTES: u64 = 1 << 20;

/// Block size a file's size is rounded up to: a filesystem stores whole blocks, so an image of
/// many small files takes more disk than the sum of its file sizes.
const BLOCK_BYTES: u64 = 4096;

/// The disk in bytes, in whole 4 KiB blocks per file, that the filesystem the image's layers
/// produce when applied in order takes, or `None` when it cannot be measured (an unreadable
/// archive). `None` skips the size check; it never blocks a build.
pub(crate) fn uncompressed_size(archive: &Path) -> Option<u64> {
    measure(archive).ok().flatten()
}

fn blob_name(digest: &str) -> String {
    format!("blobs/{}", digest.replacen(':', "/", 1))
}

fn document(archive: &Path, name: &str) -> io::Result<Value> {
    let mut tar = tar::Archive::new(File::open(archive)?);
    for entry in tar.entries_with_seek()? {
        let mut entry = entry?;
        if is_entry(&entry, name)? {
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take(MAX_DOCUMENT_BYTES)
                .read_to_end(&mut bytes)?;
            return serde_json::from_slice(&bytes).map_err(io::Error::other);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("image archive is missing {name}"),
    ))
}

/// Whether an archive entry is `name`, whether or not the archive writes a leading `./`.
fn is_entry<R: Read>(entry: &tar::Entry<'_, R>, name: &str) -> io::Result<bool> {
    let path: std::path::PathBuf = entry
        .path()?
        .components()
        .filter(|part| !matches!(part, std::path::Component::CurDir))
        .collect();
    Ok(path == Path::new(name))
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

fn measure(archive: &Path) -> io::Result<Option<u64>> {
    let mut manifest = document(archive, "index.json")?;
    // An image index points at the manifests; archives hold one image, so follow the first.
    for _ in 0..3 {
        if manifest.get("layers").is_some() {
            break;
        }
        let digest = manifest["manifests"][0]["digest"]
            .as_str()
            .ok_or_else(|| invalid("image archive has no manifest"))?;
        manifest = document(archive, &blob_name(digest))?;
    }
    let layers = manifest["layers"]
        .as_array()
        .ok_or_else(|| invalid("image manifest has no layers"))?;

    let mut files: BTreeMap<String, u64> = BTreeMap::new();
    for layer in layers {
        let digest = layer["digest"]
            .as_str()
            .ok_or_else(|| invalid("layer has no digest"))?;
        apply_layer(archive, &blob_name(digest), &mut files)?;
    }
    Ok(Some(files.values().sum()))
}

/// Applies one layer, plain, gzip or zstd, to the merged file map.
fn apply_layer(archive: &Path, name: &str, files: &mut BTreeMap<String, u64>) -> io::Result<()> {
    let mut outer = tar::Archive::new(File::open(archive)?);
    for entry in outer.entries_with_seek()? {
        let entry = entry?;
        if !is_entry(&entry, name)? {
            continue;
        }
        let mut reader = BufReader::new(entry);
        let magic = {
            use std::io::BufRead;
            let buffer = reader.fill_buf()?;
            [
                buffer.first().copied().unwrap_or(0),
                buffer.get(1).copied().unwrap_or(0),
                buffer.get(2).copied().unwrap_or(0),
                buffer.get(3).copied().unwrap_or(0),
            ]
        };
        return match magic {
            [0x1f, 0x8b, ..] => read_layer(flate2::bufread::GzDecoder::new(reader), files),
            [0x28, 0xb5, 0x2f, 0xfd] => {
                read_layer(zstd::stream::read::Decoder::with_buffer(reader)?, files)
            }
            _ => read_layer(reader, files),
        };
    }
    Err(invalid("layer blob is missing from the image archive"))
}

fn read_layer(layer: impl Read, files: &mut BTreeMap<String, u64>) -> io::Result<()> {
    // Whiteouts hide earlier layers only, whatever order the archive lists them in, so this
    // layer's own files are collected apart and merged once the layer has been read.
    let mut added = BTreeMap::new();
    let mut tar = tar::Archive::new(layer);
    for entry in tar.entries()? {
        let entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        let path = path
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string();
        let (directory, base) = match path.rsplit_once('/') {
            Some((directory, base)) => (directory.to_string(), base.to_string()),
            None => (String::new(), path.clone()),
        };
        if base == ".wh..wh..opq" {
            // The directory's earlier contents are gone; this layer's entries follow.
            remove_under(files, &directory);
        } else if let Some(removed) = base.strip_prefix(".wh.") {
            let target = if directory.is_empty() {
                removed.to_string()
            } else {
                format!("{directory}/{removed}")
            };
            files.remove(&target);
            remove_under(files, &target);
        } else if entry.header().entry_type().is_file() {
            added.insert(
                path,
                entry.header().size()?.div_ceil(BLOCK_BYTES) * BLOCK_BYTES,
            );
        } else if !entry.header().entry_type().is_dir() {
            // A link or special file occupies the name but no data.
            added.insert(path, 0);
        }
    }
    files.extend(added);
    Ok(())
}

fn remove_under(files: &mut BTreeMap<String, u64>, directory: &str) {
    if directory.is_empty() {
        files.clear();
        return;
    }
    let prefix = format!("{directory}/");
    files.retain(|path, _| !path.starts_with(&prefix));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8]) {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, data).unwrap();
    }

    fn layer(files: &[(&str, usize)], gzip: bool) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, size) in files {
            append(&mut builder, path, &vec![7u8; *size]);
        }
        let bytes = builder.into_inner().unwrap();
        if !gzip {
            return bytes;
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn archive(layers: Vec<Vec<u8>>) -> tempfile::NamedTempFile {
        archive_under(layers, "")
    }

    fn archive_under(layers: Vec<Vec<u8>>, prefix: &str) -> tempfile::NamedTempFile {
        let mut outer = tar::Builder::new(Vec::new());
        let descriptors: Vec<Value> = (0..layers.len())
            .map(|i| serde_json::json!({ "digest": format!("sha256:layer{i}") }))
            .collect();
        for (i, bytes) in layers.iter().enumerate() {
            append(&mut outer, &format!("{prefix}blobs/sha256/layer{i}"), bytes);
        }
        let manifest = serde_json::json!({ "layers": descriptors });
        append(
            &mut outer,
            &format!("{prefix}blobs/sha256/manifest"),
            manifest.to_string().as_bytes(),
        );
        let index = serde_json::json!({ "manifests": [{ "digest": "sha256:manifest" }] });
        append(
            &mut outer,
            &format!("{prefix}index.json"),
            index.to_string().as_bytes(),
        );
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&outer.into_inner().unwrap()).unwrap();
        file
    }

    /// Layers add up, compressed or not, a file a later layer rewrites counts once, and each file
    /// takes whole blocks.
    #[test]
    fn layers_are_summed_and_an_overwritten_file_counts_once() {
        let file = archive(vec![
            layer(&[("usr/a", 1000), ("usr/b", 500)], true),
            layer(&[("usr/a", 300), ("opt/c", 200)], false),
        ]);
        assert_eq!(uncompressed_size(file.path()), Some(4096 * 3));
    }

    /// A whiteout removes the file; an opaque marker removes the directory's earlier content.
    #[test]
    fn whiteouts_remove_what_earlier_layers_wrote() {
        let file = archive(vec![
            layer(&[("var/x", 100), ("var/y", 100), ("etc/z", 50)], true),
            layer(&[("var/.wh.x", 0)], true),
            layer(&[("etc/.wh..wh..opq", 0), ("etc/new", 10)], true),
        ]);
        assert_eq!(uncompressed_size(file.path()), Some(4096 * 2));
    }

    /// An opaque marker hides earlier layers only: files the same layer lists before it stay.
    #[test]
    fn an_opaque_marker_keeps_files_its_own_layer_lists_first() {
        let file = archive(vec![
            layer(&[("etc/old", 5000)], true),
            layer(&[("etc/new", 10), ("etc/.wh..wh..opq", 0)], true),
        ]);
        assert_eq!(uncompressed_size(file.path()), Some(4096));
    }

    /// dockdash writes zstd layers by default, so they are the common case.
    #[test]
    fn a_zstd_layer_is_measured() {
        let plain = layer(&[("usr/a", 10_000)], false);
        let zstd_layer = zstd::encode_all(plain.as_slice(), 1).unwrap();
        let file = archive(vec![zstd_layer]);
        assert_eq!(uncompressed_size(file.path()), Some(12288));
    }

    /// `crane pull --format oci` and `tar -C dir .` write every entry under `./`.
    #[test]
    fn entries_written_under_a_dot_prefix_are_found() {
        let file = archive_under(vec![layer(&[("a", 5000)], true)], "./");
        assert_eq!(uncompressed_size(file.path()), Some(8192));
    }

    /// An archive that cannot be read is unmeasured rather than an error.
    #[test]
    fn an_unreadable_archive_is_unmeasured() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"not a tar").unwrap();
        assert_eq!(uncompressed_size(file.path()), None);
    }

    /// Live check against an image pulled from a registry as an OCI archive: prints the measured
    /// size and what the AWS check says for the customer's ceilings (4Gi memory, 10Gi disk).
    ///
    /// ```text
    /// crane pull --platform linux/arm64 --format oci <image> dir && tar -C dir -cf image.oci.tar .
    /// PROBE_OCI_TAR=image.oci.tar cargo test -p alien-build --lib image_size::tests::live -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn live_size_and_fit_of_a_pulled_image() {
        let path = std::env::var("PROBE_OCI_TAR").expect("PROBE_OCI_TAR");
        let bytes = uncompressed_size(Path::new(&path)).expect("a measurable image");
        println!("MEASURED {} MiB ({bytes} bytes)", bytes / (1024 * 1024));
        let mut sandbox = alien_core::Sandbox::new("agents".to_string())
            .code(alien_core::SandboxCode::Image {
                image: "unused".to_string(),
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::None)
            .lifecycle(alien_core::SandboxLifecyclePolicy::default())
            .limits(alien_core::SandboxLimits {
                cpu: "2".to_string(),
                memory: "4Gi".to_string(),
                disk: "10Gi".to_string(),
                max_processes: None,
            })
            .build();
        sandbox.image_size_bytes = Some(bytes);
        match sandbox.check_microvm_image_fits(bytes) {
            Ok(()) => println!("FIT builds on the selected MicroVM size"),
            Err(error) => println!("REFUSED {error}"),
        }
    }
}
