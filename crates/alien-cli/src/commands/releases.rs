//! CLI commands for listing and inspecting releases.

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{dim_label, make_table, print_table, status_cell};
use alien_core::DeploymentStatus;
use alien_error::Context;
use alien_manager_api::types::{
    DeploymentResponse, ReleaseResponse, StackByPlatform,
};
use alien_manager_api::SdkResultExt as _;
use clap::{Parser, Subcommand};
use serde::Serialize;

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
    /// Show a release and how far it has rolled out across deployments
    Get {
        /// Release ID (`rel_…`)
        id: String,
        /// Project the release belongs to (optional, uses linked project by default)
        #[arg(long)]
        project: Option<String>,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
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
        ReleasesCmd::Get { id, project, json } => {
            let manager = crate::commands::deployments::resolve_manager_client(
                &ctx,
                project.as_deref(),
                !json,
            )
            .await?;
            get_release_task(&manager, &id, json).await
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

/// Fetch a release plus its rollout across the manager's deployments.
///
/// A release is an immutable artifact with no status of its own — creating it
/// sets `desiredReleaseId` on the project's deployments, which then roll
/// forward to it. So "how is this release doing?" is answered by correlating
/// the release id against those deployments: which have reached it, and which
/// have failed on the way.
async fn get_release_task(
    client: &alien_manager_api::Client,
    id: &str,
    json: bool,
) -> Result<()> {
    let release = client
        .get_release()
        .id(id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("fetching release '{id}'"),
            url: None,
        })?
        .into_inner();

    let deployments = client
        .list_deployments()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing deployments for rollout".to_string(),
            url: None,
        })?
        .into_inner()
        .items;

    let rollout = compute_rollout(id, &deployments);

    if json {
        return print_json(&ReleaseRollout::new(&release, rollout));
    }

    render_release(&release);
    render_rollout(&rollout);
    Ok(())
}

/// One deployment's progress toward a target release.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloutTarget {
    /// Deployment id (`dep_…`).
    pub deployment_id: String,
    /// Deployment name.
    pub name: String,
    /// Raw deployment status (kebab-case).
    pub status: String,
    /// The release this deployment is currently running, if any.
    pub current_release_id: Option<String>,
    /// `current_release_id` has reached the target release.
    pub rolled_out: bool,
    /// Deployment is in a terminal failure state (see `DeploymentStatus::is_failed`).
    pub failed: bool,
}

/// Rollout view for a release: the deployments that target it and a one-line
/// summary. `targets` is empty when nothing points at the release yet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rollout {
    /// The release being rolled out.
    pub release_id: String,
    /// Deployments whose `desiredReleaseId` is this release.
    pub targets: Vec<RolloutTarget>,
    /// Human-readable one-liner (e.g. "2/3 rolled out, 1 failed").
    pub summary: String,
}

/// The full `releases get --json` payload: the release artifact facts plus its
/// rollout. Flattened so callers see release fields at the top level.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseRollout {
    id: String,
    created_at: String,
    platforms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_message: Option<String>,
    rollout: Rollout,
}

impl ReleaseRollout {
    fn new(release: &ReleaseResponse, rollout: Rollout) -> Self {
        let git = release.git_metadata.as_ref();
        Self {
            id: release.id.clone(),
            created_at: release.created_at.clone(),
            platforms: platform_names(&release.stack),
            commit_ref: git.and_then(|g| g.commit_ref.clone()),
            commit_sha: git.and_then(|g| g.commit_sha.clone()),
            commit_message: git.and_then(|g| g.commit_message.clone()),
            rollout,
        }
    }
}

/// Correlate a release id against the deployment list: keep the deployments
/// that target it and describe each one's progress. Pure over its inputs so it
/// can be unit-tested without a client.
///
/// A deployment "targets" the release if it is either still rolling out to it
/// (`desiredReleaseId == release_id`) OR has already reached it
/// (`currentReleaseId == release_id`). Matching only `desired` dropped
/// fully-rolled-out deployments — once the rollout completed the manager clears
/// `desired`, so the release would report zero targets despite a deployment
/// actively running it.
fn compute_rollout(release_id: &str, deployments: &[DeploymentResponse]) -> Rollout {
    let targets: Vec<RolloutTarget> = deployments
        .iter()
        .filter(|d| {
            d.desired_release_id.as_deref() == Some(release_id)
                || d.current_release_id.as_deref() == Some(release_id)
        })
        .map(|d| RolloutTarget {
            deployment_id: d.id.clone(),
            name: d.name.clone(),
            status: d.status.clone(),
            current_release_id: d.current_release_id.clone(),
            rolled_out: d.current_release_id.as_deref() == Some(release_id),
            failed: is_failed_status(&d.status),
        })
        .collect();

    let summary = summarize_targets(&targets);
    Rollout {
        release_id: release_id.to_string(),
        targets,
        summary,
    }
}

