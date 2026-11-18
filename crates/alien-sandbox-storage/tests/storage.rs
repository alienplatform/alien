//! Commit, restore, takeover and collection against an in-memory store with object-storage
//! conditional-put semantics.

use std::collections::BTreeMap;
use std::io::Read;

use alien_sandbox_storage::{
    collect_garbage, read_file, read_head, restore, takeover, CommitOutcome, Committer, Entry,
    EntryKind, FileSource, Manifest, MemoryStore, ObjectStore, Result, TreeEntry, TreeEntryKind,
    TreeSink, CHUNK_SIZE,
};

/// A tree held in memory: what a walk would report and the bytes behind each file.
#[derive(Default, Clone)]
struct Tree {
    entries: Vec<TreeEntry>,
    content: BTreeMap<String, Vec<u8>>,
}

impl Tree {
    fn dir(mut self, path: &str, mode: u32) -> Self {
        self.entries.push(TreeEntry {
            path: path.into(),
            kind: TreeEntryKind::Dir,
            mode,
            uid: 1001,
            gid: 1001,
            mtime_ns: 1_700_000_000_000_000_000,
        });
        self
    }

    fn file(mut self, path: &str, bytes: Vec<u8>, change: i64) -> Self {
        self.entries.retain(|e| e.path != path);
        self.entries.push(TreeEntry {
            path: path.into(),
            kind: TreeEntryKind::File {
                size: bytes.len() as u64,
                change_token: change,
            },
            mode: 0o640,
            uid: 1001,
            gid: 100,
            mtime_ns: 1_700_000_000_123_456_789 + change,
        });
        self.content.insert(path.into(), bytes);
        self
    }

    fn symlink(mut self, path: &str, target: &str) -> Self {
        self.entries.push(TreeEntry {
            path: path.into(),
            kind: TreeEntryKind::Symlink {
                target: target.into(),
            },
            mode: 0o777,
            uid: 0,
            gid: 0,
            mtime_ns: 5,
        });
        self
    }

    /// Another name for the same inode: either name opens the same bytes.
    fn hard_link(mut self, path: &str, target: &str) -> Self {
        let bytes = self.content[target].clone();
        self.content.insert(path.into(), bytes);
        self.entries.push(TreeEntry {
            path: path.into(),
            kind: TreeEntryKind::HardLink {
                target: target.into(),
            },
            mode: 0o640,
            uid: 1001,
            gid: 100,
            mtime_ns: 1,
        });
        self
    }
}

/// Counts which files a commit actually read.
struct Source<'a> {
    tree: &'a Tree,
    opened: Vec<String>,
}

impl FileSource for Source<'_> {
    fn open(&mut self, path: &str) -> Result<Box<dyn Read + '_>> {
        self.opened.push(path.to_string());
        Ok(Box::new(&self.tree.content[path][..]))
    }
}

/// Records a restore as `path -> (entry, bytes)`.
#[derive(Default)]
struct Restored {
    files: BTreeMap<String, (Entry, Vec<u8>)>,
    current: Vec<u8>,
    dirs_finished: Vec<String>,
}

impl TreeSink for Restored {
    fn dir(&mut self, entry: &Entry) -> Result<()> {
        self.files
            .insert(entry.path.clone(), (entry.clone(), vec![]));
        Ok(())
    }
    fn begin_file(&mut self, _: &Entry) -> Result<()> {
        self.current.clear();
        Ok(())
    }
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.current.extend_from_slice(bytes);
        Ok(())
    }
    fn end_file(&mut self, entry: &Entry) -> Result<()> {
        self.files.insert(
            entry.path.clone(),
            (entry.clone(), std::mem::take(&mut self.current)),
        );
        Ok(())
    }
    fn symlink(&mut self, entry: &Entry, _: &str) -> Result<()> {
        self.files
            .insert(entry.path.clone(), (entry.clone(), vec![]));
        Ok(())
    }
    fn hard_link(&mut self, entry: &Entry, target: &str) -> Result<()> {
        let bytes = self.files[target].1.clone();
        self.files
            .insert(entry.path.clone(), (entry.clone(), bytes));
        Ok(())
    }
    fn finish(&mut self, dirs: &[&Entry]) -> Result<()> {
        self.dirs_finished = dirs.iter().map(|d| d.path.clone()).collect();
        Ok(())
    }
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn sample() -> Tree {
    Tree::default()
        .dir(".git", 0o755)
        .dir("src", 0o2775)
        .file("src/main.rs", b"fn main() {}\n".to_vec(), 1)
        .file("db.sqlite", pattern(3 * CHUNK_SIZE + 17, 1), 1)
        .file("empty", vec![], 1)
        .file("dup-a", pattern(CHUNK_SIZE, 9), 1)
        .file("dup-b", pattern(CHUNK_SIZE, 9), 1)
        .symlink("link", "../outside/not-followed")
        .hard_link("src/main-again.rs", "src/main.rs")
}

