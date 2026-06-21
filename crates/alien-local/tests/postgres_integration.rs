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
use tempfile::TempDir;
use tokio_postgres::NoTls;

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
        let _ = connection.await;
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

    drop(client);
    let _ = connection_task.await;

    manager
        .delete_postgres("e2e-pg")
        .await
        .expect("delete_postgres should tear the server down cleanly");
}
