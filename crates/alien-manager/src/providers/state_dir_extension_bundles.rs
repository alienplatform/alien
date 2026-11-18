//! Extension bundle objects in the manager's state directory:
//! `extensions/<project>/sha256-<hex>`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use alien_error::{AlienError, Context, GenericError, IntoAlienError};

use crate::auth::Subject;
use crate::error::ErrorData;
use crate::traits::{BundleObject, ExtensionBundleStore};

/// Bundle objects as files under `<state dir>/extensions/<project>/`, one per sha256.
pub struct StateDirExtensionBundles {
    dir: PathBuf,
}

impl StateDirExtensionBundles {
    /// The store under `state_dir`. An upload the process stopped in the middle of left its
    /// temporary file, which this removes: it relies on one manager per state directory, which
    /// opens the store once at start, before any upload.
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        let dir = state_dir.into().join("extensions");
        let projects = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self { dir },
            Err(error) => {
                tracing::warn!(path = %dir.display(), %error, "Failed to read the extension bundle folder to remove unfinished uploads");
                return Self { dir };
            }
        };
        for project in projects.flatten() {
            remove_unfinished_uploads(&project.path());
        }
        Self { dir }
    }

    /// The folder of `project_id`'s objects. A project id names one folder, so one that could
    /// name another path is refused.
    fn project_dir(&self, project_id: &str) -> Result<PathBuf, AlienError> {
        let plain = !project_id.is_empty()
            && project_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !plain {
            return Err(ErrorData::bad_request(format!(
                "'{project_id}' is not a project id, which is one or more letters, digits, '-' and '_'"
            ))
            .into_generic());
        }
        Ok(self.dir.join(project_id))
    }
}

fn remove_unfinished_uploads(project_dir: &Path) {
    let entries = match std::fs::read_dir(project_dir) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return
        }
        Err(error) => {
            tracing::warn!(path = %project_dir.display(), %error, "Failed to read an extension bundle project folder to remove unfinished uploads");
            return;
        }
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(".part-") {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                tracing::warn!(path = %entry.path().display(), %error, "Failed to remove an unfinished extension bundle upload");
            }
        }
    }
}