async fn commit(
    store: &MemoryStore,
    committer: &mut Committer,
    tree: &Tree,
) -> (Result<CommitOutcome>, Vec<String>) {
    let mut source = Source {
        tree,
        opened: vec![],
    };
    let outcome = committer
        .commit(store, "engine-42", tree.entries.clone(), &mut source)
        .await;
    (outcome, source.opened)
}

async fn restore_head(store: &MemoryStore) -> Restored {
    let (head, _) = read_head(store).await.unwrap().unwrap();
    let manifest = alien_sandbox_storage::read_manifest(store, head.commit.as_ref().unwrap())
        .await
        .unwrap();
    let mut restored = Restored::default();
    restore(store, &manifest, &mut restored).await.unwrap();
    restored
}

#[tokio::test]
async fn a_tree_round_trips_exactly() {
    let store = MemoryStore::new();
    let start = takeover(&store, "sandbox-1").await.unwrap();
    assert!(start.manifest.is_none(), "a new key restores nothing");
    let mut writer = Committer::new("sandbox-1", start.head_etag, None, None);
    let tree = sample();
    let (outcome, _) = commit(&store, &mut writer, &tree).await;
    assert!(matches!(outcome.unwrap(), CommitOutcome::Committed { .. }));

    let restored = restore_head(&store).await;
    assert_eq!(restored.files.len(), tree.entries.len());
    for walked in &tree.entries {
        let (entry, bytes) = &restored.files[&walked.path];
        if matches!(
            walked.kind,
            TreeEntryKind::HardLink { .. } | TreeEntryKind::File { .. }
        ) && walked.path.starts_with("src/main")
        {
            // One inode, two names: the first name in path order carries the content.
            let expected = if walked.path == "src/main-again.rs" {
                matches!(entry.kind, EntryKind::File { .. })
            } else {
                entry.kind
                    == EntryKind::HardLink {
                        target: "src/main-again.rs".into(),
                    }
            };
            assert!(expected, "{}: {:?}", walked.path, entry.kind);
            assert_eq!(bytes, &tree.content[&walked.path]);
            continue;
        }
        assert_eq!(
            (entry.mode, entry.uid, entry.gid, entry.mtime_ns),
            (walked.mode, walked.uid, walked.gid, walked.mtime_ns),
            "{}",
            walked.path
        );
        match &walked.kind {
            TreeEntryKind::File { .. } | TreeEntryKind::HardLink { .. } => {
                assert_eq!(bytes, &tree.content[&walked.path], "{}", walked.path)
            }
            TreeEntryKind::Symlink { target } => {
                assert_eq!(
                    entry.kind,
                    EntryKind::Symlink {
                        target: target.clone()
                    }
                )
            }
            TreeEntryKind::Dir => assert_eq!(entry.kind, EntryKind::Dir),
        }
    }
    assert_eq!(
        restored.dirs_finished,
        vec!["src", ".git"],
        "deepest directory last in path order first"
    );
    assert_eq!(
        read_file(&store, "engine-42", "src/main-again.rs")
            .await
            .unwrap(),
        b"fn main() {}\n"
    );
    let missing = read_file(&store, "engine-42", "nope").await.unwrap_err();
    assert_eq!(missing.code, "SANDBOX_STORAGE_FILE_NOT_FOUND");
}

#[tokio::test]
async fn an_unchanged_tree_writes_nothing_and_an_edit_uploads_only_its_chunk() {
    let store = MemoryStore::new();
    let start = takeover(&store, "sandbox-1").await.unwrap();
    let mut writer = Committer::new("sandbox-1", start.head_etag, None, None);
    let tree = sample();
    commit(&store, &mut writer, &tree).await.0.unwrap();
    let objects = store.names();

    let (outcome, opened) = commit(&store, &mut writer, &tree).await;
    assert_eq!(outcome.unwrap(), CommitOutcome::Unchanged);
    assert!(
        opened.is_empty(),
        "unchanged files are not reread: {opened:?}"
    );
    assert_eq!(store.names(), objects);

    let mut db = tree.content["db.sqlite"].clone();
    db[CHUNK_SIZE + 5] ^= 0xff;
    let edited = tree.clone().file("db.sqlite", db.clone(), 2);
    let (outcome, opened) = commit(&store, &mut writer, &edited).await;
    assert_eq!(opened, vec!["db.sqlite"]);
    let CommitOutcome::Committed { pack_bytes, .. } = outcome.unwrap() else {
        panic!("an edit commits");
    };
    assert!(
        pack_bytes < 2 * CHUNK_SIZE as u64,
        "one changed chunk, got {pack_bytes} bytes"
    );
    assert_eq!(
        read_file(&store, "engine-42", "db.sqlite").await.unwrap(),
        db
    );
}

