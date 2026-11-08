//! Runs a real `valkey-server` through the local manager.
//!
//! Needs a release directory (`v<version>/<os>-<arch>/valkey-server.tar.gz` plus a
//! `.sha256` file) reachable through `ALIEN_VALKEY_RELEASES_URL`, for example a
//! `file://` path holding a locally built binary:
//!
//! ```text
//! ALIEN_VALKEY_RELEASES_URL=file:///tmp/valkey-rel \
//!   cargo test -p alien-local --test redis_integration -- --ignored
//! ```

use alien_core::bindings::RedisBinding;
use alien_local::{LocalRedisManager, RedisSpec};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Sends one command over a fresh connection (AUTH first) and returns the raw reply.
async fn command(binding: &RedisBinding, args: &[&str]) -> String {
    let RedisBinding::Local(local) = binding else {
        panic!("expected a local binding");
    };
    let port = match &local.port {
        alien_core::bindings::BindingValue::Value(port) => *port,
        other => panic!("unexpected port {other:?}"),
    };
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let encode = |args: &[&str]| {
        let mut out = format!("*{}\r\n", args.len());
        for arg in args {
            out.push_str(&format!("${}\r\n{}\r\n", arg.len(), arg));
        }
        out
    };
    let auth = encode(&["AUTH", "default", &local.password]);
    stream.write_all(auth.as_bytes()).await.unwrap();
    stream.write_all(encode(args).as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let n = stream.read(&mut buf).await.unwrap();
    let reply = String::from_utf8_lossy(&buf[..n]).to_string();
    // Drop the "+OK" for AUTH.
    reply.strip_prefix("+OK\r\n").unwrap_or(&reply).to_string()
}

fn spec(persistence: bool, policy: &str) -> RedisSpec {
    RedisSpec {
        persistence,
        eviction_policy: policy.to_string(),
        max_memory_bytes: Some(64 * 1024 * 1024),
    }
}

#[tokio::test]
#[ignore = "needs ALIEN_VALKEY_RELEASES_URL pointing at a Valkey release directory"]
async fn data_survives_a_restart_when_persistence_is_on_and_rejects_bad_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());

    manager.start_redis("cache", spec(true, "volatile-lru")).await.unwrap();
    manager.check_health("cache").await.unwrap();
    let binding = manager.get_binding("cache").unwrap();

    assert_eq!(command(&binding, &["SET", "k", "v"]).await, "+OK\r\n");
    assert_eq!(command(&binding, &["CONFIG", "GET", "maxmemory-policy"]).await.contains("volatile-lru"), true);

    // A wrong password is refused.
    let RedisBinding::Local(local) = &binding else { unreachable!() };
    assert!(local.password.len() >= 32);
    let wrong = RedisBinding::Local(alien_core::bindings::LocalRedisBinding {
        password: "wrong".to_string(),
        ..local.clone()
    });
    assert!(command(&wrong, &["GET", "k"]).await.contains("WRONGPASS"));

    // Stop and start again: same port and password, key still there from the AOF.
    manager.stop_redis("cache").await.unwrap();
    manager.start_redis("cache", spec(true, "volatile-lru")).await.unwrap();
    assert_eq!(manager.get_binding("cache").unwrap(), binding);
    assert_eq!(command(&binding, &["GET", "k"]).await, "$1\r\nv\r\n");

    // Changing the eviction policy restarts the server with it; data is kept.
    manager.start_redis("cache", spec(true, "noeviction")).await.unwrap();
    assert!(command(&binding, &["CONFIG", "GET", "maxmemory-policy"]).await.contains("noeviction"));
    assert_eq!(command(&binding, &["GET", "k"]).await, "$1\r\nv\r\n");

    manager.delete_redis("cache").await.unwrap();
    assert!(manager.try_get_binding("cache").unwrap().is_none());
}

#[tokio::test]
#[ignore = "needs ALIEN_VALKEY_RELEASES_URL pointing at a Valkey release directory"]
async fn data_is_lost_on_restart_when_persistence_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());

    manager.start_redis("cache", spec(false, "allkeys-lru")).await.unwrap();
    let binding = manager.get_binding("cache").unwrap();
    assert_eq!(command(&binding, &["SET", "k", "v"]).await, "+OK\r\n");

    manager.stop_redis("cache").await.unwrap();
    manager.start_redis("cache", spec(false, "allkeys-lru")).await.unwrap();
    assert_eq!(command(&binding, &["GET", "k"]).await, "$-1\r\n");
    manager.delete_redis("cache").await.unwrap();
}
