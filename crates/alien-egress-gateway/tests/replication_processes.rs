//! Three real gateway processes, with independently controlled directed peer links.
//! Enforcement is observed only through actual CONNECT requests, never the store.
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_compact::{KeyPair, Seed};
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    sync::watch,
    task::JoinHandle,
};

const AUD: &str = "replication/box";
const SUB: &str = "session";
fn millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn signed(kind: &str, claims: Value) -> String {
    let key = KeyPair::from_seed(Seed::new([29; 32]));
    let message = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(json!({"alg":"EdDSA","kid":"test","typ":kind}).to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let signature = key.sk.sign(message.as_bytes(), None);
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()))
}
fn record(version: u64, writer: u8, policy: Option<Value>) -> String {
    let mut claims = json!({"v":1,"aud":AUD,"sub":SUB,"ver":[version,format!("{writer:032x}")]});
    match policy {
        Some(policy) => claims["policy"] = policy,
        None => claims["deleted"] = true.into(),
    }
    signed("alien-egress-record", claims)
}
fn identity() -> String {
    let now = millis() / 1000;
    signed(
        "JWT",
        json!({"iss":"alien-sandbox","aud":AUD,"sub":SUB,"iat":now,"exp":now+300}),
    )
}
fn admin(method: &str) -> String {
    let now = millis() / 1000;
    signed(
        "alien-egress-admin",
        json!({"iss":"alien-sandbox","aud":AUD,"sub":SUB,"method":method,"iat":now,"exp":now+60}),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    Heal,
    Drop,
    Delay,
    Cut,
}
struct Link {
    address: SocketAddr,
    control: watch::Sender<Fault>,
    task: JoinHandle<()>,
}
impl Drop for Link {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Link {
    async fn new(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (control, state) = watch::channel(Fault::Heal);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut inbound, _) = accepted.unwrap();
                        let mut state = state.clone();
                        connections.spawn(async move {
                            let fault = *state.borrow_and_update();
                            if matches!(fault, Fault::Drop | Fault::Cut) { return; }
                            if fault == Fault::Delay {
                                tokio::select! { _ = tokio::time::sleep(Duration::from_millis(250)) => {}, _ = state.changed() => {} }
                            }
                            let Ok(mut outbound) = TcpStream::connect(target).await else { return; };
                            loop {
                                tokio::select! {
                                    _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => return,
                                    changed = state.changed() => {
                                        if changed.is_err() || matches!(*state.borrow_and_update(), Fault::Cut | Fault::Drop) { return; }
                                    }
                                }
                            }
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Self {
            address,
            control,
            task,
        }
    }
    fn set(&self, fault: Fault) {
        self.control.send(fault).unwrap();
    }
}
struct Cluster {
    proxy: [SocketAddr; 3],
    admin: [SocketAddr; 3],
    links: Vec<(usize, usize, Link)>,
    children: [Option<Child>; 3],
    client: reqwest::Client,
}
impl Drop for Cluster {
    fn drop(&mut self) {
        for child in self.children.iter_mut().flatten() {
            let _ = child.start_kill();
        }
    }
}
impl Cluster {
    async fn new() -> Self {
        let mut reserved = Vec::new();
        for _ in 0..6 {
            reserved.push(std::net::TcpListener::bind("127.0.0.1:0").unwrap());
        }
        let proxy = std::array::from_fn(|i| reserved[i].local_addr().unwrap());
        let admin = std::array::from_fn(|i| reserved[i + 3].local_addr().unwrap());
        let mut links = Vec::new();
        for from in 0..3 {
            for (to, target) in admin.iter().enumerate() {
                if from != to {
                    links.push((from, to, Link::new(*target).await));
                }
            }
        }
        drop(reserved);
        let mut cluster = Self {
            proxy,
            admin,
            links,
            children: [None, None, None],
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap(),
        };
        for i in 0..3 {
            cluster.start(i);
        }
        cluster.ready_all().await;
        cluster
    }
    fn start(&mut self, index: usize) {
        assert!(self.children[index].is_none());
        let peers: Vec<String> = self
            .links
            .iter()
            .filter(|(from, _, _)| *from == index)
            .map(|(_, _, link)| link.address.to_string())
            .collect();
        let key = KeyPair::from_seed(Seed::new([29; 32]));
        let config = json!({"listen":self.proxy[index],"admin":self.admin[index],"audiences":[AUD],"keys":[{"kid":"test","publicKey":STANDARD.encode(key.pk.as_ref())}],"deniedRanges":[],"credentials":[],"peers":{"static":peers},"replicaSecret":STANDARD.encode([13;32]),"policyMemoryBytes":67108864});
        self.children[index] = Some(
            Command::new(env!("CARGO_BIN_EXE_alien-egress-gateway"))
                .kill_on_drop(true)
                .env("ALIEN_EGRESS_GATEWAY_CONFIG", config.to_string())
                .env("RUST_LOG", "error")
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
    }
    async fn stop(&mut self, index: usize) {
        let mut child = self.children[index].take().unwrap();
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
    async fn health(&self, index: usize) -> bool {
        self.client
            .get(format!("http://{}/health", self.admin[index]))
            .send()
            .await
            .is_ok_and(|r| r.status() == 200)
    }
    async fn ready_all(&self) {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if self.health(0).await && self.health(1).await && self.health(2).await {
                return;
            }
            assert!(Instant::now() < deadline, "replicas never became ready");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
    fn isolate(&self, index: usize, fault: Fault) {
        for (from, to, link) in &self.links {
            if *from == index || *to == index {
                link.set(fault);
            }
        }
    }
    async fn write(&self, index: usize, record: &str, deleted: bool) -> u16 {
        let method = if deleted {
            reqwest::Method::DELETE
        } else {
            reqwest::Method::PUT
        };
        self.client
            .request(
                method.clone(),
                format!("http://{}/v1/policies", self.admin[index]),
            )
            .bearer_auth(admin(method.as_str()))
            .json(&json!({"record":record}))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }
    async fn connect(&self, index: usize, token: &str) -> u16 {
        connect_status(self.proxy[index], token).await
    }
    async fn assert_all(&self, token: &str, status: u16) {
        for index in 0..3 {
            assert_eq!(self.connect(index, token).await, status, "replica {index}");
        }
    }
    async fn register(&self, index: usize, token: &str, record: &str) -> u16 {
        self.client
            .put(format!("http://{}/v1/policies/register", self.proxy[index]))
            .header("Proxy-Authorization", format!("Bearer {token}"))
            .json(&json!({"record":record}))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }
}

async fn connect_status(proxy: SocketAddr, token: &str) -> u16 {
    let mut connection = TcpStream::connect(proxy).await.unwrap();
    connection.write_all(format!("CONNECT one.one.one.one:443 HTTP/1.1\r\nHost: one.one.one.one:443\r\nProxy-Authorization: Bearer {token}\r\n\r\n").as_bytes()).await.unwrap();
    let mut status = String::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        BufReader::new(connection).read_line(&mut status),
    )
    .await
    .unwrap()
    .unwrap();
    status
        .split_whitespace()
        .nth(1)
        .expect("proxy HTTP status")
        .parse()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_process_replication_survives_faults_and_exposes_total_loss_replay() {
    let start = Instant::now();
    let mut cluster = Cluster::new().await;
    let token = identity();
    cluster.assert_all(&token, 403).await;
    let base = millis();
    let allow = record(base, 1, Some(json!("internet")));
    assert_eq!(cluster.write(0, &allow, false).await, 204);
    cluster.assert_all(&token, 200).await;

    let (stop_traffic, traffic_done) = watch::channel(false);
    let traffic_started = Arc::new(tokio::sync::Barrier::new(3));
    let mut traffic = tokio::task::JoinSet::new();
    for proxy in &cluster.proxy[..2] {
        let proxy = *proxy;
        let token = token.clone();
        let traffic_started = traffic_started.clone();
        let traffic_done = traffic_done.clone();
        traffic.spawn(async move {
            assert_eq!(connect_status(proxy, &token).await, 200);
            traffic_started.wait().await;
            let (mut allowed, mut denied) = (1, 0);
            loop {
                let stopping = *traffic_done.borrow();
                let status = connect_status(proxy, &token).await;
                if stopping {
                    assert_eq!(
                        status, 403,
                        "final steady request must observe the synchronized denial"
                    );
                }
                match status {
                    200 => {
                        assert_eq!(denied, 0, "allowed after observing the newer denial");
                        allowed += 1;
                    }
                    403 => denied += 1,
                    status => panic!("steady traffic returned unexpected status {status}"),
                }
                if stopping {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            (allowed, denied)
        });
    }
    traffic_started.wait().await;
    // A stopped replica cannot acknowledge a write. Its replacement stays unready
    // while every listed peer is cut, then snapshots the write before serving.
    cluster.stop(2).await;
    let deny = record(base + 1, 1, Some(json!({"allow":[]})));
    assert_eq!(cluster.write(0, &deny, false).await, 503);
    cluster.isolate(2, Fault::Cut);
    cluster.start(2);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        !cluster.health(2).await,
        "listed unreachable peers must prevent readiness"
    );
    assert_eq!(cluster.connect(2, &token).await, 403);
    cluster.isolate(2, Fault::Heal);
    cluster.ready_all().await;
    assert_eq!(
        cluster.connect(2, &token).await,
        403,
        "replacement must snapshot the pending write before ready"
    );
    // The earlier 503 did not acknowledge every surviving peer. Follow the SDK
    // contract: retry this exact record after recovery, then require all peers.
    assert_eq!(cluster.write(0, &deny, false).await, 204);
    cluster.assert_all(&token, 403).await;

    stop_traffic.send(true).unwrap();
    while let Some(result) = traffic.join_next().await {
        let (allowed, denied) = result.unwrap();
        assert!(allowed > 0 && denied > 0);
        eprintln!("steady traffic during replica restart: {allowed} allowed, {denied} denied, no errors or allowed-after-deny");
    }

    // Equal timestamp races resolve by writer id, independent of arrival order.
    let older = record(base + 2, 1, Some(json!("internet")));
    let newer = record(base + 2, 2, Some(json!({"allow":[]})));
    let (a, b) = tokio::join!(
        cluster.write(0, &older, false),
        cluster.write(1, &newer, false)
    );
    assert_eq!((a, b), (204, 204));
    cluster.assert_all(&token, 403).await;
    assert_eq!(cluster.write(2, &older, false).await, 204);
    cluster.assert_all(&token, 403).await;
    let tombstone = record(base + 3, 2, None);
    let update = record(base + 3, 1, Some(json!("internet")));
    let (a, b) = tokio::join!(
        cluster.write(0, &update, false),
        cluster.write(1, &tombstone, true)
    );
    assert_eq!((a, b), (204, 204));
    cluster.assert_all(&token, 403).await;
    assert_eq!(cluster.register(0, &token, &allow).await, 409);
    cluster.assert_all(&token, 403).await;

    // Near-limit canonical policy becomes a >256 KiB signed snapshot.
    let mut targets: Vec<String> = (0..4095)
        .map(|i| format!("{i:04}.{}.example.com", "a".repeat(44)))
        .collect();
    targets.push("one.one.one.one:443".into());
    let large_policy = json!({"allow":targets});
    assert!((250 * 1024..=256 * 1024).contains(&large_policy.to_string().len()));
    let large = record(base + 4, 1, Some(large_policy));
    assert!(large.len() > 256 * 1024);
    for (_, _, link) in &cluster.links {
        link.set(Fault::Delay);
    }
    assert_eq!(cluster.write(0, &large, false).await, 204);
    for (_, _, link) in &cluster.links {
        link.set(Fault::Heal);
    }
    cluster.stop(2).await;
    cluster.start(2);
    cluster.ready_all().await;
    cluster.assert_all(&token, 200).await;
    let future = record(millis() + 61_000, 1, Some(json!({"allow":[]})));
    assert_eq!(cluster.write(0, &future, false).await, 400);
    cluster.assert_all(&token, 200).await;

    // A partition leaves a stale allow on one replica. Healing must catch it up
    // within the documented 30-second exchange interval (plus 5 seconds IO).
    cluster.isolate(2, Fault::Drop);
    let restrictive = record(base + 5, 1, Some(json!({"allow":[]})));
    assert_eq!(cluster.write(0, &restrictive, false).await, 503);
    assert_eq!(cluster.connect(0, &token).await, 403);
    assert_eq!(cluster.connect(2, &token).await, 200);
    let healed = Instant::now();
    cluster.isolate(2, Fault::Heal);
    loop {
        if cluster.connect(2, &token).await == 403 {
            break;
        }
        assert!(
            healed.elapsed() < Duration::from_secs(35),
            "partition did not converge within 35 seconds"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!("partition convergence: {:?}", healed.elapsed());
    cluster.assert_all(&token, 403).await;

    // Total loss has no remembered version. Agent-style registration restores
    // the latest record; an old record cannot replace it once it exists.
    for i in 0..3 {
        cluster.stop(i).await;
    }
    for i in 0..3 {
        cluster.start(i);
    }
    cluster.ready_all().await;
    cluster.assert_all(&token, 403).await;
    assert_eq!(cluster.register(0, &token, &restrictive).await, 204);
    cluster.assert_all(&token, 403).await;
    assert_eq!(cluster.register(1, &token, &allow).await, 409);
    cluster.assert_all(&token, 403).await;

    // Measure the acknowledged limitation: an older signed allow can win the
    // first registration after total loss. New agent registration then refuses;
    // explicit management publication is needed to end that replay window.
    for i in 0..3 {
        cluster.stop(i).await;
    }
    for i in 0..3 {
        cluster.start(i);
    }
    cluster.ready_all().await;
    let replay_start = Instant::now();
    assert_eq!(cluster.register(0, &token, &allow).await, 204);
    cluster.assert_all(&token, 200).await;
    assert_eq!(cluster.register(1, &token, &restrictive).await, 409);
    cluster.assert_all(&token, 200).await;
    assert_eq!(cluster.write(0, &restrictive, false).await, 204);
    cluster.assert_all(&token, 403).await;
    eprintln!("observed older-record replay window until explicit management repair: {:?}; without repair bounded only by live identity expiry (300 seconds in this test)",replay_start.elapsed());
    for i in 0..3 {
        cluster.stop(i).await;
    }
    assert!(
        start.elapsed() < Duration::from_secs(60),
        "CI replication budget exceeded: {:?}",
        start.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tombstones_expire_at_the_signed_timestamp_despite_staggered_snapshots() {
    let mut cluster = Cluster::new().await;
    let retention = (25 * 60 * 60 + 60) * 1000;
    let expires = millis() + 5000;
    let written = expires - retention;
    let deleted = record(written, 2, None);
    let older = record(written - 1, 1, Some(json!("internet")));
    let expired_token = signed(
        "JWT",
        json!({
            "iss":"alien-sandbox","aud":AUD,"sub":SUB,
            "iat":written/1000,"exp":written/1000+24*60*60
        }),
    );
    // A fresh management-issued identity represents explicit named-id reuse.
    // It is only a witness for absence: the original deleted identity is expired
    // throughout this test and must remain denied even if its old policy returns.
    let reused_identity = identity();
    cluster.isolate(2, Fault::Drop);
    assert_eq!(cluster.write(0, &deleted, true).await, 503);
    assert_eq!(cluster.write(1, &deleted, true).await, 503);
    cluster.assert_all(&expired_token, 403).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    cluster.isolate(2, Fault::Heal);
    cluster.stop(2).await;
    cluster.start(2);
    cluster.ready_all().await;
    assert!(
        millis() < expires,
        "fixture must snapshot before the expiry boundary"
    );
    for index in 0..3 {
        assert_eq!(
            cluster.register(index, &reused_identity, &older).await,
            409,
            "each replica must retain the not-yet-expired tombstone"
        );
    }
    cluster.assert_all(&reused_identity, 403).await;

    // Advance real wall time to the signed boundary, not the local receipt time.
    tokio::time::sleep(Duration::from_millis(expires.saturating_sub(millis()) + 20)).await;
    assert_eq!(
        cluster.register(0, &reused_identity, &older).await,
        204,
        "all peers must discard the tombstone by its signed deadline, including the late snapshot"
    );
    cluster.assert_all(&reused_identity, 200).await;
    cluster.assert_all(&expired_token, 403).await;
    // Staggered late deliveries cannot refresh deletion or replace the older
    // record that has legitimately filled the absent entry after expiration.
    for index in 0..3 {
        assert_eq!(cluster.write(index, &deleted, true).await, 204);
        cluster.assert_all(&reused_identity, 200).await;
        cluster.assert_all(&expired_token, 403).await;
    }
    for index in 0..3 {
        cluster.stop(index).await;
    }
}
