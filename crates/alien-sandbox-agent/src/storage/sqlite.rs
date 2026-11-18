//! SQLite databases in persistent storage, copied while commands keep writing.
//!
//! A database is a group of files: the database itself and its `-wal` or `-journal`. Copied one
//! after the other while the application writes, the copies can come from different moments and
//! restore as a corrupt database. So a changed group is copied together and bracketed by the
//! database's ctime: one stat of the open database before its copy and one after the sidecars'.
//! The group is kept only if the two are equal and older than the clock read before the first
//! stat (a change in that tick could leave the stamp as it was).
//!
//! That is enough because of how SQLite writes. In WAL mode the database changes only at a
//! checkpoint, and a database that did not change across both copies, with any copy of its WAL,
//! is a crash image: SQLite ignores WAL frames past the last valid checksum or with an old salt.
//! In rollback mode the database changes at every commit, so an unchanged database is the last
//! committed state, and a hot journal next to it rolls back to that same state.
//!
//! A group that changes during every attempt keeps what HEAD has for it this commit (a new
//! database waits) and is tried at the next commit; the caller reports how long a group waits.
//! The agent links no SQLite: files are read with the same no-follow tree code as every other
//! file, so a symlink planted at a database or a sidecar redirects nothing. A `-shm` is copied
//! as an ordinary file: SQLite rebuilds it when the first connection opens.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, OwnedFd};

use alien_sandbox_storage::{EntryKind, Manifest, TreeEntry, TreeEntryKind, UNKNOWN_CHANGE};

use super::tree::{self, Staging};
use super::Phase;

/// The first bytes of every SQLite database file.
const HEADER: &[u8; 16] = b"SQLite format 3\0";

/// The files SQLite keeps beside a database that must be copied with it.
const SIDECARS: [&str; 2] = ["-wal", "-journal"];

/// Copies tried per database in one commit before it waits for the next.
const ATTEMPTS: usize = 3;

/// What [`stage`] did with the databases it found.
#[derive(Debug, Default)]
pub(super) struct Groups {
    /// Paths [`stage`] decided; the ordinary copy leaves them alone.
    pub handled: HashSet<String>,
    /// Databases that changed during every attempt and kept HEAD's files.
    pub deferred: usize,
}

fn members(db: &str) -> [String; 3] {
    [
        db.to_string(),
        format!("{db}{}", SIDECARS[0]),
        format!("{db}{}", SIDECARS[1]),
    ]
}

fn is_sidecar(path: &str) -> bool {
    SIDECARS
        .iter()
        .chain(&["-shm"])
        .any(|suffix| path.ends_with(suffix))
}

/// Whether the regular file at `path` starts with the SQLite header.
fn is_database(root: &OwnedFd, path: &str) -> bool {
    let Ok(mut file) = tree::open_file(root, path) else {
        return false;
    };
    let mut header = [0u8; 16];
    file.read_exact(&mut header).is_ok() && &header == HEADER
}

fn not_a_file(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENOENT | libc::ELOOP | libc::ENOTDIR)
    ) || error.kind() == io::ErrorKind::InvalidInput
}

/// Whether HEAD has `entry` with the same size and stamps, so the commit reuses its chunks.
fn unchanged(head: Option<&Manifest>, entry: &TreeEntry) -> bool {
    let TreeEntryKind::File { size, change_token } = entry.kind else {
        return false;
    };
    head.and_then(|m| m.entry(&entry.path)).is_some_and(|old| {
        matches!(
            old.kind,
            EntryKind::File { size: old_size, change_token: old_token, .. }
                if old_size == size
                    && old_token == change_token
                    && change_token != UNKNOWN_CHANGE
                    && old.mtime_ns == entry.mtime_ns
        )
    })
}

/// A sidecar's entry. One that changed in the bracket's own tick gets a token no walk will
/// match, instead of none: the next walk rereads it either way, and a commit that later keeps
/// HEAD's group can still reuse its chunks.
fn sidecar_entry(path: &str, st: &libc::stat, started: i64) -> TreeEntry {
    let mut entry = tree::file_entry(path, st, started);
    if let TreeEntryKind::File { change_token, .. } = &mut entry.kind {
        if *change_token == UNKNOWN_CHANGE {
            *change_token = -tree::ctime_ns(st).max(1);
        }
    }
    entry
}

enum Outcome {
    /// The group's entries, copied together.
    Taken(Vec<TreeEntry>),
    /// The database is gone (or is no longer a regular file); the next walk sees what is there.
    Gone,
    /// The database changed during every attempt.
    Busy,
}

