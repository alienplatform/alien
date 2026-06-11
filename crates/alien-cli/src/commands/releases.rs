//! CLI command for listing releases.

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{make_table, print_table};
use alien_error::Context;
use alien_manager_api::types::{ReleaseResponse, StackByPlatform};
use alien_manager_api::SdkResultExt as _;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(about = "List releases")]
pub struct ReleasesArgs {
    #[command(subcommand)]
    pub cmd: ReleasesCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ReleasesCmd {
    /// List releases, newest first
    Ls {
        /// Project to list releases for (optional, uses linked project by default)
        #[arg(long)]
        project: Option<String>,
    },
}

pub async fn releases_task(args: ReleasesArgs, ctx: ExecutionMode) -> Result<()> {
    ctx.ensure_ready().await?;

    match args.cmd {
        ReleasesCmd::Ls { project } => {
            // Releases are a core feature, so they go through the manager, not
            // the platform API directly.
            let manager = crate::commands::deployments::resolve_manager_client(
                &ctx,
                project.as_deref(),
                true,
            )
            .await?;
            list_releases_task(&manager).await
        }
    }
}

async fn list_releases_task(client: &alien_manager_api::Client) -> Result<()> {
    let response = client
        .list_releases()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing releases".to_string(),
            url: None,
        })?
        .into_inner();

    if response.items.is_empty() {
        println!("(no releases)");
        return Ok(());
    }

    let mut table = make_table(&["ID", "Created", "Commit", "Platforms"]);
    for release in &response.items {
        table.add_row(vec![
            release.id.clone().into(),
            release.created_at.clone().into(),
            commit_cell(release),
            platforms_cell(&release.stack),
        ]);
    }
    print_table(table);

    Ok(())
}

/// A branch/tag ref reads better than a bare SHA, so prefer it.
fn commit_cell(release: &ReleaseResponse) -> comfy_table::Cell {
    let label = release
        .git_metadata
        .as_ref()
        .and_then(|g| g.commit_ref.clone().or_else(|| g.commit_sha.clone()))
        .unwrap_or_else(|| "—".to_string());
    comfy_table::Cell::new(label)
}

fn platforms_cell(stack: &StackByPlatform) -> comfy_table::Cell {
    let names: Vec<&str> = [
        ("aws", stack.aws.is_some()),
        ("gcp", stack.gcp.is_some()),
        ("azure", stack.azure.is_some()),
        ("kubernetes", stack.kubernetes.is_some()),
        ("local", stack.local.is_some()),
        ("test", stack.test.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, present)| present.then_some(name))
    .collect();

    let label = if names.is_empty() {
        "—".to_string()
    } else {
        names.join(", ")
    };
    comfy_table::Cell::new(label)
}
