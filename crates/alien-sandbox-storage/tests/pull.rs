//! The pull transport: an agent committing into its spool, a binding publishing the spool to the
//! store, and a restore seeded from the store into a new sandbox's spool.

use std::collections::BTreeMap;
use std::io::Read;

use alien_sandbox_storage::{
    read_head, read_manifest, restore, seed, takeover, AckOutcome, CommitOutcome, Committer, Entry,
    ErrorData, FileSource, MemoryStore, ObjectStore, Pending, PullChannel, PullOutcome, Puller,
    Result, Spool, TreeEntry, TreeEntryKind, TreeSink, CHUNK_SIZE,
};
use async_trait::async_trait;

/// A small page, so every object crosses the channel in several reads.
const PAGE: u64 = 10_000;

#[derive(Default, Clone)]
struct Tree {
    entries: Vec<TreeEntry>,
    content: BTreeMap<String, Vec<u8>>,
}

impl Tree {
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
            mtime_ns: 1_700_000_000_000_000_000 + change,
        });
        self.content.insert(path.into(), bytes);
        self
    }

    fn dir(mut self, path: &str) -> Self {
        self.entries.push(TreeEntry {
            path: path.into(),
            kind: TreeEntryKind::Dir,
            mode: 0o755,
            uid: 1001,
            gid: 1001,
            mtime_ns: 7,
        });
        self
    }

    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        self.content.clone()
    }
}

impl FileSource for &Tree {
    fn open(&mut self, path: &str) -> Result<Box<dyn Read + '_>> {
        Ok(Box::new(&self.content[path][..]))
    }
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn sample() -> Tree {
    Tree::default()
        .dir("src")
        .file("src/main.rs", b"fn main() {}\n".to_vec(), 1)
        .file("db.sqlite", pattern(3 * CHUNK_SIZE + 17, 1), 1)
        .file("empty", vec![], 1)
}

#[derive(Default)]
struct Files(BTreeMap<String, Vec<u8>>, Vec<u8>);

impl TreeSink for Files {
    fn dir(&mut self, _: &Entry) -> Result<()> {
        Ok(())
    }
    fn begin_file(&mut self, _: &Entry) -> Result<()> {
        self.1.clear();
        Ok(())
    }
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.1.extend_from_slice(bytes);
        Ok(())
    }
    fn end_file(&mut self, entry: &Entry) -> Result<()> {
        self.0
            .insert(entry.path.clone(), std::mem::take(&mut self.1));
        Ok(())
    }
    fn symlink(&mut self, _: &Entry, _: &str) -> Result<()> {
        Ok(())
    }
    fn hard_link(&mut self, entry: &Entry, target: &str) -> Result<()> {
        let bytes = self.0[target].clone();
        self.0.insert(entry.path.clone(), bytes);
        Ok(())
    }
    fn finish(&mut self, _: &[&Entry]) -> Result<()> {
        Ok(())
    }
}

async fn files_at_head(store: &dyn ObjectStore) -> BTreeMap<String, Vec<u8>> {
    let (head, _) = read_head(store).await.unwrap().unwrap();
    let manifest = read_manifest(store, head.commit.as_ref().unwrap())
        .await
        .unwrap();
    let mut files = Files::default();
    restore(store, &manifest, &mut files).await.unwrap();
    files.0
}

/// A sandbox that took `key` over in `store` and commits into its own spool.
struct Sandbox {
    _dir: tempfile::TempDir,
    spool: Spool,
    committer: Committer,
    puller: Puller,
}

async fn sandbox(store: &MemoryStore, writer: &str) -> Sandbox {
    let start = takeover(store, writer).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let spool = Spool::new(dir.path().join("spool"), start.head_etag.clone()).unwrap();
    Sandbox {
        _dir: dir,
        spool,
        committer: Committer::new(writer, start.head_etag.clone(), start.manifest, None),
        puller: Puller::new(writer, start.head_etag, PAGE),
    }
}

impl Sandbox {
    async fn commit(&mut self, tree: &Tree) -> Result<CommitOutcome> {
        let mut source = tree;
        self.committer
            .commit(&self.spool, "engine-42", tree.entries.clone(), &mut source)
            .await
    }

    async fn pull(&mut self, store: &MemoryStore) -> Result<PullOutcome> {
        self.puller.pull(&self.spool, store, "engine-42").await
    }
}

#[tokio::test]
async fn a_pulled_commit_restores_exactly_and_empties_the_spool() {
    let store = MemoryStore::new();
    let mut sandbox = sandbox(&store, "sandbox-1").await;
    let tree = sample();
    sandbox.commit(&tree).await.unwrap();
    assert!(
        read_head(&store).await.unwrap().unwrap().0.commit.is_none(),
        "nothing reaches the store until the binding pulls"
    );

    let outcome = sandbox.pull(&store).await.unwrap();
    assert!(matches!(
        outcome,
        PullOutcome::Published {
            commit: Some(_),
            ..
        }
    ));
    assert_eq!(files_at_head(&store).await, tree.files());
    assert_eq!(sandbox.spool.pending(), None);
    assert!(
        sandbox.spool.list("").await.unwrap().is_empty(),
        "published objects leave the spool"
    );
    assert_eq!(sandbox.pull(&store).await.unwrap(), PullOutcome::Idle);
}

