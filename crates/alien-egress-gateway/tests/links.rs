//! Sandbox links: a declared service is reached by its exact internal hostname on its declared
//! ports, and only while the sandbox's policy keeps it.
#![cfg(feature = "local")]
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{atomic::AtomicU64, Arc},
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    policy::{check_link_addresses, Policy, Target},
    record::Record,
    runtime,
    server::Gateway,
    token::Claims,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_compact::{KeyPair, Seed};
use serde_json::{json, Value};

const AUD: &str = "prefix/agents";

fn config(key: &KeyPair) -> Value {
    json!({
        "listen":"127.0.0.1:3128", "admin":"127.0.0.1:3129",
        "audiences":[AUD, "prefix/other"],
        "keys":[{"kid":"test","publicKey":STANDARD.encode(key.pk.as_ref())}],
        "replicaSecret":STANDARD.encode([21;32]), "deniedRanges":["10.0.0.0/8"],
        "policyMemoryBytes":1048576,
        "links":{AUD:[{"id":"api","host":"api.svc","ports":[8080],"ranges":["10.244.0.0/16"]}]}
    })
}

async fn gateway(
    config: &Value,
) -> alien_error::Result<Gateway, alien_egress_gateway::error::ErrorData> {
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
    runtime::build(
        &config.to_string(),
        bindings,
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await
}

fn apply(gateway: &Gateway, key: &KeyPair, aud: &str, sub: &str, policy: &Policy) {
    let now = chrono::Utc::now().timestamp_millis() as u64;
    let payload = json!({"v":1,"aud":aud,"sub":sub,"ver":[now,"00000000000000000000000000000001"],"policy":policy});
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"test","typ":"alien-egress-record"}"#),
        URL_SAFE_NO_PAD.encode(payload.to_string())
    );
    let signed = format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(key.sk.sign(input.as_bytes(), None).as_ref())
    );
    gateway
        .policies
        .apply(
            Record::verify(signed, &gateway.keys, &gateway.config.audiences, now).unwrap(),
            false,
        )
        .unwrap();
}

fn claims(aud: &str, sub: &str) -> Claims {
    let now = chrono::Utc::now().timestamp() as u64;
    Claims {
        iss: "alien-sandbox".into(),
        aud: aud.into(),
        sub: sub.into(),
        iat: now,
        exp: now + 600,
    }
}

fn target(value: &str) -> Target {
    Target::parse(value, None).unwrap()
}

#[tokio::test]
async fn a_link_is_reached_only_by_its_name_and_port_and_only_while_the_policy_keeps_it() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let gateway = gateway(&config(&key)).await.unwrap();
    let policies = [
        ("open", Policy::Internet),
        (
            "named",
            Policy::Allow {
                allow: vec!["link:api".into()],
            },
        ),
        ("deny-all", Policy::Allow { allow: vec![] }),
        (
            "other-link",
            Policy::Allow {
                allow: vec!["link:billing".into(), "api.svc".into()],
            },
        ),
    ];
    for (sub, policy) in &policies {
        apply(&gateway, &key, AUD, sub, policy);
    }
    apply(&gateway, &key, "prefix/other", "open", &Policy::Internet);

    // `internet` keeps the declared link, and `link:api` names it.
    for sub in ["open", "named"] {
        let link = gateway
            .authorize(&claims(AUD, sub), &target("api.svc:8080"))
            .await
            .unwrap()
            .expect("declared link");
        assert_eq!(link.id, "api");
    }
    // An empty allowlist still means nothing, links included; so does a list that names a
    // different link or the link's hostname as a destination.
    for sub in ["deny-all", "other-link"] {
        let error = gateway
            .authorize(&claims(AUD, sub), &target("api.svc:8080"))
            .await
            .unwrap_err();
        assert_eq!(error.code, "EGRESS_POLICY_DENIED", "{sub}");
    }
    // Another port, another audience, or the address instead of the name is not the link:
    // those take the ordinary path, which never dials a private answer.
    for (aud, value) in [
        (AUD, "api.svc:8081"),
        ("prefix/other", "api.svc:8080"),
        (AUD, "10.244.0.5:8080"),
        (AUD, "other.svc:8080"),
    ] {
        let result = gateway
            .authorize(&claims(aud, "open"), &target(value))
            .await;
        assert!(
            !matches!(result, Ok(Some(_))),
            "{aud} {value} must not match a link"
        );
    }
}

