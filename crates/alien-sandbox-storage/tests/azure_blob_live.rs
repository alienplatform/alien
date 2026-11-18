//! The storage core against real Azure Blob Storage, through a directory SAS for one key, as
//! the agent uses it. Ignored: it needs a hierarchical-namespace account the service principal
//! holds Storage Blob Data Contributor on.
//!
//! ```text
//! AZURE_TARGET_SUBSCRIPTION_ID=… AZURE_TARGET_TENANT_ID=… AZURE_TARGET_CLIENT_ID=… \
//! AZURE_TARGET_CLIENT_SECRET=… AZURE_PERSISTENT_STORAGE_ACCOUNT=… \
//! cargo test -p alien-sandbox-storage --features azure-blob --test azure_blob_live -- --ignored
//! ```

use std::collections::BTreeMap;
use std::io::Read;

use alien_azure_clients::azure::blob_sas::{
    directory_sas, AzureBlobDelegationClient, BlobDelegationApi, DirectorySasPermissions,
};
use alien_azure_clients::{AzureClientConfig, AzureCredentials, AzureTokenCache};
use alien_sandbox_storage::{
    read_head, read_manifest, restore, takeover, AzureBlobStore, CommitOutcome, Committer, Entry,
    FileSource, ObjectStore, Precondition, PutOutcome, Result, TreeEntry, TreeEntryKind, TreeSink,
};
use chrono::{Duration, Utc};

const CONTAINER: &str = "store";

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn token_cache() -> AzureTokenCache {
    AzureTokenCache::new(AzureClientConfig {
        subscription_id: env("AZURE_TARGET_SUBSCRIPTION_ID"),
        tenant_id: env("AZURE_TARGET_TENANT_ID"),
        region: None,
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("AZURE_TARGET_CLIENT_ID"),
            client_secret: env("AZURE_TARGET_CLIENT_SECRET"),
        },
        service_overrides: None,
    })
}

fn account() -> String {
    env("AZURE_PERSISTENT_STORAGE_ACCOUNT")
}

/// The binding's side: a SAS for one key's directory, minted with its own identity.
async fn sas_for(directory: &str) -> String {
    let start = Utc::now() - Duration::minutes(5);
    let expiry = Utc::now() + Duration::minutes(30);
    let key = AzureBlobDelegationClient::new(reqwest::Client::new(), token_cache())
        .get_user_delegation_key(&account(), start, expiry)
        .await
        .expect("a user delegation key");
    directory_sas(
        &key,
        &account(),
        CONTAINER,
        directory,
        DirectorySasPermissions {
            read: true,
            add: true,
            create: true,
            write: true,
        },
        start,
        expiry,
    )
    .expect("a directory SAS")
}

fn key_url(directory: &str) -> String {
    format!(
        "https://{}.blob.core.windows.net/{CONTAINER}/{directory}/",
        account()
    )
}

/// A fresh key directory for one test run, so runs never see each other's objects.
fn fresh_root() -> String {
    format!("sandbox-storage/live-{}", uuid::Uuid::new_v4().simple())
}

