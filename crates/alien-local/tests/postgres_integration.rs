//! Integration test for the Local Postgres manager.
//!
//! Boots an embedded pgvector instance through `LocalPostgresManager`, resolves its binding the
//! same way a workload does (`LocalPostgres::from_binding(...).connection_string()`), then connects
//! with a real driver and round-trips a query plus `CREATE EXTENSION vector`. This exercises the
//! exact path a linked worker takes — the unit/round-trip tests never open a real connection.
//!
//! Unlike the sibling integration tests, this needs an external artifact host (the pgvector binary),
//! so it is `#[ignore]`d — run it explicitly in the e2e job with `ALIEN_PGVECTOR_RELEASES_URL` set to
//! a host serving `v<ver>/<os>-<arch>/pg<major>/pgvector_compiled.zip` (the public release host or a
//! local mirror). The primary Local-Postgres coverage is the `alien-test` `pull` suite; this is a
//! focused, fast guard on the manager → binding → driver path. `#[ignore]` keeps it out of the
//! default run so it can never report a green pass without actually connecting.

use alien_bindings::providers::postgres::local::LocalPostgres;
use alien_bindings::traits::Postgres;
use alien_local::LocalPostgresManager;
use std::{fs, path::PathBuf};
use tempfile::TempDir;
use tokio_postgres::{Client, NoTls};

#[tokio::test]
#[ignore = "needs ALIEN_PGVECTOR_RELEASES_URL + a reachable pgvector host; run explicitly in the e2e job"]
async fn local_postgres_binding_connects_and_round_trips() {
    // Reaching here means the test was invoked explicitly (it is `#[ignore]`d), so it must actually
    // connect — assert the precondition loudly rather than no-op.
    std::env::var("ALIEN_PGVECTOR_RELEASES_URL")
        .expect("ALIEN_PGVECTOR_RELEASES_URL must point at a host serving pgvector_compiled.zip");

    let temp_dir = TempDir::new().expect("temp dir");
    // Keep the sender alive for the test's duration so the manager's monitor loop stays up.
    let (_shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let (manager, _monitor) =
        LocalPostgresManager::new_with_shutdown(temp_dir.path().to_path_buf(), shutdown_rx);

    manager
        .start_postgres("e2e-pg", "17")
        .await
        .expect("start_postgres should boot the embedded server and install pgvector");

    let binding = manager
        .get_binding("e2e-pg")
        .expect("get_binding should read the persisted metadata");

    let connection_string = LocalPostgres::from_binding("e2e-pg", &binding)
        .expect("from_binding should resolve the connection params")
        .connection_string();

    let (client, connection) = tokio_postgres::connect(&connection_string, NoTls)
        .await
        .expect("driver should connect with the resolved binding credentials");
    let connection_task = tokio::spawn(async move {
        connection.await.expect("database connection task");
    });

    let row = client
        .query_one("SELECT 1::int AS one", &[])
        .await
        .expect("SELECT 1 should run");
    let one: i32 = row.get("one");
    assert_eq!(one, 1, "round-trip query should return 1");

    // pgvector must be installed at boot and loadable as an extension.
    client
        .batch_execute("CREATE EXTENSION IF NOT EXISTS vector")
        .await
        .expect("CREATE EXTENSION vector should succeed (pgvector installed at boot)");

    client
        .batch_execute(
            "CREATE TABLE runtime_check (id integer PRIMARY KEY, embedding vector(3));
             INSERT INTO runtime_check VALUES (7, '[1,2,3]');
             CREATE INDEX runtime_check_hnsw ON runtime_check USING hnsw (embedding vector_l2_ops);
             SET enable_seqscan = off;",
        )
        .await
        .expect("vector storage and HNSW indexing should work");
    assert_indexed_vector_lookup(&client).await;

    drop(client);
    connection_task
        .await
        .expect("connection task should finish");
    manager
        .stop_postgres("e2e-pg")
        .await
        .expect("stop should preserve the database");
    if cfg!(target_os = "linux") {
        // Exercise upgrade from metadata written before runtime provenance existed.
        let path = temp_dir.path().join("postgres/e2e-pg/metadata.json");
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("metadata")).expect("parse metadata");
        metadata
            .as_object_mut()
            .expect("metadata object")
            .remove("runtimeTarget");
        fs::write(
            &path,
            serde_json::to_vec(&metadata).expect("serialize legacy metadata"),
        )
        .expect("write legacy metadata");
    }
    manager
        .start_postgres("e2e-pg", "17")
        .await
        .expect("restart should reuse the database and compatible runtime");

    let restarted_binding = manager.get_binding("e2e-pg").expect("restarted binding");
    let restarted_connection_string = LocalPostgres::from_binding("e2e-pg", &restarted_binding)
        .expect("resolve restarted binding")
        .connection_string();
    // Do not print either DSN if this assertion fails: both contain the password.
    assert!(
        restarted_connection_string == connection_string,
        "restart must preserve the connection identity"
    );
    let (client, connection) = tokio_postgres::connect(&restarted_connection_string, NoTls)
        .await
        .expect("driver should reconnect after restart");
    let connection_task = tokio::spawn(async move {
        connection
            .await
            .expect("restarted database connection task");
    });
    assert_indexed_vector_lookup(&client).await;

    drop(client);
    connection_task
        .await
        .expect("restarted connection task should finish");

    manager
        .delete_postgres("e2e-pg")
        .await
        .expect("delete_postgres should tear the server down cleanly");
}

