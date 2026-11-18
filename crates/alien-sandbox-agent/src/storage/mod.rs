//! Persistent storage in the agent: restore a key's tree before the image command starts, then
//! replicate the directory to the key's store with one commit in flight at a time.
//!
//! The layout, the one-writer rule and integrity checks are `alien-sandbox-storage`'s. This
//! module is the part only the agent can do: read and write the directory as root without
//! following anything a command placed there, and freeze the guest for the instant a commit
//! copies changed files to staging.

pub mod attach;
pub mod freeze;
pub mod push;
pub mod tree;

use std::os::fd::OwnedFd;
use std::path::PathBuf;

use alien_sandbox_storage::{
    CommitOutcome, Committer, ErrorData as StorageError, Manifest, ObjectStore,
    Result as StorageResult, TreeEntry, TreeEntryKind,
};

use self::freeze::{Freezer, FREEZE_BOUND};
use self::tree::Staging;

/// Where commits stage changed files: agent-only, outside the session root.
pub const STAGING_DIR: &str = "/var/lib/alien/storage-staging";

fn io_error(operation: &str, error: std::io::Error) -> alien_error::AlienError<StorageError> {
    alien_error::AlienError::new(StorageError::TreeIo {
        operation: operation.to_string(),
        path: String::new(),
        reason: error.to_string(),
    })
}

/// One key's writer inside the sandbox.
pub struct Replicator {
    key: String,
    root: OwnedFd,
    exclude: Vec<String>,
    staging: PathBuf,
    freezer: Option<Freezer>,
    committer: Committer,
}

impl Replicator {
    /// A writer for `key` over the directory `root`, continuing from the HEAD its claim wrote.
    pub fn new(
        key: impl Into<String>,
        root: OwnedFd,
        exclude: Vec<String>,
        staging: PathBuf,
        freezer: Option<Freezer>,
        committer: Committer,
    ) -> Self {
        Self {
            key: key.into(),
            root,
            exclude,
            staging,
            freezer,
            committer,
        }
    }

    /// The manifest HEAD names.
    pub fn head(&self) -> Option<&Manifest> {
        self.committer.head()
    }

    /// Whether another sandbox claimed the key; every later commit is refused.
    pub fn is_superseded(&self) -> bool {
        self.committer.is_superseded()
    }

    /// Walks the directory and copies every changed file to staging, with the guest frozen
    /// where the platform has a freezer. Blocking.
    pub fn stage(&self) -> StorageResult<(Vec<TreeEntry>, Staging)> {
        let mut staging = Staging::new(&self.staging).map_err(|e| io_error("stage", e))?;
        let frozen = match &self.freezer {
            Some(freezer) => Some(
                freezer
                    .freeze(FREEZE_BOUND)
                    .map_err(|e| io_error("freeze the guest", e))?,
            ),
            None => None,
        };
        let mut walk = tree::walk(&self.root, &self.exclude).map_err(|e| io_error("walk", e))?;
        let changed: Vec<String> = tree::changed_files(&walk, self.committer.head())
            .into_iter()
            .map(|entry| entry.path.clone())
            .collect();
        for path in &changed {
            staging
                .copy(&self.root, path)
                .map_err(|e| io_error("copy to staging", e))?;
        }
        if let Some(frozen) = frozen {
            frozen.thaw().map_err(|e| io_error("thaw the guest", e))?;
        }
        // A file that vanished between the walk and its copy is not in this commit.
        walk.retain(|entry| {
            !matches!(entry.kind, TreeEntryKind::File { .. })
                || !changed.contains(&entry.path)
                || staging.contains(&entry.path)
        });
        Ok((walk, staging))
    }

    /// Whether the directory differs from what HEAD names, by a walk without freezing.
    pub fn dirty(&self) -> std::io::Result<bool> {
        let walk = tree::walk(&self.root, &self.exclude)?;
        let Some(head) = self.committer.head() else {
            return Ok(true);
        };
        if walk.len() != head.entries.len() {
            return Ok(true);
        }
        Ok(walk.iter().any(|item| {
            let Some(old) = head.entry(&item.path) else {
                return true;
            };
            let same_kind = match (&item.kind, &old.kind) {
                (TreeEntryKind::Dir, alien_sandbox_storage::EntryKind::Dir) => true,
                (
                    TreeEntryKind::File { size, change_token },
                    alien_sandbox_storage::EntryKind::File {
                        size: old_size,
                        change_token: old_token,
                        ..
                    },
                ) => {
                    size == old_size
                        && change_token == old_token
                        && *change_token != alien_sandbox_storage::UNKNOWN_CHANGE
                }
                (
                    TreeEntryKind::Symlink { target },
                    alien_sandbox_storage::EntryKind::Symlink { target: old_target },
                ) => target == old_target,
                (
                    TreeEntryKind::HardLink { .. },
                    alien_sandbox_storage::EntryKind::HardLink { .. },
                ) => true,
                // A hard-link group may be listed under another first name; let the commit decide.
                _ => false,
            };
            !same_kind
                || item.mode != old.mode
                || item.uid != old.uid
                || item.gid != old.gid
                || (item.mtime_ns != old.mtime_ns && !matches!(item.kind, TreeEntryKind::Dir))
        }))
    }

    /// Stages and publishes one commit.
    pub async fn commit(&mut self, store: &dyn ObjectStore) -> StorageResult<CommitOutcome> {
        let (walk, mut staging) = tokio::task::block_in_place(|| self.stage())?;
        self.committer
            .commit(store, &self.key, walk, &mut staging)
            .await
    }
}

#[cfg(test)]
mod tests;
