//! Transfer a deployment to another instance of the same workspace without replacing its resources.
//!
//! The platform runs the steps and keeps the deployment frozen while the transfer is open.
//! `start` begins it, `status` reports where it is, `continue` takes the next step, and `cancel`
//! stops it. Every call goes to `/v1/deployments/{id}/instance-move`.

use alien_error::{AlienError, Context, IntoAlienError};
use clap::Subcommand;
use reqwest::Method;
use serde_json::{json, Value};

use crate::commands::value_access;
use crate::error::{ErrorData, Result};
use crate::output::print_json;
use crate::ui::{contextual_heading, dim_label};

#[derive(Subcommand, Debug, Clone)]
pub enum TransferCmd {
    /// Check whether the deployment can transfer, and start unless --dry-run is given
    Start {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>
        id: String,
        /// ID of the instance to transfer the deployment to (same workspace)
        #[arg(long = "to")]
        to: String,
        /// Only report what blocks the transfer
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show the current step, what holds it, and the next step
    Status {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Take the next step: check that running work has drained and create the setup links, then
    /// switch the deployment over, then finish once the frozen period is over. Refuses with a
    /// reason when the next step is not ready.
    Continue {
        id: String,
        /// While waiting for the account admin: create new links (unused ones stop working)
        #[arg(long)]
        new_links: bool,
        #[arg(long)]
        json: bool,
    },
    /// Cancel the transfer. If the account admin already updated the setup, they must open the
    /// restore link; the deployment stays frozen until then.
    Cancel {
        id: String,
        #[arg(long)]
        json: bool,
    },
}

impl TransferCmd {
    pub fn id(&self) -> &str {
        match self {
            Self::Start { id, .. }
            | Self::Status { id, .. }
            | Self::Continue { id, .. }
            | Self::Cancel { id, .. } => id,
        }
    }

    pub fn json(&self) -> bool {
        match self {
            Self::Start { json, .. }
            | Self::Status { json, .. }
            | Self::Continue { json, .. }
            | Self::Cancel { json, .. } => *json,
        }
    }
}

/// What `continue` does for a transfer in a given step.
#[derive(Debug, PartialEq)]
enum Next {
    Drain,
    Links,
    Cutover,
    Complete,
    Refuse(&'static str),
}

fn next_step(step: Option<&str>, new_links: bool) -> Next {
    match step {
        Some("draining") => Next::Drain,
        Some("awaiting-account-admin") if new_links => Next::Links,
        Some("awaiting-account-admin") => Next::Refuse(
            "Waiting for the account admin to open the update link. Use --new-links to create new links.",
        ),
        Some("target-trusted") => Next::Cutover,
        Some("cut-over") => Next::Complete,
        Some("restoring") => Next::Refuse(
            "The transfer was cancelled. The account admin must open the restore link; the deployment is frozen until then.",
        ),
        Some("completed") => Next::Refuse("The transfer is already complete."),
        _ => Next::Refuse("No transfer is in progress. Start one with `alien deployments transfer start`."),
    }
}

fn base(deployment_id: &str) -> String {
    format!("/v1/deployments/{deployment_id}/instance-move")
}

fn request_for(cmd: &TransferCmd, deployment_id: &str) -> (Method, String, Option<Value>) {
    let base = base(deployment_id);
    match cmd {
        TransferCmd::Start { to, dry_run, .. } => {
            let suffix = if *dry_run { "/preflight" } else { "" };
            (
                Method::POST,
                format!("{base}{suffix}"),
                Some(json!({ "targetManagerId": to })),
            )
        }
        TransferCmd::Status { .. } => (Method::GET, base, None),
        TransferCmd::Cancel { .. } => (Method::POST, format!("{base}/cancel"), None),
        TransferCmd::Continue { .. } => (Method::POST, format!("{base}/advance"), None),
    }
}

/// A server that predates transfers answers 404 for the route itself, with no error body of its
/// own. A 404 that names a deployment or transfer carries an error `code`.
fn is_route_missing(status: u16, body: &str) -> bool {
    status == 404
        && serde_json::from_str::<Value>(body)
            .ok()
            .and_then(|parsed| parsed.get("code").and_then(Value::as_str).map(str::to_string))
            .is_none()
}

async fn call(
    client: &alien_platform_api::Client,
    workspace: &str,
    method: Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let (status, text) =
        value_access::platform_request(client, method, path, workspace, body).await?;
    if is_route_missing(status, &text) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: "This server does not support transferring deployments yet. \
                      Update the server, or use a newer one."
                .to_string(),
        }));
    }
    if !(200..300).contains(&status) {
        return Err(value_access::platform_error(status, &text)).context(
            ErrorData::ApiRequestFailed {
                message: "transferring the deployment".to_string(),
                url: None,
            },
        );
    }
    serde_json::from_str(&text)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "The platform's answer to the transfer request was not JSON".to_string(),
        })
}

