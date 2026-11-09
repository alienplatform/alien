//! With no Docker engine reachable, Local Redis fails with a message that says what to do.

use alien_local::{LocalRedisManager, RedisSpec};

#[tokio::test]
async fn start_without_docker_says_redis_needs_docker() {
    // Nothing listens on this socket. This binary holds only this test, so changing the
    // environment cannot affect another test.
    std::env::set_var("DOCKER_HOST", "unix:///nonexistent/alien-no-docker.sock");
    let dir = tempfile::tempdir().unwrap();
    let manager = LocalRedisManager::new(dir.path().to_path_buf());

    let error = manager
        .start_redis(
            "cache",
            RedisSpec {
                persistence: false,
                eviction_policy: "volatile-lru".to_string(),
                max_memory_bytes: None,
            },
        )
        .await
        .unwrap_err();

    assert_eq!(error.code, "DOCKER_CONNECTION_FAILED");
    assert!(
        error.to_string().contains("Redis on Local runs in Docker"),
        "{error}"
    );
    // Nothing was recorded for a server that never started.
    assert!(manager.try_get_binding("cache").unwrap().is_none());
}
