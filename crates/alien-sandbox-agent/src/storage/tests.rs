use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use alien_sandbox_storage::{
    read_manifest, restore, takeover, CommitOutcome, Committer, MemoryStore, TreeEntryKind,
};

use super::push::GrantStore;
use super::tree::{self, DirSink};
use super::Replicator;

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, bytes).expect("write");
}

/// Waits past the coarse clock tick inode times are stamped with (at most 10 ms at HZ=100), so
/// files written before are not ones a walk must reread for having changed in its own tick.
fn let_the_clock_tick() {
    std::thread::sleep(std::time::Duration::from_millis(50));
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
        Committer::new("writer-a", claim.head_version, claim.manifest, None),
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
    let_the_clock_tick();
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
    let_the_clock_tick();
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

/// A fake S3 bucket with the semantics the push transport relies on: POST form uploads that
/// require the signed fields and stay under the policy's prefix, a HEAD PUT honoring
/// `If-Match`/`If-None-Match`, and ranged GETs.
mod fake_s3 {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use axum::body::Bytes;
    use axum::extract::{Path, State};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::Router;

    #[derive(Default)]
    pub struct Bucket {
        pub objects: Mutex<BTreeMap<String, (Vec<u8>, u64)>>,
        version: Mutex<u64>,
    }

    impl Bucket {
        fn next(&self) -> u64 {
            let mut v = self.version.lock().unwrap();
            *v += 1;
            *v
        }
    }

    fn part<'a>(body: &'a [u8], boundary: &str, name: &str) -> Option<&'a [u8]> {
        let marker = format!("name=\"{name}\"");
        let start = body
            .windows(marker.len())
            .position(|w| w == marker.as_bytes())?;
        let data = start + body[start..].windows(4).position(|w| w == b"\r\n\r\n")? + 4;
        let end_marker = format!("\r\n--{boundary}");
        let end = data
            + body[data..]
                .windows(end_marker.len())
                .position(|w| w == end_marker.as_bytes())?;
        Some(&body[data..end])
    }

    async fn upload(
        State(bucket): State<Arc<Bucket>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let Some(boundary) = content_type.split("boundary=").nth(1) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let (Some(key), Some(policy), Some(file)) = (
            part(&body, boundary, "key"),
            part(&body, boundary, "policy"),
            part(&body, boundary, "file"),
        ) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let key = String::from_utf8_lossy(key).to_string();
        // The fake's policy is the allowed prefix itself; "expired" stands for a lapsed grant.
        // As S3 answers a lapsed POST policy (measured in the test account).
        if policy == b"expired" {
            return (
                StatusCode::BAD_REQUEST,
                "<Error><Code>AccessDenied</Code><Message>Invalid according to Policy: Policy expired.</Message></Error>",
            )
                .into_response();
        }
        if !key.starts_with(std::str::from_utf8(policy).unwrap()) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let version = bucket.next();
        bucket
            .objects
            .lock()
            .unwrap()
            .insert(key, (file.to_vec(), version));
        (StatusCode::NO_CONTENT, [("etag", format!("\"{version}\""))]).into_response()
    }

    async fn put(
        State(bucket): State<Arc<Bucket>>,
        Path(key): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let version = bucket.next();
        let mut objects = bucket.objects.lock().unwrap();
        let current = objects.get(&key).map(|(_, v)| format!("\"{v}\""));
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let allowed = match (header("if-match"), header("if-none-match"), &current) {
            (Some(expected), _, Some(current)) => &expected == current,
            (Some(_), _, None) => false,
            (None, Some(_), current) => current.is_none(),
            (None, None, _) => false, // the grant's HEAD PUT is always conditional
        };
        if !allowed {
            return StatusCode::PRECONDITION_FAILED.into_response();
        }
        objects.insert(key, (body.to_vec(), version));
        (StatusCode::OK, [("etag", format!("\"{version}\""))]).into_response()
    }

    async fn read(
        State(bucket): State<Arc<Bucket>>,
        Path(key): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        let objects = bucket.objects.lock().unwrap();
        let Some((bytes, version)) = objects.get(&key) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let range = headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
            .and_then(|v| v.split_once('-'))
            .map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
        let body = match range {
            Some((a, b)) => bytes[a..=b.min(bytes.len() - 1)].to_vec(),
            None => bytes.clone(),
        };
        (StatusCode::OK, [("etag", format!("\"{version}\""))], body).into_response()
    }

    pub async fn serve() -> (String, Arc<Bucket>) {
        let bucket = Arc::new(Bucket::default());
        let app = Router::new()
            .route("/", post(upload))
            .route("/{*key}", get(read).put(put))
            .with_state(Arc::clone(&bucket));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, bucket)
    }
}