pub async fn run(
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment_id: &str,
    deployment_name: &str,
    cmd: &TransferCmd,
) -> Result<()> {
    let mut links: Option<Value> = None;
    let answer = if let TransferCmd::Continue { new_links, .. } = cmd {
        let base = base(deployment_id);
        let current = call(client, workspace, Method::GET, &base, None).await?;
        let step = current
            .get("move")
            .and_then(|m| m.get("status"))
            .and_then(Value::as_str);
        match next_step(step, *new_links) {
            Next::Refuse(reason) => {
                return Err(value_access::platform_error(
                    409,
                    &json!({ "message": reason }).to_string(),
                ))
                .context(ErrorData::ApiRequestFailed {
                    message: "transferring the deployment".to_string(),
                    url: None,
                });
            }
            Next::Drain => {
                let advanced = call(
                    client,
                    workspace,
                    Method::POST,
                    &format!("{base}/advance"),
                    None,
                )
                .await?;
                let now = advanced
                    .get("move")
                    .and_then(|m| m.get("status"))
                    .and_then(Value::as_str);
                if now == Some("awaiting-account-admin") {
                    links = Some(
                        call(
                            client,
                            workspace,
                            Method::POST,
                            &format!("{base}/links"),
                            Some(&json!({})),
                        )
                        .await?,
                    );
                }
                advanced
            }
            Next::Links => {
                let issued = call(
                    client,
                    workspace,
                    Method::POST,
                    &format!("{base}/links"),
                    Some(&json!({})),
                )
                .await?;
                links = Some(issued.clone());
                issued
            }
            Next::Cutover => {
                call(
                    client,
                    workspace,
                    Method::POST,
                    &format!("{base}/cutover"),
                    None,
                )
                .await?
            }
            Next::Complete => {
                call(
                    client,
                    workspace,
                    Method::POST,
                    &format!("{base}/complete"),
                    None,
                )
                .await?
            }
        }
    } else {
        let (method, path, body) = request_for(cmd, deployment_id);
        call(client, workspace, method, &path, body.as_ref()).await?
    };
    if cmd.json() {
        return print_json(&with_links(answer, links.as_ref()));
    }
    println!(
        "{}",
        contextual_heading("Deployment transfer", deployment_name, &[])
    );
    for line in describe(&answer) {
        println!("{line}");
    }
    if let Some(issued) = links {
        for line in describe_links(&issued) {
            println!("{line}");
        }
    }
    Ok(())
}

/// The JSON result includes freshly issued links, which are shown only once.
fn with_links(mut answer: Value, links: Option<&Value>) -> Value {
    if let (Some(issued), Some(object)) =
        (links.and_then(|l| l.get("links")), answer.as_object_mut())
    {
        object.insert("links".to_string(), issued.clone());
    }
    answer
}

