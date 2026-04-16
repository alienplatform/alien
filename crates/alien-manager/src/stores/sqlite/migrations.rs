//! Database migrations for alien-manager SQLite store.

use alien_error::{AlienError, IntoAlienError};
use sea_query::{ColumnDef, Expr, Iden, SqliteQueryBuilder, Table};

use super::database::{db_error, SqliteDatabase};

// =============================================================================
// Table Definitions (sea-query Iden)
// =============================================================================

#[derive(Iden, Clone, Copy)]
pub(crate) enum Deployments {
    Table,
    Id,
    Name,
    DeploymentGroupId,
    Platform,
    Status,
    StackSettings,
    StackState,
    EnvironmentInfo,
    RuntimeMetadata,
    CurrentReleaseId,
    DesiredReleaseId,
    RetryRequested,
    EnvironmentVariables,
    DeploymentToken,
    LockedBy,
    LockedAt,
    CreatedAt,
    UpdatedAt,
    Error,
}

#[derive(Iden, Clone, Copy)]
pub(crate) enum Releases {
    Table,
    Id,
    Stack,
    Platform,
    GitCommitSha,
    GitCommitRef,
    GitCommitMessage,
    CreatedAt,
}

#[derive(Iden, Clone, Copy)]
pub(crate) enum DeploymentGroups {
    Table,
    Id,
    Name,
    MaxDeployments,
    DeploymentCount,
    CreatedAt,
}

#[derive(Iden, Clone, Copy)]
pub(crate) enum Tokens {
    Table,
    Id,
    Type,
    KeyPrefix,
    KeyHash,
    DeploymentGroupId,
    DeploymentId,
    CreatedAt,
}

#[derive(Iden, Clone, Copy)]
pub(crate) enum Commands {
    Table,
    Id,
    DeploymentId,
    Name,
    State,
    DeploymentModel,
    Attempt,
    Deadline,
    CreatedAt,
    DispatchedAt,
    CompletedAt,
    RequestSizeBytes,
    ResponseSizeBytes,
    Error,
}

