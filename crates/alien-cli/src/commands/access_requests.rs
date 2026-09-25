//! CLI commands for access requests — a complete, non-Slack path to REQUEST
//! time-boxed operation and/or remote-debugging access. Approval always
//! happens on the customer's side, in-cluster via `kubectl patch` on the
//! grant custom resource (or the operator's own reporting loop) — there is
//! deliberately no CLI or dashboard action that approves a request; Alien is
//! never the approver.

use std::time::Duration;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{CreateAccessRequest, CreateAccessRequestMaxRisk, DebugGrantTool};
use alien_platform_api::SdkResultExt as _;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::dim_label;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Request time-boxed operation and remote-debugging access",
    long_about = "Request time-boxed access to operations, remote debugging, or both.

Access requests work the same way whether they come from Slack, an AI agent, or here.
An exact operation request covers one operation; a wildcard request covers every
operation a plugin exposes right now, up to a risk cap — approval freezes that list, so
operations added to the plugin later are never included. A debug-tool request covers a
remote-debugging session for kubectl, aws, gcloud, or az. Combine --operation and
--debug-tool to request both on the same row; approving, denying, expiring, or revoking
the request applies to both at once.

Approval always happens on the customer's side (in-cluster, via kubectl or the
operator's own reporting loop) — there is no command here that approves a request.

EXAMPLES:
    # Request access to one operation
    alien access-requests create --deployment mycustomer/prod \\
      --operation kubernetes/restart-pod \\
      --params '{\"namespace\":\"braintrust\",\"pod\":\"api-123\"}' \\
      --duration 1h

    # Request temporary access to every read-only Kubernetes operation
    alien access-requests create --deployment mycustomer/prod \\
      --operation 'kubernetes/*' --max-risk read-only --duration 1h

    # Request a kubectl debug session scoped to one namespace
    alien access-requests create --deployment mycustomer/prod \\
      --debug-tool kubectl --debug-namespace braintrust --duration 30m

    # Request both an operation and a debug session in one row
    alien access-requests create --deployment mycustomer/prod \\
      --operation 'kubernetes/*' --max-risk read-only \\
      --debug-tool kubectl --debug-namespace braintrust --duration 30m

    # Review a request, then wait for the customer to approve it
    alien access-requests get ar_123
    alien access-requests wait ar_123
"
)]
pub struct AccessRequestsArgs {
    #[command(subcommand)]
    pub action: AccessRequestsAction,

