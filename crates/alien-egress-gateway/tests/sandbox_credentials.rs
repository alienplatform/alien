//! Per-sandbox credential rules end to end: a real proxy listener, the admin API, a linked
//! origin over plain HTTP, TLS interception to an origin, and two replicas.
#![cfg(feature = "local")]
use std::{
    collections::HashMap,
    io::Write,
    net::{IpAddr, SocketAddr},
    sync::{atomic::AtomicU64, Arc, Mutex, OnceLock},
    time::Duration,
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    admin,
    credentials::{Credentials, Injection},
    policy::{Policy, Target},
    record::Record,
    runtime::{self, LiveGateway},
    sandbox_credentials::RuleTarget,
    server::{self, Gateway},
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_compact::{KeyPair, Seed};
use rustls::{
    crypto::ring,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
    ClientConfig, RootCertStore, ServerConfig,
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt};

const AUD: &str = "prefix/agents";
const FIRST: &str = "first-turn-value-5c1d9e";
const SECOND: &str = "second-turn-value-8b2f04";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);
impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Every log line any test in this binary writes, at every level, in the gateway's format.
fn logs() -> &'static Captured {
    static LOGS: OnceLock<Captured> = OnceLock::new();
    LOGS.get_or_init(|| {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::registry()
            .with(alien_egress_gateway::logging::json_layer(captured.clone()))
            .with(tracing_subscriber::filter::LevelFilter::TRACE);
        tracing::subscriber::set_global_default(subscriber).unwrap();
        captured
    })
}

fn key() -> KeyPair {
    KeyPair::from_seed(Seed::new([42; 32]))
}

fn sign(key: &KeyPair, typ: Option<&str>, claims: &Value) -> String {
    let header = match typ {
        Some(typ) => json!({"alg":"EdDSA","kid":"test","typ":typ}),
        None => json!({"alg":"EdDSA","kid":"test"}),
    };
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(key.sk.sign(input.as_bytes(), None).as_ref())
    )
}

fn now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn proxy_token(key: &KeyPair, sub: &str) -> String {
    sign(
        key,
        None,
        &json!({"iss":"alien-sandbox","aud":AUD,"sub":sub,"iat":now(),"exp":now()+600}),
    )
}

fn grant(key: &KeyPair, sub: &str, method: &str) -> String {
    sign(
        key,
        Some("alien-egress-admin"),
        &json!({"iss":"alien-sandbox","aud":AUD,"sub":sub,"method":method,"iat":now(),"exp":now()+60}),
    )
}

fn credential_record(key: &KeyPair, sub: &str, version: u64, rules: Value) -> String {
    sign(
        key,
        Some("alien-egress-credentials"),
        &json!({"v":1,"aud":AUD,"sub":sub,"ver":[version,"0".repeat(32)],"rules":rules}),
    )
}

fn apply_policy(gateway: &Gateway, key: &KeyPair, sub: &str, policy: Option<&Policy>, ver: u64) {
    let mut claims = json!({"v":1,"aud":AUD,"sub":sub,"ver":[ver,"0".repeat(31) + "1"]});
    match policy {
        Some(policy) => claims["policy"] = json!(policy),
        None => claims["deleted"] = json!(true),
    }
    let signed = sign(key, Some("alien-egress-record"), &claims);
    gateway
        .apply_policy(
            Record::verify(signed, &gateway.keys, &gateway.config.audiences, now_ms()).unwrap(),
            false,
        )
        .unwrap();
}

/// A non-loopback address of this host inside a link address space, or `None` (then the
/// tests that need a linked origin are skipped, as the replica identity test is).
fn link_ip() -> Option<std::net::Ipv4Addr> {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("192.0.2.1:9").ok()?;
    match probe.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if alien_core::sandbox_links::is_link_address(ip) => Some(ip),
        _ => None,
    }
}

