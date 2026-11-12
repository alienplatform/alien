//! Move a deployment to another manager of the same workspace without replacing its resources.
//!
//! The platform runs the steps and keeps the deployment frozen while the move is open. These
//! commands start the move, report where it is, hand the account admin their links, and cancel,
//! cut over or complete it. Every call goes to `/v1/deployments/{id}/instance-move`.

use alien_error::{Context, IntoAlienError};
use clap::Subcommand;
use reqwest::Method;
use serde_json::{json, Value};

use crate::commands::value_access;
use crate::error::{ErrorData, Result};
use crate::output::print_json;
use crate::ui::{contextual_heading, dim_label};

#[derive(Subcommand, Debug, Clone)]
pub enum ManagerMoveCmd {
    /// Check whether the deployment can move, and start the move unless --dry-run is given
    Start {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,
        /// Manager ID to move the deployment to (same workspace)
        #[arg(long = "to-manager")]
        to_manager: String,
        /// Only report what blocks the move
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show the move's step, what holds it, and which manager manages the deployment
    Status {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Check whether running work has drained and go to the next step
    Continue {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Create the links the account admin opens to update the setup. Shown once.
    Links {
        id: String,
        /// Only (re)create one link: new-role or restore
        #[arg(long, value_parser = ["new-role", "restore"])]
        only: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Cancel the move. If the account admin already updated the setup, use the restore link.
    Cancel {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Copy values and switch the deployment to the new manager. After values are copied the
    /// move can't be cancelled.
    Cutover {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Finish the move once the frozen period is over
    Complete {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

impl ManagerMoveCmd {
    pub fn id(&self) -> &str {
        match self {
            Self::Start { id, .. }
            | Self::Status { id, .. }
            | Self::Continue { id, .. }
            | Self::Links { id, .. }
            | Self::Cancel { id, .. }
            | Self::Cutover { id, .. }
            | Self::Complete { id, .. } => id,
        }
    }

    pub fn json(&self) -> bool {
        match self {
            Self::Start { json, .. }
            | Self::Status { json, .. }
            | Self::Continue { json, .. }
            | Self::Links { json, .. }
            | Self::Cancel { json, .. }
            | Self::Cutover { json, .. }
            | Self::Complete { json, .. } => *json,
        }
    }
}

/// The request one command sends: method, path suffix and optional body.
fn request_for(cmd: &ManagerMoveCmd, deployment_id: &str) -> (Method, String, Option<Value>) {
    let base = format!("/v1/deployments/{deployment_id}/instance-move");
    match cmd {
        ManagerMoveCmd::Start {
            to_manager,
            dry_run,
            ..
        } => {
            let suffix = if *dry_run { "/preflight" } else { "" };
            (
                Method::POST,
                format!("{base}{suffix}"),
                Some(json!({ "targetManagerId": to_manager })),
            )
        }
        ManagerMoveCmd::Status { .. } => (Method::GET, base, None),
        ManagerMoveCmd::Continue { .. } => (Method::POST, format!("{base}/advance"), None),
        ManagerMoveCmd::Links { only, .. } => (
            Method::POST,
            format!("{base}/links"),
            Some(match only {
                Some(kind) => json!({ "only": kind }),
                None => json!({}),
            }),
        ),
        ManagerMoveCmd::Cancel { .. } => (Method::POST, format!("{base}/cancel"), None),
        ManagerMoveCmd::Cutover { .. } => (Method::POST, format!("{base}/cutover"), None),
        ManagerMoveCmd::Complete { .. } => (Method::POST, format!("{base}/complete"), None),
    }
}

pub async fn run(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    deployment_name: &str,
    cmd: &ManagerMoveCmd,
) -> Result<()> {
    let (method, path, body) = request_for(cmd, deployment_id);
    let (status, text) =
        value_access::platform_request(client, method, &path, workspace, body.as_ref()).await?;
    if !(200..300).contains(&status) {
        return Err(value_access::platform_error(status, &text)).context(
            ErrorData::ApiRequestFailed {
                message: "moving the deployment to another manager".to_string(),
                url: None,
            },
        );
    }
    let answer: Value =
        serde_json::from_str(&text)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "parse".to_string(),
                reason: "The platform's answer to the move request was not JSON".to_string(),
            })?;
    if cmd.json() {
        return print_json(&answer);
    }
    println!(
        "{}",
        contextual_heading("Deployment move", deployment_name, &[])
    );
    for line in describe(cmd, &answer) {
        println!("{line}");
    }
    Ok(())
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Plain-language lines for a platform answer. Pure, so it is tested without a server.
fn describe(cmd: &ManagerMoveCmd, answer: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if let ManagerMoveCmd::Links { .. } = cmd {
        for link in answer
            .get("links")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let kind = text(link, "kind").unwrap_or("link");
            let purpose = if kind == "restore" {
                "restore (keep this one: it puts the old setup back)"
            } else {
                "update (send this to the account admin)"
            };
            lines.push(format!(
                "{} {}",
                dim_label(purpose),
                text(link, "token").unwrap_or("")
            ));
            lines.push(format!(
                "{} {}",
                dim_label("Expires"),
                text(link, "expiresAt").unwrap_or("")
            ));
        }
        lines.push("These links are shown once.".to_string());
        return lines;
    }
    if let Some(can_start) = answer.get("canStart").and_then(Value::as_bool) {
        lines.push(format!(
            "{} {}",
            dim_label("Can move"),
            if can_start { "yes" } else { "no" }
        ));
    }
    for blocker in answer
        .get("blockers")
        .and_then(Value::as_array)
        .into_iter()
        .chain(answer.get("holding").and_then(Value::as_array))
        .flatten()
    {
        lines.push(format!(
            "  - {}",
            text(blocker, "message")
                .or_else(|| text(blocker, "code"))
                .unwrap_or("blocked")
        ));
    }
    if let Some(move_state) = answer.get("move").filter(|m| !m.is_null()) {
        lines.push(format!(
            "{} {}",
            dim_label("Step"),
            text(move_state, "status").unwrap_or("unknown")
        ));
        if let Some(until) = text(move_state, "frozenUntil") {
            lines.push(format!("{} {until}", dim_label("Frozen until")));
        }
        if text(move_state, "status") == Some("restoring") {
            lines.push(
                "The account admin must open the restore link to put the old setup back."
                    .to_string(),
            );
        }
    } else if answer.get("deploymentId").is_some() && answer.get("canStart").is_none() {
        lines.push(format!("{} none in progress", dim_label("Move")));
    }
    if let Some(by) = answer.get("managedBy").filter(|m| !m.is_null()) {
        lines.push(format!(
            "{} {} since {}",
            dim_label("Managed by"),
            text(by, "managerId").unwrap_or(""),
            text(by, "since").unwrap_or("")
        ));
    }
    if let Some(changed) = answer
        .get("trustChangedOutsideMove")
        .filter(|m| !m.is_null())
    {
        lines.push(format!(
            "Warning: the stack trusts a different management role ({}) than the platform recorded, and no move is open.",
            text(changed, "recordedRoleArn").unwrap_or("")
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd_start(dry_run: bool) -> ManagerMoveCmd {
        ManagerMoveCmd::Start {
            id: "dep_x".into(),
            to_manager: "mgr_y".into(),
            dry_run,
            json: false,
        }
    }

    #[test]
    fn start_checks_first_with_dry_run_and_sends_only_the_target() {
        let (method, path, body) = request_for(&cmd_start(true), "dep_x");
        assert_eq!(method, Method::POST);
        assert_eq!(path, "/v1/deployments/dep_x/instance-move/preflight");
        assert_eq!(body, Some(json!({ "targetManagerId": "mgr_y" })));
        let (_, path, _) = request_for(&cmd_start(false), "dep_x");
        assert_eq!(path, "/v1/deployments/dep_x/instance-move");
    }

    #[test]
    fn each_command_hits_its_own_step() {
        let id = || "dep_x".to_string();
        let cases = [
            (
                ManagerMoveCmd::Status {
                    id: id(),
                    json: false,
                },
                Method::GET,
                "",
            ),
            (
                ManagerMoveCmd::Continue {
                    id: id(),
                    json: false,
                },
                Method::POST,
                "/advance",
            ),
            (
                ManagerMoveCmd::Cancel {
                    id: id(),
                    json: false,
                },
                Method::POST,
                "/cancel",
            ),
            (
                ManagerMoveCmd::Cutover {
                    id: id(),
                    json: false,
                },
                Method::POST,
                "/cutover",
            ),
            (
                ManagerMoveCmd::Complete {
                    id: id(),
                    json: false,
                },
                Method::POST,
                "/complete",
            ),
        ];
        for (cmd, method, suffix) in cases {
            let (m, path, _) = request_for(&cmd, "dep_x");
            assert_eq!(m, method);
            assert_eq!(path, format!("/v1/deployments/dep_x/instance-move{suffix}"));
        }
    }

    #[test]
    fn links_can_ask_for_one_kind() {
        let cmd = ManagerMoveCmd::Links {
            id: "dep_x".into(),
            only: Some("restore".into()),
            json: false,
        };
        let (_, path, body) = request_for(&cmd, "dep_x");
        assert!(path.ends_with("/links"));
        assert_eq!(body, Some(json!({ "only": "restore" })));
    }

    #[test]
    fn links_are_labelled_by_purpose() {
        let cmd = ManagerMoveCmd::Links {
            id: "dep_x".into(),
            only: None,
            json: false,
        };
        let lines = describe(
            &cmd,
            &json!({ "links": [
                { "kind": "new-role", "token": "tok-new", "expiresAt": "2026-11-14T00:00:00Z" },
                { "kind": "restore", "token": "tok-old", "expiresAt": "2026-11-14T00:00:00Z" },
            ]}),
        );
        let joined = lines.join("\n");
        assert!(joined.contains("tok-new") && joined.contains("tok-old"));
        assert!(joined.contains("send this to the account admin"));
        assert!(joined.contains("shown once"));
    }

    #[test]
    fn status_reports_blockers_restore_and_unexpected_trust() {
        let cmd = ManagerMoveCmd::Status {
            id: "dep_x".into(),
            json: false,
        };
        let lines = describe(
            &cmd,
            &json!({
                "deploymentId": "dep_x",
                "move": { "status": "restoring", "frozenUntil": null },
                "holding": [{ "code": "COMMAND_ACTIVE", "message": "A command is running." }],
                "managedBy": null,
                "trustChangedOutsideMove": { "recordedRoleArn": "arn:x", "managerId": "mgr_y" }
            }),
        )
        .join("\n");
        assert!(lines.contains("restoring"));
        assert!(lines.contains("restore link"));
        assert!(lines.contains("A command is running."));
        assert!(lines.contains("different management role"));
    }
}
