use axum::{
    extract::{Path, State},
    response::Json,
};
use chrono::Utc;
use tokio_postgres::NoTls;
use tracing::{info, warn};

use crate::{
    models::{AppState, PostgresTestResponse},
    ErrorData, Result,
};
use alien_error::{AlienError, Context, IntoAlienError};

/// Exercise a Postgres binding end to end: resolve the connection, open a real driver connection,
/// and round-trip a write through a query. Unlike the other bindings there is no gRPC surface to
/// call — proving the resource works means actually speaking the wire protocol against it.
#[utoipa::path(
    post,
    path = "/postgres-test/{binding_name}",
    tag = "postgres",
    params(
        ("binding_name" = String, Path, description = "Name of the postgres binding to test")
    ),
    responses(
        (status = 200, description = "Postgres test completed", body = PostgresTestResponse),
        (status = 400, description = "Binding not found", body = AlienError),
        (status = 500, description = "Postgres operation failed", body = AlienError),
    ),
    operation_id = "test_postgres",
    summary = "Test postgres operations",
    description = "Resolves a postgres connection, connects with a driver, and round-trips a write/read query"
)]
pub async fn test_postgres(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<PostgresTestResponse>> {
    info!(%binding_name, "Received postgres test request");

    let postgres = app_state
        .internal_bindings
        .load_postgres(&binding_name)
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: binding_name.clone(),
        })?;

    // The binding never exposes the password to logs (redacting Debug); only the connection string,
    // built and consumed here, carries it. Local Postgres is plain TCP (sslmode=disable), so NoTls
    // is the correct transport — a cloud backend (sslmode=require) would need a TLS connector.
    let connection_string = postgres.connection_string();

    let (client, connection) = tokio_postgres::connect(&connection_string, NoTls)
        .await
        .into_alien_error()
        .context(ErrorData::PostgresOperationFailed {
            operation: "connect".to_string(),
        })?;

    // tokio-postgres splits the client from the connection task that drives the protocol; the task
    // must be polled for the client to make progress. It finishes once `client` is dropped.
    let connection_task = tokio::spawn(async move {
        if let Err(error) = connection.await {
            warn!(%error, "postgres connection task ended with error");
        }
    });

    let outcome = round_trip(&client).await;

    drop(client);
    let _ = connection_task.await;

    outcome?;

    info!(%binding_name, "Postgres test completed successfully");

    Ok(Json(PostgresTestResponse {
        binding_name,
        success: true,
    }))
}

/// Create a temporary table, write a row, read it back, and verify the value. A TEMP table is
/// dropped automatically when the session ends, so the probe leaves no state behind and cannot
/// collide with a concurrent run.
async fn round_trip(client: &tokio_postgres::Client) -> Result<()> {
    const EXPECTED: &str = "hello-from-alien-e2e";
    let probe_id: i32 = (Utc::now().timestamp_millis() % 1_000_000) as i32;

    client
        .batch_execute("CREATE TEMP TABLE alien_e2e_probe (id INT PRIMARY KEY, note TEXT NOT NULL)")
        .await
        .into_alien_error()
        .context(ErrorData::PostgresOperationFailed {
            operation: "create_temp_table".to_string(),
        })?;

    let inserted = client
        .execute(
            "INSERT INTO alien_e2e_probe (id, note) VALUES ($1, $2)",
            &[&probe_id, &EXPECTED],
        )
        .await
        .into_alien_error()
        .context(ErrorData::PostgresOperationFailed {
            operation: "insert".to_string(),
        })?;

    if inserted != 1 {
        return Err(AlienError::new(ErrorData::PostgresOperationFailed {
            operation: format!("insert affected {inserted} rows, expected 1"),
        }));
    }

    let row = client
        .query_one(
            "SELECT note FROM alien_e2e_probe WHERE id = $1",
            &[&probe_id],
        )
        .await
        .into_alien_error()
        .context(ErrorData::PostgresOperationFailed {
            operation: "select".to_string(),
        })?;

    let note: String = row.get(0);
    if note != EXPECTED {
        return Err(AlienError::new(ErrorData::PostgresOperationFailed {
            operation: format!("read-back mismatch: got '{note}', expected '{EXPECTED}'"),
        }));
    }

    Ok(())
}