fn bindings() -> (tempfile::TempDir, Arc<Bindings>) {
    let directory = tempfile::tempdir().unwrap();
    let bindings = Arc::new(
        Bindings::from_env_map(HashMap::from([
            ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
            (
                "ALIEN_SECRETS_BINDING".into(),
                json!({"service":"local-vault","dataDir":directory.path(),"vaultName":"secrets"})
                    .to_string(),
            ),
        ]))
        .unwrap(),
    );
    (directory, bindings)
}

struct Running {
    live: Arc<LiveGateway>,
    proxy: SocketAddr,
    admin: SocketAddr,
    _directory: tempfile::TempDir,
}

async fn start(config: Value, admin_listener: TcpListener) -> Running {
    let (directory, bindings) = bindings();
    let gateway = runtime::build(
        &config.to_string(),
        bindings,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await
    .unwrap();
    let live = Arc::new(LiveGateway::static_config(gateway));
    let admin = admin_listener.local_addr().unwrap();
    let router = admin::router(live.clone());
    tokio::spawn(async move { axum::serve(admin_listener, router).await.unwrap() });
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = proxy_listener.local_addr().unwrap();
    let (_sender, receiver) = tokio::sync::watch::channel(false);
    let served = live.clone();
    tokio::spawn(async move {
        let _sender = _sender;
        server::serve(proxy_listener, served, receiver)
            .await
            .unwrap()
    });
    Running {
        live,
        proxy,
        admin,
        _directory: directory,
    }
}

fn config(key: &KeyPair, links: Value, admin: SocketAddr) -> Value {
    json!({
        "listen":"127.0.0.1:3128", "admin":admin,
        "audiences":[AUD],
        "keys":[{"kid":"test","publicKey":STANDARD.encode(key.pk.as_ref())}],
        "replicaSecret":STANDARD.encode([21;32]), "deniedRanges":[],
        "policyMemoryBytes":4194304, "credentials":[],
        "links":{AUD: links}
    })
}

async fn put_credentials(admin: SocketAddr, grant: &str, record: &str) -> (u16, String) {
    let response = reqwest::Client::new()
        .put(format!("http://{admin}/v1/credentials"))
        .bearer_auth(grant)
        .json(&json!({"record":record}))
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.text().await.unwrap())
}

async fn version(admin: SocketAddr, key: &KeyPair, sub: &str) -> Value {
    reqwest::Client::new()
        .get(format!("http://{admin}/v1/credentials"))
        .query(&[("aud", AUD), ("sub", sub)])
        .bearer_auth(grant(key, sub, "GET"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap()["version"]
        .clone()
}

/// A plain HTTP origin that answers 200 and sends back each request head it received.
async fn echo_origin(
    ip: std::net::Ipv4Addr,
) -> (u16, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind((ip, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let sender = sender.clone();
            tokio::spawn(async move {
                let mut socket = BufReader::new(socket);
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if socket.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    head.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let _ = sender.send(head.to_ascii_lowercase());
                let _ = socket
                    .get_mut()
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .await;
            });
        }
    });
    (port, receiver)
}

/// Send one request through the proxy and return the status code.
async fn through(proxy: SocketAddr, request: String) -> u16 {
    let mut stream = TcpStream::connect(proxy).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response)).await;
    let text = String::from_utf8_lossy(&response);
    text.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

fn absolute(host: &str, port: u16, path: &str, token: &str) -> String {
    format!("GET http://{host}:{port}{path} HTTP/1.1\r\nHost: {host}:{port}\r\nProxy-Authorization: Bearer {token}\r\nAuthorization: placeholder\r\n\r\n")
}

