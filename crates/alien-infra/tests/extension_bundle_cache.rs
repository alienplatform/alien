//! A loader's bundle cache: every object it keeps is the one its digest pins, and a download it
//! refuses or loses leaves nothing under the object's name.

use alien_core::BinaryTarget;
use alien_infra::extension_bundle::testing::{core_zip, hooks_module, TestBundle};
use alien_infra::extension_bundle::{host_target, BundleCache, BundleSource, ExtensionBundle};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// What the test server answers for one path: the body, and whether to close the connection
/// halfway through it.
#[derive(Clone)]
struct Reply {
    body: Vec<u8>,
    cut_short: bool,
}

/// A plain HTTP server that counts the requests for each path.
struct Server {
    base: String,
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl Server {
    async fn start(replies: HashMap<&'static str, Reply>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
        let counted = hits.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let replies = replies.clone();
                let counted = counted.clone();
                tokio::spawn(async move {
                    let mut request = vec![0u8; 4096];
                    let read = socket.read(&mut request).await.unwrap();
                    let line = String::from_utf8_lossy(&request[..read]).to_string();
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    *counted.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let Some(reply) = replies.get(path.as_str()) else {
                        let _ = socket
                            .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                            .await;
                        return;
                    };
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        reply.body.len()
                    );
                    socket.write_all(head.as_bytes()).await.unwrap();
                    let sent = if reply.cut_short {
                        &reply.body[..reply.body.len() / 2]
                    } else {
                        &reply.body[..]
                    };
                    socket.write_all(sent).await.unwrap();
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self { base, hits }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}

/// This machine's controller target; these tests run where bundles have one.
fn host() -> BinaryTarget {
    host_target().expect("tests run on a machine with a controller target")
}

/// A target other than this machine's.
fn other_target() -> BinaryTarget {
    if host() == BinaryTarget::WindowsX64 {
        BinaryTarget::LinuxX64
    } else {
        BinaryTarget::WindowsX64
    }
}

fn whole(body: &[u8]) -> Reply {
    Reply {
        body: body.to_vec(),
        cut_short: false,
    }
}

fn source<'a>(client: &'a reqwest::Client, url: &'a str) -> BundleSource<'a> {
    BundleSource {
        client,
        url,
        bearer: None,
    }
}

fn files(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test]
async fn downloads_a_new_digest_once_into_its_own_folder() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host(), b"controller v1")]);
    let server = Server::start(HashMap::from([
        ("/core", whole(&bundle.zip)),
        ("/controller", whole(b"controller v1")),
    ]))
    .await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let client = reqwest::Client::new();
    let (core_url, controller_url) = (server.url("/core"), server.url("/controller"));

    for _ in 0..2 {
        let opened = cache
            .core(&bundle.manifest, source(&client, &core_url))
            .await
            .unwrap();
        let path = cache
            .controller(
                &bundle.manifest,
                &opened,
                Some(source(&client, &controller_url)),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"controller v1");
    }

    let hex = bundle.manifest.digest.strip_prefix("sha256:").unwrap();
    let dir = root.path().join(format!("acme@sha256-{hex}"));
    assert_eq!(
        files(&dir),
        [format!("controller-{}", host()), "core.zip".to_string()]
    );
    assert_eq!(
        server.hits("/core"),
        1,
        "a digest it has is not downloaded again"
    );
    assert_eq!(server.hits("/controller"), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join(format!("controller-{}", host())))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the controller is executable");
    }
}

#[tokio::test]
async fn refuses_a_core_zip_whose_sha256_is_not_the_pinned_digest() {
    let pinned = TestBundle::new("acme", "1.0.0", &[]);
    let other = TestBundle::new("acme", "1.0.1", &[]);
    let server = Server::start(HashMap::from([("/core", whole(&other.zip))])).await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let client = reqwest::Client::new();
    let url = server.url("/core");

    let error = cache
        .core(&pinned.manifest, source(&client, &url))
        .await
        .unwrap_err();

    assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
    assert!(
        error.message.contains("Extension 'acme'"),
        "{}",
        error.message
    );
    assert!(error.message.contains("sha256"), "{}", error.message);
    let dir = cache.dir(&pinned.manifest).unwrap();
    assert_eq!(files(&dir), Vec::<String>::new(), "nothing is kept");
}