#[tokio::test]
async fn commits_the_binding_did_not_see_are_published_with_the_latest() {
    let store = MemoryStore::new();
    let mut sandbox = sandbox(&store, "sandbox-1").await;
    sandbox.commit(&sample()).await.unwrap();
    let edited = sample()
        .file("src/main.rs", b"fn main() { edited() }\n".to_vec(), 2)
        .file("new.txt", pattern(CHUNK_SIZE + 1, 4), 2);
    sandbox.commit(&edited).await.unwrap();

    sandbox.pull(&store).await.unwrap();
    let (head, _) = read_head(&store).await.unwrap().unwrap();
    assert_eq!(head.commit.unwrap().seq, 2);
    // The unchanged db.sqlite lives in commit 1's pack, published with commit 2.
    assert_eq!(files_at_head(&store).await, edited.files());

    // And the agent keeps committing on top of what was published.
    let third = edited.clone().file("empty", b"x".to_vec(), 3);
    sandbox.commit(&third).await.unwrap();
    sandbox.pull(&store).await.unwrap();
    assert_eq!(files_at_head(&store).await, third.files());
}

#[tokio::test]
async fn a_commit_made_while_the_binding_pulls_stays_pending() {
    let store = MemoryStore::new();
    let mut sandbox = sandbox(&store, "sandbox-1").await;
    sandbox.commit(&sample()).await.unwrap();
    let first: Pending = sandbox.spool.pending().unwrap();

    let edited = sample().file("late.txt", pattern(100, 3), 2);
    sandbox.commit(&edited).await.unwrap();
    sandbox
        .spool
        .ack(first.generation, AckOutcome::Published)
        .unwrap();

    let second = sandbox
        .spool
        .pending()
        .expect("the later commit is still pending");
    assert!(second.generation > first.generation);
    assert!(second
        .objects
        .iter()
        .all(|object| !first.objects.contains(object)));
    assert!(!second.objects.is_empty());
}

#[tokio::test]
async fn a_superseded_sandbox_publishes_nothing_and_its_agent_stops() {
    let store = MemoryStore::new();
    let mut old = sandbox(&store, "sandbox-1").await;
    let tree = sample();
    old.commit(&tree).await.unwrap();
    old.pull(&store).await.unwrap();

    let mut new = sandbox(&store, "sandbox-2").await;
    let mine = sample().file("owner.txt", b"sandbox-2".to_vec(), 9);
    new.commit(&mine).await.unwrap();
    new.pull(&store).await.unwrap();

    let stale = sample().file("owner.txt", b"sandbox-1".to_vec(), 8);
    old.commit(&stale).await.unwrap();
    let error = old.pull(&store).await.unwrap_err();
    assert!(
        matches!(error.error, Some(ErrorData::Superseded { .. })),
        "{error:?}"
    );
    assert!(old.spool.is_superseded());
    let error = old
        .commit(&stale.clone().file("more", vec![1], 10))
        .await
        .unwrap_err();
    assert!(matches!(error.error, Some(ErrorData::Superseded { .. })));
    assert_eq!(files_at_head(&store).await, mine.files());
}

/// Hands every read back with one byte flipped, as a damaged channel would.
struct Damaging<'a>(&'a Spool);

#[async_trait]
impl PullChannel for Damaging<'_> {
    async fn pending(&self) -> Result<Option<Pending>> {
        Ok(self.0.pending())
    }
    async fn read(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        let mut bytes = self.0.read(object, offset, len)?;
        if offset == 0 {
            bytes[0] ^= 1;
        }
        Ok(bytes)
    }
    async fn ack(&self, generation: u64, outcome: AckOutcome) -> Result<()> {
        self.0.ack(generation, outcome)
    }
    async fn seed(&self, object: &str, offset: u64, bytes: Vec<u8>, last: bool) -> Result<()> {
        self.0.seed(object, offset, &bytes, last)
    }
}

#[tokio::test]
async fn damaged_bytes_are_refused_and_head_does_not_move() {
    let store = MemoryStore::new();
    let mut sandbox = sandbox(&store, "sandbox-1").await;
    sandbox.commit(&sample()).await.unwrap();
    let before = store.get("HEAD").await.unwrap().unwrap();

    let error = sandbox
        .puller
        .pull(&Damaging(&sandbox.spool), &store, "engine-42")
        .await
        .unwrap_err();
    assert!(
        matches!(error.error, Some(ErrorData::Corrupt { .. })),
        "{error:?}"
    );
    assert_eq!(store.get("HEAD").await.unwrap().unwrap(), before);
    assert!(store.names().iter().all(|name| name == "HEAD"));

    // The spool still holds the commit; an intact channel publishes it.
    sandbox.pull(&store).await.unwrap();
    assert_eq!(files_at_head(&store).await, sample().files());
}