#[tokio::test]
async fn linked_answers_must_all_fall_in_the_link_ranges() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let gateway = gateway(&config(&key)).await.unwrap();
    let link = gateway.config.link(AUD, "api.svc", 8080).unwrap();
    let inside: IpAddr = "10.244.1.9".parse().unwrap();
    check_link_addresses(&[inside], link).unwrap();
    assert!(check_link_addresses(&[], link).is_err());
    // A rebinding answer, metadata, loopback or IPv6 next to a good answer refuses them all.
    for other in [
        "10.245.0.1",
        "169.254.169.254",
        "127.0.0.1",
        "8.8.8.8",
        "fd00:ec2::254",
        "fe80::1",
    ] {
        assert!(
            check_link_addresses(&[inside, other.parse().unwrap()], link).is_err(),
            "{other}"
        );
    }
}

#[tokio::test]
async fn invalid_links_refuse_the_whole_configuration() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let link = |range: &str, ports: Value| json!({"id":"api","host":"api.svc","ports":ports,"ranges":[range]});
    for links in [
        json!({"prefix/unknown":[link("10.244.0.0/16", json!([8080]))]}),
        json!({AUD:[link("169.254.0.0/16", json!([80]))]}),
        json!({AUD:[link("0.0.0.0/0", json!([80]))]}),
        json!({AUD:[link("10.0.0.0/7", json!([80]))]}),
        json!({AUD:[link("10.244.0.0/16", json!([53]))]}),
        json!({AUD:[link("10.244.0.0/16", json!([]))]}),
        json!({AUD:[link("10.244.0.0/16", json!([80])), link("10.245.0.0/16", json!([81]))]}),
        json!({AUD:[{"id":"api","host":"api.svc","ports":[80],"ranges":["10.0.0.0/8"],"extra":1}]}),
    ] {
        let mut config = config(&key);
        config["links"] = links.clone();
        assert!(gateway(&config).await.is_err(), "{links}");
    }
    // A configuration without links is read exactly as before.
    let mut config = config(&key);
    config.as_object_mut().unwrap().remove("links");
    assert!(gateway(&config).await.unwrap().config.links.is_empty());
}

#[tokio::test]
async fn fixed_link_addresses_must_lie_in_the_link_ranges() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let link = |addresses: Value| {
        json!({AUD:[{"id":"api","host":"api.svc","ports":[8080],
        "ranges":["10.244.0.0/16"],"addresses":addresses}]})
    };
    for addresses in [
        json!(["127.0.0.1"]),
        json!(["169.254.169.254"]),
        json!(["10.245.0.4"]),
        json!(["10.244.0.4", "8.8.8.8"]),
        json!(["fd00:ec2::254"]),
    ] {
        let mut config = config(&key);
        config["links"] = link(addresses.clone());
        assert!(gateway(&config).await.is_err(), "{addresses}");
    }
    let mut config = config(&key);
    config["links"] = link(json!(["10.244.7.9"]));
    let gateway = gateway(&config).await.unwrap();
    let link = gateway.config.link(AUD, "api.svc", 8080).unwrap();
    assert_eq!(
        link.addresses,
        ["10.244.7.9".parse::<std::net::Ipv4Addr>().unwrap()]
    );
}

