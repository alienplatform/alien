//! Three real gateway processes move from an embedded replica secret to a sourced one while
//! traffic runs. Each process reads its configuration from its own vault or file, refreshes
//! every second, and is moved independently, so every mix of moved and unmoved replicas occurs.
#![cfg(feature = "local")]

use std::{
    collections::HashMap,
    net::SocketAddr,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use alien_bindings::Bindings;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_compact::{KeyPair, Seed};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::{Child, Command},
    sync::watch,
};

const AUD: &str = "transition/box";
const CONFIG: &str = "sandbox-egress-gateway-config";
const SOURCE_KEY: &str = "sandbox-egress-replica-secret";
const ALLOWED: &str = "one.one.one.one:443";
const UNLISTED: &str = "example.com:443";

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

fn record(version: u64) -> String {
    signed(
        "alien-egress-record",
        json!({"v":1,"aud":AUD,"sub":"session","ver":[version,format!("{:032x}", 1)],
            "policy":{"allow":[ALLOWED]}}),
    )
}

fn identity() -> String {
    let now = millis() / 1000;
    signed(
        "JWT",
        json!({"iss":"alien-sandbox","aud":AUD,"sub":"session","iat":now,"exp":now+600}),
    )
}

fn admin_token(method: &str) -> String {
    let now = millis() / 1000;
    signed(
        "alien-egress-admin",
        json!({"iss":"alien-sandbox","aud":AUD,"sub":"session","method":method,"iat":now,"exp":now+60}),
    )
}

/// Whether a replica's configuration still embeds the old secret or names the new source.
#[derive(Clone, Copy)]
enum Secret {
    Embedded,
    Sourced,
}

/// How a replica receives its configuration.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Delivery {
    /// Its vault, refreshed (AWS).
    Vault,
    /// Fixed in its environment at start, never refreshed (the earlier Kubernetes pods).
    Static,
    /// A mounted file that someone else updates in place, refreshed (Kubernetes pods that
    /// mount the stored configuration and the replica secret).
    File,
}

struct Replica {
    vault_dir: tempfile::TempDir,
    proxy: SocketAddr,
    admin: SocketAddr,
    delivery: Delivery,
    /// The configuration a `Static` replica gets at its next start.
    static_config: Option<String>,
    child: Option<Child>,
}

impl Replica {
    fn config_file(&self) -> std::path::PathBuf {
        self.vault_dir.path().join("config.json")
    }

    fn secret_file(&self) -> std::path::PathBuf {
        self.vault_dir.path().join("replica-secret")
    }
}

struct Cluster {
    replicas: Vec<Replica>,
    client: reqwest::Client,
}

impl Drop for Cluster {
    fn drop(&mut self) {
        for replica in &mut self.replicas {
            if let Some(child) = replica.child.as_mut() {
                let _ = child.start_kill();
            }
        }
    }
}

fn vault_env(dir: &std::path::Path) -> HashMap<String, String> {
    HashMap::from([
        ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
        (
            "ALIEN_SECRETS_BINDING".into(),
            json!({"service":"local-vault","dataDir":dir.join("vault"),"vaultName":"secrets"})
                .to_string(),
        ),
    ])
}

impl Cluster {
    async fn new() -> Self {
        Self::delivered(Delivery::Vault).await
    }

    /// Three ready replicas that each embed the old secret, receiving it by `delivery`.
    async fn delivered(delivery: Delivery) -> Self {
        let mut reserved = Vec::new();
        for _ in 0..6 {
            reserved.push(std::net::TcpListener::bind("127.0.0.1:0").unwrap());
        }
        let addresses: Vec<SocketAddr> = reserved.iter().map(|l| l.local_addr().unwrap()).collect();
        drop(reserved);
        let replicas = (0..3)
            .map(|index| Replica {
                vault_dir: tempfile::tempdir().unwrap(),
                proxy: addresses[index],
                admin: addresses[index + 3],
                delivery,
                static_config: None,
                child: None,
            })
            .collect();
        let mut cluster = Self {
            replicas,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap(),
        };
        for index in 0..3 {
            // The same sourced value in each replica's vault and in its mounted secret file.
            let vault = cluster.vault(index).await;
            vault
                .set_secret(SOURCE_KEY, &STANDARD.encode([22u8; 32]))
                .await
                .unwrap();
            std::fs::write(
                cluster.replicas[index].secret_file(),
                STANDARD.encode([22u8; 32]),
            )
            .unwrap();
            cluster.publish(index, Secret::Embedded).await;
            cluster.start(index);
        }
        cluster.ready(&[0, 1, 2], Duration::from_secs(15)).await;
        cluster
    }