    /// Project ID or name. Defaults to the linked project.
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum AccessRequestsAction {
    /// Create an access request — an operations grant (exact <plugin>/<operation> or
    /// wildcard <plugin>/*), a remote-debugging grant (--debug-tool), or both.
    Create {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>.
        #[arg(long)]
        deployment: String,

        /// Operation to request: an exact <plugin>/<operation>, or a wildcard
        /// <plugin>/* covering every operation the plugin exposes right now.
        #[arg(long)]
        operation: Option<String>,

        /// Operation parameters as JSON. Only valid for an exact operation.
        #[arg(long)]
        params: Option<String>,

        /// Highest risk tier a wildcard grant may cover: read-only | mutating | destructive. Required for a wildcard operation.
        #[arg(long = "max-risk")]
        max_risk: Option<String>,

        /// Remote-debugging tool to request access for: kubectl | aws | gcloud | az.
        /// May be combined with --operation to request both in one row.
        #[arg(long = "debug-tool")]
        debug_tool: Option<String>,

        /// Kubernetes namespace to scope a `kubectl` debug grant to. Requires --debug-tool kubectl.
        #[arg(long = "debug-namespace")]
        debug_namespace: Option<String>,

        /// Cloud account/project/subscription to scope an `aws`/`gcloud`/`az` debug
        /// grant to (e.g. an AWS account id, a GCP project id). Requires --debug-tool
        /// set to that provider.
        #[arg(long = "debug-cloud-scope")]
        debug_cloud_scope: Option<String>,

        /// Requested approval duration, e.g. 1h, 30m. Approvals cannot extend past this deadline.
        #[arg(long)]
        duration: Option<String>,

        /// Human-readable title. Defaults to the operation or pattern.
        #[arg(long)]
        title: Option<String>,

        /// Why access is needed.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Get an access request by id.
    Get { id: String },
    /// Wait for an access request to be approved by the customer.
    Wait {
        id: String,

        /// Timeout in seconds.
        #[arg(long, default_value = "3600")]
        timeout: u64,
    },
}

pub async fn access_requests_task(args: AccessRequestsArgs, ctx: ExecutionMode) -> Result<()> {
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let (_, project_link) = ctx
        .resolve_project(args.project.as_deref(), !args.json)
        .await?;
    let project = project_link.project_id;
    let sdk_client = ctx.sdk_client().await?;

    match args.action {
        AccessRequestsAction::Create {
            deployment,
            operation,
            params,
            max_risk,
            debug_tool,
            debug_namespace,
            debug_cloud_scope,
            duration,
            title,
            reason,
        } => {
            create_task(
                &ctx,
                &sdk_client,
                &workspace,
                &project,
                CreateTaskOptions {
                    deployment: &deployment,
                    operation: operation.as_deref(),
                    params: params.as_deref(),
                    max_risk: max_risk.as_deref(),
                    debug_tool: debug_tool.as_deref(),
                    debug_namespace: debug_namespace.as_deref(),
                    debug_cloud_scope: debug_cloud_scope.as_deref(),
                    title: title.as_deref(),
                    reason: reason.as_deref(),
                    duration: duration.as_deref(),
                    json: args.json,
                },
            )
            .await
        }
        AccessRequestsAction::Get { id } => get_task(&sdk_client, &workspace, &id, args.json).await,
        AccessRequestsAction::Wait { id, timeout } => {
            wait_task(&sdk_client, &workspace, &id, timeout, args.json).await
        }
    }
}

struct CreateTaskOptions<'a> {
    deployment: &'a str,
    operation: Option<&'a str>,
    params: Option<&'a str>,
    max_risk: Option<&'a str>,
    debug_tool: Option<&'a str>,
    debug_namespace: Option<&'a str>,
    debug_cloud_scope: Option<&'a str>,
    title: Option<&'a str>,
    reason: Option<&'a str>,
    duration: Option<&'a str>,
    json: bool,
}

async fn create_task(
    ctx: &ExecutionMode,
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    project: &str,
    options: CreateTaskOptions<'_>,
) -> Result<()> {
    let deployment_id = crate::platform_deployment_resolver::resolve(
        ctx,
        sdk_client,
        workspace,
        options.deployment,
        Some(project),
        !options.json,
    )
    .await?
    .id
    .to_string();
    let requested_expires_at = requested_expiration(Utc::now(), options.duration)?;

    if options.operation.is_none() && options.debug_tool.is_none() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "operation".to_string(),
            message: "Pass --operation (for operations access), --debug-tool (for remote \
                debugging), or both."
                .to_string(),
        }));
    }

    // `<plugin>/*` is a wildcard request; anything else is an exact operation.
    let is_wildcard = options.operation.is_some_and(|op| op.ends_with("/*"));

    if is_wildcard && options.params.is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "params".to_string(),
            message: format!(
                "--params only applies to an exact operation, not a wildcard pattern like '{}'.",
                options.operation.unwrap_or_default()
            ),
        }));
    }

    let (operation, params, operation_pattern, max_risk) = match options.operation {
        None => (None, None, None, None),
        Some(operation) if !is_wildcard => {
            let params: Option<Value> = options
                .params
                .map(|raw| {
                    serde_json::from_str(raw).into_alien_error().context(
                        ErrorData::ValidationError {
                            field: "params".to_string(),
                            message: "Invalid JSON".to_string(),
                        },
                    )
                })
                .transpose()?;
            (Some(operation.to_string()), params, None, None)
        }
        Some(operation) => {
            let Some(max_risk) = options.max_risk else {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "max-risk".to_string(),
                    message: format!(
                        "'{operation}' is a wildcard pattern and requires --max-risk (read-only | mutating | destructive).",
                    ),
                }));
            };
            let max_risk = parse_max_risk(max_risk)?;
            (None, None, Some(operation.to_string()), Some(max_risk))
        }
    };

    let debug_tool = options.debug_tool.map(parse_debug_tool).transpose()?;
    if options.debug_namespace.is_some() && debug_tool != Some(DebugGrantTool::Kubectl) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "debug-namespace".to_string(),
            message: "--debug-namespace requires --debug-tool kubectl.".to_string(),
        }));
    }
    if options.debug_cloud_scope.is_some() && debug_tool.is_none() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "debug-cloud-scope".to_string(),
            message: "--debug-cloud-scope requires --debug-tool.".to_string(),
        }));
    }
    if options.debug_cloud_scope.is_some() && debug_tool == Some(DebugGrantTool::Kubectl) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "debug-cloud-scope".to_string(),
            message:
                "--debug-cloud-scope does not apply to --debug-tool kubectl; use --debug-namespace."
                    .to_string(),
        }));
    }

    let body = CreateAccessRequest {
        deployment_id,
        operation,
        params,
        operation_pattern,
        max_risk,
        debug_tool,
        debug_namespace: options.debug_namespace.map(str::to_string),
        debug_cloud_scope: options.debug_cloud_scope.map(str::to_string),
        title: options.title.map(str::to_string),
        reason: options.reason.map(str::to_string),
        remediation_plan_id: None,
        commands: Vec::new(),
        replay_key: None,
        requested_expires_at,
    };

    let created = sdk_client
        .create_access_request()
        .workspace(workspace)
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "creating access request".to_string(),
            url: None,
        })?
        .into_inner();

    // Plan-less requests are queued immediately, but the operator materializing
    // the grant CR and reporting its coordinates back happens on its own ~5s
    // poll cycle — the command is essentially never ready in the instant right
    // after create returns. Poll briefly here (same as `wait`'s approach) so we
    // can hand the customer the command right away, the way Slack's card does,
    // instead of making them run `get` again themselves.
    let kubectl_approve = poll_for_kubectl_approve(sdk_client, workspace, &created.id).await?;

    if options.json {
        print_json(&serde_json::json!({
            "id": created.id,
            "deploymentId": created.deployment_id,
            "title": created.title,
            "reason": created.reason,
            "status": created.status,
            "operationPattern": created.operation_pattern,
            "maxRisk": created.max_risk,
            "commands": created.commands,
            "debugGrant": created.debug_grant,
            "approvedUntil": created.approved_until,
            "kubectlApprove": kubectl_approve,
        }))?;
    } else {
        println!("Access request created: {}", created.id);
        println!("{} {}", dim_label("Status"), created.status);
        if !created.commands.is_empty() {
            println!("{}", dim_label("Included operations:"));
            for command in &created.commands {
                let tier = command
                    .tier
                    .as_ref()
                    .map(|t| format!(" [{t}]"))
                    .unwrap_or_default();
                println!("  - {}{tier} — {}", command.command, command.summary);
            }
        }
        if let Some(debug_grant) = created.debug_grant.as_ref() {
            let scope = debug_grant
                .namespace
                .as_deref()
                .or(debug_grant.cloud_scope.as_deref())
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            println!("{} {}{scope}", dim_label("Debug access:"), debug_grant.tool);
        }
        print_kubectl_approve(&kubectl_approve, created.status);
        println!();
        println!("Review it:  alien access-requests get {}", created.id);
        println!(
            "Approval happens on your side — share the request with whoever approves access \
             in-cluster, then run: alien access-requests wait {}",
            created.id
        );
    }
    Ok(())
}