/// A deployment status is a terminal failure per `DeploymentStatus::is_failed`.
/// The status arrives as a kebab-case string; parse it back into the enum so
/// the single source of truth in `alien-core` decides. An unrecognized status
/// (schema drift) is treated as not-failed rather than guessed.
fn is_failed_status(status: &str) -> bool {
    serde_json::from_value::<DeploymentStatus>(serde_json::Value::String(status.to_string()))
        .map(|s| s.is_failed())
        .unwrap_or(false)
}

fn summarize_targets(targets: &[RolloutTarget]) -> String {
    if targets.is_empty() {
        return "No deployments target this release yet.".to_string();
    }
    let rolled_out = targets.iter().filter(|t| t.rolled_out).count();
    // Don't double-count a deployment that failed earlier but has since caught up.
    let failed = targets.iter().filter(|t| t.failed && !t.rolled_out).count();
    let mut summary = format!("{rolled_out}/{} rolled out", targets.len());
    if failed > 0 {
        summary.push_str(&format!(", {failed} failed"));
    }
    summary
}

fn render_release(release: &ReleaseResponse) {
    println!("{} {}", dim_label("Release"), release.id);
    println!("{} {}", dim_label("Created"), release.created_at);
    if let Some(git) = &release.git_metadata {
        if let Some(commit) = git.commit_ref.as_ref().or(git.commit_sha.as_ref()) {
            println!("{} {}", dim_label("Commit"), commit);
        }
        if let Some(message) = &git.commit_message {
            println!("{} {}", dim_label("Message"), message);
        }
    }
    let platforms = platform_names(&release.stack);
    if !platforms.is_empty() {
        println!("{} {}", dim_label("Platforms"), platforms.join(", "));
    }
}

fn render_rollout(rollout: &Rollout) {
    println!("{} {}", dim_label("Rollout"), rollout.summary);
    if rollout.targets.is_empty() {
        return;
    }
    let mut table = make_table(&["Deployment", "Status", "Current release", "Rolled out"]);
    for target in &rollout.targets {
        table.add_row(vec![
            target.name.clone().into(),
            status_cell(&target.status),
            target
                .current_release_id
                .clone()
                .unwrap_or_else(|| "—".to_string())
                .into(),
            rolled_out_cell(target).into(),
        ]);
    }
    print_table(table);
}