#[tokio::test]
async fn refuses_a_controller_whose_sha256_is_not_the_one_the_manifest_pins() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host(), b"controller v1")]);
    let server = Server::start(HashMap::from([
        ("/core", whole(&bundle.zip)),
        ("/controller", whole(b"tampered")),
    ]))
    .await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let client = reqwest::Client::new();
    let (core_url, controller_url) = (server.url("/core"), server.url("/controller"));
    let opened = cache
        .core(&bundle.manifest, source(&client, &core_url))
        .await
        .unwrap();

    let error = cache
        .controller(
            &bundle.manifest,
            &opened,
            Some(source(&client, &controller_url)),
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
    assert!(
        error.message.contains("Extension 'acme'")
            && error
                .message
                .contains(&format!("controller for {}", host())),
        "{}",
        error.message
    );
    assert_eq!(files(&cache.dir(&bundle.manifest).unwrap()), ["core.zip"]);
}

#[tokio::test]
async fn a_download_cut_short_leaves_only_a_part_file() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let server = Server::start(HashMap::from([(
        "/core",
        Reply {
            body: bundle.zip.clone(),
            cut_short: true,
        },
    )]))
    .await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let client = reqwest::Client::new();
    let url = server.url("/core");

    let error = cache
        .core(&bundle.manifest, source(&client, &url))
        .await
        .unwrap_err();

    assert_eq!(error.code, "EXTENSION_BUNDLE_UNAVAILABLE");
    assert!(
        error.message.contains("Extension 'acme'"),
        "{}",
        error.message
    );
    assert_eq!(
        files(&cache.dir(&bundle.manifest).unwrap()),
        ["core.zip.part"]
    );
    assert!(cache.cached(&bundle.manifest).await.unwrap().is_none());
}

#[tokio::test]
async fn a_bundle_with_no_controller_for_the_machine_fails_naming_the_extension_and_target() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(other_target(), b"controller")]);
    let opened = ExtensionBundle::open(&bundle.manifest, &bundle.zip).unwrap();
    let root = tempfile::tempdir().unwrap();

    let error = BundleCache::new(root.path())
        .controller(&bundle.manifest, &opened, None)
        .await
        .unwrap_err();

    assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
    assert!(
        error.message.contains("Extension 'acme'")
            && error
                .message
                .contains(&format!("no controller for {}", host())),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn a_cached_core_zip_changed_on_disk_is_deleted_and_downloaded_again() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let server = Server::start(HashMap::from([("/core", whole(&bundle.zip))])).await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let dir = cache.dir(&bundle.manifest).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let mut tampered = bundle.zip.clone();
    *tampered.last_mut().unwrap() ^= 1;
    std::fs::write(dir.join("core.zip"), tampered).unwrap();

    assert!(cache.cached(&bundle.manifest).await.unwrap().is_none());
    assert!(
        !dir.join("core.zip").exists(),
        "the file that failed its check is deleted"
    );
    let client = reqwest::Client::new();
    let url = server.url("/core");
    cache
        .core(&bundle.manifest, source(&client, &url))
        .await
        .unwrap();

    assert_eq!(std::fs::read(dir.join("core.zip")).unwrap(), bundle.zip);
    assert_eq!(server.hits("/core"), 1);
}