/// Poll `GET /access-requests/{id}/coordinates` for up to ~15s (a few of the
/// operator's ~5s materialization cycles) so `create` can hand back the
/// customer's approve command immediately, instead of requiring a separate
/// `get` call. Returns `None` if it's still not ready after the deadline —
/// the operator may simply be slower than usual (`get`/`wait` remain the
/// fallback).
async fn poll_for_kubectl_approve(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
) -> Result<Option<String>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let kubectl_approve = fetch_kubectl_approve(sdk_client, workspace, id).await?;
        if kubectl_approve.is_some() || std::time::Instant::now() >= deadline {
            return Ok(kubectl_approve);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn parse_max_risk(value: &str) -> Result<CreateAccessRequestMaxRisk> {
    match value {
        "read-only" => Ok(CreateAccessRequestMaxRisk::ReadOnly),
        "mutating" => Ok(CreateAccessRequestMaxRisk::Mutating),
        "destructive" => Ok(CreateAccessRequestMaxRisk::Destructive),
        other => Err(AlienError::new(ErrorData::ValidationError {
            field: "max-risk".to_string(),
            message: format!(
                "'{other}' is not a valid risk tier. Use read-only, mutating, or destructive."
            ),
        })),
    }
}

pub(crate) fn parse_debug_tool(value: &str) -> Result<DebugGrantTool> {
    match value {
        "kubectl" => Ok(DebugGrantTool::Kubectl),
        "aws" => Ok(DebugGrantTool::Aws),
        "gcloud" => Ok(DebugGrantTool::Gcloud),
        "az" => Ok(DebugGrantTool::Az),
        other => Err(AlienError::new(ErrorData::ValidationError {
            field: "debug-tool".to_string(),
            message: format!(
                "'{other}' is not a supported debug tool. Use kubectl, aws, gcloud, or az."
            ),
        })),
    }
}

async fn get_task(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
    json: bool,
) -> Result<()> {
    let request = sdk_client
        .get_access_request()
        .id(id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("getting access request '{id}'"),
            url: None,
        })?
        .into_inner();

    // Only worth polling while queued and not yet materialized — if it's
    // pending-approval there's genuinely nothing to wait for yet, and any
    // other status already has its final answer.
    let kubectl_approve =
        if request.status == alien_platform_api::types::AccessRequestStatus::Queued {
            poll_for_kubectl_approve(sdk_client, workspace, id).await?
        } else {
            fetch_kubectl_approve(sdk_client, workspace, id).await?
        };

    if json {
        print_json(&serde_json::json!({
            "id": request.id,
            "deploymentId": request.deployment_id,
            "title": request.title,
            "reason": request.reason,
            "status": request.status,
            "operationPattern": request.operation_pattern,
            "maxRisk": request.max_risk,
            "commands": request.commands,
            "debugGrant": request.debug_grant,
            "approvedUntil": request.approved_until,
            "kubectlApprove": kubectl_approve,
        }))?;
    } else {
        println!("{} {}", dim_label("ID"), request.id);
        println!("{} {}", dim_label("Title"), request.title);
        println!("{} {}", dim_label("Status"), request.status);
        if let Some(pattern) = &request.operation_pattern {
            println!("{} {}", dim_label("Pattern"), pattern);
        }
        if let Some(debug_grant) = request.debug_grant.as_ref() {
            let scope = debug_grant
                .namespace
                .as_deref()
                .or(debug_grant.cloud_scope.as_deref())
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            println!("{} {}{scope}", dim_label("Debug access:"), debug_grant.tool);
        }
        if let Some(until) = &request.approved_until {
            println!("{} {}", dim_label("Approved until"), until);
        }
        println!("{}", dim_label("Operations:"));
        for command in &request.commands {
            let tier = command
                .tier
                .as_ref()
                .map(|t| format!(" [{t}]"))
                .unwrap_or_default();
            println!("  - {}{tier} — {}", command.command, command.summary);
        }
        print_kubectl_approve(&kubectl_approve, request.status);
    }
    Ok(())
}