#[tokio::test]
async fn a_superseded_writer_is_refused_and_stops() {
    let store = MemoryStore::new();
    let first = takeover(&store, "sandbox-1").await.unwrap();
    let mut old = Committer::new("sandbox-1", first.head_etag, None, None);
    let tree = sample();
    commit(&store, &mut old, &tree).await.0.unwrap();

    let second = takeover(&store, "sandbox-2").await.unwrap();
    assert_eq!(
        second.manifest.as_ref().map(|m| m.entries.len()),
        Some(tree.entries.len())
    );
    let mut new = Committer::new("sandbox-2", second.head_etag, second.manifest, None);

    let changed = tree.clone().file("dup-a", b"stale".to_vec(), 9);
    let refused = commit(&store, &mut old, &changed).await.0.unwrap_err();
    assert_eq!(refused.code, "SANDBOX_STORAGE_SUPERSEDED");
    assert!(old.is_superseded());
    let again = commit(&store, &mut old, &changed).await.0.unwrap_err();
    assert_eq!(again.code, "SANDBOX_STORAGE_SUPERSEDED");

    let (head, _) = read_head(&store).await.unwrap().unwrap();
    assert_eq!(head.writer, "sandbox-2");
    assert_eq!(
        read_file(&store, "engine-42", "dup-a").await.unwrap(),
        pattern(CHUNK_SIZE, 9)
    );

    let fresh = tree.clone().file("dup-a", b"new".to_vec(), 10);
    commit(&store, &mut new, &fresh).await.0.unwrap();
    assert_eq!(
        read_file(&store, "engine-42", "dup-a").await.unwrap(),
        b"new"
    );

    // The stale writer's pack and manifest are leftovers at or below HEAD and are collected.
    let report = collect_garbage(&store).await.unwrap();
    assert!(!report.manifests.is_empty());
    restore_head(&store).await;
}

#[tokio::test]
async fn max_size_refuses_a_commit_whole() {
    let store = MemoryStore::new();
    let start = takeover(&store, "sandbox-1").await.unwrap();
    let mut writer = Committer::new("sandbox-1", start.head_etag, None, Some(CHUNK_SIZE as u64));
    let before = store.names();
    let refused = commit(&store, &mut writer, &sample()).await.0.unwrap_err();
    assert_eq!(refused.code, "SANDBOX_STORAGE_MAX_SIZE_EXCEEDED");
    assert_eq!(store.names(), before, "nothing was written");
    assert!(
        !writer.is_superseded(),
        "the writer keeps going once the tree shrinks"
    );

    let small = Tree::default().file("a", vec![1; 10], 1);
    commit(&store, &mut writer, &small).await.0.unwrap();
}

#[tokio::test]
async fn collection_keeps_what_head_reads_and_drops_the_rest() {
    let store = MemoryStore::new();
    let start = takeover(&store, "sandbox-1").await.unwrap();
    let mut writer = Committer::new("sandbox-1", start.head_etag, None, None);
    let tree = sample();
    commit(&store, &mut writer, &tree).await.0.unwrap();
    // Replace every file so the first pack is no longer read by HEAD.
    let mut replaced = Tree::default();
    for (index, path) in ["a", "b"].iter().enumerate() {
        replaced = replaced.file(path, pattern(1000, index as u8 + 50), 3);
    }
    commit(&store, &mut writer, &replaced).await.0.unwrap();
    // A commit that keeps one old file keeps its pack.
    let kept = replaced.clone().file("c", pattern(10, 99), 4);
    commit(&store, &mut writer, &kept).await.0.unwrap();

    let report = collect_garbage(&store).await.unwrap();
    assert_eq!(report.packs.len(), 1, "{report:?}");
    assert_eq!(report.manifests.len(), 2, "{report:?}");
    let remaining: Vec<String> = store.names();
    assert_eq!(
        remaining.iter().filter(|n| n.starts_with("packs/")).count(),
        2
    );
    assert_eq!(
        remaining
            .iter()
            .filter(|n| n.starts_with("manifests/"))
            .count(),
        1
    );
    let restored = restore_head(&store).await;
    assert_eq!(restored.files["a"].1, pattern(1000, 50));
    assert_eq!(restored.files["c"].1, pattern(10, 99));
}