    async fn vault(&self, index: usize) -> Arc<dyn alien_bindings::Vault> {
        Bindings::from_env_map(vault_env(self.replicas[index].vault_dir.path()))
            .unwrap()
            .vault("secrets")
            .await
            .unwrap()
    }

    /// What the controller publishes: the old form embeds the secret, the new one names its
    /// source and carries no secret. It reaches the replica the way that replica reads it.
    async fn publish(&mut self, index: usize, secret: Secret) {
        let peers: Vec<String> = (0..3)
            .filter(|other| *other != index)
            .map(|other| self.replicas[other].admin.to_string())
            .collect();
        let key = KeyPair::from_seed(Seed::new([29; 32]));
        let mut config = json!({"listen":self.replicas[index].proxy,"admin":self.replicas[index].admin,
            "audiences":[AUD],"keys":[{"kid":"test","publicKey":STANDARD.encode(key.pk.as_ref())}],
            "deniedRanges":[],"credentials":[],"peers":{"static":peers},"policyMemoryBytes":67108864});
        match secret {
            Secret::Embedded => config["replicaSecret"] = STANDARD.encode([11u8; 32]).into(),
            Secret::Sourced if self.replicas[index].delivery == Delivery::Vault => {
                config["replicaSecretSource"] =
                    json!({"vault":{"binding":"secrets","key":SOURCE_KEY}})
            }
            Secret::Sourced => {
                config["replicaSecretSource"] =
                    json!({"file":{"path":self.replicas[index].secret_file()}})
            }
        }
        match self.replicas[index].delivery {
            Delivery::Vault => self
                .vault(index)
                .await
                .set_secret(CONFIG, &config.to_string())
                .await
                .unwrap(),
            Delivery::Static => self.replicas[index].static_config = Some(config.to_string()),
            // Replaced whole, as the kubelet swaps a mounted Secret's files.
            Delivery::File => {
                let staged = self.replicas[index]
                    .vault_dir
                    .path()
                    .join("config.json.new");
                std::fs::write(&staged, config.to_string()).unwrap();
                std::fs::rename(staged, self.replicas[index].config_file()).unwrap();
            }
        }
    }

    fn start(&mut self, index: usize) {
        assert!(self.replicas[index].child.is_none());
        let mut command = Command::new(env!("CARGO_BIN_EXE_alien-egress-gateway"));
        command
            .kill_on_drop(true)
            .envs(vault_env(self.replicas[index].vault_dir.path()));
        match self.replicas[index].delivery {
            Delivery::Vault => {}
            Delivery::Static => {
                command.env(
                    "ALIEN_EGRESS_GATEWAY_CONFIG",
                    self.replicas[index].static_config.as_ref().unwrap(),
                );
            }
            Delivery::File => {
                command.env(
                    "ALIEN_EGRESS_GATEWAY_CONFIG_FILE",
                    self.replicas[index].config_file(),
                );
            }
        }
        command
            .env("ALIEN_EGRESS_GATEWAY_REFRESH_SECONDS", "1")
            .env("RUST_LOG", "error")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        self.replicas[index].child = Some(command.spawn().unwrap());
    }

    async fn restart(&mut self, index: usize) {
        self.stop(index).await;
        self.start(index);
    }

    async fn stop(&mut self, index: usize) {
        let mut child = self.replicas[index].child.take().unwrap();
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }

    /// Replaces a replica's process with one that receives `secret` by `delivery`, as a rolling
    /// update replaces a pod, and waits until it is ready.
    async fn roll(&mut self, index: usize, delivery: Delivery, secret: Secret) {
        self.stop(index).await;
        self.replicas[index].delivery = delivery;
        self.publish(index, secret).await;
        self.start(index);
        self.ready(&[index], Duration::from_secs(15)).await;
    }

    /// Whether the original process still runs: a refresh never restarts a replica.
    fn still_running(&mut self, index: usize) -> bool {
        self.replicas[index]
            .child
            .as_mut()
            .is_some_and(|child| child.try_wait().unwrap().is_none())
    }