fn rolled_out_cell(target: &RolloutTarget) -> String {
    if target.rolled_out {
        "yes".to_string()
    } else if target.failed {
        "no (failed)".to_string()
    } else {
        "no".to_string()
    }
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

fn platform_names(stack: &StackByPlatform) -> Vec<String> {
    [
        ("aws", stack.aws.is_some()),
        ("gcp", stack.gcp.is_some()),
        ("azure", stack.azure.is_some()),
        ("kubernetes", stack.kubernetes.is_some()),
        ("local", stack.local.is_some()),
        ("test", stack.test.is_some()),
    ]
    .into_iter()
    .filter_map(|(name, present)| present.then(|| name.to_string()))
    .collect()
}

fn platforms_cell(stack: &StackByPlatform) -> comfy_table::Cell {
    let names = platform_names(stack);
    let label = if names.is_empty() {
        "—".to_string()
    } else {
        names.join(", ")
    };
    comfy_table::Cell::new(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_manager_api::types::Platform;

    /// Build a minimal `DeploymentResponse` for rollout tests. Only the fields
    /// `compute_rollout` reads are meaningful; the rest are filler.
    fn deployment(
        id: &str,
        name: &str,
        status: &str,
        current: Option<&str>,
        desired: Option<&str>,
    ) -> DeploymentResponse {
        DeploymentResponse::builder()
            .id(id)
            .name(name)
            .status(status)
            .platform(Platform::Aws)
            .deployment_group_id("dg_1")
            .deployment_protocol_version(1u32)
            .project_id("prj_1")
            .workspace_id("ws_1")
            .retry_requested(false)
            .created_at("2026-07-05T00:00:00Z")
            .current_release_id(current.map(|s| s.to_string()))
            .desired_release_id(desired.map(|s| s.to_string()))
            .try_into()
            .expect("valid deployment response")
    }

    const REL: &str = "rel_target";

    #[test]
    fn keeps_only_deployments_targeting_the_release() {
        let deployments = vec![
            deployment("dep_a", "prod", "running", Some(REL), Some(REL)),
            deployment(
                "dep_b",
                "staging",
                "running",
                Some("rel_other"),
                Some("rel_other"),
            ),
            deployment("dep_c", "observe", "running", None, None),
        ];

        let rollout = compute_rollout(REL, &deployments);

        let ids: Vec<&str> = rollout
            .targets
            .iter()
            .map(|t| t.deployment_id.as_str())
            .collect();
        assert_eq!(ids, vec!["dep_a"]);
    }

    #[test]
    fn rolled_out_only_when_current_reaches_release() {
        let deployments = vec![
            deployment("dep_pending", "prod", "provisioning", Some("rel_prev"), Some(REL)),
            deployment("dep_done", "eu", "running", Some(REL), Some(REL)),
        ];

        let rollout = compute_rollout(REL, &deployments);

        let pending = rollout
            .targets
            .iter()
            .find(|t| t.deployment_id == "dep_pending")
            .expect("pending target present");
        let done = rollout
            .targets
            .iter()
            .find(|t| t.deployment_id == "dep_done")
            .expect("done target present");
        assert!(!pending.rolled_out);
        assert!(done.rolled_out);
        assert_eq!(rollout.summary, "1/2 rolled out");
    }

    #[test]
    fn completed_rollout_with_cleared_desired_still_counts_as_target() {
        // Once a deployment reaches the target release the manager clears
        // `desired_release_id`; the release must still report it as a
        // rolled-out target (matching on `current`), not drop it to zero.
        let deployments = vec![deployment("dep_done", "prod", "running", Some(REL), None)];

        let rollout = compute_rollout(REL, &deployments);

        assert_eq!(rollout.targets.len(), 1, "completed rollout must be a target");
        assert!(rollout.targets[0].rolled_out);
        assert_eq!(rollout.summary, "1/1 rolled out");
    }

    #[test]
    fn every_failed_status_is_flagged_and_healthy_ones_are_not() {
        let failing = [
            "preflights-failed",
            "initial-setup-failed",
            "provisioning-failed",
            "update-failed",
            "delete-failed",
            "teardown-failed",
            "refresh-failed",
            "error",
        ];
        for status in failing {
            let deployments = vec![deployment("d", "n", status, None, Some(REL))];
            let rollout = compute_rollout(REL, &deployments);
            assert!(
                rollout.targets[0].failed,
                "expected '{status}' to be flagged failed"
            );
        }

        let healthy = ["pending", "provisioning", "running", "updating", "deleted"];
        for status in healthy {
            let deployments = vec![deployment("d", "n", status, None, Some(REL))];
            let rollout = compute_rollout(REL, &deployments);
            assert!(
                !rollout.targets[0].failed,
                "expected '{status}' to be healthy"
            );
        }
    }

    #[test]
    fn unrecognized_status_is_treated_as_not_failed() {
        let deployments = vec![deployment("d", "n", "some-future-state", None, Some(REL))];
        let rollout = compute_rollout(REL, &deployments);
        assert!(!rollout.targets[0].failed);
    }

    #[test]
    fn summarizes_mixed_rolled_out_failed_and_in_flight() {
        let deployments = vec![
            deployment("dep_ok", "a", "running", Some(REL), Some(REL)),
            deployment(
                "dep_fail",
                "b",
                "provisioning-failed",
                Some("rel_prev"),
                Some(REL),
            ),
            deployment("dep_wip", "c", "provisioning", Some("rel_prev"), Some(REL)),
        ];

        let rollout = compute_rollout(REL, &deployments);

        assert_eq!(rollout.summary, "1/3 rolled out, 1 failed");
        let fail = rollout
            .targets
            .iter()
            .find(|t| t.deployment_id == "dep_fail")
            .expect("failed target present");
        assert!(fail.failed);
        assert!(!fail.rolled_out);
    }

    #[test]
    fn reports_no_targets_when_nothing_points_at_the_release() {
        let deployments = vec![deployment(
            "dep_x",
            "x",
            "running",
            None,
            Some("rel_other"),
        )];

        let rollout = compute_rollout(REL, &deployments);

        assert!(rollout.targets.is_empty());
        assert_eq!(rollout.summary, "No deployments target this release yet.");
    }
}