/// Fetch the customer's `kubectl patch` approve command via `GET
/// /access-requests/{id}/coordinates` — `None` until the operator has
/// materialized the grant CR in-cluster and reported its namespace/CRD
/// coordinates back (i.e. before `queued`, or briefly after, before the
/// operator's next ~5s pull).
pub(crate) async fn fetch_kubectl_approve(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
) -> Result<Option<String>> {
    let coordinates = sdk_client
        .get_access_request_coordinates()
        .id(id)
        .workspace(workspace)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("getting approve command for access request '{id}'"),
            url: None,
        })?
        .into_inner();
    Ok(coordinates.kubectl_approve)
}

fn print_kubectl_approve(
    kubectl_approve: &Option<String>,
    status: alien_platform_api::types::AccessRequestStatus,
) {
    use alien_platform_api::types::AccessRequestStatus;
    match (kubectl_approve, status) {
        (Some(command), _) => {
            println!();
            println!("{}", dim_label("Run this in-cluster to approve:"));
            println!("  {command}");
        }
        (None, AccessRequestStatus::PendingApproval) => {
            // No CR to approve yet — the engineer gate hasn't queued this
            // request, so the operator has nothing to materialize.
        }
        (None, AccessRequestStatus::Queued) => {
            println!();
            println!(
                "{}",
                dim_label(
                    "Queued — waiting for the operator to materialize the grant CR \
                     (usually within a few seconds). Run this command again shortly \
                     for the approve command."
                )
            );
        }
        (None, _) => {}
    }
}