async fn assert_indexed_vector_lookup(client: &Client) {
    client
        .batch_execute("SET enable_seqscan = off")
        .await
        .expect("force the vector index for the runtime check");
    let plan = client
        .query(
            "EXPLAIN (COSTS OFF)
             SELECT id FROM runtime_check ORDER BY embedding <-> '[1,2,3]'::vector LIMIT 1",
            &[],
        )
        .await
        .expect("explain the real vector lookup");
    assert!(
        plan.iter()
            .map(|row| row.get::<_, String>(0))
            .any(|line| line.contains("Index Scan using runtime_check_hnsw")),
        "the persisted HNSW index must serve the query"
    );
    let row = client
        .query_one(
            "SELECT id FROM runtime_check ORDER BY embedding <-> '[1,2,3]'::vector LIMIT 1",
            &[],
        )
        .await
        .expect("execute the indexed vector lookup");
    assert_eq!(row.get::<_, i32>(0), 7);
}

// The fixture must be a stopped, real MUSL cluster with postmaster.opts pointing
// at /pg/bin/postgres. On a GNU host, mount that data read-only at the directory
// named below; mount the original MUSL installation read-only at /pg for the
// incompatible case, and omit /pg for the unverifiable case. No network is needed.
#[tokio::test]
#[ignore = "requires a stopped MUSL PostgreSQL fixture on a GNU Linux host"]
async fn local_postgres_refuses_incompatible_legacy_cluster() {
    assert_runtime_refused("LOCAL_POSTGRES_RUNTIME_CHANGED", None).await;
}

#[tokio::test]
#[ignore = "requires a stopped MUSL fixture with its original installation absent on GNU Linux"]
async fn local_postgres_refuses_unverifiable_legacy_cluster() {
    assert_runtime_refused("LOCAL_POSTGRES_RUNTIME_UNAVAILABLE", None).await;
}

#[tokio::test]
#[ignore = "requires a stopped MUSL PostgreSQL fixture on a GNU Linux host"]
async fn local_postgres_refuses_recorded_runtime_change() {
    let target = format!("{}-unknown-linux-musl", std::env::consts::ARCH);
    assert_runtime_refused("LOCAL_POSTGRES_RUNTIME_CHANGED", Some(&target)).await;
}

async fn assert_runtime_refused(expected_code: &str, previous_target: Option<&str>) {
    assert_eq!(std::env::consts::OS, "linux");
    let data_dir = PathBuf::from(
        std::env::var("ALIEN_POSTGRES_LEGACY_DATA_DIR")
            .expect("real stopped legacy cluster required"),
    );
    assert!(
        !data_dir.join("postmaster.pid").exists(),
        "fixture must be stopped"
    );
    let control_before =
        fs::read(data_dir.join("global/pg_control")).expect("real cluster control");
    let options_before = fs::read(data_dir.join("postmaster.opts")).expect("real server options");
    let state = TempDir::new().expect("manager state");
    let resource_dir = state.path().join("postgres/legacy");
    fs::create_dir_all(&resource_dir).expect("resource directory");
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("free port");
    let port = listener.local_addr().expect("bound address").port();
    drop(listener);
    let mut metadata = serde_json::json!({
        "resourceId": "legacy",
        "version": "17",
        "port": port,
        "username": "nobody",
        "password": "unused-fixture-password",
        "database": "postgres",
        "dataDir": data_dir,
    });
    if let Some(target) = previous_target {
        metadata["runtimeTarget"] = target.into();
    }
    let metadata_path = resource_dir.join("metadata.json");
    let metadata_before = serde_json::to_vec(&metadata).expect("legacy metadata");
    fs::write(&metadata_path, &metadata_before).expect("persist legacy metadata");
    let (_shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
    let (manager, _monitor) =
        LocalPostgresManager::new_with_shutdown(state.path().to_path_buf(), shutdown_rx);
    let error = manager
        .start_postgres("legacy", "17")
        .await
        .expect_err("ABI change must be refused");
    assert_eq!(error.code, expected_code);
    assert!(fs::read(metadata_path).expect("unchanged metadata") == metadata_before);
    assert!(fs::read(data_dir.join("global/pg_control")).expect("control") == control_before);
    assert!(fs::read(data_dir.join("postmaster.opts")).expect("options") == options_before);
    assert!(
        !data_dir.join("postmaster.pid").exists(),
        "incompatible server must never start"
    );
}
