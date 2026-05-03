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
    /// Workspace this deployment belongs to. Always `"default"` in this
    /// store. Required by the unified Authz policy.
    WorkspaceId,
    /// Project this deployment belongs to. Always `"default"` in this store.
    ProjectId,
}

#[derive(Iden, Clone, Copy)]
pub(crate) enum Releases {
    Table,
    Id,
    /// JSON-encoded `HashMap<Platform, Stack>`. A release targets one or
    /// more platforms; the JSON keys are the platform discriminators.
    Stack,
    GitCommitSha,
    GitCommitRef,
    GitCommitMessage,
    CreatedAt,
    WorkspaceId,
    ProjectId,
}

#[derive(Iden, Clone, Copy)]
pub(crate) enum DeploymentGroups {
    Table,
    Id,
    Name,
    MaxDeployments,
    DeploymentCount,
    CreatedAt,
    WorkspaceId,
    ProjectId,
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
            .col(
                ColumnDef::new(Deployments::WorkspaceId)
                    .text()
                    .not_null()
                    .default("default"),
            )
            .col(
                ColumnDef::new(Deployments::ProjectId)
                    .text()
                    .not_null()
                    .default("default"),
            )
            .build(SqliteQueryBuilder),
        // releases
        Table::create()
            .table(Releases::Table)
            .if_not_exists()
            .col(ColumnDef::new(Releases::Id).text().primary_key())
            .col(ColumnDef::new(Releases::Stack).text().not_null())
            .col(ColumnDef::new(Releases::GitCommitSha).text())
            .col(ColumnDef::new(Releases::GitCommitRef).text())
            .col(ColumnDef::new(Releases::GitCommitMessage).text())
            .col(
                ColumnDef::new(Releases::CreatedAt)
                    .text()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .col(
                ColumnDef::new(Releases::WorkspaceId)
                    .text()
                    .not_null()
                    .default("default"),
            )
            .col(
                ColumnDef::new(Releases::ProjectId)
                    .text()
                    .not_null()
                    .default("default"),
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
            .col(
                ColumnDef::new(DeploymentGroups::WorkspaceId)
                    .text()
                    .not_null()
                    .default("default"),
            )
            .col(
                ColumnDef::new(DeploymentGroups::ProjectId)
                    .text()
                    .not_null()
                    .default("default"),
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

    // Backfill workspace_id / project_id columns on existing OSS databases.
    // SQLite doesn't support `ADD COLUMN IF NOT EXISTS`, and there's no
    // central schema-version table, so we run the ALTER and tolerate the
    // "duplicate column" failure on already-migrated DBs.
    let alter_statements: &[&str] = &[
        "ALTER TABLE deployments ADD COLUMN workspace_id TEXT NOT NULL DEFAULT 'default'",
        "ALTER TABLE deployments ADD COLUMN project_id TEXT NOT NULL DEFAULT 'default'",
        "ALTER TABLE releases ADD COLUMN workspace_id TEXT NOT NULL DEFAULT 'default'",
        "ALTER TABLE releases ADD COLUMN project_id TEXT NOT NULL DEFAULT 'default'",
        "ALTER TABLE deployment_groups ADD COLUMN workspace_id TEXT NOT NULL DEFAULT 'default'",
        "ALTER TABLE deployment_groups ADD COLUMN project_id TEXT NOT NULL DEFAULT 'default'",
    ];
    for sql in alter_statements {
        if let Err(e) = conn.execute(sql, ()).await {
            let msg = format!("{}", e);
            // SQLite reports "duplicate column name: <col>" when the column
            // is already present from a prior migration run; that's the
            // expected idempotent path.
            if !msg.contains("duplicate column name") {
                return Err(db_error(&format!(
                    "Schema upgrade failed running `{}`: {}",
                    sql, msg
                )));
            }
        }
    }

    let post_index_statements: &[&str] = &[
        "CREATE INDEX IF NOT EXISTS idx_releases_project ON releases(workspace_id, project_id)",
        "CREATE INDEX IF NOT EXISTS idx_deployments_project ON deployments(workspace_id, project_id)",
        "CREATE INDEX IF NOT EXISTS idx_deployment_groups_project ON deployment_groups(workspace_id, project_id)",
    ];
    for sql in post_index_statements {
        conn.execute(sql, ())
            .await
            .into_alien_error()
            .map_err(|e| db_error(&format!("Index creation failed: {}", e.message)))?;
    }

    Ok(())
}