async fn wait_task(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
    timeout_secs: u64,
    json: bool,
) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    // Print the kubectl approve command exactly once, the first time it
    // becomes available — not on every poll, and not just after approval
    // (by then it's too late to be useful; the point is to hand it to
    // whoever is going to run it while we're still waiting).
    let mut printed_kubectl_approve = false;

    loop {
        let request = sdk_client
            .get_access_request()
            .id(id)
            .workspace(workspace)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("waiting for access request '{id}'"),
                url: None,
            })?
            .into_inner();

        if !json && !printed_kubectl_approve {
            let kubectl_approve = fetch_kubectl_approve(sdk_client, workspace, id).await?;
            if let Some(command) = &kubectl_approve {
                println!("{}", dim_label("Run this in-cluster to approve:"));
                println!("  {command}");
                println!();
                printed_kubectl_approve = true;
            }
        }

        match request.status {
            alien_platform_api::types::AccessRequestStatus::CustomerApproved => {
                if json {
                    print_json(&request)?;
                } else {
                    println!("Approved: {}", request.id);
                    if let Some(until) = &request.approved_until {
                        println!("{} {}", dim_label("Approved until"), until);
                    }
                }
                return Ok(());
            }
            alien_platform_api::types::AccessRequestStatus::Rejected
            | alien_platform_api::types::AccessRequestStatus::Expired => {
                return Err(AlienError::new(ErrorData::ApiRequestFailed {
                    message: format!(
                        "access request '{id}' is '{}', not approved",
                        request.status
                    ),
                    url: None,
                }));
            }
            _ => {}
        }

        if std::time::Instant::now() >= deadline {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "timed out after {timeout_secs}s waiting for access request '{id}' to be approved"
                ),
                url: None,
            }));
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Poll a queued access request until the customer approves it in-cluster,
/// printing the `kubectl patch` approve command once it becomes available.
/// Shared by `--request-access` shortcuts (`alien operations invoke
/// --request-access`, `alien debug --request-access`) that need to wait
/// silently on stdout (reserved for the caller's own eventual result) and
/// just get the id back on success.
pub(crate) async fn wait_for_approval(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
) -> Result<String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(3600);
    let mut printed_kubectl_approve = false;
    loop {
        let request = sdk_client
            .get_access_request()
            .id(id)
            .workspace(workspace)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("waiting for access request '{id}'"),
                url: None,
            })?
            .into_inner();

        if !printed_kubectl_approve {
            let kubectl_approve = fetch_kubectl_approve(sdk_client, workspace, id).await?;
            if let Some(command) = &kubectl_approve {
                eprintln!("{}", dim_label("Run this in-cluster to approve:"));
                eprintln!("  {command}");
                printed_kubectl_approve = true;
            }
        }

        match request.status {
            alien_platform_api::types::AccessRequestStatus::CustomerApproved => {
                return Ok(request.id);
            }
            alien_platform_api::types::AccessRequestStatus::Rejected
            | alien_platform_api::types::AccessRequestStatus::Expired => {
                return Err(AlienError::new(ErrorData::ApiRequestFailed {
                    message: format!(
                        "access request '{id}' is '{}', not approved",
                        request.status
                    ),
                    url: None,
                }));
            }
            _ => {}
        }

        if std::time::Instant::now() >= deadline {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "timed out after 3600s waiting for access request '{id}' to be approved"
                ),
                url: None,
            }));
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Parse a short duration string (`1h`, `30m`, `90s`) without rounding.
pub(crate) fn parse_duration_seconds(value: &str) -> Result<u64> {
    let invalid = || {
        AlienError::new(ErrorData::ValidationError {
            field: "duration".to_string(),
            message: format!("'{value}' is not a valid duration. Use e.g. 1h, 30m, 90s."),
        })
    };
    let trimmed = value.trim();
    let (digits, unit) = trimmed.split_at(
        trimmed
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(invalid)?,
    );
    let amount: u64 = digits.parse().map_err(|_| invalid())?;
    let seconds = match unit {
        "h" => amount.checked_mul(60 * 60),
        "m" => amount.checked_mul(60),
        "s" => Some(amount),
        _ => return Err(invalid()),
    }
    .ok_or_else(invalid)?;
    if seconds == 0 {
        return Err(invalid());
    }
    Ok(seconds)
}