/// Run all table creation migrations.
pub async fn run_migrations(db: &SqliteDatabase) -> Result<(), AlienError> {
    let statements: Vec<String> = vec![
        // deployments
        Table::create()
            .table(Deployments::Table)
            .if_not_exists()
            .col(ColumnDef::new(Deployments::Id).text().primary_key())
            .col(ColumnDef::new(Deployments::Name).text().not_null())
            .col(
                ColumnDef::new(Deployments::DeploymentGroupId)
                    .text()
                    .not_null(),
            )
            .col(ColumnDef::new(Deployments::Platform).text().not_null())
            .col(ColumnDef::new(Deployments::Status).text().not_null())
            .col(ColumnDef::new(Deployments::StackSettings).text().not_null())
            .col(ColumnDef::new(Deployments::StackState).text())
            .col(ColumnDef::new(Deployments::EnvironmentInfo).text())
            .col(ColumnDef::new(Deployments::RuntimeMetadata).text())
            .col(ColumnDef::new(Deployments::CurrentReleaseId).text())
            .col(ColumnDef::new(Deployments::DesiredReleaseId).text())
            .col(ColumnDef::new(Deployments::EnvironmentVariables).text())
            .col(ColumnDef::new(Deployments::DeploymentToken).text())
            .col(
                ColumnDef::new(Deployments::RetryRequested)
                    .integer()
                    .not_null()
                    .default(0),
            )
            .col(ColumnDef::new(Deployments::LockedBy).text())
            .col(ColumnDef::new(Deployments::LockedAt).text())
            .col(
                ColumnDef::new(Deployments::CreatedAt)
                    .text()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .col(ColumnDef::new(Deployments::UpdatedAt).text())
            .col(ColumnDef::new(Deployments::Error).text())
            .build(SqliteQueryBuilder),
        // releases
        Table::create()
            .table(Releases::Table)
            .if_not_exists()
            .col(ColumnDef::new(Releases::Id).text().primary_key())
            .col(ColumnDef::new(Releases::Stack).text().not_null())
            .col(ColumnDef::new(Releases::Platform).text())
            .col(ColumnDef::new(Releases::GitCommitSha).text())
            .col(ColumnDef::new(Releases::GitCommitRef).text())
            .col(ColumnDef::new(Releases::GitCommitMessage).text())
            .col(
                ColumnDef::new(Releases::CreatedAt)
                    .text()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .build(SqliteQueryBuilder),
        // deployment_groups
        Table::create()
            .table(DeploymentGroups::Table)
            .if_not_exists()
            .col(ColumnDef::new(DeploymentGroups::Id).text().primary_key())
            .col(ColumnDef::new(DeploymentGroups::Name).text().not_null())
            .col(
                ColumnDef::new(DeploymentGroups::MaxDeployments)
                    .integer()
                    .not_null()
                    .default(100),
            )
            .col(
                ColumnDef::new(DeploymentGroups::DeploymentCount)
                    .integer()
                    .not_null()
                    .default(0),
            )
            .col(
                ColumnDef::new(DeploymentGroups::CreatedAt)
                    .text()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .build(SqliteQueryBuilder),
        // tokens
        Table::create()
            .table(Tokens::Table)
            .if_not_exists()
            .col(ColumnDef::new(Tokens::Id).text().primary_key())
            .col(ColumnDef::new(Tokens::Type).text().not_null())
            .col(ColumnDef::new(Tokens::KeyPrefix).text().not_null())
            .col(
                ColumnDef::new(Tokens::KeyHash)
                    .text()
                    .not_null()
                    .unique_key(),
            )
            .col(ColumnDef::new(Tokens::DeploymentGroupId).text())
            .col(ColumnDef::new(Tokens::DeploymentId).text())
            .col(
                ColumnDef::new(Tokens::CreatedAt)
                    .text()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .build(SqliteQueryBuilder),
        // commands
        Table::create()
            .table(Commands::Table)
            .if_not_exists()
            .col(ColumnDef::new(Commands::Id).text().primary_key())
            .col(ColumnDef::new(Commands::DeploymentId).text().not_null())
            .col(ColumnDef::new(Commands::Name).text().not_null())
            .col(ColumnDef::new(Commands::State).text().not_null())
            .col(ColumnDef::new(Commands::DeploymentModel).text().not_null())
            .col(
                ColumnDef::new(Commands::Attempt)
                    .integer()
                    .not_null()
                    .default(1),
            )
            .col(ColumnDef::new(Commands::Deadline).text())
            .col(
                ColumnDef::new(Commands::CreatedAt)
                    .text()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .col(ColumnDef::new(Commands::DispatchedAt).text())
            .col(ColumnDef::new(Commands::CompletedAt).text())
            .col(ColumnDef::new(Commands::RequestSizeBytes).integer())
            .col(ColumnDef::new(Commands::ResponseSizeBytes).integer())
            .col(ColumnDef::new(Commands::Error).text())
            .build(SqliteQueryBuilder),
    ];

    // Index creation statements (IF NOT EXISTS for idempotency)
    let index_statements: Vec<String> = vec![
        "CREATE INDEX IF NOT EXISTS idx_deployments_group ON deployments(deployment_group_id)"
            .to_string(),
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_deployments_group_name ON deployments(deployment_group_id, name)".to_string(),
        "CREATE INDEX IF NOT EXISTS idx_deployments_status ON deployments(status)".to_string(),
        "CREATE INDEX IF NOT EXISTS idx_deployments_locked ON deployments(locked_by, locked_at)"
            .to_string(),
        "CREATE INDEX IF NOT EXISTS idx_commands_deployment ON commands(deployment_id)".to_string(),
    ];

    let conn = db.conn().lock().await;
    for sql in statements {
        conn.execute(&sql, ())
            .await
            .into_alien_error()
            .map_err(|e| db_error(&format!("Migration failed: {}", e.message)))?;
    }
    for sql in index_statements {
        conn.execute(&sql, ())
            .await
            .into_alien_error()
            .map_err(|e| db_error(&format!("Index creation failed: {}", e.message)))?;
    }

    Ok(())
}