/// A cached controller that changes on disk, even to the same size and modification time, is
/// checked again, refused, deleted and downloaded again.
#[tokio::test]
async fn a_cached_controller_that_changes_on_disk_is_checked_again() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(host(), b"controller v1")]);
    let server = Server::start(HashMap::from([("/controller", whole(b"controller v1"))])).await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let opened = ExtensionBundle::open(&bundle.manifest, &bundle.zip).unwrap();
    let client = reqwest::Client::new();
    let url = server.url("/controller");
    let path = cache
        .controller(&bundle.manifest, &opened, Some(source(&client, &url)))
        .await
        .unwrap()
        .unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();

    std::fs::write(&path, b"controller v2").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    cache
        .controller(&bundle.manifest, &opened, Some(source(&client, &url)))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"controller v1");
    assert_eq!(server.hits("/controller"), 2);
}

#[test]
fn opens_only_the_bundle_of_the_extension_and_version_the_stack_pins() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let mut renamed = bundle.manifest.clone();
    renamed.name = "other".to_string();
    let mut reversioned = bundle.manifest.clone();
    reversioned.version = "2.0.0".to_string();

    for pinned in [renamed, reversioned] {
        let error = ExtensionBundle::open(&pinned, &bundle.zip).unwrap_err();
        assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
        assert!(
            error.message.contains("is the bundle of acme 1.0.0"),
            "{}",
            error.message
        );
    }
}

#[test]
fn refuses_a_hooks_module_that_is_not_the_one_the_manifest_pins() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let mut opened = ExtensionBundle::open(&bundle.manifest, &bundle.zip)
        .unwrap()
        .manifest()
        .clone();
    opened.hooks_sha256 = "0".repeat(64);
    let zip = core_zip(&opened, &hooks_module("acme"));
    let mut pinned = bundle.manifest.clone();
    pinned.digest = format!("sha256:{:x}", <sha2::Sha256 as sha2::Digest>::digest(&zip));

    let error = ExtensionBundle::open(&pinned, &zip).unwrap_err();

    assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED");
    assert!(error.message.contains("hooks.wasm"), "{}", error.message);
}

/// Answers every request once with `head` and `body`, and reports whether it carried an
/// `Authorization` header.
async fn one_shot(head: String, body: Vec<u8>) -> (String, tokio::sync::oneshot::Receiver<bool>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (sent, received) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0u8; 8192];
        let read = socket.read(&mut request).await.unwrap();
        let text = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        let _ = sent.send(text.contains("\r\nauthorization:"));
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&body).await.unwrap();
        let _ = socket.shutdown().await;
    });
    (base, received)
}

/// A manager that stores bundles elsewhere answers a download with a redirect to a presigned
/// URL, which refuses a second credential: the token must not follow the redirect.
#[tokio::test]
async fn a_redirect_to_a_presigned_url_drops_the_managers_token() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let authorized_client = {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer operator-token"),
        );
        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap()
    };
    let plain_client = reqwest::Client::new();
    for (client, bearer) in [
        (&authorized_client, None),
        (&plain_client, Some("cli-token")),
    ] {
        let (storage, storage_saw_token) = one_shot(
            format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                bundle.zip.len()
            ),
            bundle.zip.clone(),
        )
        .await;
        let (manager, manager_saw_token) = one_shot(
            format!(
                "HTTP/1.1 307 Temporary Redirect\r\nlocation: {}/presigned?X-Amz-Signature=x\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                storage.replace("127.0.0.1", "localhost")
            ),
            Vec::new(),
        )
        .await;
        let root = tempfile::tempdir().unwrap();
        let url = format!("{manager}/v1/extensions/bundles/{}", bundle.manifest.digest);

        BundleCache::new(root.path())
            .core(
                &bundle.manifest,
                BundleSource {
                    client,
                    url: &url,
                    bearer,
                },
            )
            .await
            .unwrap();

        assert!(
            manager_saw_token.await.unwrap(),
            "the manager gets the token"
        );
        assert!(
            !storage_saw_token.await.unwrap(),
            "the presigned URL's host never does"
        );
    }
}
