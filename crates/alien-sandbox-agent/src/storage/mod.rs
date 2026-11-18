//! Persistent storage in the agent: restore a key's tree before the image command starts, then
//! replicate the directory to the key's store with one commit in flight at a time.
//!
//! The layout, the one-writer rule and integrity checks are `alien-sandbox-storage`'s. This
//! module is the part only the agent can do: read and write the directory as root without
//! following anything a command placed there, and freeze the guest for the instant a commit
//! copies changed files to staging.

pub mod attach;
pub mod freeze;
mod http;
pub mod push;
mod sqlite;
pub mod tree;

use std::collections::HashSet;
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
    /// Since when an SQLite database has kept HEAD's copy because it changed during every copy.
    deferred_since: Mutex<Option<Instant>>,
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
            deferred_since: Mutex::new(None),
        }
    }

    /// How long an SQLite database has been left at HEAD's copy because it changed during every
    /// copy (only without a freezer); `None` when the last commit took every database.
    pub fn deferred_for(&self) -> Option<Duration> {
        self.deferred_since
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map(|since| since.elapsed())
    }

    fn note_deferred(&self, deferred: bool) {
        let mut since = self
            .deferred_since
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match (deferred, *since) {
            (true, None) => *since = Some(Instant::now()),
            (false, _) => *since = None,
            (true, Some(_)) => {}
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

    /// Walks the directory and copies every changed file to staging. Blocking.
    ///
    /// Where the platform has a freezer, one walk and its copies run with the guest frozen, so
    /// the commit is the state of one instant. Without one (gVisor has no freezer controller),
    /// each file is copied as it was when read, files that name git objects are taken first,
    /// and an SQLite database is copied together with its journal: see
    /// [`Replicator::stage_unfrozen`].
    pub fn stage(&self) -> StorageResult<(Vec<TreeEntry>, Staging)> {
        self.stage_with(&mut |_| {})
    }

    fn stage_with(
        &self,
        between: &mut dyn FnMut(Phase),
    ) -> StorageResult<(Vec<TreeEntry>, Staging)> {
        let mut staging = Staging::new(&self.staging).map_err(|e| io_error("stage", e))?;
        let Some(freezer) = &self.freezer else {
            return self.stage_unfrozen(staging, between);
        };
        let frozen = freezer
            .freeze(FREEZE_BOUND)
            .map_err(|e| io_error("freeze the guest", e))?;
        let walk = tree::walk(&self.root, &self.exclude).map_err(|e| io_error("walk", e))?;
        let mut changed = HashSet::new();
        self.copy_changed(&walk, &mut staging, &mut changed, &HashSet::new())?;
        frozen.thaw().map_err(|e| io_error("thaw the guest", e))?;
        self.note_deferred(false);
        Ok((drop_vanished(walk, &changed, &staging), staging))
    }

    /// Without a freezer, a ref copied after the objects were listed can name an object written
    /// in between, and the commit would hold a repository whose ref points at nothing. So the
    /// files that name git objects (refs, HEADs, the index) are walked and copied first, then
    /// everything else is walked and copied: every object a copied ref names already existed
    /// when the second walk listed the objects. Git writes an object before anything names it
    /// and never rewrites one, so the order holds for any number of repositories in the
    /// directory. A ref that appears only in the second walk is left for the next commit.
    ///
    /// Each SQLite database is copied with its `-wal` or `-journal` and kept only if the
    /// database did not change across the copies (see [`sqlite`]).
    fn stage_unfrozen(
        &self,
        mut staging: Staging,
        between: &mut dyn FnMut(Phase),
    ) -> StorageResult<(Vec<TreeEntry>, Staging)> {
        let first = tree::walk(&self.root, &self.exclude).map_err(|e| io_error("walk", e))?;
        let names_objects = |entry: &TreeEntry| {
            !matches!(entry.kind, TreeEntryKind::Dir) && tree::names_git_objects(&entry.path)
        };
        let refs: Vec<TreeEntry> = first.iter().filter(|e| names_objects(e)).cloned().collect();
        let mut changed = HashSet::new();
        self.copy_changed(&refs, &mut staging, &mut changed, &HashSet::new())?;
        between(Phase::RefsCopied);

        let second = tree::walk(&self.root, &self.exclude).map_err(|e| io_error("walk", e))?;
        let mut entries: Vec<TreeEntry> =
            second.into_iter().filter(|e| !names_objects(e)).collect();
        between(Phase::SecondWalk);
        let groups = sqlite::stage(
            &self.root,
            &self.exclude,
            &mut entries,
            self.committer.head(),
            &mut staging,
            between,
        )
        .map_err(|e| io_error("copy a database to staging", e))?;
        self.copy_changed(&entries, &mut staging, &mut changed, &groups.handled)?;
        self.note_deferred(groups.deferred > 0);

        // A ref kept from the first walk keeps its directories, even if they went in between.
        let mut paths: HashSet<String> = entries.iter().map(|e| e.path.clone()).collect();
        for item in &refs {
            let mut parent = item.path.as_str();
            while let Some((dir, _)) = parent.rsplit_once('/') {
                parent = dir;
                if paths.insert(dir.to_string()) {
                    if let Some(dir_entry) = first.iter().find(|e| e.path == dir) {
                        entries.push(dir_entry.clone());
                    }
                }
            }
        }
        entries.extend(refs);
        Ok((drop_vanished(entries, &changed, &staging), staging))
    }

    fn copy_changed(
        &self,
        walk: &[TreeEntry],
        staging: &mut Staging,
        changed: &mut HashSet<String>,
        skip: &HashSet<String>,
    ) -> StorageResult<()> {
        for entry in tree::changed_files(walk, self.committer.head()) {
            if skip.contains(&entry.path) {
                continue;
            }
            changed.insert(entry.path.clone());
            staging
                .copy(&self.root, &entry.path)
                .map_err(|e| io_error("copy to staging", e))?;
        }
        Ok(())
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

/// Points in an unfrozen stage where a test can change the directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// The first walk's refs are copied; the second walk has not started.
    RefsCopied,
    /// The second walk is listed; its files are not copied yet.
    SecondWalk,
    /// A database is copied; its sidecars are not yet.
    DatabaseCopied,
}

/// A changed file that vanished between the walk and its copy is not in this commit; the next
/// walk sees the new state.
fn drop_vanished(
    mut walk: Vec<TreeEntry>,
    changed: &HashSet<String>,
    staging: &Staging,
) -> Vec<TreeEntry> {
    walk.retain(|entry| {
        !matches!(entry.kind, TreeEntryKind::File { .. })
            || !changed.contains(&entry.path)
            || staging.contains(&entry.path)
    });
    walk
}

#[cfg(test)]
mod tests;
