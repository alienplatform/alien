//! Optional durable BuildKit snapshots using the existing toolchain cache store.
use super::{ToolchainContext, docker::DockerToolchain};
use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use futures::TryStreamExt;
use object_store::{MultipartUpload, ObjectStore, path::Path as ObjectPath};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    path::{Component, Path},
    sync::Arc,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter as FileWriter};
use tracing::{info, warn};

pub(super) struct DockerCache {
    store: Arc<dyn ObjectStore>,
    key: ObjectPath,
    directory: TempDir,
    restored: bool,
}

fn error(reason: impl Into<String>) -> ErrorData {
    ErrorData::ImageBuildFailed {
        resource_name: "docker-cache".to_string(),
        reason: reason.into(),
        build_output: None,
    }
}

impl DockerCache {
    pub(super) async fn restore(
        context: &ToolchainContext,
        toolchain: &DockerToolchain,
    ) -> Result<Option<Self>> {
        let Some(store) = &context.cache_store else {
            return Ok(None);
        };
        // Do not key on source contents or build arguments: BuildKit must be able to
        // reuse unchanged dependency steps while invalidating changed inputs itself.
        let scope = serde_json::to_vec(&(
            &context.cache_prefix,
            context.build_target.runtime_platform_id(),
            &context.runtime_platform_name,
            context.workload.as_str(),
            toolchain.dockerfile.as_deref().unwrap_or("Dockerfile"),
            &toolchain.target,
        ))
        .into_alien_error()
        .context(error("Could not encode Docker cache scope"))?;
        let key = ObjectPath::from(format!("cache/docker-v1/{:x}.tar", Sha256::digest(scope)));
        let directory = tempfile::Builder::new()
            .prefix("alien-buildkit-")
            .tempdir()
            .into_alien_error()
            .context(error("Could not create cache staging directory"))?;
        let mut cache = Self {
            store: Arc::clone(store),
            key,
            directory,
            restored: false,
        };
        let result = match store.get(&cache.key).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(Some(cache)),
            Err(source) => {
                return Err(source)
                    .into_alien_error()
                    .context(error("Could not retrieve Docker cache"));
            }
        };
        let archive_path = cache.directory.path().join("snapshot.tar");
        let mut output = FileWriter::new(
            tokio::fs::File::create(&archive_path)
                .await
                .into_alien_error()
                .context(error("Could not create cache snapshot file"))?,
        );
        let mut stream = result.into_stream();
        let mut bytes = 0_u64;
        while let Some(chunk) = stream
            .try_next()
            .await
            .into_alien_error()
            .context(error("Could not download Docker cache"))?
        {
            bytes += chunk.len() as u64;
            output
                .write_all(&chunk)
                .await
                .into_alien_error()
                .context(error("Could not write Docker cache snapshot"))?;
        }
        output
            .flush()
            .await
            .into_alien_error()
            .context(error("Could not flush cache snapshot"))?;
        drop(output);
        let destination = cache.directory.path().join("import");
        tokio::task::spawn_blocking(move || -> Result<()> {
            extract_snapshot(&archive_path, &destination)?;
            std::fs::remove_file(archive_path)
                .into_alien_error()
                .context(error("Could not remove downloaded cache snapshot"))?;
            Ok(())
        })
        .await
        .into_alien_error()
        .context(error("Cache extraction task failed"))??;
        if !cache.directory.path().join("import/index.json").is_file() {
            return Err(AlienError::new(error("Docker cache is missing its index")));
        }
        cache.restored = true;
        info!(cache_download_bytes = bytes, "Restored Docker build cache");
        Ok(Some(cache))
    }

    pub(super) fn append_args(&self, args: &mut Vec<String>) {
        if self.restored {
            args.extend([
                "--cache-from".to_string(),
                format!(
                    "type=local,src={}",
                    self.directory.path().join("import").display()
                ),
            ]);
        }
        // A new destination each time prevents the local exporter from retaining
        // historical blobs. Import and export never share a writable directory.
        args.extend([
            "--cache-to".to_string(),
            format!(
                "type=local,dest={},mode=max",
                self.directory.path().join("export").display()
            ),
        ]);
    }

    pub(super) async fn save(self) -> Result<()> {
        let export = self.directory.path().join("export");
        let archive_path = self.directory.path().join("snapshot.tar");
        let path = archive_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = File::create(path)
                .into_alien_error()
                .context(error("Could not create cache snapshot"))?;
            let mut archive = tar::Builder::new(file);
            archive
                .append_dir_all(".", export)
                .into_alien_error()
                .context(error("Could not archive exported cache"))?;
            archive
                .finish()
                .into_alien_error()
                .context(error("Could not finish cache snapshot"))?;
            Ok(())
        })
        .await
        .into_alien_error()
        .context(error("Cache archive task failed"))??;
        let mut input = tokio::fs::File::open(&archive_path)
            .await
            .into_alien_error()
            .context(error("Could not read cache snapshot"))?;
        let mut upload = self
            .store
            .put_multipart(&self.key)
            .await
            .into_alien_error()
            .context(error("Could not start Docker cache upload"))?;
        let bytes = upload_snapshot(&mut input, upload.as_mut()).await?;
        info!(cache_upload_bytes = bytes, "Saved Docker build cache");
        Ok(())
    }
}