#[tokio::test]
async fn a_head_naming_another_writer_is_refused() {
    let store = MemoryStore::new();
    let mut sandbox = sandbox(&store, "sandbox-1").await;
    sandbox.commit(&sample()).await.unwrap();
    let mut puller = Puller::new("sandbox-9", "whatever", PAGE);
    let error = puller
        .pull(&sandbox.spool, &store, "engine-42")
        .await
        .unwrap_err();
    assert!(
        matches!(error.error, Some(ErrorData::Corrupt { .. })),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_restore_seeded_into_the_spool_matches_the_store() {
    let store = MemoryStore::new();
    let mut first = sandbox(&store, "sandbox-1").await;
    first.commit(&sample()).await.unwrap();
    first.pull(&store).await.unwrap();
    let edited = sample().file("src/main.rs", b"changed\n".to_vec(), 2);
    first.commit(&edited).await.unwrap();
    first.pull(&store).await.unwrap();

    // The binding takes the key over for a new sandbox and seeds its spool.
    let start = takeover(&store, "sandbox-2").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let spool = Spool::new(dir.path().join("spool"), start.head_etag.clone()).unwrap();
    let (head, _) = read_head(&store).await.unwrap().unwrap();
    let manifest = seed(&spool, &store, head.commit.as_ref().unwrap(), PAGE)
        .await
        .unwrap();
    assert_eq!(Some(&manifest), start.manifest.as_ref());

    let mut files = Files::default();
    restore(&spool, &manifest, &mut files).await.unwrap();
    assert_eq!(files.0, edited.files());
    assert_eq!(spool.pending(), None, "seeded objects are not pending");

    spool.clear_seeded();
    assert!(spool.list("").await.unwrap().is_empty());

    // The new sandbox continues from the restored manifest without re-sending unchanged data.
    let mut committer = Committer::new("sandbox-2", start.head_etag.clone(), Some(manifest), None);
    let mut puller = Puller::new("sandbox-2", start.head_etag, PAGE);
    let more = edited.clone().file("added", b"new".to_vec(), 3);
    let mut source = &more;
    committer
        .commit(&spool, "engine-42", more.entries.clone(), &mut source)
        .await
        .unwrap();
    let PullOutcome::Published { bytes, .. } =
        puller.pull(&spool, &store, "engine-42").await.unwrap()
    else {
        panic!("published");
    };
    assert!(
        bytes < 4 * 1024 * 4,
        "only the new file and the manifest cross: {bytes}"
    );
    assert_eq!(files_at_head(&store).await, more.files());
}

#[tokio::test]
async fn the_spool_holds_only_packs_and_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let spool = Spool::new(dir.path().join("spool"), "v1").unwrap();
    for name in [
        "../escape",
        "packs/../../x",
        "HEAD",
        "packs/1-abc.tar",
        "manifests/x.json",
    ] {
        assert!(spool.seed(name, 0, b"x", true).is_err(), "{name}");
        assert!(spool.read(name, 0, 1).is_err(), "{name}");
    }
    let hash = "a".repeat(64);
    let pack = format!("packs/{:020}-{hash}.tar", 1);
    spool.seed(&pack, 0, b"ab", false).unwrap();
    assert!(
        spool.seed(&pack, 5, b"cd", true).is_err(),
        "a page out of order is refused"
    );
    spool.seed(&pack, 2, b"cd", true).unwrap();
    assert_eq!(spool.read(&pack, 1, 10).unwrap(), b"bcd");
}

/// Leaves the manifests out of what the agent reports pending.
struct HidingManifests<'a>(&'a Spool);

#[async_trait]
impl PullChannel for HidingManifests<'_> {
    async fn pending(&self) -> Result<Option<Pending>> {
        Ok(self.0.pending().map(|mut pending| {
            pending
                .objects
                .retain(|object| !object.name.starts_with("manifests/"));
            pending
        }))
    }
    async fn read(&self, object: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        self.0.read(object, offset, len)
    }
    async fn ack(&self, generation: u64, outcome: AckOutcome) -> Result<()> {
        self.0.ack(generation, outcome)
    }
    async fn seed(&self, object: &str, offset: u64, bytes: Vec<u8>, last: bool) -> Result<()> {
        self.0.seed(object, offset, &bytes, last)
    }
}

#[tokio::test]
async fn head_never_names_a_manifest_the_store_lacks() {
    let store = MemoryStore::new();
    let mut sandbox = sandbox(&store, "sandbox-1").await;
    sandbox.commit(&sample()).await.unwrap();
    let before = store.get("HEAD").await.unwrap().unwrap();
    let error = sandbox
        .puller
        .pull(&HidingManifests(&sandbox.spool), &store, "engine-42")
        .await
        .unwrap_err();
    assert!(
        matches!(error.error, Some(ErrorData::Corrupt { .. })),
        "{error:?}"
    );
    assert_eq!(store.get("HEAD").await.unwrap().unwrap(), before);
}
