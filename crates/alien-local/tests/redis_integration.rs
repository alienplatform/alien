//! Runs the real Valkey image through the local Redis manager. Needs a Docker engine:
//!
//! ```text
//! cargo test -p alien-local --test redis_integration -- --ignored --test-threads=1
//! ```

use alien_core::bindings::{BindingValue, LocalRedisBinding, RedisBinding};
use alien_local::{LocalRedisManager, RedisSpec};
use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Sends one command over a fresh connection (AUTH first) and returns the raw reply.
async fn command(binding: &RedisBinding, args: &[&str]) -> String {
    let RedisBinding::Local(local) = binding else {
        panic!("expected a local binding");
    };
    let BindingValue::Value(port) = &local.port else {
        panic!("unexpected port {:?}", local.port);
    };
    let mut stream = TcpStream::connect(("127.0.0.1", *port)).await.unwrap();
    let encode = |args: &[&str]| {
        let mut out = format!("*{}\r\n", args.len());
        for arg in args {
            out.push_str(&format!("${}\r\n{}\r\n", arg.len(), arg));
        }
        out
    };
    let password = local.password.clone();
    stream
        .write_all(encode(&["AUTH", "default", &password]).as_bytes())
        .await
        .unwrap();
    stream.write_all(encode(args).as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 4096];
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let n = stream.read(&mut buf).await.unwrap();
    let reply = String::from_utf8_lossy(&buf[..n]).to_string();
    // Drop the "+OK" for AUTH.
    reply.strip_prefix("+OK\r\n").unwrap_or(&reply).to_string()
}

fn spec(persistence: bool, policy: &str, memory_mib: u64) -> RedisSpec {
    RedisSpec {
        persistence,
        eviction_policy: policy.to_string(),
        max_memory_bytes: Some(memory_mib * 1024 * 1024),
    }
}

/// The Docker engine the manager talks to, to check what it left behind.
fn docker() -> Docker {
    alien_local::connect_docker().expect("docker connection")
}

async fn containers_named(docker: &Docker, fragment: &str) -> usize {
    let list = docker
        .list_containers(Some(bollard::query_parameters::ListContainersOptions {
            all: true,
            filters: Some(std::collections::HashMap::from([(
                "name".to_string(),
                vec![fragment.to_string()],
            )])),
            ..Default::default()
        }))
        .await
        .unwrap();
    list.len()
}

