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
        if policy == b"expired" || !key.starts_with(std::str::from_utf8(policy).unwrap()) {
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
        None,
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
        None,
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