#[tokio::test]
async fn a_damaged_pack_fails_the_restore() {
    let store = MemoryStore::new();
    let start = takeover(&store, "sandbox-1").await.unwrap();
    let mut writer = Committer::new("sandbox-1", start.head_etag, None, None);
    commit(&store, &mut writer, &sample()).await.0.unwrap();
    let pack = store
        .names()
        .into_iter()
        .find(|n| n.starts_with("packs/"))
        .unwrap();
    let mut bytes = store.get(&pack).await.unwrap().unwrap().bytes;
    bytes[600] ^= 1;
    store.overwrite(&pack, bytes);

    let error = read_file(&store, "engine-42", "db.sqlite")
        .await
        .unwrap_err();
    assert_eq!(error.code, "SANDBOX_STORAGE_CORRUPT");
}

#[test]
fn a_manifest_cannot_name_a_path_outside_the_root() {
    for path in ["../etc/passwd", "/etc/passwd", "a/../../b"] {
        let manifest = serde_json::json!({
            "version": 1,
            "seq": 1,
            "writer": "w",
            "entries": [{ "path": path, "type": "dir", "mode": 493, "uid": 0, "gid": 0, "mtimeNs": 0 }],
            "chunks": {},
        });
        let error = Manifest::parse("m", &serde_json::to_vec(&manifest).unwrap()).unwrap_err();
        assert_eq!(error.code, "SANDBOX_STORAGE_CORRUPT", "{path}");
    }
}

#[test]
fn a_hard_link_must_point_at_an_earlier_file() {
    let manifest = serde_json::json!({
        "version": 1,
        "seq": 1,
        "writer": "w",
        "entries": [{ "path": "a", "type": "hardLink", "target": "b", "mode": 420, "uid": 0, "gid": 0, "mtimeNs": 0 }],
        "chunks": {},
    });
    assert!(Manifest::parse("m", &serde_json::to_vec(&manifest).unwrap()).is_err());
}

/// Every object a store holds, with its bytes.
async fn snapshot(store: &MemoryStore) -> BTreeMap<String, Vec<u8>> {
    let mut objects = BTreeMap::new();
    for name in store.names() {
        objects.insert(name.clone(), store.get(&name).await.unwrap().unwrap().bytes);
    }
    objects
}

/// The race seen on another cloud: a superseded writer, still at the sequence number the new
/// owner has just used, publishes its pack and manifest before its HEAD put is refused. Nothing
/// the owner published may change, and a restore must return the owner's tree.
#[tokio::test]
async fn a_stale_writer_racing_the_new_owner_never_changes_the_owners_tree() {
    let store = MemoryStore::new();
    let first = takeover(&store, "sandbox-1").await.unwrap();
    let mut stale = Committer::new("sandbox-1", first.head_etag, None, None);
    commit(&store, &mut stale, &sample()).await.0.unwrap();

    // The owner takes over and commits seq 2 while the stale writer still believes it is at 1.
    let second = takeover(&store, "sandbox-2").await.unwrap();
    let mut owner = Committer::new("sandbox-2", second.head_etag, second.manifest, None);
    let owners = sample()
        .file("db.sqlite", pattern(2 * CHUNK_SIZE, 77), 20)
        .file("notes.md", b"owner".to_vec(), 20);
    let CommitOutcome::Committed { commit: owner_commit, .. } =
        commit(&store, &mut owner, &owners).await.0.unwrap()
    else {
        panic!("the owner's tree changed")
    };
    assert_eq!(owner_commit.seq, 2);
    let published = snapshot(&store).await;

    // The stale writer's commit is also seq 2.
    let stales = sample()
        .file("db.sqlite", pattern(2 * CHUNK_SIZE, 13), 30)
        .file("notes.md", b"stale".to_vec(), 30);
    let refused = commit(&store, &mut stale, &stales).await.0.unwrap_err();
    assert_eq!(refused.code, "SANDBOX_STORAGE_SUPERSEDED");

    let after = snapshot(&store).await;
    for (name, bytes) in &published {
        assert_eq!(after.get(name), Some(bytes), "{name} changed");
    }
    let restored = restore_head(&store).await;
    assert_eq!(restored.files["notes.md"].1, b"owner");
    assert_eq!(restored.files["db.sqlite"].1, pattern(2 * CHUNK_SIZE, 77));
}