async fn volumes_named(docker: &Docker, fragment: &str) -> usize {
    docker
        .list_volumes(Some(bollard::query_parameters::ListVolumesOptions {
            filters: Some(std::collections::HashMap::from([(
                "name".to_string(),
                vec![fragment.to_string()],
            )])),
        }))
        .await
        .unwrap()
        .volumes
        .unwrap_or_default()
        .len()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn persistent_server_keeps_data_across_restarts_and_setting_changes_then_deletes_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());
    let docker = docker();

    manager
        .start_redis("cache", spec(true, "volatile-lru", 64))
        .await
        .unwrap();
    manager.check_health("cache").await.unwrap();
    let binding = manager.get_binding("cache").unwrap();

    assert_eq!(command(&binding, &["SET", "k", "v"]).await, "+OK\r\n");
    assert!(command(&binding, &["CONFIG", "GET", "maxmemory-policy"])
        .await
        .contains("volatile-lru"));

    // A wrong password is refused.
    let RedisBinding::Local(local) = &binding else {
        unreachable!()
    };
    let wrong = RedisBinding::Local(LocalRedisBinding {
        password: "wrong".to_string(),
        ..local.clone()
    });
    assert!(command(&wrong, &["GET", "k"]).await.contains("WRONGPASS"));

    // Stop and start: same port and password, key still there from the append-only file.
    manager.stop_redis("cache").await.unwrap();
    assert!(manager.check_health("cache").await.is_err());
    manager
        .start_redis("cache", spec(true, "volatile-lru", 64))
        .await
        .unwrap();
    assert_eq!(manager.get_binding("cache").unwrap(), binding);
    assert_eq!(command(&binding, &["GET", "k"]).await, "$1\r\nv\r\n");

    // A new manager over the same state directory (an `alien dev` restart) finds the server
    // it left behind and keeps the data.
    let restarted = LocalRedisManager::new(dir.path().to_path_buf());
    restarted
        .start_redis("cache", spec(true, "volatile-lru", 64))
        .await
        .unwrap();
    assert_eq!(command(&binding, &["GET", "k"]).await, "$1\r\nv\r\n");

    // Changing the eviction policy and the memory recreates the container with them; the
    // data stays.
    manager
        .start_redis("cache", spec(true, "noeviction", 128))
        .await
        .unwrap();
    assert!(command(&binding, &["CONFIG", "GET", "maxmemory-policy"])
        .await
        .contains("noeviction"));
    assert!(command(&binding, &["CONFIG", "GET", "maxmemory"])
        .await
        .contains(&(128u64 * 1024 * 1024).to_string()));
    assert_eq!(command(&binding, &["GET", "k"]).await, "$1\r\nv\r\n");

    // Delete removes the container, the volume and the state.
    manager.delete_redis("cache").await.unwrap();
    assert!(manager.try_get_binding("cache").unwrap().is_none());
    assert_eq!(containers_named(&docker, "alien-redis-").await, 0);
    assert_eq!(volumes_named(&docker, "alien-redis-").await, 0);
    assert!(!dir.path().join("redis").join("cache").exists());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn data_is_lost_on_restart_when_persistence_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());

    manager
        .start_redis("cache", spec(false, "allkeys-lru", 64))
        .await
        .unwrap();
    let binding = manager.get_binding("cache").unwrap();
    assert_eq!(command(&binding, &["SET", "k", "v"]).await, "+OK\r\n");

    manager.stop_redis("cache").await.unwrap();
    manager
        .start_redis("cache", spec(false, "allkeys-lru", 64))
        .await
        .unwrap();
    assert_eq!(command(&binding, &["GET", "k"]).await, "$-1\r\n");

    // Turning persistence on starts empty and from then on keeps data.
    manager
        .start_redis("cache", spec(true, "allkeys-lru", 64))
        .await
        .unwrap();
    assert_eq!(command(&binding, &["SET", "k2", "v"]).await, "+OK\r\n");
    manager
        .start_redis("cache", spec(true, "noeviction", 64))
        .await
        .unwrap();
    assert_eq!(command(&binding, &["GET", "k2"]).await, "$1\r\nv\r\n");
    manager.delete_redis("cache").await.unwrap();
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn refuses_to_adopt_or_delete_a_container_with_the_same_name_that_is_not_ours() {
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());
    let docker = docker();

    manager
        .start_redis("cache", spec(false, "allkeys-lru", 64))
        .await
        .unwrap();
    // Find the manager's container, then replace it with a foreign one of the same name.
    let found = docker
        .list_containers(Some(bollard::query_parameters::ListContainersOptions {
            all: true,
            filters: Some(std::collections::HashMap::from([(
                "name".to_string(),
                vec!["alien-redis-".to_string()],
            )])),
            ..Default::default()
        }))
        .await
        .unwrap();
    let name = found[0].names.clone().unwrap()[0]
        .trim_start_matches('/')
        .to_string();
    docker
        .remove_container(
            &name,
            Some(bollard::query_parameters::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    docker
        .create_container(
            Some(bollard::query_parameters::CreateContainerOptions {
                name: Some(name.clone()),
                ..Default::default()
            }),
            bollard::models::ContainerCreateBody {
                image: Some(
                    "valkey/valkey:9.0@sha256:7e1deda01b64ece3167d370ab5be3452d29734feec22cbcbc69b3a64f6859d6d"
                        .to_string(),
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let error = manager
        .start_redis("cache", spec(false, "allkeys-lru", 64))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("not created by Alien"),
        "{error}"
    );
    assert!(manager.delete_redis("cache").await.is_err());
    // The foreign container is untouched.
    docker.inspect_container(&name, None).await.unwrap();

    docker
        .remove_container(
            &name,
            Some(bollard::query_parameters::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_server_that_fails_to_start_is_replaced_by_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());

    // The server refuses an unknown policy and exits during startup.
    let error = manager
        .start_redis("cache", spec(true, "not-a-policy", 64))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("exited during startup"),
        "{error}"
    );

    // Correcting the setting recreates the container and the server comes up.
    manager
        .start_redis("cache", spec(true, "volatile-lru", 64))
        .await
        .unwrap();
    let binding = manager.get_binding("cache").unwrap();
    assert_eq!(command(&binding, &["SET", "k", "v"]).await, "+OK\r\n");
    manager.delete_redis("cache").await.unwrap();
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_settings_change_keeps_every_acknowledged_write() {
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());
    manager
        .start_redis("cache", spec(true, "volatile-lru", 64))
        .await
        .unwrap();
    let binding = manager.get_binding("cache").unwrap();
    for i in 0..200 {
        assert_eq!(
            command(&binding, &["SET", &format!("k{i}"), "v"]).await,
            "+OK\r\n"
        );
    }
    // Recreated right after the last write: the stop is graceful, so the append-only file is
    // flushed before the container goes.
    manager
        .start_redis("cache", spec(true, "noeviction", 64))
        .await
        .unwrap();
    assert!(command(&binding, &["DBSIZE"]).await.contains(":200"));
    manager.delete_redis("cache").await.unwrap();
}
