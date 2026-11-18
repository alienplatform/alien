use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use alien_sandbox_storage::{
    read_manifest, restore, takeover, CommitOutcome, Committer, MemoryStore, TreeEntryKind,
};

use super::tree::{self, DirSink};
use super::Replicator;

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

async fn replicator(
    store: &MemoryStore,
    root: &Path,
    staging: &Path,
    exclude: &[&str],
) -> Replicator {
    let claim = takeover(store, "writer-a").await.expect("claim");
    Replicator::new(
        "k",
        tree::open_root(root).expect("open root"),
        exclude.iter().map(|s| s.to_string()).collect(),
        staging.to_path_buf(),
        None,
        Committer::new("writer-a", claim.head_etag, claim.manifest, None),
    )
}

/// Everything a restore must reproduce, by path.
fn snapshot(root: &Path) -> Vec<(String, String, u32, i64)> {
    let fd = tree::open_root(root).expect("open");
    let mut walk = tree::walk(&fd, &[]).expect("walk");
    walk.sort_by(|a, b| a.path.cmp(&b.path));
    walk.into_iter()
        .map(|e| {
            let kind = match &e.kind {
                TreeEntryKind::Dir => "dir".to_string(),
                TreeEntryKind::File { size, .. } => {
                    let bytes = std::fs::read(root.join(&e.path)).expect("read");
                    format!("file {size} {}", alien_sandbox_storage::chunk_hash(&bytes))
                }
                TreeEntryKind::Symlink { target } => format!("symlink {target}"),
                TreeEntryKind::HardLink { target } => format!("link {target}"),
            };
            let mtime = if matches!(e.kind, TreeEntryKind::Dir) {
                0
            } else {
                e.mtime_ns
            };
            (e.path, kind, e.mode, mtime)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tree_round_trips_with_modes_links_and_mtimes() {
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    let root = source.path();
    write(root, "a.txt", b"hello");
    write(root, "dir/nested/big.bin", &vec![7u8; 200 * 1024]);
    std::fs::set_permissions(root.join("a.txt"), std::fs::Permissions::from_mode(0o751))
        .expect("chmod");
    std::os::unix::fs::symlink("../a.txt", root.join("dir/link")).expect("symlink");
    std::fs::hard_link(root.join("a.txt"), root.join("dir/hard.txt")).expect("hard link");
    std::fs::create_dir_all(root.join("empty")).expect("mkdir");
    write(root, "tmp/scratch", b"local only");

    let store = MemoryStore::new();
    let mut replicator = replicator(&store, root, staging.path(), &["tmp/"]).await;
    let outcome = replicator.commit(&store).await.expect("commit");
    assert!(
        matches!(outcome, CommitOutcome::Committed { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        replicator.commit(&store).await.expect("commit"),
        CommitOutcome::Unchanged
    );

    let target = tempfile::tempdir().expect("tempdir");
    let claim = takeover(&store, "writer-b").await.expect("claim");
    let manifest = claim.manifest.expect("a commit exists");
    restore(
        &store,
        &manifest,
        &mut DirSink::new(tree::open_root(target.path()).expect("open"), false),
    )
    .await
    .expect("restore");

    let mut expected = snapshot(root);
    expected.retain(|(path, ..)| !path.starts_with("tmp"));
    assert_eq!(snapshot(target.path()), expected);
    let a = std::fs::metadata(target.path().join("a.txt")).expect("stat");
    let hard = std::fs::metadata(target.path().join("dir/hard.txt")).expect("stat");
    assert_eq!(a.ino(), hard.ino(), "hard links stay one inode");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_changed_files_are_staged_and_a_superseded_writer_stops() {
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    write(source.path(), "one", b"1");
    write(source.path(), "two", b"2");
    let store = MemoryStore::new();
    let mut a = replicator(&store, source.path(), staging.path(), &[]).await;
    a.commit(&store).await.expect("first commit");

    write(source.path(), "two", b"22");
    let (_, staged) = a.stage().expect("stage");
    assert!(staged.contains("two") && !staged.contains("one"));

    // Claim-then-restore: the new owner claims HEAD first, so the old writer, still running,
    // can never publish over what the new owner restores.
    let claim = takeover(&store, "writer-b").await.expect("claim");
    let claimed = claim.manifest.expect("a commit");
    a.commit(&store)
        .await
        .expect_err("the old writer is refused");
    assert!(a.is_superseded());
    let head = alien_sandbox_storage::read_head(&store)
        .await
        .expect("read")
        .expect("HEAD");
    let current = read_manifest(&store, head.0.commit.as_ref().expect("commit"))
        .await
        .expect("manifest");
    assert_eq!(current, claimed);
}

#[test]
fn the_walk_never_follows_a_symlink() {
    let outside = tempfile::tempdir().expect("tempdir");
    write(outside.path(), "secret", b"agent-only");
    let source = tempfile::tempdir().expect("tempdir");
    std::os::unix::fs::symlink(outside.path(), source.path().join("escape")).expect("symlink");
    std::os::unix::fs::symlink(outside.path().join("secret"), source.path().join("file"))
        .expect("symlink");
    let walk = tree::walk(&tree::open_root(source.path()).expect("open"), &[]).expect("walk");
    assert_eq!(walk.len(), 2);
    assert!(
        walk.iter()
            .all(|e| matches!(e.kind, TreeEntryKind::Symlink { .. })),
        "{walk:?}"
    );
    let root = tree::open_root(source.path()).expect("open");
    tree::open_file(&root, "file").expect_err("a symlinked file is not opened");
    tree::open_file(&root, "escape/secret").expect_err("a symlinked directory is not entered");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_replaces_a_planted_symlink_instead_of_writing_through_it() {
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    write(source.path(), "dir/file", b"restored");
    let store = MemoryStore::new();
    let mut a = replicator(&store, source.path(), staging.path(), &[]).await;
    a.commit(&store).await.expect("commit");

    let outside = tempfile::tempdir().expect("tempdir");
    let target = tempfile::tempdir().expect("tempdir");
    std::os::unix::fs::symlink(outside.path(), target.path().join("dir")).expect("symlink");
    let manifest = a.head().expect("head").clone();
    restore(
        &store,
        &manifest,
        &mut DirSink::new(tree::open_root(target.path()).expect("open"), false),
    )
    .await
    .expect("restore");
    assert!(
        std::fs::read_dir(outside.path())
            .expect("list")
            .next()
            .is_none(),
        "nothing written outside"
    );
    assert_eq!(
        std::fs::read(target.path().join("dir/file")).expect("read"),
        b"restored"
    );
    assert!(std::fs::symlink_metadata(target.path().join("dir"))
        .expect("stat")
        .is_dir());
}

#[test]
fn prune_removes_what_the_key_does_not_hold_but_keeps_excluded_paths() {
    let target = tempfile::tempdir().expect("tempdir");
    write(target.path(), "stale/file", b"x");
    write(target.path(), "tmp/keep", b"x");
    tree::prune(
        &tree::open_root(target.path()).expect("open"),
        None,
        &["tmp".to_string()],
    )
    .expect("prune");
    assert!(!target.path().join("stale").exists());
    assert!(target.path().join("tmp/keep").exists());
}

#[test]
fn exclude_matches_whole_components() {
    let exclude = vec!["tmp/".to_string(), ".cache".to_string()];
    assert!(tree::excluded("tmp", &exclude));
    assert!(tree::excluded("tmp/a", &exclude));
    assert!(!tree::excluded("tmpfile", &exclude));
    assert!(tree::excluded(".cache/x/y", &exclude));
    assert!(!tree::excluded("a/tmp", &exclude));
}