/// No writer can replace an object once it exists: creating a name that is taken is refused.
#[tokio::test]
async fn a_published_object_cannot_be_created_again() {
    let store = MemoryStore::new();
    alien_sandbox_storage::create(&store, "manifests/x.json", b"one".to_vec())
        .await
        .unwrap();
    let error = alien_sandbox_storage::create(&store, "manifests/x.json", b"two".to_vec())
        .await
        .unwrap_err();
    assert_eq!(error.code, "SANDBOX_STORAGE_STORE_FAILED");
    assert_eq!(store.get("manifests/x.json").await.unwrap().unwrap().bytes, b"one");
}

/// A manifest replaced behind the store's back (raw access, damage) is refused by its hash in
/// HEAD rather than restored.
#[tokio::test]
async fn a_replaced_manifest_is_refused_by_heads_hash() {
    let store = MemoryStore::new();
    let start = takeover(&store, "sandbox-1").await.unwrap();
    let mut writer = Committer::new("sandbox-1", start.head_etag, None, None);
    commit(&store, &mut writer, &sample()).await.0.unwrap();
    let (head, _) = read_head(&store).await.unwrap().unwrap();
    let object = head.commit.as_ref().unwrap().manifest();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&store.get(&object).await.unwrap().unwrap().bytes).unwrap();
    manifest["writer"] = serde_json::json!("someone-else");
    store.overwrite(&object, serde_json::to_vec(&manifest).unwrap());

    let error = read_file(&store, "engine-42", "src/main.rs")
        .await
        .unwrap_err();
    assert_eq!(error.code, "SANDBOX_STORAGE_CORRUPT");
}

/// A store whose uploads ignore `If-None-Match`, as an S3 presigned POST does (measured: a
/// second POST to the same name overwrites). Only HEAD's `If-Match` is honored.
struct OverwritingStore(MemoryStore);

#[async_trait::async_trait]
impl ObjectStore for OverwritingStore {
    async fn get(&self, object: &str) -> Result<Option<alien_sandbox_storage::StoredObject>> {
        self.0.get(object).await
    }
    async fn get_range(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        self.0.get_range(object, offset, len).await
    }
    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: alien_sandbox_storage::Precondition,
    ) -> Result<alien_sandbox_storage::PutOutcome> {
        if precondition == alien_sandbox_storage::Precondition::Absent && object != "HEAD" {
            self.0.overwrite(object, bytes);
            return Ok(alien_sandbox_storage::PutOutcome::Written {
                etag: "overwritten".to_string(),
            });
        }
        self.0.put_if(object, bytes, precondition).await
    }
    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        self.0.list(prefix).await
    }
    async fn delete(&self, object: &str) -> Result<()> {
        self.0.delete(object).await
    }
}

/// The same race on a store that cannot refuse an upload: names are content hashes, so the
/// stale writer's same-sequence objects land under other names and the owner's tree survives.
#[tokio::test]
async fn a_stale_writer_cannot_change_the_owners_tree_even_when_uploads_overwrite() {
    let store = OverwritingStore(MemoryStore::new());
    let first = takeover(&store, "sandbox-1").await.unwrap();
    let mut stale = Committer::new("sandbox-1", first.head_etag, None, None);
    let tree = sample();
    let mut source = Source { tree: &tree, opened: vec![] };
    stale
        .commit(&store, "engine-42", tree.entries.clone(), &mut source)
        .await
        .unwrap();

    let second = takeover(&store, "sandbox-2").await.unwrap();
    let mut owner = Committer::new("sandbox-2", second.head_etag, second.manifest, None);
    let owners = sample().file("notes.md", b"owner".to_vec(), 20);
    let mut source = Source { tree: &owners, opened: vec![] };
    owner
        .commit(&store, "engine-42", owners.entries.clone(), &mut source)
        .await
        .unwrap();
    let published = snapshot(&store.0).await;

    let stales = sample().file("notes.md", b"stale".to_vec(), 30);
    let mut source = Source { tree: &stales, opened: vec![] };
    let refused = stale
        .commit(&store, "engine-42", stales.entries.clone(), &mut source)
        .await
        .unwrap_err();
    assert_eq!(refused.code, "SANDBOX_STORAGE_SUPERSEDED");

    let after = snapshot(&store.0).await;
    for (name, bytes) in &published {
        assert_eq!(after.get(name), Some(bytes), "{name} changed");
    }
    assert_eq!(read_file(&store, "engine-42", "notes.md").await.unwrap(), b"owner");
}