/// Cancellation stops being possible once values are copied at cutover.
fn cancel_warning(step: Option<&str>) -> Option<&'static str> {
    match step {
        Some("target-trusted") => Some(
            "Warning: the next step copies values to the new manager. After that, the transfer cannot be cancelled.",
        ),
        Some("cut-over") | Some("completed") => Some(
            "The transfer can no longer be cancelled: values were copied to the new manager.",
        ),
        _ => None,
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Lines for freshly created links.
fn describe_links(answer: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    {
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
}

/// Plain-language lines for a platform answer. Pure, so it is tested without a server.
fn describe(answer: &Value) -> Vec<String> {
    let mut lines = Vec::new();
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
        if let Some(warning) = cancel_warning(text(move_state, "status")) {
            lines.push(warning.to_string());
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

    fn start(dry_run: bool) -> TransferCmd {
        TransferCmd::Start {
            id: "dep_x".into(),
            to: "mgr_y".into(),
            dry_run,
            json: false,
        }
    }

    #[test]
    fn a_missing_route_is_told_apart_from_a_missing_deployment() {
        assert!(is_route_missing(404, "404 Not Found"));
        assert!(is_route_missing(404, ""));
        assert!(is_route_missing(404, r#"{"message":"nope"}"#));
        assert!(!is_route_missing(
            404,
            r#"{"code":"DEPLOYMENT_NOT_FOUND","message":"Deployment not found"}"#
        ));
        assert!(!is_route_missing(409, "Conflict"));
        assert!(!is_route_missing(200, "{}"));
    }

    #[test]
    fn json_output_keeps_issued_links_and_cancel_limit_is_shown() {
        let answer = json!({"move": {"status": "awaiting-account-admin"}});
        let issued = json!({"links": [{"kind": "restore", "token": "t"}]});
        let merged = with_links(answer.clone(), Some(&issued));
        assert_eq!(merged["links"][0]["token"], "t");
        assert_eq!(with_links(answer, None).get("links"), None);
        assert!(cancel_warning(Some("target-trusted")).is_some());
        assert!(cancel_warning(Some("cut-over")).is_some());
        assert!(cancel_warning(Some("draining")).is_none());
        let lines = describe(&json!({"move": {"status": "target-trusted"}}));
        assert!(lines.iter().any(|l| l.contains("cannot be cancelled")));
    }

    #[test]
    fn start_checks_first_with_dry_run_and_sends_only_the_target() {
        let (method, path, body) = request_for(&start(true), "dep_x");
        assert_eq!(method, Method::POST);
        assert_eq!(path, "/v1/deployments/dep_x/instance-move/preflight");
        assert_eq!(body, Some(json!({ "targetManagerId": "mgr_y" })));
        let (_, path, _) = request_for(&start(false), "dep_x");
        assert_eq!(path, "/v1/deployments/dep_x/instance-move");
    }

    #[test]
    fn status_and_cancel_hit_their_own_step() {
        let status = TransferCmd::Status {
            id: "d".into(),
            json: false,
        };
        let cancel = TransferCmd::Cancel {
            id: "d".into(),
            json: false,
        };
        assert_eq!(request_for(&status, "d").0, Method::GET);
        assert_eq!(
            request_for(&cancel, "d").1,
            "/v1/deployments/d/instance-move/cancel"
        );
    }

    #[test]
    fn continue_takes_the_next_step_or_says_why_not() {
        assert_eq!(next_step(Some("draining"), false), Next::Drain);
        assert_eq!(next_step(Some("target-trusted"), false), Next::Cutover);
        assert_eq!(next_step(Some("cut-over"), false), Next::Complete);
        assert_eq!(next_step(Some("awaiting-account-admin"), true), Next::Links);
        for step in [
            Some("awaiting-account-admin"),
            Some("restoring"),
            Some("completed"),
            Some("cancelled"),
            None,
        ] {
            assert!(
                matches!(next_step(step, false), Next::Refuse(_)),
                "{step:?}"
            );
        }
    }

    #[test]
    fn links_are_labelled_by_purpose() {
        let lines = describe_links(&json!({ "links": [
            { "kind": "new-role", "token": "tok-new", "expiresAt": "2026-11-14T00:00:00Z" },
            { "kind": "restore", "token": "tok-old", "expiresAt": "2026-11-14T00:00:00Z" },
        ]}))
        .join("\n");
        assert!(lines.contains("tok-new") && lines.contains("tok-old"));
        assert!(lines.contains("send this to the account admin"));
        assert!(lines.contains("shown once"));
    }

    #[test]
    fn status_reports_blockers_restore_and_unexpected_trust() {
        let lines = describe(&json!({
            "deploymentId": "dep_x",
            "move": { "status": "restoring", "frozenUntil": null },
            "holding": [{ "code": "COMMAND_ACTIVE", "message": "A command is running." }],
            "managedBy": null,
            "trustChangedOutsideMove": { "recordedRoleArn": "arn:x", "managerId": "mgr_y" }
        }))
        .join("\n");
        assert!(lines.contains("restoring"));
        assert!(lines.contains("restore link"));
        assert!(lines.contains("A command is running."));
        assert!(lines.contains("different management role"));
    }
}