/// The 30-second sweep closes an open linked tunnel once the configuration stops declaring that
/// link, and once the link's ranges no longer admit the address the tunnel is connected to. A
/// tunnel whose link and address are still declared stays open.
#[tokio::test]
async fn the_sweep_closes_a_linked_tunnel_after_an_unlink_or_a_range_change() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
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
    let path = directory.path().join("config.json");
    let write = |config: &Value| std::fs::write(&path, config.to_string()).unwrap();
    let linked = config(&key);
    write(&linked);
    let gateway = runtime::build(
        &linked.to_string(),
        bindings.clone(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await
    .unwrap();
    apply(&gateway, &key, AUD, "open", &Policy::Internet);
    let live = runtime::LiveGateway::refreshing(gateway, runtime::ConfigSource::File(path.clone()));
    let registry = live.snapshot().unwrap().connections.clone();
    let open = |peer: &str| {
        let (permit, cancelled) = registry
            .register(
                claims(AUD, "open"),
                Some(target("api.svc:8080")),
                Arc::new(alien_egress_gateway::io::Counters::default()),
            )
            .unwrap();
        permit
            .record_peer(peer.parse().unwrap(), Some("api".into()))
            .unwrap();
        (permit, cancelled)
    };

    // Declared link, address inside its range: the sweep keeps it.
    let (_kept, kept) = open("10.244.0.5");
    registry.revalidate(&live).await;
    assert_eq!(*kept.borrow(), None);

    // The link's ranges move away from the connected address: closed.
    let mut moved = linked.clone();
    moved["links"][AUD][0]["ranges"] = json!(["10.245.0.0/16"]);
    write(&moved);
    live.refresh(&bindings).await.unwrap();
    registry.revalidate(&live).await;
    assert_eq!(*kept.borrow(), Some("policy revoked or unavailable"));

    // The link is removed from the configuration: a tunnel opened before is closed.
    write(&linked);
    live.refresh(&bindings).await.unwrap();
    let (_unlinked, unlinked) = open("10.244.0.6");
    registry.revalidate(&live).await;
    assert_eq!(*unlinked.borrow(), None);
    let mut removed = linked.clone();
    removed.as_object_mut().unwrap().remove("links");
    write(&removed);
    live.refresh(&bindings).await.unwrap();
    registry.revalidate(&live).await;
    assert_eq!(*unlinked.borrow(), Some("policy revoked or unavailable"));
}

/// The linked service's fixed address in the socket test below. The test needs it on a local
/// interface: `sudo ip addr add 10.244.7.9/32 dev lo`.
const LINKED_ADDRESS: &str = "10.244.7.9";

/// A linked service on `LINKED_ADDRESS:port`. It reads a connection's first bytes until they
/// form a complete request head or can no longer be one (anything not starting `GET `), then:
/// answers `GET /refuse` with `400 Bad Request`, keeps the connection open and records each
/// later read; answers any other `GET` with `101 Switching Protocols` and echoes every byte after
/// the head; echoes any other protocol from the first byte. Records each connection's first
/// bytes.
async fn echo_service(port: u16) -> Arc<std::sync::Mutex<Vec<String>>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind((LINKED_ADDRESS, port))
        .await
        .unwrap_or_else(|error| {
            panic!("bind {LINKED_ADDRESS}:{port} ({error}); run `sudo ip addr add {LINKED_ADDRESS}/32 dev lo`")
        });
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let record = record.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                let mut first = Vec::new();
                let head_end = loop {
                    let n = socket.read(&mut buffer).await.unwrap_or(0);
                    if n == 0 {
                        break None;
                    }
                    first.extend_from_slice(&buffer[..n]);
                    if !(first.starts_with(b"GET ") || b"GET ".starts_with(&first)) {
                        break None;
                    }
                    if let Some(at) = first.windows(4).position(|v| v == b"\r\n\r\n") {
                        break Some(at + 4);
                    }
                };
                record
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&first).to_string());
                match head_end {
                    Some(_) if first.starts_with(b"GET /refuse") => {
                        socket
                            .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                            .await
                            .unwrap();
                        while let Ok(n) = socket.read(&mut buffer).await {
                            if n == 0 {
                                break;
                            }
                            record
                                .lock()
                                .unwrap()
                                .push(String::from_utf8_lossy(&buffer[..n]).to_string());
                        }
                        return;
                    }
                    Some(end) => {
                        socket
                            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
                            .await
                            .unwrap();
                        socket.write_all(&first[end..]).await.unwrap();
                    }
                    None => socket.write_all(&first).await.unwrap(),
                }
                let (mut reader, mut writer) = socket.split();
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            });
        }
    });
    seen
}