async fn next(origin: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(5), origin.recv())
        .await
        .expect("origin received no request")
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_link_rule_injects_over_plain_http_for_its_sandbox_only_and_is_replaced_whole() {
    let logs = logs();
    let Some(ip) = link_ip() else {
        eprintln!("no link-space address on this host; skipped");
        return;
    };
    let key = key();
    let (port, mut origin) = echo_origin(ip).await;
    let admin_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let links = json!([{"id":"connector","host":"connector.svc","ports":[port],
        "ranges":[format!("{ip}/32")],"addresses":[ip.to_string()]}]);
    let running = start(
        config(&key, links, admin_listener.local_addr().unwrap()),
        admin_listener,
    )
    .await;
    let gateway = running.live.snapshot().unwrap();
    let policy = Policy::Allow {
        allow: vec!["link:connector".into(), "api.example.com".into()],
    };
    for sub in ["box", "other"] {
        apply_policy(&gateway, &key, sub, Some(&policy), now_ms());
    }
    let admin = running.admin;

    // The admin protocol says credentials are served.
    let protocol: Value = reqwest::get(format!("http://{admin}/v1/protocol"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(protocol["protocol"], 2);

    // Nothing set yet: the request goes out with the sandbox's own placeholder.
    let token = proxy_token(&key, "box");
    assert_eq!(
        through(
            running.proxy,
            absolute("connector.svc", port, "/turn", &token)
        )
        .await,
        200
    );
    assert!(next(&mut origin)
        .await
        .contains("authorization: placeholder"));

    let base = now_ms();
    let rules = |value: &str| {
        json!([
            {"link":"connector","header":"Authorization","valuePrefix":"Bearer ","value":value},
            {"host":"api.example.com","port":80,"header":"Authorization","value":value}
        ])
    };
    let (status, body) = put_credentials(
        admin,
        &grant(&key, "box", "PUT"),
        &credential_record(&key, "box", base, rules(FIRST)),
    )
    .await;
    assert_eq!(status, 204, "{body}");
    assert_eq!(version(admin, &key, "box").await[0], base);

    // Absolute-form plain HTTP to the link: the real value replaces the placeholder.
    assert_eq!(
        through(
            running.proxy,
            absolute("connector.svc", port, "/turn", &token)
        )
        .await,
        200
    );
    let head = next(&mut origin).await;
    assert!(
        head.contains(&format!("authorization: bearer {FIRST}")),
        "{head}"
    );
    assert!(!head.contains("placeholder"), "{head}");
    assert!(!head.contains("proxy-authorization"), "{head}");

    // Plain HTTP inside a CONNECT tunnel to the link: the same.
    let connect = format!(
        "CONNECT connector.svc:{port} HTTP/1.1\r\nHost: connector.svc:{port}\r\nProxy-Authorization: Bearer {token}\r\n\r\nGET /inside HTTP/1.1\r\nHost: connector.svc:{port}\r\nAuthorization: placeholder\r\nAuthorization: second\r\n\r\n"
    );
    let mut stream = TcpStream::connect(running.proxy).await.unwrap();
    stream.write_all(connect.as_bytes()).await.unwrap();
    let head = next(&mut origin).await;
    assert!(
        head.contains(&format!("authorization: bearer {FIRST}")),
        "{head}"
    );
    assert_eq!(head.matches("authorization:").count(), 1, "{head}");
    drop(stream);

    // `Connection: authorization` cannot strip the injected header.
    let strip = format!("GET http://connector.svc:{port}/turn HTTP/1.1\r\nHost: connector.svc:{port}\r\nProxy-Authorization: Bearer {token}\r\nConnection: authorization\r\n\r\n");
    assert_eq!(through(running.proxy, strip).await, 200);
    assert!(next(&mut origin)
        .await
        .contains(&format!("authorization: bearer {FIRST}")));

    // An ambiguous path is refused while a rule covers the destination.
    let ambiguous = absolute("connector.svc", port, "/v1/%2e%2e/admin", &token);
    assert_eq!(through(running.proxy, ambiguous).await, 403);

    // Another sandbox of the same Sandbox resource gets nothing.
    let other = proxy_token(&key, "other");
    assert_eq!(
        through(
            running.proxy,
            absolute("connector.svc", port, "/turn", &other)
        )
        .await,
        200
    );
    assert!(next(&mut origin)
        .await
        .contains("authorization: placeholder"));

    // Plain HTTP to an internet host with a rule is refused before any connection.
    assert_eq!(
        through(running.proxy, absolute("api.example.com", 80, "/", &token)).await,
        403
    );

    // A host the policy refuses gets a policy denial; a rule never widens reach.
    let refused = credential_record(
        &key,
        "box",
        base + 1,
        json!([{"host":"blocked.example.com","port":80,"header":"Authorization","value":FIRST}]),
    );
    assert_eq!(
        put_credentials(admin, &grant(&key, "box", "PUT"), &refused)
            .await
            .0,
        204
    );
    let mut stream = TcpStream::connect(running.proxy).await.unwrap();
    stream
        .write_all(absolute("blocked.example.com", 80, "/", &token).as_bytes())
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8_lossy(&response);
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert!(
        response.contains("X-Alien-Egress-Reason: policy"),
        "{response}"
    );

    // Replace: the next request carries the new value, never the old.
    let (status, _) = put_credentials(
        admin,
        &grant(&key, "box", "PUT"),
        &credential_record(&key, "box", base + 2, rules(SECOND)),
    )
    .await;
    assert_eq!(status, 204);
    assert_eq!(
        through(
            running.proxy,
            absolute("connector.svc", port, "/turn", &token)
        )
        .await,
        200
    );
    let head = next(&mut origin).await;
    assert!(
        head.contains(&format!("authorization: bearer {SECOND}")),
        "{head}"
    );
    assert!(!head.contains(FIRST), "{head}");

    // An older set arriving late is acknowledged and ignored.
    let late = credential_record(&key, "box", base + 1, rules(FIRST));
    assert_eq!(
        put_credentials(admin, &grant(&key, "box", "PUT"), &late)
            .await
            .0,
        204
    );
    assert_eq!(version(admin, &key, "box").await[0], base + 2);

    // An empty list removes every rule.
    let (status, _) = put_credentials(
        admin,
        &grant(&key, "box", "PUT"),
        &credential_record(&key, "box", base + 3, json!([])),
    )
    .await;
    assert_eq!(status, 204);
    assert_eq!(
        through(
            running.proxy,
            absolute("connector.svc", port, "/turn", &token)
        )
        .await,
        200
    );
    assert!(next(&mut origin)
        .await
        .contains("authorization: placeholder"));

    // Grants are bound to one sandbox and method; records to a known link and a live policy.
    let record = credential_record(&key, "box", base + 4, rules(FIRST));
    assert_eq!(
        put_credentials(admin, &grant(&key, "other", "PUT"), &record)
            .await
            .0,
        403
    );
    assert_eq!(
        put_credentials(admin, &grant(&key, "box", "GET"), &record)
            .await
            .0,
        403
    );
    assert_eq!(put_credentials(admin, "not-a-grant", &record).await.0, 401);
    let unknown_link = credential_record(
        &key,
        "box",
        base + 4,
        json!([{"link":"elsewhere","header":"Authorization","value":FIRST}]),
    );
    let (status, body) = put_credentials(admin, &grant(&key, "box", "PUT"), &unknown_link).await;
    assert_eq!(status, 400);
    assert!(!body.contains(FIRST), "{body}");
    let unregistered = credential_record(&key, "never-created", base, rules(FIRST));
    let (status, body) =
        put_credentials(admin, &grant(&key, "never-created", "PUT"), &unregistered).await;
    assert_eq!(status, 400, "{body}");
    let (status, body) = put_credentials(admin, &grant(&key, "box", "PUT"), "{not json").await;
    assert_eq!(status, 400);
    assert!(!body.contains(FIRST));

    // Deleting the sandbox's policy drops its rules at once.
    assert_eq!(
        put_credentials(admin, &grant(&key, "box", "PUT"), &record)
            .await
            .0,
        204
    );
    apply_policy(&gateway, &key, "box", None, now_ms() + 10);
    assert_eq!(version(admin, &key, "box").await, Value::Null);

    // No value ever reached a log line or the connection audit.
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        text.contains("egress connection"),
        "audit records were captured"
    );
    for value in [FIRST, SECOND] {
        assert!(!text.contains(value), "a credential value reached the logs");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_is_acknowledged_only_once_every_replica_holds_it() {
    logs();
    let key = key();
    let first_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (first_admin, second_admin) = (
        first_listener.local_addr().unwrap(),
        second_listener.local_addr().unwrap(),
    );
    let with_peers = |admin: SocketAddr, peer: SocketAddr| {
        let mut config = config(&key, json!([]), admin);
        config["peers"] = json!({"static":[peer.to_string()]});
        config
    };
    let first = start(with_peers(first_admin, second_admin), first_listener).await;
    let second = start(with_peers(second_admin, first_admin), second_listener).await;
    for running in [&first, &second] {
        let gateway = running.live.snapshot().unwrap();
        gateway.policies.set_ready();
        apply_policy(&gateway, &key, "box", Some(&Policy::Internet), 1);
    }
    let rules = json!([{"host":"api.example.com","header":"Authorization","value":FIRST}]);
    let base = now_ms();
    let record = credential_record(&key, "box", base, rules.clone());
    assert_eq!(
        put_credentials(first_admin, &grant(&key, "box", "PUT"), &record)
            .await
            .0,
        204
    );
    assert_eq!(version(second_admin, &key, "box").await[0], base);

    // A new grant counts once one replica holds it. A write for that sandbox to the replica
    // that has not caught up yet takes the grant from its peer first, then applies.
    apply_policy(
        &first.live.snapshot().unwrap(),
        &key,
        "fresh",
        Some(&Policy::Internet),
        now_ms(),
    );
    assert!(second
        .live
        .snapshot()
        .unwrap()
        .policies
        .resolve_now(AUD, "fresh")
        .is_none());
    let fresh = credential_record(&key, "fresh", now_ms(), rules.clone());
    let (status, body) = put_credentials(second_admin, &grant(&key, "fresh", "PUT"), &fresh).await;
    assert_eq!(status, 204, "{body}");
    assert!(version(first_admin, &key, "fresh").await[0].is_u64());
    // A sandbox no replica knows stays refused.
    let unknown = credential_record(&key, "unknown", now_ms(), rules.clone());
    let (status, body) =
        put_credentials(second_admin, &grant(&key, "unknown", "PUT"), &unknown).await;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("SANDBOX_EGRESS_POLICY_MISSING"), "{body}");

    // A restarted replica starts empty and takes the sets from its peer before it is ready.
    let third_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let third_admin = third_listener.local_addr().unwrap();
    let third = start(with_peers(third_admin, first_admin), third_listener).await;
    let gateway = third.live.snapshot().unwrap();
    assert!(!gateway.policies.ready());
    gateway.replication.catch_up(&third.live).await.unwrap();
    assert!(gateway.policies.ready());
    assert_eq!(version(third_admin, &key, "box").await[0], base);

    // A peer that is away means no acknowledgement.
    second.live.stop();
    let later = credential_record(&key, "box", base + 1, rules.clone());
    assert_eq!(
        put_credentials(first_admin, &grant(&key, "box", "PUT"), &later)
            .await
            .0,
        503
    );

    // A peer that does not know the route (an older gateway) means no acknowledgement either.
    let old_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let old_admin = old_listener.local_addr().unwrap();
    let old = axum::Router::new().fallback(|| async { axum::http::StatusCode::NOT_FOUND });
    tokio::spawn(async move { axum::serve(old_listener, old).await.unwrap() });
    let fourth_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fourth_admin = fourth_listener.local_addr().unwrap();
    let fourth = start(with_peers(fourth_admin, old_admin), fourth_listener).await;
    let gateway = fourth.live.snapshot().unwrap();
    gateway.policies.set_ready();
    apply_policy(&gateway, &key, "box", Some(&Policy::Internet), 1);
    let record = credential_record(&key, "box", base + 2, rules);
    assert_eq!(
        put_credentials(fourth_admin, &grant(&key, "box", "PUT"), &record)
            .await
            .0,
        503
    );
}

fn guest_client(pem: &str) -> TlsConnector {
    let der = STANDARD
        .decode(
            pem.lines()
                .filter(|line| !line.starts_with("---"))
                .collect::<String>(),
        )
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(der)).unwrap();
    TlsConnector::from(Arc::new(
        ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))
}

/// TLS interception to an internet host: the leaf comes from the sandbox's CA, the rules are
/// read per request, and a host without a current rule gets no leaf at all.
#[tokio::test(flavor = "multi_thread")]
async fn interception_injects_the_current_rules_of_the_sandbox_and_nothing_else() {
    let logs = logs();
    let key = key();
    let origin_key = rcgen::KeyPair::generate().unwrap();
    let origin_certificate = rcgen::CertificateParams::new(vec!["api.example.com".into()])
        .unwrap()
        .self_signed(&origin_key)
        .unwrap();
    let server_tls = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![origin_certificate.der().clone()],
                PrivatePkcs8KeyDer::from(origin_key.serialize_der()).into(),
            )
            .unwrap(),
    );
    let credentials = Arc::new(
        Credentials::new(&[STANDARD.encode(origin_certificate.der())], Some([7; 32])).unwrap(),
    );
    let (_directory, bindings) = bindings();
    let gateway = runtime::build(
        &config(&key, json!([]), "127.0.0.1:3129".parse().unwrap()).to_string(),
        bindings,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await
    .unwrap();
    apply_policy(&gateway, &key, "box", Some(&Policy::Internet), 1);
    let store = gateway.sandbox_credentials.clone();
    let set = |ver: u64, rules: Value| {
        let record = gateway
            .verify_credentials(credential_record(&key, "box", ver, rules))
            .unwrap();
        gateway.apply_credentials(record).unwrap();
    };
    set(
        1,
        json!([{"host":"api.example.com","header":"X-Api-Key","method":"GET","path":"/v1/","value":FIRST}]),
    );
    let guest_tls = guest_client(&credentials.issue_ca(AUD, "box").await.unwrap());

    let exchange = |method: &'static str, path: &'static str, sub: &'static str| {
        let credentials = credentials.clone();
        let server_tls = server_tls.clone();
        let guest_tls = guest_tls.clone();
        let store = store.clone();
        async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let origin = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let Ok(socket) = TlsAcceptor::from(server_tls).accept(socket).await else {
                    return String::new();
                };
                let mut socket = BufReader::new(socket);
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if socket.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return head;
                    }
                    head.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                socket
                    .get_mut()
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
                head.to_ascii_lowercase()
            });
            let upstream = TcpStream::connect(address).await.unwrap();
            let (guest, proxy) = tokio::io::duplex(32 * 1024);
            let forwarding = tokio::spawn(async move {
                let injection = Injection {
                    store,
                    aud: AUD.into(),
                    sub: sub.into(),
                    target: RuleTarget::Host("api.example.com".into()),
                    port: 443,
                };
                credentials
                    .forward(
                        proxy,
                        upstream,
                        Target::parse("api.example.com:443", None).unwrap(),
                        &injection,
                    )
                    .await
            });
            let Ok(socket) = guest_tls
                .connect(ServerName::try_from("api.example.com").unwrap(), guest)
                .await
            else {
                assert!(forwarding.await.unwrap().is_err());
                return None;
            };
            let mut socket = BufReader::new(socket);
            socket
                .get_mut()
                .write_all(format!("{method} {path} HTTP/1.1\r\nHost: api.example.com\r\nX-Api-Key: placeholder\r\nContent-Length: 0\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut status = String::new();
            socket.read_line(&mut status).await.unwrap();
            drop(socket);
            let _ = forwarding.await.unwrap();
            Some(origin.await.unwrap())
        }
    };
    let head = exchange("GET", "/v1/messages", "box").await.unwrap();
    assert!(head.contains(&format!("x-api-key: {FIRST}")), "{head}");
    assert!(exchange("POST", "/v1/messages", "box")
        .await
        .unwrap()
        .contains("x-api-key: placeholder"));
    assert!(exchange("GET", "/v2", "box")
        .await
        .unwrap()
        .contains("x-api-key: placeholder"));
    // Another sandbox has no rule for the host, so the gateway issues it no leaf.
    assert!(exchange("GET", "/v1/messages", "other").await.is_none());
    // Replace, then remove: the next request follows the current set.
    set(
        2,
        json!([{"host":"api.example.com","header":"X-Api-Key","value":SECOND}]),
    );
    let head = exchange("GET", "/v2", "box").await.unwrap();
    assert!(head.contains(&format!("x-api-key: {SECOND}")), "{head}");
    set(3, json!([]));
    assert!(exchange("GET", "/v1/messages", "box").await.is_none());
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    for value in [FIRST, SECOND] {
        assert!(!text.contains(value), "a credential value reached the logs");
    }
}