fn copy_group(
    root: &OwnedFd,
    db: &str,
    exclude: &[String],
    head: Option<&Manifest>,
    staging: &mut Staging,
    between: &mut dyn FnMut(Phase),
) -> io::Result<Outcome> {
    for _ in 0..ATTEMPTS {
        let started = tree::coarse_now_ns();
        let mut file = match tree::open_file(root, db) {
            Ok(file) => file,
            Err(error) if not_a_file(&error) => return Ok(Outcome::Gone),
            Err(error) => return Err(error),
        };
        let before = tree::fstat(file.as_raw_fd())?;
        let mut copied = Vec::new();
        let mut taken = Vec::new();
        let db_entry = tree::file_entry(db, &before, started);
        if !unchanged(head, &db_entry) {
            staging.copy_open(db, &mut file)?;
            copied.push(db.to_string());
        }
        taken.push(db_entry);
        between(Phase::DatabaseCopied);
        for path in members(db).into_iter().skip(1) {
            if tree::excluded(&path, exclude) {
                continue;
            }
            match tree::open_file(root, &path) {
                Ok(mut sidecar) => {
                    let st = tree::fstat(sidecar.as_raw_fd())?;
                    staging.copy_open(&path, &mut sidecar)?;
                    taken.push(sidecar_entry(&path, &st, started));
                    copied.push(path);
                }
                Err(error) if not_a_file(&error) => {}
                Err(error) => return Err(error),
            }
        }
        let after = tree::fstat(file.as_raw_fd())?;
        let still_named = tree::stat_at(root, db)
            .is_ok_and(|st| st.st_ino == before.st_ino && st.st_dev == before.st_dev);
        let ctime = tree::ctime_ns(&before);
        if still_named
            && ctime == tree::ctime_ns(&after)
            && ctime < started
            && before.st_size == after.st_size
        {
            return Ok(Outcome::Taken(taken));
        }
        for path in &copied {
            staging.forget(path);
        }
    }
    Ok(Outcome::Busy)
}

/// HEAD's files of a database's group, to keep while it is busy. `None` when HEAD holds one of
/// them without a token a commit can reuse (written before groups were copied together); that
/// group is then copied file by file as before.
fn kept_from_head(head: Option<&Manifest>, db: &str) -> Option<Vec<TreeEntry>> {
    let Some(head) = head else {
        return Some(Vec::new());
    };
    if head.entry(db).is_none() {
        return Some(Vec::new());
    }
    members(db)
        .iter()
        .filter_map(|path| head.entry(path))
        .map(|old| match old.kind {
            EntryKind::File {
                size, change_token, ..
            } if change_token != UNKNOWN_CHANGE => Some(TreeEntry {
                path: old.path.clone(),
                kind: TreeEntryKind::File { size, change_token },
                mode: old.mode,
                uid: old.uid,
                gid: old.gid,
                mtime_ns: old.mtime_ns,
            }),
            _ => None,
        })
        .collect()
}

/// Finds the SQLite databases among `entries` whose group changed since HEAD, copies each group
/// together into `staging` and puts the group's entries in `entries`.
pub(super) fn stage(
    root: &OwnedFd,
    exclude: &[String],
    entries: &mut Vec<TreeEntry>,
    head: Option<&Manifest>,
    staging: &mut Staging,
    between: &mut dyn FnMut(Phase),
) -> io::Result<Groups> {
    let files: HashMap<&str, &TreeEntry> = entries
        .iter()
        .filter(|e| matches!(e.kind, TreeEntryKind::File { .. }))
        .map(|e| (e.path.as_str(), e))
        .collect();
    let changed: HashSet<&str> = tree::changed_files(entries, head)
        .into_iter()
        .map(|e| e.path.as_str())
        .collect();
    let mut databases: Vec<String> = files
        .keys()
        .filter(|path| !is_sidecar(path))
        .filter(|path| members(path).iter().any(|m| changed.contains(m.as_str())))
        .filter(|path| is_database(root, path))
        .map(|path| path.to_string())
        .collect();
    databases.sort();

    let mut groups = Groups::default();
    for db in databases {
        let group = members(&db);
        let replacement = match copy_group(root, &db, exclude, head, staging, between)? {
            Outcome::Taken(taken) => taken,
            Outcome::Gone => Vec::new(),
            Outcome::Busy => match kept_from_head(head, &db) {
                Some(kept) => {
                    groups.deferred += 1;
                    kept
                }
                None => continue,
            },
        };
        entries.retain(|e| !group.contains(&e.path));
        entries.extend(replacement);
        groups.handled.extend(group);
    }
    Ok(groups)
}