pub(crate) fn requested_expiration(
    now: DateTime<Utc>,
    duration: Option<&str>,
) -> Result<Option<DateTime<Utc>>> {
    let Some(duration) = duration else {
        return Ok(None);
    };
    let seconds = parse_duration_seconds(duration)?;
    if seconds > 6 * 60 * 60 {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "duration".to_string(),
            message: "duration cannot exceed 6h".to_string(),
        }));
    }
    let seconds = i64::try_from(seconds).map_err(|_| {
        AlienError::new(ErrorData::ValidationError {
            field: "duration".to_string(),
            message: "duration is too large".to_string(),
        })
    })?;
    now.checked_add_signed(chrono::Duration::seconds(seconds))
        .map(Some)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "duration".to_string(),
                message: "duration is too large".to_string(),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_strings_parse_to_seconds_without_rounding() {
        assert_eq!(parse_duration_seconds("1h").unwrap(), 3_600);
        assert_eq!(parse_duration_seconds("90m").unwrap(), 5_400);
        assert_eq!(parse_duration_seconds("1s").unwrap(), 1);
        assert_eq!(parse_duration_seconds("90s").unwrap(), 90);
        assert!(parse_duration_seconds("").is_err());
        assert!(parse_duration_seconds("abc").is_err());
        assert!(parse_duration_seconds("1d").is_err());
        assert!(parse_duration_seconds("0m").is_err());
    }

    #[test]
    fn requested_duration_becomes_an_absolute_utc_deadline() {
        let now = "2026-09-17T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            requested_expiration(now, Some("90s")).unwrap(),
            Some("2026-09-17T00:01:30Z".parse().unwrap())
        );
        assert_eq!(requested_expiration(now, None).unwrap(), None);
        assert!(requested_expiration(now, Some("361m")).is_err());
    }

    #[test]
    fn max_risk_strings_parse_to_the_generated_enum() {
        assert!(matches!(
            parse_max_risk("read-only").unwrap(),
            CreateAccessRequestMaxRisk::ReadOnly
        ));
        assert!(matches!(
            parse_max_risk("destructive").unwrap(),
            CreateAccessRequestMaxRisk::Destructive
        ));
        assert!(parse_max_risk("write").is_err());
    }
}