/// Configurations from earlier controllers carry `credentials: []` and still load; a rule in
/// configuration is refused rather than silently ignored.
#[tokio::test]
async fn configured_credential_rules_are_refused_and_an_empty_list_still_loads() {
    let key = key();
    let (_directory, bindings) = bindings();
    let mut config = config(&key, json!([]), "127.0.0.1:3129".parse().unwrap());
    let build = |config: Value| {
        let bindings = bindings.clone();
        async move {
            runtime::build(
                &config.to_string(),
                bindings,
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicU64::new(0)),
            )
            .await
        }
    };
    assert!(build(config.clone()).await.is_ok());
    config["credentials"] = json!([{"aud":AUD,"host":"api.example.com","port":443,"method":"GET",
        "path":"/","header":"Authorization","vaultBinding":"secrets","secret":"key"}]);
    let error = build(config).await.err().unwrap();
    assert!(error.message.contains("set at runtime"), "{error:?}");
}

/// Forwards replica requests to `target`; after the first credential summary it has answered,
/// it writes `late` to a replica the caller cannot see, as a write acknowledged while the
/// caller was still catching up would be.
async fn lagging_peer(
    target: SocketAddr,
    after_summary: impl Fn() + Send + Sync + 'static,
) -> SocketAddr {
    use axum::{
        body::Bytes,
        extract::State,
        http::{HeaderMap, Uri},
        response::IntoResponse,
    };
    struct Shared {
        hook: Box<dyn Fn() + Send + Sync>,
        fired: std::sync::atomic::AtomicBool,
    }
    let shared = Arc::new(Shared {
        hook: Box::new(after_summary),
        fired: false.into(),
    });
    let handler = move |State(shared): State<Arc<Shared>>,
                        uri: Uri,
                        headers: HeaderMap,
                        body: Bytes| async move {
        let mut request = reqwest::Client::new().post(format!("http://{target}{}", uri.path()));
        for name in [
            "content-type",
            "x-alien-replica-time",
            "x-alien-replica-signature",
        ] {
            if let Some(value) = headers.get(name) {
                request = request.header(name, value);
            }
        }
        let response = request.body(body).send().await.unwrap();
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        if uri.path() == "/v1/replicas/credentials-summary"
            && !shared.fired.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            (shared.hook)();
        }
        (status, bytes).into_response()
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().fallback(handler).with_state(shared);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    address
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replica_catching_up_also_gets_writes_acknowledged_while_it_pulled() {
    logs();
    let key = key();
    let listeners = [
        TcpListener::bind("127.0.0.1:0").await.unwrap(),
        TcpListener::bind("127.0.0.1:0").await.unwrap(),
        TcpListener::bind("127.0.0.1:0").await.unwrap(),
    ];
    let admins: Vec<SocketAddr> = listeners.iter().map(|l| l.local_addr().unwrap()).collect();
    let [la, lb, lc] = listeners;
    let with_peers = |admin: SocketAddr, peer: SocketAddr| {
        let mut config = config(&key, json!([]), admin);
        config["peers"] = json!({"static":[peer.to_string()]});
        config
    };
    // A and B see each other. C is not in their peer lists: replicas are found only once ready.
    let a = start(with_peers(admins[0], admins[1]), la).await;
    let b = start(with_peers(admins[1], admins[0]), lb).await;
    for running in [&a, &b] {
        let gateway = running.live.snapshot().unwrap();
        gateway.policies.set_ready();
        apply_policy(&gateway, &key, "box", Some(&Policy::Internet), 1);
    }
    let rules = json!([{"host":"api.example.com","header":"Authorization","value":FIRST}]);
    let base = now_ms();
    assert_eq!(
        put_credentials(
            admins[0],
            &grant(&key, "box", "PUT"),
            &credential_record(&key, "box", base, rules.clone())
        )
        .await
        .0,
        204
    );

    let writer_key = key.clone();
    let late_rules = rules.clone();
    let late_admin = admins[0];
    let stub = lagging_peer(admins[0], move || {
        // Runs on the proxy's task; a blocking client on its own thread keeps it simple.
        let (key, rules) = (writer_key.clone(), late_rules.clone());
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let record = credential_record(&key, "box", base + 5, rules);
                assert_eq!(
                    put_credentials(late_admin, &grant(&key, "box", "PUT"), &record)
                        .await
                        .0,
                    204
                );
            })
        })
        .join()
        .unwrap();
    })
    .await;
    let c = start(with_peers(admins[2], stub), lc).await;
    let gateway = c.live.snapshot().unwrap();
    gateway.replication.catch_up(&c.live).await.unwrap();
    assert!(gateway.policies.ready());
    assert_eq!(
        version(admins[2], &key, "box").await[0],
        base + 5,
        "the write acknowledged during the catch-up reached the replica before it was ready"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn catch_up_skips_a_set_naming_a_link_this_replica_no_longer_declares() {
    logs();
    let Some(ip) = link_ip() else {
        eprintln!("no link-space address on this host; skipped");
        return;
    };
    let key = key();
    let links = json!([{"id":"connector","host":"connector.svc","ports":[8080],
        "ranges":[format!("{ip}/32")],"addresses":[ip.to_string()]}]);
    let (la, lb) = (
        TcpListener::bind("127.0.0.1:0").await.unwrap(),
        TcpListener::bind("127.0.0.1:0").await.unwrap(),
    );
    let (admin_a, admin_b) = (la.local_addr().unwrap(), lb.local_addr().unwrap());
    // The peer still declares the link; this replica's config (after a stack update) does not.
    let a = start(config(&key, links, admin_a), la).await;
    let mut updated = config(&key, json!([]), admin_b);
    updated["peers"] = json!({"static":[admin_a.to_string()]});
    let b = start(updated, lb).await;
    let policy = Policy::Allow {
        allow: vec!["link:connector".into(), "api.example.com".into()],
    };
    let gateway = a.live.snapshot().unwrap();
    gateway.policies.set_ready();
    apply_policy(&gateway, &key, "gone", Some(&policy), now_ms());
    apply_policy(&gateway, &key, "kept", Some(&policy), now_ms());
    let link_rule = json!([{"link":"connector","header":"Authorization","value":FIRST}]);
    let host_rule = json!([{"host":"api.example.com","header":"Authorization","value":FIRST}]);
    for (sub, rules) in [("gone", link_rule), ("kept", host_rule)] {
        let record = credential_record(&key, sub, now_ms(), rules);
        assert_eq!(
            put_credentials(admin_a, &grant(&key, sub, "PUT"), &record)
                .await
                .0,
            204
        );
    }
    let restarted = b.live.snapshot().unwrap();
    restarted.replication.catch_up(&b.live).await.unwrap();
    assert!(
        restarted.policies.ready(),
        "one stale record must not keep the replica unready"
    );
    assert!(version(admin_b, &key, "kept").await[0].is_u64());
    assert!(version(admin_b, &key, "gone").await[0].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_catch_up_before_a_write_is_unavailable_not_a_bad_request() {
    logs();
    let key = key();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin = listener.local_addr().unwrap();
    let dead = {
        let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
        socket.local_addr().unwrap()
    };
    let mut config = config(&key, json!([]), admin);
    config["peers"] = json!({"static":[dead.to_string()]});
    let running = start(config, listener).await;
    running.live.snapshot().unwrap().policies.set_ready();
    let record = credential_record(
        &key,
        "fresh",
        now_ms(),
        json!([{"host":"api.example.com","header":"Authorization","value":FIRST}]),
    );
    let (status, body) = put_credentials(admin, &grant(&key, "fresh", "PUT"), &record).await;
    assert_eq!(status, 503, "{body}");
}