    async fn healthy(&self, index: usize) -> bool {
        self.client
            .get(format!("http://{}/health", self.replicas[index].admin))
            .send()
            .await
            .is_ok_and(|response| response.status() == 200)
    }

    /// Waits until a replica's admin listener answers, ready or not.
    async fn listening(&self, index: usize) {
        let start = Instant::now();
        while self
            .client
            .get(format!("http://{}/health", self.replicas[index].admin))
            .send()
            .await
            .is_err()
        {
            assert!(start.elapsed() < Duration::from_secs(10), "replica {index} never listened");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn ready(&self, indexes: &[usize], within: Duration) -> Duration {
        let start = Instant::now();
        loop {
            let mut all = true;
            for index in indexes {
                all &= self.healthy(*index).await;
            }
            if all {
                return start.elapsed();
            }
            assert!(start.elapsed() < within, "replicas {indexes:?} not ready");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// A policy write at one replica, acknowledged only when every peer stored it.
    async fn write(&self, index: usize, record: &str) -> u16 {
        self.client
            .put(format!("http://{}/v1/policies", self.replicas[index].admin))
            .bearer_auth(admin_token("PUT"))
            .json(&json!({ "record": record }))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// Every replica accepts a write at every replica.
    async fn replicates_everywhere(&self) {
        for index in 0..3 {
            assert_eq!(
                self.write(index, &record(millis())).await,
                204,
                "a write at replica {index} must reach every peer"
            );
        }
    }
}

async fn connect(proxy: SocketAddr, token: &str, target: &str) -> u16 {
    let mut connection = TcpStream::connect(proxy).await.unwrap();
    connection
        .write_all(
            format!(
                "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Authorization: Bearer {token}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
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

/// Waits past at least two refreshes of every replica.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(2500)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replicas_move_to_a_sourced_secret_without_restarts_or_lost_replication() {
    let mut cluster = Cluster::new().await;
    let token = identity();
    assert_eq!(cluster.write(0, &record(millis())).await, 204);

    // Sandbox traffic on the two replicas that are never restarted, for the whole transition:
    // the listed host is always allowed and an unlisted one is never allowed.
    let (stop, stopped) = watch::channel(false);
    let unlisted_allowed = Arc::new(AtomicU64::new(0));
    let mut traffic = tokio::task::JoinSet::new();
    for proxy in [cluster.replicas[0].proxy, cluster.replicas[1].proxy] {
        let token = token.clone();
        let stopped = stopped.clone();
        let unlisted_allowed = unlisted_allowed.clone();
        traffic.spawn(async move {
            let mut allowed = 0u64;
            while !*stopped.borrow() {
                assert_eq!(connect(proxy, &token, ALLOWED).await, 200, "listed host refused");
                allowed += 1;
                if connect(proxy, &token, UNLISTED).await != 403 {
                    unlisted_allowed.fetch_add(1, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            allowed
        });
    }

    // Mixed: one replica moved, two not. Each verifies the others in both directions.
    cluster.publish(0, Secret::Sourced).await;
    settle().await;
    cluster.replicates_everywhere().await;

    // A replica restarting mid-transition boots knowing only the new secret, while replica 1
    // still signs with the old one alone. Replica 0 holds both and replicates with everyone;
    // replica 1's writes cannot reach the restarted replica until replica 1 refreshes.
    cluster.publish(2, Secret::Sourced).await;
    cluster.restart(2).await;
    cluster.listening(2).await;
    assert_eq!(cluster.write(0, &record(millis())).await, 204);
    assert_ne!(cluster.write(1, &record(millis())).await, 204);
    let moved = Instant::now();
    cluster.publish(1, Secret::Sourced).await;
    let ready = cluster.ready(&[2], Duration::from_secs(10)).await;
    loop {
        if cluster.write(1, &record(millis())).await == 204 {
            break;
        }
        assert!(
            moved.elapsed() < Duration::from_secs(5),
            "replica 1 must reach the restarted replica within one refresh"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!(
        "restarted replica: ready {ready:?} and reachable from replica 1 {:?} after replica 1 moved",
        moved.elapsed()
    );

    // Every replica moved: replication works everywhere, in any write order.
    settle().await;
    cluster.replicates_everywhere().await;
    for index in [2, 1, 0] {
        assert_eq!(cluster.write(index, &record(millis())).await, 204);
    }
    assert_eq!(connect(cluster.replicas[2].proxy, &token, ALLOWED).await, 200);
    assert_eq!(connect(cluster.replicas[2].proxy, &token, UNLISTED).await, 403);

    stop.send(true).unwrap();
    while let Some(allowed) = traffic.join_next().await {
        assert!(allowed.unwrap() > 0);
    }
    assert_eq!(unlisted_allowed.load(Ordering::Relaxed), 0, "unlisted host allowed");
    assert!(cluster.still_running(0) && cluster.still_running(1), "a refresh restarted a replica");
}

/// A source whose value changes under a running replica is a second rotation; the replica
/// keeps its last good configuration rather than accept a secret its peers may not know.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_changed_source_value_is_not_taken_by_a_running_replica() {
    let mut cluster = Cluster::new().await;
    for index in 0..3 {
        cluster.publish(index, Secret::Sourced).await;
    }
    settle().await;
    cluster.replicates_everywhere().await;
    for index in 0..3 {
        cluster
            .vault(index)
            .await
            .set_secret(SOURCE_KEY, &STANDARD.encode([33u8; 32]))
            .await
            .unwrap();
    }
    settle().await;
    cluster.replicates_everywhere().await;
}

/// The Kubernetes move. The running pods hold a static configuration that embeds the secret.
/// A rolling update replaces them, one at a time, with pods that mount the stored
/// configuration (still embedding the secret) and the replica secret as files and refresh
/// them. Only then is the configuration rewritten to name the mounted secret, file by file as
/// each kubelet updates its mount. Every pod joins and stays ready throughout, replication
/// never fails, refreshes restart nothing, and a pod that later restarts on the new secret alone
/// is ready at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rolling_update_moves_static_replicas_to_a_mounted_secret_without_lost_replication() {
    let mut cluster = Cluster::delivered(Delivery::Static).await;
    let token = identity();
    assert_eq!(cluster.write(0, &record(millis())).await, 204);

    // A new pod catches up from pods that know only the embedded secret, and the pods it
    // replaces in turn catch up from it.
    for index in [2, 1, 0] {
        cluster.roll(index, Delivery::File, Secret::Embedded).await;
        cluster.replicates_everywhere().await;
        assert_eq!(
            connect(cluster.replicas[index].proxy, &token, ALLOWED).await,
            200
        );
        assert_eq!(
            connect(cluster.replicas[index].proxy, &token, UNLISTED).await,
            403
        );
    }

    // Traffic on two pods for the rest of the move.
    let (stop, stopped) = watch::channel(false);
    let unlisted_allowed = Arc::new(AtomicU64::new(0));
    let mut traffic = tokio::task::JoinSet::new();
    for proxy in [cluster.replicas[1].proxy, cluster.replicas[2].proxy] {
        let token = token.clone();
        let stopped = stopped.clone();
        let unlisted_allowed = unlisted_allowed.clone();
        traffic.spawn(async move {
            let mut allowed = 0u64;
            while !*stopped.borrow() {
                assert_eq!(
                    connect(proxy, &token, ALLOWED).await,
                    200,
                    "listed host refused"
                );
                allowed += 1;
                if connect(proxy, &token, UNLISTED).await != 403 {
                    unlisted_allowed.fetch_add(1, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            allowed
        });
    }

    // The source-only configuration reaches each pod's mount at a different time.
    for index in [0, 1, 2] {
        cluster.publish(index, Secret::Sourced).await;
        settle().await;
        cluster.replicates_everywhere().await;
    }

    // A pod that restarts now knows only the mounted secret; its peers sign with both.
    cluster.restart(0).await;
    cluster.ready(&[0], Duration::from_secs(15)).await;
    cluster.replicates_everywhere().await;
    for index in [0, 2, 1] {
        assert_eq!(cluster.write(index, &record(millis())).await, 204);
    }

    stop.send(true).unwrap();
    while let Some(allowed) = traffic.join_next().await {
        assert!(allowed.unwrap() > 0);
    }
    assert_eq!(
        unlisted_allowed.load(Ordering::Relaxed),
        0,
        "unlisted host allowed"
    );
    assert!(
        cluster.still_running(1) && cluster.still_running(2),
        "a refresh restarted a replica"
    );
}