fn proxy_token(key: &KeyPair, aud: &str, sub: &str) -> String {
    let claims = claims(aud, sub);
    let input =
        format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"test"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
            "iss":claims.iss,"aud":claims.aud,"sub":claims.sub,"iat":claims.iat,"exp":claims.exp
        })).unwrap())
    );
    format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(key.sk.sign(input.as_bytes(), None).as_ref())
    )
}

/// Waits up to 15 seconds for an answer (a refused name may first time out in DNS), then reads
/// until the peer closes or stays quiet for half a second.
async fn read_window(socket: &mut tokio::net::TcpStream, received: &mut Vec<u8>) {
    use tokio::io::AsyncReadExt;
    let mut buffer = vec![0u8; 4096];
    let mut wait = std::time::Duration::from_secs(15);
    while let Ok(Ok(n)) = tokio::time::timeout(wait, socket.read(&mut buffer)).await {
        if n == 0 {
            break;
        }
        received.extend_from_slice(&buffer[..n]);
        wait = std::time::Duration::from_millis(500);
    }
}

/// Sends `head` to the gateway, then `payload` once the gateway (or the service) answers,
/// and returns everything read back within a short window.
async fn exchange(gateway: std::net::SocketAddr, head: &str, payload: &[u8]) -> String {
    use tokio::io::AsyncWriteExt;
    let mut socket = tokio::net::TcpStream::connect(gateway).await.unwrap();
    socket.write_all(head.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    read_window(&mut socket, &mut received).await;
    if !payload.is_empty() && !String::from_utf8_lossy(&received).contains(" 403 ") {
        let _ = socket.write_all(payload).await;
        read_window(&mut socket, &mut received).await;
    }
    String::from_utf8_lossy(&received).to_string()
}

/// A declared link carries any protocol: a WebSocket through a CONNECT tunnel or through a
/// plain-proxy `Upgrade` request, and a non-HTTP protocol through a CONNECT tunnel. Anything
/// that is not the link (an unlinked service, an undeclared port) is refused before a byte
/// reaches a service.
#[tokio::test]
#[ignore = "needs 10.244.7.9 on a local interface: sudo ip addr add 10.244.7.9/32 dev lo"]
async fn a_link_carries_websocket_and_other_protocols_and_nothing_else_does() {
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let seen = echo_service(9000).await;
    let other = echo_service(9001).await;
    let credentialed = echo_service(9002).await;
    let mut config = config(&key);
    config["links"] = json!({AUD:[
        {"id":"api","host":"api.svc","ports":[9000],"ranges":["10.244.0.0/16"],"addresses":[LINKED_ADDRESS]},
        {"id":"vendor","host":"vendor.svc","ports":[9002],"ranges":["10.244.0.0/16"],"addresses":[LINKED_ADDRESS]}
    ]});
    // A credential rule on the second link keeps it on the inspected TLS path.
    config["credentials"] = json!([{"aud":AUD,"host":"vendor.svc","port":9002,"method":"GET",
        "path":"/","header":"Authorization","vaultBinding":"secrets","secret":"vendor-key"}]);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    std::fs::write(&path, config.to_string()).unwrap();
    let mut gateway = gateway(&config).await.unwrap();
    // Short limits for connections that are not links, so a quiet link tunnel can be shown to
    // outlive them.
    gateway.connections = Arc::new(
        alien_egress_gateway::connections::Connections::with_timeouts(
            alien_egress_gateway::connections::Timeouts {
                idle: std::time::Duration::from_secs(1),
                lifetime: std::time::Duration::from_secs(2),
                link_idle: std::time::Duration::from_secs(1800),
            },
        ),
    );
    apply(&gateway, &key, AUD, "open", &Policy::Internet);
    apply(
        &gateway,
        &key,
        AUD,
        "deny-all",
        &Policy::Allow { allow: vec![] },
    );
    let live = Arc::new(runtime::LiveGateway::refreshing(
        gateway,
        runtime::ConfigSource::File(path),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (_shutdown, receiver) = tokio::sync::watch::channel(false);
    tokio::spawn(alien_egress_gateway::server::serve(
        listener, live, receiver,
    ));

    let token = proxy_token(&key, AUD, "open");
    let connect = |authority: &str, token: &str| {
        format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Authorization: Bearer {token}\r\n\r\n")
    };
    let websocket = |authority: &str| {
        format!(
            "GET /ws?x=1 HTTP/1.1\r\nHost: {authority}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
             Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        )
    };
    let frame: &[u8] = b"\x81\x05hello";
    let redis: &[u8] = b"*1\r\n$4\r\nPING\r\n";

    // WebSocket through a CONNECT tunnel, as the sandbox agent's transparent path sends it.
    let tunnel = exchange(
        address,
        &(connect("api.svc:9000", &token) + &websocket("api.svc:9000")),
        frame,
    )
    .await;
    assert!(
        tunnel.starts_with("HTTP/1.1 200 Connection Established"),
        "{tunnel}"
    );
    assert!(tunnel.contains("101 Switching Protocols"), "{tunnel}");
    assert!(
        tunnel.ends_with("\u{1}\u{5}hello") || tunnel.contains("hello"),
        "{tunnel}"
    );

    // WebSocket as a plain-proxy request: the service receives the origin-form handshake
    // without the proxy credential.
    let plain = format!(
        "GET http://api.svc:9000/ws?x=1 HTTP/1.1\r\nHost: api.svc:9000\r\nConnection: Upgrade\r\n\
         Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Proxy-Authorization: Bearer {token}\r\n\r\n"
    );
    let upgraded = exchange(address, &plain, frame).await;
    assert!(
        upgraded.starts_with("HTTP/1.1 101 Switching Protocols"),
        "{upgraded}"
    );
    assert!(upgraded.contains("hello"), "{upgraded}");
    let handshake = seen.lock().unwrap().last().cloned().unwrap();
    assert!(
        handshake.starts_with("GET /ws?x=1 HTTP/1.1\r\n"),
        "{handshake}"
    );
    assert!(
        !handshake
            .to_ascii_lowercase()
            .contains("proxy-authorization"),
        "{handshake}"
    );

    // A non-HTTP protocol through a CONNECT tunnel.
    let ping = exchange(address, &connect("api.svc:9000", &token), redis).await;
    assert!(
        ping.starts_with("HTTP/1.1 200 Connection Established"),
        "{ping}"
    );
    assert!(ping.contains("PING"), "{ping}");

    // A link tunnel left quiet past the idle limit and the lifetime of other connections stays
    // open: the service still answers.
    {
        use tokio::io::AsyncWriteExt;
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket
            .write_all(connect("api.svc:9000", &token).as_bytes())
            .await
            .unwrap();
        let mut received = Vec::new();
        read_window(&mut socket, &mut received).await;
        assert!(
            String::from_utf8_lossy(&received).starts_with("HTTP/1.1 200 Connection Established")
        );
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        socket.write_all(redis).await.unwrap();
        read_window(&mut socket, &mut received).await;
        assert!(
            String::from_utf8_lossy(&received).contains("PING"),
            "{}",
            String::from_utf8_lossy(&received)
        );
    }

    // A service that answers the WebSocket handshake with an ordinary error and keeps the
    // connection open: the sandbox gets that answer with `Connection: close`, the connection
    // ends, and a request pipelined after the handshake never reaches the service.
    let refusing = format!(
        "GET http://api.svc:9000/refuse HTTP/1.1\r\nHost: api.svc:9000\r\nConnection: Upgrade\r\n\
         Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Proxy-Authorization: Bearer {token}\r\n\r\n\
         GET http://api.svc:9000/pipelined HTTP/1.1\r\nHost: api.svc:9000\r\n\
         Proxy-Authorization: Bearer {token}\r\n\r\n"
    );
    let answered = exchange(
        address,
        &refusing,
        b"GET /after HTTP/1.1\r\nHost: api.svc:9000\r\n\r\n",
    )
    .await;
    assert!(
        answered.starts_with("HTTP/1.1 400 Bad Request"),
        "{answered}"
    );
    assert!(
        answered.to_ascii_lowercase().contains("connection: close"),
        "{answered}"
    );
    assert!(!answered.contains("101"), "{answered}");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let received = seen.lock().unwrap().clone();
    let refuse_at = received
        .iter()
        .position(|first| first.starts_with("GET /refuse HTTP/1.1\r\n"))
        .unwrap_or_else(|| panic!("{received:?}"));
    assert!(
        !received[refuse_at..]
            .iter()
            .any(|bytes| bytes.contains("/pipelined") || bytes.contains("/after")),
        "{received:?}"
    );

    // A malformed WebSocket handshake to the link gets a refusal, not a closed connection.
    let malformed = format!(
        "GET http://api.svc:9000/ws HTTP/1.1\r\nHost: api.svc:9000\r\nConnection: Upgrade\r\n\
         Upgrade: websocket\r\nContent-Length: 0\r\nProxy-Authorization: Bearer {token}\r\n\r\n"
    );
    let refused = exchange(address, &malformed, b"").await;
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");

    // A link with a credential rule carries no plaintext WebSocket or other protocol, through
    // a CONNECT tunnel or a plain-proxy request: the service never receives the bytes.
    for (head, payload) in [
        (
            connect("vendor.svc:9002", &token),
            websocket("vendor.svc:9002").into_bytes(),
        ),
        (connect("vendor.svc:9002", &token), redis.to_vec()),
        (
            format!(
                "GET http://vendor.svc:9002/ws HTTP/1.1\r\nHost: vendor.svc:9002\r\nConnection: Upgrade\r\n\
                 Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                 Proxy-Authorization: Bearer {token}\r\n\r\n"
            ),
            frame.to_vec(),
        ),
    ] {
        let answer = exchange(address, &head, &payload).await;
        assert!(!answer.contains("101 Switching"), "{answer}");
        assert!(!answer.contains("hello") && !answer.contains("PING"), "{answer}");
    }
    assert!(
        credentialed
            .lock()
            .unwrap()
            .iter()
            .all(|first| first.is_empty()),
        "a credential link received plaintext: {:?}",
        credentialed.lock().unwrap()
    );

    // None of the three reaches a service that is not the link, nor the link's address on an
    // undeclared port, nor the link under a policy that does not keep it.
    let before = (seen.lock().unwrap().len(), other.lock().unwrap().len());
    let deny_all = proxy_token(&key, AUD, "deny-all");
    for (authority, token) in [
        ("other.svc:9001", token.as_str()),
        ("api.svc:9001", token.as_str()),
        ("10.244.7.9:9001", token.as_str()),
        ("api.svc:9000", deny_all.as_str()),
    ] {
        for (head, payload) in [
            // The handshake follows only once a tunnel is acknowledged.
            (connect(authority, token), websocket(authority).into_bytes()),
            (connect(authority, token), redis.to_vec()),
            (
                format!(
                    "GET http://{authority}/ws HTTP/1.1\r\nHost: {authority}\r\nConnection: Upgrade\r\n\
                     Upgrade: websocket\r\nProxy-Authorization: Bearer {token}\r\n\r\n"
                ),
                frame.to_vec(),
            ),
        ] {
            // A refusal is a 403, or a closed connection where a plain-proxy request finds
            // no destination.
            let answer = exchange(address, &head, &payload).await;
            assert!(
                answer.is_empty() || answer.starts_with("HTTP/1.1 403"),
                "{authority}: {answer}"
            );
            assert!(!answer.contains("hello") && !answer.contains("PING"), "{authority}: {answer}");
        }
    }
    assert_eq!(
        (seen.lock().unwrap().len(), other.lock().unwrap().len()),
        before,
        "a refused request reached a service"
    );
}
