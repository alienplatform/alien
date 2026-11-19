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
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let server = Server::start(HashMap::from([("/core", whole(&bundle.zip))])).await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let client = reqwest::Client::new();
    let core_url = server.url("/core");

    for _ in 0..2 {
        cache
            .core(&bundle.manifest, source(&client, &core_url))
            .await
            .unwrap();
    }

    let hex = bundle.manifest.digest.strip_prefix("sha256:").unwrap();
    let dir = root.path().join(format!("acme@sha256-{hex}"));
    assert_eq!(files(&dir), ["core.zip"]);
    assert_eq!(
        server.hits("/core"),
        1,
        "a digest it has is not downloaded again"
    );
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
async fn a_download_cut_short_leaves_nothing_behind() {
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
    assert!(files(&cache.dir(&bundle.manifest).unwrap()).is_empty());
    assert!(cache.cached(&bundle.manifest).await.unwrap().is_none());
}

#[test]
fn a_bundle_with_no_controller_for_the_machine_fails_naming_the_extension_and_target() {
    let bundle = TestBundle::new("acme", "1.0.0", &[(other_target(), b"controller")]);
    let opened = ExtensionBundle::open(&bundle.manifest, &bundle.zip).unwrap();

    let error = opened.host_controller().unwrap_err();

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

/// A kept core zip is checked against each stack that pins its digest: one that lists other
/// egress or another version is refused, and the zip still serves the stack it matches.
#[tokio::test]
async fn a_kept_core_zip_is_refused_to_a_stack_that_describes_it_differently() {
    let bundle = TestBundle::with_controller_module(
        "acme",
        "1.0.0",
        b"module",
        &["https://api.acme.example"],
    );
    let server = Server::start(HashMap::from([("/core", whole(&bundle.zip))])).await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let client = reqwest::Client::new();
    cache
        .core(&bundle.manifest, source(&client, &server.url("/core")))
        .await
        .unwrap();
    let mut other_egress = bundle.manifest.clone();
    other_egress.egress = Vec::new();
    let mut reversioned = bundle.manifest.clone();
    reversioned.version = "1.0.1".to_string();

    for other in [other_egress, reversioned] {
        let error = cache.cached(&other).await.unwrap_err();
        assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED", "{error}");
    }
    assert!(cache.cached(&bundle.manifest).await.unwrap().is_some());
    assert_eq!(server.hits("/core"), 1);
}

#[tokio::test]
async fn a_stack_that_describes_a_core_zip_differently_leaves_it_on_disk() {
    let bundle = TestBundle::with_controller_module(
        "acme",
        "1.0.0",
        b"module",
        &["https://api.acme.example"],
    );
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let dir = cache.dir(&bundle.manifest).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("core.zip"), &bundle.zip).unwrap();
    let mut other_egress = bundle.manifest.clone();
    other_egress.egress = Vec::new();

    let error = cache.cached(&other_egress).await.unwrap_err();
    assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED", "{error}");
    assert_eq!(std::fs::read(dir.join("core.zip")).unwrap(), bundle.zip);
    assert!(cache.cached(&bundle.manifest).await.unwrap().is_some());
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
    // The Operator's client carries its token as a default header and follows no redirect, so
    // the download follows it, through a client of its own.
    let operator_client = {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer operator-token"),
        );
        reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    };
    for (client, bearer) in [
        (&authorized_client, None),
        (&plain_client, Some("cli-token")),
        (&operator_client, None),
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

/// A server that takes the request and never answers fails the download once its time is up,
/// as an error another step may retry, and leaves no file behind.
#[tokio::test]
async fn a_download_that_stops_answering_fails_in_time() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/v1/extensions/bundles/{}",
        listener.local_addr().unwrap(),
        bundle.manifest.digest
    );
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        // Holds the connection open without a reply.
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        drop(socket);
    });
    let root = tempfile::tempdir().unwrap();
    let cache =
        BundleCache::new(root.path()).with_download_timeout(std::time::Duration::from_millis(300));
    let started = std::time::Instant::now();
    let error = cache
        .core(
            &bundle.manifest,
            BundleSource {
                client: &reqwest::Client::new(),
                url: &url,
                bearer: None,
            },
        )
        .await
        .unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(error.retryable, "{error:?}");
    assert!(error.message.contains("took longer"), "{}", error.message);
    let dir = cache.dir(&bundle.manifest).unwrap();
    let left: Vec<_> = std::fs::read_dir(&dir)
        .map(|entries| entries.filter_map(|entry| entry.ok()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

/// Two downloads of one bundle at once each write their own file, so neither renames the other's
/// away, and both end with the bundle in place.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_downloads_of_one_bundle_at_once_both_succeed() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let server = Server::start(HashMap::from([(
        "/core",
        Reply {
            body: bundle.zip.clone(),
            cut_short: false,
        },
    )]))
    .await;
    let root = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let url = server.url("/core");

    let downloads = (0..8).map(|_| {
        let (cache, client, url, manifest) = (
            BundleCache::new(root.path()),
            client.clone(),
            url.clone(),
            bundle.manifest.clone(),
        );
        tokio::spawn(async move { cache.core(&manifest, source(&client, &url)).await })
    });
    for download in futures::future::join_all(downloads).await {
        download
            .unwrap()
            .expect("each download ends with the bundle");
    }
    let cache = BundleCache::new(root.path());
    assert_eq!(files(&cache.dir(&bundle.manifest).unwrap()), ["core.zip"]);
}

/// A download a stopped process left behind is removed by the next download once it is old; a
/// recent one, maybe another download still running, stays.
#[tokio::test]
async fn a_download_removes_files_abandoned_long_ago() {
    let bundle = TestBundle::new("acme", "1.0.0", &[]);
    let server = Server::start(HashMap::from([(
        "/core",
        Reply {
            body: bundle.zip.clone(),
            cut_short: false,
        },
    )]))
    .await;
    let root = tempfile::tempdir().unwrap();
    let cache = BundleCache::new(root.path());
    let dir = cache.dir(&bundle.manifest).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let old = std::fs::File::create(dir.join("core.zip.part-old")).unwrap();
    old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60))
        .unwrap();
    std::fs::write(dir.join("core.zip.part-recent"), b"half").unwrap();
    let client = reqwest::Client::new();
    let url = server.url("/core");

    cache
        .core(&bundle.manifest, source(&client, &url))
        .await
        .unwrap();

    assert_eq!(files(&dir), ["core.zip", "core.zip.part-recent"]);
}