fn s3_grant(url: &str, key: &str, reads: &[&str]) -> alien_sandbox_storage::StorageGrant {
    alien_sandbox_storage::StorageGrant::S3 {
        post: alien_sandbox_storage::S3PostPolicy {
            url: format!("{url}/"),
            fields: [("policy".to_string(), format!("{key}/"))]
                .into_iter()
                .collect(),
            key_prefix: format!("{key}/"),
        },
        head_put_url: format!("{url}/{key}/HEAD"),
        reads: reads
            .iter()
            .map(|object| (object.to_string(), format!("{url}/{key}/{object}")))
            .collect(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
    }
}

/// A store the binding sees: the whole bucket under one key's prefix, with full rights.
struct KeyView(std::sync::Arc<fake_s3::Bucket>, String);

#[async_trait::async_trait]
impl alien_sandbox_storage::ObjectStore for KeyView {
    async fn get(
        &self,
        object: &str,
    ) -> alien_sandbox_storage::Result<Option<alien_sandbox_storage::StoredObject>> {
        Ok(self
            .0
            .objects
            .lock()
            .unwrap()
            .get(&format!("{}/{object}", self.1))
            .map(|(b, v)| alien_sandbox_storage::StoredObject {
                bytes: b.clone(),
                version: alien_sandbox_storage::ObjectVersion::etag(format!("\"{v}\"")),
            }))
    }
    async fn get_range(
        &self,
        object: &str,
        offset: u64,
        len: u64,
    ) -> alien_sandbox_storage::Result<Vec<u8>> {
        let objects = self.0.objects.lock().unwrap();
        let bytes = &objects[&format!("{}/{object}", self.1)].0;
        Ok(bytes[offset as usize..(offset + len) as usize].to_vec())
    }
    async fn put_if(
        &self,
        object: &str,
        bytes: Vec<u8>,
        precondition: alien_sandbox_storage::Precondition,
    ) -> alien_sandbox_storage::Result<alien_sandbox_storage::PutOutcome> {
        let name = format!("{}/{object}", self.1);
        let mut objects = self.0.objects.lock().unwrap();
        let current = objects.get(&name).map(|(_, v)| format!("\"{v}\""));
        let ok = match (&precondition, &current) {
            (alien_sandbox_storage::Precondition::Absent, None) => true,
            (alien_sandbox_storage::Precondition::Matches(v), Some(c)) => &v.etag == c,
            _ => false,
        };
        if !ok {
            return Ok(alien_sandbox_storage::PutOutcome::Conflict);
        }
        let version = objects.values().map(|(_, v)| *v).max().unwrap_or(0) + 1000;
        objects.insert(name, (bytes, version));
        Ok(alien_sandbox_storage::PutOutcome::Written {
            version: alien_sandbox_storage::ObjectVersion::etag(format!("\"{version}\"")),
        })
    }
    async fn list(&self, _: &str) -> alien_sandbox_storage::Result<Vec<String>> {
        unimplemented!()
    }
    async fn delete(&self, _: &str) -> alien_sandbox_storage::Result<()> {
        unimplemented!()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_commits_and_restores_through_an_s3_grant() {
    let (url, bucket) = fake_s3::serve().await;
    let binding = KeyView(std::sync::Arc::clone(&bucket), "engine-42".to_string());

    // The binding claims the key, then grants the agent its rights.
    let claim = takeover(&binding, "sandbox-1").await.expect("claim");
    let agent = GrantStore::new(s3_grant(&url, "engine-42", &[]));
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    write(source.path(), "notes.md", b"remember this");
    write(source.path(), "big.bin", &vec![3u8; 300 * 1024]);
    let mut writer = Replicator::new(
        "engine-42",
        tree::open_root(source.path()).expect("open"),
        vec![],
        staging.path().to_path_buf(),
        Committer::new("sandbox-1", claim.head_version, claim.manifest, None),
    );
    let CommitOutcome::Committed { commit, .. } = writer.commit(&agent).await.expect("commit")
    else {
        panic!("expected a commit");
    };
    assert!(bucket
        .objects
        .lock()
        .unwrap()
        .keys()
        .all(|k| k.starts_with("engine-42/")));

    // A new sandbox: the binding claims, then grants reads of what the restore needs.
    let claim = takeover(&binding, "sandbox-2").await.expect("claim");
    let manifest = claim.manifest.expect("committed");
    let mut reads = vec![commit.manifest()];
    reads.extend(manifest.packs().into_iter().map(|p| p.pack()));
    let reads: Vec<&str> = reads.iter().map(String::as_str).collect();
    let restorer = GrantStore::new(s3_grant(&url, "engine-42", &reads));
    let target = tempfile::tempdir().expect("tempdir");
    restore(
        &restorer,
        &manifest,
        &mut DirSink::new(tree::open_root(target.path()).expect("open"), false),
    )
    .await
    .expect("restore through presigned GETs");
    assert_eq!(snapshot(target.path()), snapshot(source.path()));

    // The first sandbox, superseded by the claim, is refused at HEAD.
    write(source.path(), "notes.md", b"late write");
    writer.commit(&agent).await.expect_err("superseded");
    assert!(writer.is_superseded());
}

/// The agent's attach as the binding drives it: the key restored into a fresh directory before
/// the image gate opens, then `sync` publishes a write and the status follows.
#[tokio::test(flavor = "multi_thread")]
async fn attach_restores_then_sync_publishes() {
    use super::attach::{AttachRequest, PersistentStorage, ReplicationState, StorageConfig};
    let (url, bucket) = fake_s3::serve().await;
    let binding = KeyView(std::sync::Arc::clone(&bucket), "engine-42".to_string());

    // An earlier sandbox's commit.
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    write(source.path(), "repo/README", b"hello");
    let claim = takeover(&binding, "sandbox-1").await.expect("claim");
    let mut first = Replicator::new(
        "engine-42",
        tree::open_root(source.path()).expect("open"),
        vec![],
        staging.path().to_path_buf(),
        Committer::new("sandbox-1", claim.head_version, None, None),
    );
    first
        .commit(&GrantStore::new(s3_grant(&url, "engine-42", &[])))
        .await
        .expect("commit");

    // The new sandbox: claim, then attach with reads of what the restore needs.
    let claim = takeover(&binding, "sandbox-2").await.expect("claim");
    let head = alien_sandbox_storage::read_head(&binding)
        .await
        .unwrap()
        .unwrap()
        .0;
    let commit = head.commit.clone().expect("a commit");
    let manifest = claim.manifest.clone().expect("manifest");
    let mut reads = vec![commit.manifest()];
    reads.extend(manifest.packs().into_iter().map(|p| p.pack()));
    let reads: Vec<&str> = reads.iter().map(String::as_str).collect();

    let sandbox = tempfile::tempdir().expect("tempdir");
    let workspace = sandbox.path().join("workspace");
    write(&workspace, "stale-from-image", b"x");
    let storage = std::sync::Arc::new(PersistentStorage::with_staging(
        StorageConfig {
            path: workspace.display().to_string(),
            exclude: vec![],
            max_size_bytes: None,
        },
        staging.path().join("agent"),
    ));
    let image = crate::image_command::ImageProcess::default().awaiting_restore();
    // SAFETY: plain getters.
    let me = unsafe {
        crate::exec::ExecIdentity {
            uid: libc::getuid(),
            gid: libc::getgid(),
        }
    };
    let request: AttachRequest = serde_json::from_value(serde_json::json!({
        "key": "engine-42",
        "writer": "sandbox-2",
        "headVersion": claim.head_version,
        "commit": commit,
        "grant": s3_grant(&url, "engine-42", &reads),
    }))
    .expect("the wire shape parses");
    let status = storage.attach(request, &image, me).await.expect("attach");
    assert_eq!(status.state, ReplicationState::Replicating);
    assert!(!image.restore_pending().unwrap());
    assert_eq!(
        std::fs::read(workspace.join("repo/README")).unwrap(),
        b"hello"
    );
    assert!(!workspace.join("stale-from-image").exists());

    write(&workspace, "repo/new", b"after restore");
    let synced = storage.sync().await.expect("sync");
    assert_eq!(synced.head, Some(2));
    assert_eq!(
        alien_sandbox_storage::read_file(&binding, "engine-42", "repo/new")
            .await
            .unwrap(),
        b"after restore"
    );
}

/// What setup writes into the image is exactly what the agent reads, and a sandbox without
/// persistent storage gets no variable at all.
#[test]
fn the_setup_variable_parses_as_the_agents_config() {
    use alien_core::sandbox_egress::persistent_storage_environment;
    let mut sandbox = alien_core::Sandbox::new("agent".to_string())
        .code(alien_core::SandboxCode::Image {
            image: "example".to_string(),
            bundled_from: None,
        })
        .limits(alien_core::SandboxLimits {
            cpu: "1".to_string(),
            memory: "2Gi".to_string(),
            disk: "10Gi".to_string(),
            max_processes: None,
        })
        .egress(alien_core::SandboxEgress::None)
        .lifecycle(alien_core::SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .preview_ports(vec![])
        .build();
    assert_eq!(persistent_storage_environment(&sandbox), None);
    sandbox.persistent_storage = Some(alien_core::SandboxPersistentStorage {
        path: "/workspace".to_string(),
        exclude: vec!["tmp/".to_string()],
        max_size: Some("8Gi".to_string()),
        encryption_key: None,
    });
    let variable = persistent_storage_environment(&sandbox).expect("declared");
    let config: super::attach::StorageConfig = serde_json::from_str(&variable).expect("parses");
    assert_eq!(config.path, "/workspace");
    assert_eq!(config.exclude, vec!["tmp/".to_string()]);
    assert_eq!(config.max_size_bytes, Some(8 << 30));
}

/// An expired grant is reported as `denied`, not as a silent stall, and a renewal resumes.
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_grant_reports_denied_until_renewed() {
    use super::attach::{AttachRequest, PersistentStorage, ReplicationState, StorageConfig};
    let (url, bucket) = fake_s3::serve().await;
    let binding = KeyView(std::sync::Arc::clone(&bucket), "engine-42".to_string());
    let claim = takeover(&binding, "sandbox-1").await.expect("claim");
    let sandbox = tempfile::tempdir().expect("tempdir");
    let workspace = sandbox.path().join("workspace");
    let storage = std::sync::Arc::new(PersistentStorage::with_staging(
        StorageConfig {
            path: workspace.display().to_string(),
            exclude: vec![],
            max_size_bytes: None,
        },
        sandbox.path().join("staging"),
    ));
    let mut expired = s3_grant(&url, "engine-42", &[]);
    let alien_sandbox_storage::StorageGrant::S3 { post, .. } = &mut expired;
    post.fields
        .insert("policy".to_string(), "expired".to_string());
    let image = crate::image_command::ImageProcess::default().awaiting_restore();
    // SAFETY: plain getters.
    let me = unsafe {
        crate::exec::ExecIdentity {
            uid: libc::getuid(),
            gid: libc::getgid(),
        }
    };
    let request: AttachRequest = serde_json::from_value(serde_json::json!({
        "key": "engine-42",
        "writer": "sandbox-1",
        "headVersion": claim.head_version,
        "grant": expired,
    }))
    .expect("parses");
    storage.attach(request, &image, me).await.expect("attach");
    write(&workspace, "notes.md", b"x");
    storage
        .sync()
        .await
        .expect_err("the store refuses the grant");
    assert_eq!(storage.status().state, ReplicationState::Denied);
    let first_issued = storage.status().grant_issued_at.expect("issued at attach");

    storage
        .renew(
            serde_json::from_value(
                serde_json::json!({ "grant": s3_grant(&url, "engine-42", &[]) }),
            )
            .unwrap(),
        )
        .await
        .expect("renew");
    let synced = storage.sync().await.expect("renewed grant works");
    assert_eq!(synced.state, ReplicationState::Replicating);
    // The binding reads the renewed grant's life from these two.
    assert!(synced.grant_issued_at.expect("issued at renewal") >= first_issued);
    assert!(synced.grant_expires_at.expect("expiry") > synced.grant_issued_at.unwrap());
}

/// A grant past its expiry is refused before anything is sent.
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_grant_sends_nothing() {
    use alien_sandbox_storage::{ObjectStore, Precondition};
    let (url, bucket) = fake_s3::serve().await;
    let mut grant = s3_grant(&url, "engine-42", &[]);
    let alien_sandbox_storage::StorageGrant::S3 { expires_at, .. } = &mut grant;
    *expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    let store = GrantStore::new(grant);
    store
        .put_if("packs/1-x", b"x".to_vec(), Precondition::Absent)
        .await
        .expect_err("expired");
    assert!(store.denied());
    assert!(bucket.objects.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_same_size_rewrite_right_after_a_commit_is_committed() {
    // Inode times tick with the coarse clock (a few ms), so a rewrite of the same length right
    // after a commit read the file often keeps size, ctime and mtime. Many rounds make one
    // such rewrite near certain.
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    let root = source.path();
    let store = MemoryStore::new();
    write(root, "f", b"round-000");
    let mut replicator = replicator(&store, root, staging.path(), &[]).await;
    for round in 1..=200 {
        replicator.commit(&store).await.expect("commit");
        let next = format!("round-{round:03}");
        write(root, "f", next.as_bytes());
        replicator.commit(&store).await.expect("commit");
        let target = tempfile::tempdir().expect("tempdir");
        restore(
            &store,
            replicator.head().expect("head"),
            &mut DirSink::new(tree::open_root(target.path()).expect("open"), false),
        )
        .await
        .expect("restore");
        assert_eq!(
            std::fs::read_to_string(target.path().join("f")).expect("read"),
            next,
            "round {round}"
        );
    }
}

#[test]
fn files_that_name_git_objects_are_refs_heads_and_the_index() {
    for path in [
        ".git/HEAD",
        ".git/ORIG_HEAD",
        ".git/FETCH_HEAD",
        ".git/packed-refs",
        ".git/index",
        ".git/refs/heads/main",
        ".git/refs/tags/v1/x",
        "repos/a/.git/refs/heads/main",
        ".git/worktrees/wt/HEAD",
        ".git/worktrees/wt/index",
    ] {
        assert!(tree::names_git_objects(path), "{path}");
    }
    for path in [
        "HEAD",
        "refs/heads/main",
        ".git",
        ".git/refs",
        ".git/config",
        ".git/objects/ab/cdef",
        ".git/objects/pack/pack-1.pack",
        ".git/logs/refs/heads/main",
        ".git/worktrees/wt/logs/HEAD",
        "src/index",
    ] {
        assert!(!tree::names_git_objects(path), "{path}");
    }
}

/// Commits what `stage_with` staged, restores it elsewhere, and returns the restored directory.
async fn commit_staged(
    store: &MemoryStore,
    replicator: &mut Replicator,
    between: &mut dyn FnMut(super::Phase),
) -> tempfile::TempDir {
    let (walk, mut staging) = replicator.stage_with(between).expect("stage");
    replicator
        .committer
        .commit(store, "k", walk, &mut staging)
        .await
        .expect("commit");
    let target = tempfile::tempdir().expect("tempdir");
    restore(
        store,
        replicator.head().expect("head"),
        &mut DirSink::new(tree::open_root(target.path()).expect("open"), false),
    )
    .await
    .expect("restore");
    target
}

/// A loose object `.git/objects/<2>/<rest>` for a name.
fn object(root: &Path, name: &str) {
    write(
        root,
        &format!(".git/objects/{}/{}", &name[..2], &name[2..]),
        name.as_bytes(),
    );
}

fn object_exists(root: &Path, name: &str) -> bool {
    root.join(format!(".git/objects/{}/{}", &name[..2], &name[2..]))
        .exists()
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_freezer_a_ref_never_names_an_object_the_commit_lacks() {
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    let root = source.path().to_path_buf();
    object(&root, "aa01");
    write(&root, ".git/refs/heads/main", b"aa01");
    write(&root, ".git/HEAD", b"ref: refs/heads/main");
    let store = MemoryStore::new();
    let mut replicator = replicator(&store, &root, staging.path(), &[]).await;
    replicator.commit(&store).await.expect("first commit");
    object(&root, "ab0005");
    write(&root, ".git/refs/heads/main", b"ab0005");

    // Another commit lands after the objects were listed and before the files are copied: git
    // writes the object, then moves the ref.
    let restored = commit_staged(&store, &mut replicator, &mut |phase| {
        if phase == super::Phase::SecondWalk {
            object(&root, "bb000002");
            write(&root, ".git/refs/heads/main", b"bb000002");
        }
    })
    .await;
    let main = std::fs::read_to_string(restored.path().join(".git/refs/heads/main")).expect("ref");
    assert!(
        object_exists(restored.path(), &main),
        "main names {main}, which the commit lacks"
    );
    assert_eq!(
        main, "ab0005",
        "the ref is the one copied before the objects were listed"
    );

    // The next commit takes the new ref with its object.
    let restored = commit_staged(&store, &mut replicator, &mut |_| {}).await;
    let main = std::fs::read_to_string(restored.path().join(".git/refs/heads/main")).expect("ref");
    assert_eq!(main, "bb000002");
    assert!(object_exists(restored.path(), "bb000002"));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_freezer_refs_come_from_the_first_walk_with_their_directories() {
    let source = tempfile::tempdir().expect("tempdir");
    let staging = tempfile::tempdir().expect("tempdir");
    let root = source.path().to_path_buf();
    object(&root, "aa01");
    write(&root, ".git/refs/heads/topic/one", b"aa01");
    let store = MemoryStore::new();
    let mut replicator = replicator(&store, &root, staging.path(), &[]).await;

    // Between the walks a branch is deleted with its directory and another is created.
    let restored = commit_staged(&store, &mut replicator, &mut |phase| {
        if phase == super::Phase::RefsCopied {
            std::fs::remove_dir_all(root.join(".git/refs/heads/topic")).expect("rm");
            object(&root, "cc03");
            write(&root, ".git/refs/heads/new", b"cc03");
        }
    })
    .await;
    assert_eq!(
        std::fs::read(restored.path().join(".git/refs/heads/topic/one")).expect("kept ref"),
        b"aa01"
    );
    assert!(object_exists(restored.path(), "aa01"));
    assert!(
        !restored.path().join(".git/refs/heads/new").exists(),
        "a ref seen only by the second walk waits for the next commit"
    );
}

/// SQLite without a freezer: a database is copied with its journal, and kept only when it did
/// not change across the copies.
mod sqlite_without_a_freezer {
    use super::*;
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    const ROWS: i64 = 200;
    const TOTAL: i64 = ROWS * 1000;

    /// A database whose rows always add up to `TOTAL`; each transaction moves one unit.
    fn create(path: &Path, wal: bool) -> Connection {
        let db = Connection::open(path).expect("open");
        db.pragma_update(None, "journal_mode", if wal { "WAL" } else { "DELETE" })
            .expect("journal mode");
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS t(id INTEGER PRIMARY KEY, v INTEGER, pad BLOB);",
        )
        .expect("table");
        let tx = db.unchecked_transaction().expect("tx");
        for id in 0..ROWS {
            tx.execute(
                "INSERT OR REPLACE INTO t VALUES (?1, 1000, zeroblob(2000))",
                [id],
            )
            .expect("row");
        }
        tx.commit().expect("commit");
        db
    }

    fn step(db: &Connection, n: i64) {
        let tx = db.unchecked_transaction().expect("tx");
        tx.execute("UPDATE t SET v = v - 1 WHERE id = ?1", [n % ROWS])
            .expect("debit");
        tx.execute("UPDATE t SET v = v + 1 WHERE id = ?1", [(n * 7 + 3) % ROWS])
            .expect("credit");
        tx.commit().expect("commit");
    }

    /// Opens a restored database (recovering its WAL or journal) and returns its total, or why
    /// it is not a good database.
    fn total(path: &Path) -> Result<i64, String> {
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        let check: String = db
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if check != "ok" {
            return Err(check);
        }
        db.query_row("SELECT sum(v) FROM t", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    }

    async fn restored(store: &MemoryStore, replicator: &Replicator) -> tempfile::TempDir {
        let target = tempfile::tempdir().expect("tempdir");
        restore(
            store,
            replicator.head().expect("head"),
            &mut DirSink::new(tree::open_root(target.path()).expect("open"), false),
        )
        .await
        .expect("restore");
        target
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_database_written_during_every_copy_keeps_heads_copy_until_it_is_quiet() {
        let source = tempfile::tempdir().expect("tempdir");
        let staging = tempfile::tempdir().expect("tempdir");
        let root = source.path().to_path_buf();
        let db = create(&root.join("app.db"), false);
        write(&root, "notes.txt", b"one");
        let_the_clock_tick();
        let store = MemoryStore::new();
        let mut replicator = replicator(&store, &root, staging.path(), &[]).await;
        replicator.commit(&store).await.expect("first commit");
        assert!(replicator.deferred_for().is_none());

        // The application commits a transaction while each copy of the database is taken.
        for n in 0..3 {
            step(&db, n);
        }
        write(&root, "notes.txt", b"two");
        let_the_clock_tick();
        let mut n = 100;
        let target = commit_staged(&store, &mut replicator, &mut |phase| {
            if phase == super::super::Phase::DatabaseCopied {
                step(&db, n);
                n += 1;
            }
        })
        .await;
        assert_eq!(n, 103, "every attempt saw the database change");
        assert_eq!(
            std::fs::read(target.path().join("notes.txt")).expect("notes"),
            b"two",
            "the rest of the tree is committed"
        );
        let head_db = target.path().join("app.db");
        assert_eq!(total(&head_db), Ok(TOTAL), "HEAD's copy is kept, whole");
        let kept: i64 = Connection::open(&head_db)
            .expect("open")
            .query_row("SELECT v FROM t WHERE id = 0", [], |r| r.get(0))
            .expect("row");
        assert_eq!(kept, 1000, "the database is the first commit's");
        assert!(replicator.deferred_for().is_some(), "the wait is reported");

        // A quiet moment: the database is taken with every transaction.
        let_the_clock_tick();
        replicator.commit(&store).await.expect("commit");
        assert!(replicator.deferred_for().is_none());
        let target = restored(&store, &replicator).await;
        let db_now = Connection::open(target.path().join("app.db")).expect("open");
        let v: i64 = db_now
            .query_row("SELECT v FROM t WHERE id = 0", [], |r| r.get(0))
            .expect("row");
        assert_ne!(v, 1000, "the later transactions are in");
        assert_eq!(total(&target.path().join("app.db")), Ok(TOTAL));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_new_database_busy_during_every_copy_waits_for_the_next_commit() {
        let source = tempfile::tempdir().expect("tempdir");
        let staging = tempfile::tempdir().expect("tempdir");
        let root = source.path().to_path_buf();
        let store = MemoryStore::new();
        let mut replicator = replicator(&store, &root, staging.path(), &[]).await;
        let db = create(&root.join("new.db"), true);
        let_the_clock_tick();
        let mut n = 0;
        // In WAL mode the database changes at a checkpoint; force one during every copy.
        let target = commit_staged(&store, &mut replicator, &mut |phase| {
            if phase == super::super::Phase::DatabaseCopied {
                step(&db, n);
                db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                    .expect("checkpoint");
                n += 1;
            }
        })
        .await;
        assert!(!target.path().join("new.db").exists());
        assert!(!target.path().join("new.db-wal").exists());
        let_the_clock_tick();
        let target = commit_staged(&store, &mut replicator, &mut |_| {}).await;
        assert_eq!(total(&target.path().join("new.db")), Ok(TOTAL));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_symlink_planted_at_a_database_or_its_sidecars_redirects_nothing() {
        let outside = tempfile::tempdir().expect("tempdir");
        let secret = outside.path().join("secret.db");
        create(&secret, false)
            .execute_batch("CREATE TABLE s(x TEXT); INSERT INTO s VALUES ('outside-marker');")
            .expect("marker");
        let marker = b"outside-marker";
        let holds_marker =
            |bytes: &[u8]| bytes.windows(marker.len()).any(|w| w == marker.as_slice());
        assert!(holds_marker(&std::fs::read(&secret).expect("secret")));
        let source = tempfile::tempdir().expect("tempdir");
        let staging = tempfile::tempdir().expect("tempdir");
        let root = source.path().to_path_buf();
        create(&root.join("app.db"), false);
        for name in ["app.db-wal", "app.db-journal", "app.db-shm", "linked.db"] {
            std::os::unix::fs::symlink(&secret, root.join(name)).expect("symlink");
        }
        let_the_clock_tick();
        let store = MemoryStore::new();
        let mut replicator = replicator(&store, &root, staging.path(), &[]).await;
        replicator.commit(&store).await.expect("commit");
        let target = restored(&store, &replicator).await;
        for entry in std::fs::read_dir(target.path()).expect("list") {
            let path = entry.expect("entry").path();
            let meta = std::fs::symlink_metadata(&path).expect("stat");
            if meta.file_type().is_file() {
                assert!(
                    !holds_marker(&std::fs::read(&path).expect("read")),
                    "{} holds the file outside",
                    path.display()
                );
            }
        }
        assert_eq!(total(&target.path().join("app.db")), Ok(TOTAL));
        assert!(
            std::fs::symlink_metadata(target.path().join("linked.db"))
                .expect("stat")
                .file_type()
                .is_symlink(),
            "a symlink stays a symlink"
        );
    }

    /// A writer that never stops, in each journal mode: every commit's database opens, passes
    /// the integrity check and keeps the total. Many rounds make a copy that crosses a write
    /// near certain, so a missing or wrong bracket fails here.
    async fn many_rounds(wal: bool, pace: std::time::Duration) -> (usize, usize) {
        let source = tempfile::tempdir().expect("tempdir");
        let staging = tempfile::tempdir().expect("tempdir");
        let root = source.path().to_path_buf();
        create(&root.join("app.db"), wal);
        let_the_clock_tick();
        let store = MemoryStore::new();
        let mut replicator = replicator(&store, &root, staging.path(), &[]).await;
        replicator.commit(&store).await.expect("first commit");

        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let stop = stop.clone();
            let path = root.join("app.db");
            std::thread::spawn(move || {
                let db = Connection::open(path).expect("open");
                db.busy_timeout(std::time::Duration::from_secs(5))
                    .expect("busy timeout");
                // No fsync per transaction: as many writes as possible cross each copy.
                db.pragma_update(None, "synchronous", "OFF")
                    .expect("synchronous");
                let mut n = 0;
                while !stop.load(Ordering::Relaxed) {
                    step(&db, n);
                    n += 1;
                    if !pace.is_zero() {
                        std::thread::sleep(pace);
                    }
                }
                n
            })
        };
        let (mut taken, rounds) = (0usize, 150usize);
        let mut last_head = replicator.head().map(|m| m.seq);
        for round in 0..rounds {
            replicator.commit(&store).await.expect("commit");
            if replicator.deferred_for().is_none() {
                taken += 1;
            }
            let head = replicator.head().map(|m| m.seq);
            if head != last_head {
                let target = restored(&store, &replicator).await;
                assert_eq!(
                    total(&target.path().join("app.db")),
                    Ok(TOTAL),
                    "round {round} (wal={wal}) committed a bad database"
                );
                last_head = head;
            }
        }
        stop.store(true, Ordering::Relaxed);
        let writes = writer.join().expect("writer");
        assert!(
            writes as usize > rounds,
            "the writer kept writing ({writes})"
        );
        eprintln!(
            "wal={wal} pace={pace:?}: {writes} writes, {taken}/{rounds} rounds took the database"
        );
        (taken, rounds)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_database_under_a_busy_writer_is_never_committed_torn() {
        let (taken, rounds) = many_rounds(true, std::time::Duration::ZERO).await;
        assert!(
            taken > 0,
            "WAL mode: no round took the database ({taken}/{rounds})"
        );
        let (taken, rounds) = many_rounds(false, std::time::Duration::from_millis(2)).await;
        assert!(
            taken > 0,
            "rollback mode: no round took the database ({taken}/{rounds})"
        );
    }
}