#[async_trait]
impl ExtensionBundleStore for StateDirExtensionBundles {
    async fn put(
        &self,
        _caller: &Subject,
        project_id: &str,
        sha256: &str,
        bytes: Vec<u8>,
    ) -> Result<(), AlienError> {
        let folder = self.project_dir(project_id)?;
        let path = folder.join(format!("sha256-{sha256}"));
        // One file per upload: concurrent uploads of a digest each write their own, and the
        // last rename leaves the same bytes in place.
        let part = folder.join(format!(
            "sha256-{sha256}.part-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let failed = |operation: &str| GenericError {
            message: format!(
                "Failed to {operation} extension bundle object {}",
                path.display()
            ),
        };
        // Written whole, then renamed: a reader sees the complete object or none. One blocking
        // task, started on the first poll, creates the folder, writes, renames and cleans up, so
        // a caller that goes away mid-upload leaves the object and no temporary file behind.
        let (written, renamed) = (part.clone(), path.clone());
        let stored = tokio::task::spawn_blocking(move || {
            let result = std::fs::create_dir_all(&folder)
                .map_err(|error| ("create the folder of", error))
                .and_then(|()| std::fs::write(&written, &bytes).map_err(|error| ("write", error)))
                .and_then(|()| {
                    std::fs::rename(&written, &renamed).map_err(|error| ("store", error))
                });
            if result.is_err() {
                let _ = std::fs::remove_file(&written);
            }
            result
        })
        .await
        .into_alien_error()
        .context(failed("finish storing"))?;
        match stored {
            Ok(()) => Ok(()),
            Err((operation, error)) => Err(error).into_alien_error().context(failed(operation)),
        }
    }

    async fn get(
        &self,
        _caller: &Subject,
        project_id: &str,
        sha256: &str,
    ) -> Result<Option<BundleObject>, AlienError> {
        let path = self
            .project_dir(project_id)?
            .join(format!("sha256-{sha256}"));
        match tokio::fs::read(path).await {
            Ok(bytes) => Ok(Some(BundleObject::Bytes(bytes))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).into_alien_error().context(GenericError {
                message: format!("Failed to read extension bundle object sha256-{sha256}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Concurrent uploads of one digest all succeed and leave one complete object.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_uploads_of_one_digest_all_store_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(StateDirExtensionBundles::new(dir.path()));
        let bytes: Vec<u8> = (0..8 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let sha256 = "a".repeat(64);

        let uploads = (0..16).map(|_| {
            let (store, bytes, sha256) = (store.clone(), bytes.clone(), sha256.clone());
            tokio::spawn(async move { store.put(&Subject::system(), "p", &sha256, bytes).await })
        });
        for upload in futures::future::join_all(uploads).await {
            upload.unwrap().expect("each upload stores the object");
        }

        let Some(BundleObject::Bytes(stored)) =
            store.get(&Subject::system(), "p", &sha256).await.unwrap()
        else {
            panic!("the object is stored");
        };
        assert!(stored == bytes, "the stored object is the uploaded one");
        let entries: Vec<String> = std::fs::read_dir(dir.path().join("extensions").join("p"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, [format!("sha256-{sha256}")], "{entries:?}");
    }

    /// An upload whose caller goes away still ends with the object, and no temporary file.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_upload_whose_caller_goes_away_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateDirExtensionBundles::new(dir.path());
        let bytes = vec![7u8; 64 * 1024 * 1024];
        let sha256 = "b".repeat(64);

        let gone = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            store.put(&Subject::system(), "p", &sha256, bytes),
        )
        .await;
        assert!(gone.is_err(), "the caller left before the upload finished");

        let entries = || -> Vec<String> {
            match std::fs::read_dir(dir.path().join("extensions").join("p")) {
                Ok(entries) => entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(error) => panic!("{error}"),
            }
        };
        // The write started on the timeout's one poll, so it finishes with the object.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while entries() != [format!("sha256-{sha256}")] && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(entries(), [format!("sha256-{sha256}")]);
    }

    /// An upload the process stopped in the middle of leaves a temporary file, which the next
    /// start removes; the stored objects stay.
    #[test]
    fn opening_the_store_removes_unfinished_uploads() {
        let dir = tempfile::tempdir().unwrap();
        let objects = dir.path().join("extensions").join("p");
        std::fs::create_dir_all(&objects).unwrap();
        let stored = format!("sha256-{}", "c".repeat(64));
        std::fs::write(objects.join(&stored), b"object").unwrap();
        std::fs::write(objects.join(format!("{stored}.part-0123")), b"half").unwrap();

        StateDirExtensionBundles::new(dir.path());

        let left: Vec<String> = std::fs::read_dir(&objects)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, [stored]);
    }

    /// A project reads only the objects stored under it: another project's digest reads nothing.
    #[tokio::test]
    async fn a_project_reads_no_object_another_project_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateDirExtensionBundles::new(dir.path());
        let sha256 = "d".repeat(64);
        store
            .put(&Subject::system(), "prj_a", &sha256, b"object".to_vec())
            .await
            .unwrap();

        assert_eq!(
            store
                .get(&Subject::system(), "prj_b", &sha256)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .get(&Subject::system(), "prj_a", &sha256)
                .await
                .unwrap(),
            Some(BundleObject::Bytes(b"object".to_vec()))
        );
    }

    /// A project id that could name another folder is refused.
    #[tokio::test]
    async fn a_project_id_that_names_another_path_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateDirExtensionBundles::new(dir.path());
        let sha256 = "e".repeat(64);
        for project in ["", ".", "..", "../prj_a", "prj_a/x", "prj a"] {
            let error = store
                .get(&Subject::system(), project, &sha256)
                .await
                .expect_err(project);
            assert_eq!(error.http_status_code, Some(400), "{project}: {error}");
            let error = store
                .put(&Subject::system(), project, &sha256, vec![])
                .await
                .expect_err(project);
            assert_eq!(error.http_status_code, Some(400), "{project}: {error}");
        }
    }
}