async fn ensure_container() {
    let token = token_cache()
        .get_bearer_token_with_scope("https://storage.azure.com/.default")
        .await
        .unwrap();
    let status = reqwest::Client::new()
        .put(format!(
            "https://{}.blob.core.windows.net/{CONTAINER}?restype=container",
            account()
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("x-ms-version", "2026-10-06")
        .header("Content-Length", "0")
        .send()
        .await
        .unwrap()
        .status();
    assert!(status == 201 || status == 409, "container: {status}");
}

/// Deletes one blob with the binding's identity, as erasing a key does.
async fn delete_with_identity(path: &str) {
    let token = token_cache()
        .get_bearer_token_with_scope("https://storage.azure.com/.default")
        .await
        .unwrap();
    let status = reqwest::Client::new()
        .delete(format!(
            "https://{}.blob.core.windows.net/{CONTAINER}/{path}",
            account()
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("x-ms-version", "2026-10-06")
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, 202, "delete {path}");
}

struct Files(BTreeMap<String, Vec<u8>>);

impl FileSource for Files {
    fn open(&mut self, path: &str) -> Result<Box<dyn Read + Send + '_>> {
        // `src/again` is another name for `src/main.rs`'s inode; the core opens a hard-link
        // group by its first name.
        let path = if path == "src/again" {
            "src/main.rs"
        } else {
            path
        };
        Ok(Box::new(&self.0[path][..]))
    }
}

#[derive(Default)]
struct Restored {
    files: BTreeMap<String, (Entry, Vec<u8>)>,
    current: Vec<u8>,
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
        let bytes = std::mem::take(&mut self.current);
        self.files
            .insert(entry.path.clone(), (entry.clone(), bytes));
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
    fn finish(&mut self, _: &[&Entry]) -> Result<()> {
        Ok(())
    }
}

fn walk(files: &BTreeMap<String, Vec<u8>>, change: i64) -> Vec<TreeEntry> {
    let mut entries = vec![TreeEntry {
        path: "src".into(),
        kind: TreeEntryKind::Dir,
        mode: 0o2775,
        uid: 1001,
        gid: 1001,
        mtime_ns: 1,
    }];
    for (path, bytes) in files {
        entries.push(TreeEntry {
            path: path.clone(),
            kind: TreeEntryKind::File {
                size: bytes.len() as u64,
                change_token: change,
            },
            mode: 0o640,
            uid: 1001,
            gid: 100,
            mtime_ns: 1_700_000_000_123_456_789 + change,
        });
    }
    entries.push(TreeEntry {
        path: "src/again".into(),
        kind: TreeEntryKind::HardLink {
            target: "src/main.rs".into(),
        },
        mode: 0o640,
        uid: 1001,
        gid: 100,
        mtime_ns: 1,
    });
    entries
}

/// Two sandboxes for one key, each with its own SAS: the second takes over, the first's next
/// commit is refused, and the second restores exactly what the first committed.
#[tokio::test]
#[ignore = "needs a live Azure hierarchical-namespace account"]
async fn live_commit_takeover_and_restore_through_a_key_sas() {
    ensure_container().await;
    let directory = format!("{}/engine-42", fresh_root());
    let first = AzureBlobStore::new(&key_url(&directory), &sas_for(&directory).await).unwrap();

    let start = takeover(&first, "sandbox-1").await.unwrap();
    assert!(start.manifest.is_none());
    let mut writer = Committer::new("sandbox-1", start.head_version, None, None);
    let mut files = BTreeMap::new();
    files.insert("src/main.rs".to_string(), b"fn main() {}\n".to_vec());
    files.insert(
        "db.sqlite".to_string(),
        (0..300_000u32).map(|i| (i % 251) as u8).collect(),
    );
    let outcome = writer
        .commit(
            &first,
            "engine-42",
            walk(&files, 1),
            &mut Files(files.clone()),
        )
        .await
        .unwrap();
    assert!(
        matches!(outcome, CommitOutcome::Committed { .. }),
        "{outcome:?}"
    );

    let second = AzureBlobStore::new(&key_url(&directory), &sas_for(&directory).await).unwrap();
    let taken = takeover(&second, "sandbox-2").await.unwrap();
    let manifest = taken.manifest.expect("the first writer's commit");
    let mut restored = Restored::default();
    restore(&second, &manifest, &mut restored).await.unwrap();
    for (path, bytes) in &files {
        assert_eq!(&restored.files[path].1, bytes, "{path}");
        assert_eq!(restored.files[path].0.uid, 1001);
        assert_eq!(restored.files[path].0.mode, 0o640);
    }
    assert_eq!(restored.files["src/again"].1, files["src/main.rs"]);

    files.insert("late.txt".to_string(), b"from the stale writer".to_vec());
    let stale = writer
        .commit(
            &first,
            "engine-42",
            walk(&files, 2),
            &mut Files(files.clone()),
        )
        .await
        .expect_err("the superseded writer is refused");
    assert_eq!(stale.code, "SANDBOX_STORAGE_SUPERSEDED");
    assert!(writer.is_superseded());

    let (head, _) = read_head(&second).await.unwrap().unwrap();
    assert_eq!(head.writer, "sandbox-2");
    let current = read_manifest(&second, head.commit.as_ref().unwrap())
        .await
        .unwrap();
    assert!(
        current.entry("late.txt").is_none(),
        "the stale commit never landed"
    );
}

/// A key's SAS reaches nothing outside its directory, and the store says so as a failure, never
/// as a missing object or a lost race.
#[tokio::test]
#[ignore = "needs a live Azure hierarchical-namespace account"]
async fn live_a_key_sas_cannot_reach_another_key() {
    ensure_container().await;
    let root = fresh_root();
    let mine = format!("{root}/key1");
    let theirs = format!("{root}/key2");
    let owner = AzureBlobStore::new(&key_url(&theirs), &sas_for(&theirs).await).unwrap();
    takeover(&owner, "sandbox-other").await.unwrap();

    let my_sas = sas_for(&mine).await;
    for target in [
        theirs.clone(),
        format!("{root}/key10"),
        format!("{root}/key1x"),
        root.clone(),
    ] {
        let pointed = AzureBlobStore::new(&key_url(&target), &my_sas).unwrap();
        let read = pointed.get("HEAD").await.expect_err(&target);
        assert!(read.message.contains("403"), "{target}: {}", read.message);
        let write = pointed
            .put_if("HEAD", b"evil".to_vec(), Precondition::Absent)
            .await
            .expect_err(&target);
        assert!(write.message.contains("403"), "{target}: {}", write.message);
        assert!(
            !write.message.contains("sig="),
            "the SAS never reaches an error"
        );
    }
    let (head, _) = read_head(&owner).await.unwrap().unwrap();
    assert_eq!(head.writer, "sandbox-other", "the other key is untouched");
}

/// Create-only and compare-and-swap behave as the core expects on Azure: an existing name is a
/// conflict, a stale or erased HEAD is a conflict, and HEAD moves only on its current ETag.
#[tokio::test]
#[ignore = "needs a live Azure hierarchical-namespace account"]
async fn live_conditional_puts_match_the_core() {
    ensure_container().await;
    let directory = format!("{}/engine-7", fresh_root());
    let store = AzureBlobStore::new(&key_url(&directory), &sas_for(&directory).await).unwrap();

    let PutOutcome::Written { version: etag } = store
        .put_if("HEAD", b"one".to_vec(), Precondition::Absent)
        .await
        .unwrap()
    else {
        panic!("a new HEAD is written")
    };
    assert_eq!(
        store
            .put_if("HEAD", b"again".to_vec(), Precondition::Absent)
            .await
            .unwrap(),
        PutOutcome::Conflict
    );
    let PutOutcome::Written { version: moved } = store
        .put_if("HEAD", b"two".to_vec(), Precondition::Matches(etag.clone()))
        .await
        .unwrap()
    else {
        panic!("HEAD moves on its current ETag")
    };
    assert_eq!(
        store
            .put_if("HEAD", b"stale".to_vec(), Precondition::Matches(etag))
            .await
            .unwrap(),
        PutOutcome::Conflict
    );
    assert_eq!(store.get("HEAD").await.unwrap().unwrap().bytes, b"two");
    assert_eq!(store.get_range("HEAD", 1, 2).await.unwrap(), b"wo".to_vec());
    assert!(store.get("packs/none.tar").await.unwrap().is_none());

    delete_with_identity(&format!("{directory}/HEAD")).await;
    assert_eq!(
        store
            .put_if(
                "HEAD",
                b"after erase".to_vec(),
                Precondition::Matches(moved)
            )
            .await
            .unwrap(),
        PutOutcome::Conflict,
        "a writer whose key was erased is superseded, it does not recreate the key"
    );
    assert!(store.list("").await.is_err());
    assert!(store.delete("HEAD").await.is_err());
}