fn extract_snapshot(archive_path: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir(destination)
        .into_alien_error()
        .context(error("Could not create cache import directory"))?;
    let file = File::open(archive_path)
        .into_alien_error()
        .context(error("Could not open cache snapshot"))?;
    let mut archive = tar::Archive::new(file);
    for entry in archive
        .entries()
        .into_alien_error()
        .context(error("Could not read cache snapshot"))?
    {
        let mut entry = entry
            .into_alien_error()
            .context(error("Could not read cache entry"))?;
        let path = entry
            .path()
            .into_alien_error()
            .context(error("Could not read cache entry path"))?;
        let kind = entry.header().entry_type();
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
            || !(kind.is_file() || kind.is_dir())
        {
            return Err(AlienError::new(error(
                "Docker cache contains an unsafe archive entry",
            )));
        }
        if !entry
            .unpack_in(destination)
            .into_alien_error()
            .context(error("Could not restore cache entry"))?
        {
            return Err(AlienError::new(error(
                "Docker cache entry escapes its staging directory",
            )));
        }
    }
    Ok(())
}

// Keep the handle through commit: the buffered writer drops access to it on
// shutdown, preventing explicit cleanup when multipart completion fails.
async fn upload_snapshot(
    input: &mut tokio::fs::File,
    upload: &mut dyn MultipartUpload,
) -> Result<u64> {
    let result: Result<u64> = async {
        let length = input
            .metadata()
            .await
            .into_alien_error()
            .context(error("Could not stat cache snapshot"))?
            .len();
        let mut remaining = length;
        while remaining > 0 {
            let size = remaining.min(8 * 1024 * 1024) as usize;
            let mut part = vec![0; size];
            input
                .read_exact(&mut part)
                .await
                .into_alien_error()
                .context(error("Could not read Docker cache upload part"))?;
            upload
                .put_part(part.into())
                .await
                .into_alien_error()
                .context(error("Could not upload Docker cache part"))?;
            remaining -= size as u64;
        }
        upload
            .complete()
            .await
            .into_alien_error()
            .context(error("Could not commit Docker cache snapshot"))?;
        Ok(length)
    }
    .await;
    if result.is_err() {
        if let Err(cleanup_error) = upload.abort().await {
            warn!(%cleanup_error, "Failed to abort Docker cache upload after an error");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::{WorkloadKind, cache_utils::store_from_url};
    use super::*;
    use alien_core::BinaryTarget;
    use std::path::PathBuf;

    fn context(store: Arc<dyn ObjectStore>) -> ToolchainContext {
        ToolchainContext {
            src_dir: PathBuf::from("synthetic"),
            build_dir: PathBuf::from("unused"),
            cache_store: Some(store),
            cache_prefix: "stack-resource".into(),
            build_target: BinaryTarget::LinuxX64,
            runtime_platform_name: "local".into(),
            debug_mode: false,
            pull_base_images: false,
            workload: WorkloadKind::Container,
        }
    }

    fn toolchain() -> DockerToolchain {
        DockerToolchain {
            dockerfile: None,
            build_args: None,
            target: None,
        }
    }

    async fn write_snapshot(context: &ToolchainContext, bytes: &[u8]) {
        let cache = DockerCache::restore(context, &toolchain())
            .await
            .unwrap()
            .unwrap();
        let export = cache.directory.path().join("export");
        std::fs::create_dir(&export).unwrap();
        std::fs::write(export.join("index.json"), bytes).unwrap();
        cache.save().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_replacement_and_scope_isolation() {
        let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
        let context = context(store);
        write_snapshot(&context, b"first snapshot").await;
        write_snapshot(&context, b"replacement snapshot").await;
        let restored = DockerCache::restore(&context, &toolchain())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read(restored.directory.path().join("import/index.json")).unwrap(),
            b"replacement snapshot"
        );
        let mut other = context;
        other.cache_prefix = "other-stack-resource".into();
        assert!(
            !DockerCache::restore(&other, &toolchain())
                .await
                .unwrap()
                .unwrap()
                .restored
        );
        other.cache_prefix = "stack-resource".into();
        other.build_target = BinaryTarget::LinuxArm64;
        assert!(
            !DockerCache::restore(&other, &toolchain())
                .await
                .unwrap()
                .unwrap()
                .restored
        );
    }

    #[tokio::test]
    async fn cache_url_prefixes_are_isolated_on_disk() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        let first_url = url::Url::from_directory_path(&first).unwrap();
        let second_url = url::Url::from_directory_path(&second).unwrap();
        let first_context = context(store_from_url(Some(first_url.as_str())).unwrap().unwrap());
        let second_context = context(store_from_url(Some(second_url.as_str())).unwrap().unwrap());
        write_snapshot(&first_context, b"first prefix").await;
        assert!(
            !DockerCache::restore(&second_context, &toolchain())
                .await
                .unwrap()
                .unwrap()
                .restored
        );
        write_snapshot(&second_context, b"second prefix").await;
        for (context, expected, prefix) in [
            (&first_context, b"first prefix".as_slice(), &first),
            (&second_context, b"second prefix".as_slice(), &second),
        ] {
            let cache = DockerCache::restore(context, &toolchain())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                std::fs::read(cache.directory.path().join("import/index.json")).unwrap(),
                expected
            );
            assert!(prefix.join(cache.key.as_ref()).is_file());
        }
        assert!(!root.path().join("cache").exists());
    }

    #[tokio::test]
    async fn unsafe_and_partial_snapshots_fail_without_extracting_outside_staging() {
        let context = context(Arc::new(object_store::memory::InMemory::new()));
        let cache = DockerCache::restore(&context, &toolchain())
            .await
            .unwrap()
            .unwrap();
        let mut archives = vec![b"not a tar".to_vec()];
        for (path, kind) in [
            ("../outside", tar::EntryType::Regular),
            ("/outside", tar::EntryType::Regular),
            ("link", tar::EntryType::Symlink),
            ("hardlink", tar::EntryType::Link),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_entry_type(kind);
            header.set_size(0);
            header.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
            if kind.is_symlink() || kind.is_hard_link() {
                header.set_link_name("../../outside").unwrap();
            }
            header.set_cksum();
            let mut archive = tar::Builder::new(Vec::new());
            archive.append(&header, std::io::empty()).unwrap();
            archives.push(archive.into_inner().unwrap());
        }
        let mut header = tar::Header::new_gnu();
        header.set_path("index.json").unwrap();
        header.set_size(2048);
        header.set_cksum();
        archives.push(header.as_bytes().to_vec()); // valid header, missing promised body
        for archive in archives {
            context
                .cache_store
                .as_ref()
                .unwrap()
                .put(&cache.key, archive.into())
                .await
                .unwrap();
            assert!(DockerCache::restore(&context, &toolchain()).await.is_err());
        }
    }
    #[test]
    fn traversal_cannot_overwrite_a_known_canary() {
        let root = tempfile::tempdir().unwrap();
        let canary = root.path().join("canary");
        std::fs::write(&canary, b"untouched").unwrap();
        let mut header = tar::Header::new_gnu();
        let path = b"../canary";
        header.as_mut_bytes()[..path.len()].copy_from_slice(path);
        header.set_mode(0o644);
        header.set_size(7);
        header.set_cksum();
        let archive_path = root.path().join("snapshot.tar");
        let mut archive = tar::Builder::new(File::create(&archive_path).unwrap());
        archive.append(&header, b"changed".as_slice()).unwrap();
        archive.finish().unwrap();
        drop(archive);
        assert!(extract_snapshot(&archive_path, &root.path().join("import")).is_err());
        assert_eq!(std::fs::read(canary).unwrap(), b"untouched");
    }

    #[derive(Debug)]
    struct FaultyUpload {
        inner: Box<dyn MultipartUpload>,
        fail_part: bool,
        fail_commit: bool,
        fail_abort: bool,
        parts: usize,
        aborts: usize,
    }

    fn injected(message: &str) -> object_store::Error {
        object_store::Error::Generic {
            store: "synthetic",
            source: Box::new(std::io::Error::other(message.to_string())),
        }
    }

    #[async_trait::async_trait]
    impl MultipartUpload for FaultyUpload {
        fn put_part(&mut self, payload: object_store::PutPayload) -> object_store::UploadPart {
            self.parts += 1;
            if self.fail_part && self.parts == 2 {
                return Box::pin(async { Err(injected("part failure")) });
            }
            self.inner.put_part(payload)
        }
        async fn complete(&mut self) -> object_store::Result<object_store::PutResult> {
            if self.fail_commit {
                return Err(injected("commit failure"));
            }
            self.inner.complete().await
        }
        async fn abort(&mut self) -> object_store::Result<()> {
            self.aborts += 1;
            self.inner.abort().await?;
            if self.fail_abort {
                return Err(injected("abort failure"));
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn failed_upload_preserves_previous_object_and_aborts_without_masking_error() {
        let store = object_store::memory::InMemory::new();
        let key = ObjectPath::from("snapshot");
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(17 * 1024 * 1024).unwrap();
        for (fail_part, fail_commit, fail_abort) in [
            (true, false, false),
            (false, true, false),
            (true, false, true),
            (false, true, true),
            (false, false, false),
        ] {
            store.put(&key, b"previous".to_vec().into()).await.unwrap();
            let mut upload = FaultyUpload {
                inner: store.put_multipart(&key).await.unwrap(),
                fail_part,
                fail_commit,
                fail_abort,
                parts: 0,
                aborts: 0,
            };
            let mut input = tokio::fs::File::open(file.path()).await.unwrap();
            let result = upload_snapshot(&mut input, &mut upload).await;
            let visible = store.get(&key).await.unwrap().bytes().await.unwrap();
            if fail_part || fail_commit {
                let message = result.unwrap_err().to_string();
                assert!(
                    message.contains(if fail_part {
                        "Could not upload Docker cache part"
                    } else {
                        "Could not commit Docker cache snapshot"
                    }),
                    "{message}"
                );
                assert_eq!(upload.aborts, 1);
                assert_eq!(visible.as_ref(), b"previous");
            } else {
                assert_eq!(result.unwrap(), 17 * 1024 * 1024);
                assert_eq!(visible.len(), 17 * 1024 * 1024);
                assert!(visible.iter().all(|byte| *byte == 0));
                assert_eq!(upload.aborts, 0);
            }
            assert_eq!(upload.parts, if fail_part { 2 } else { 3 });
        }
    }
}
