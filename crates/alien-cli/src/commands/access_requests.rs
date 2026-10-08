//! CLI commands for access requests — a complete, non-Slack path to REQUEST
//! time-boxed operation and/or remote-debugging access. Someone other than
//! the requester approves: on Kubernetes the customer, in-cluster via `kubectl
//! patch` on the grant custom resource; elsewhere a workspace member other
//! than the requester. There is deliberately no CLI action that approves a
//! request.

use std::fmt::Display;
use std::time::Duration;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    AccessRequestStatus, CreateAccessRequest, CreateAccessRequestMaxRisk, DebugGrantTool,
    RevokeAccessRequest,
};
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

Someone other than the requester approves: on Kubernetes the customer, in-cluster with
kubectl; otherwise a workspace member other than the requester. There is no command here
that approves a request.

Revoking a request withdraws the grant immediately: operations still waiting to be
dispatched fail, open debug sessions stop, and the request can no longer be approved.
Operations already running finish. Expired and rejected requests cannot be revoked.

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

    # Withdraw a grant that is no longer needed
    alien access-requests revoke ar_123 --reason 'incident resolved'
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
    /// Revoke an access request: pending operations fail and open debug sessions stop.
    Revoke {
        id: String,

        /// Why the grant is withdrawn (kept in the audit trail).
        #[arg(long)]
        reason: Option<String>,
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
        AccessRequestsAction::Revoke { id, reason } => {
            revoke_task(&sdk_client, &workspace, &id, reason.as_deref(), args.json).await
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
            "kubectlApprove": kubectl_approve.kubectl_command(),
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
            "Someone other than you approves it — share the request with whoever approves \
             access, then run: alien access-requests wait {}",
            created.id
        );
    }
    Ok(())
}

/// Poll `GET /access-requests/{id}/coordinates` for up to ~15s (a few of the
/// operator's ~5s materialization cycles) so `create` can hand back the
/// customer's approve command immediately, instead of requiring a separate
/// `get` call. Returns right away when the deployment has no in-cluster
/// approval, and without a command if the operator is slower than usual
/// (`get`/`wait` remain the fallback).
async fn poll_for_kubectl_approve(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
) -> Result<ApprovalInstructions> {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let instructions = fetch_approval_instructions(sdk_client, workspace, id).await?;
        if instructions.is_final() || std::time::Instant::now() >= deadline {
            return Ok(instructions);
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
            fetch_approval_instructions(sdk_client, workspace, id).await?
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
            "kubectlApprove": kubectl_approve.kubectl_command(),
            "agentSessionId": request.agent_session_id,
            "createdAt": request.created_at,
            "queuedBy": request.queued_by,
            "queuedAt": request.queued_at,
            "approvedBy": request.approved_by,
            "deniedBy": request.denied_by,
            "revokedBy": request.revoked_by,
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
        if let Some(revoked_by) = request.revoked_by.as_ref() {
            print_revoked_by(
                &revoked_by.actor_kind,
                &revoked_by.actor_id,
                &revoked_by.at,
                revoked_by.reason.as_ref(),
            );
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

async fn revoke_task(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
    reason: Option<&str>,
    json: bool,
) -> Result<()> {
    let reason = reason
        .map(|value| {
            value.try_into().map_err(|error| {
                AlienError::new(ErrorData::ValidationError {
                    field: "reason".to_string(),
                    message: format!("Invalid reason: {error}"),
                })
            })
        })
        .transpose()?;

    let request = sdk_client
        .revoke_access_request()
        .id(id)
        .workspace(workspace)
        .body(RevokeAccessRequest { reason })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("revoking access request '{id}'"),
            url: None,
        })?
        .into_inner();

    if json {
        print_json(&request)?;
    } else {
        println!("Access request revoked: {}", request.id);
        println!("{} {}", dim_label("Title"), request.title);
        println!("{} {}", dim_label("Status"), request.status);
        if let Some(revoked_by) = request.revoked_by.as_ref() {
            print_revoked_by(
                &revoked_by.actor_kind,
                &revoked_by.actor_id,
                &revoked_by.at,
                revoked_by.reason.as_ref(),
            );
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
        println!("{}", dim_label("Operations:"));
        for command in &request.commands {
            let tier = command
                .tier
                .as_ref()
                .map(|t| format!(" [{t}]"))
                .unwrap_or_default();
            println!("  - {}{tier} — {}", command.command, command.summary);
        }
    }
    Ok(())
}

fn print_revoked_by(
    actor_kind: impl Display,
    actor_id: impl Display,
    at: impl Display,
    reason: Option<impl Display>,
) {
    println!(
        "{} {actor_kind} {actor_id} at {at}",
        dim_label("Revoked by")
    );
    if let Some(reason) = reason {
        println!("{} {reason}", dim_label("Revocation reason"));
    }
}

/// How the customer approves a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApprovalInstructions {
    /// In-cluster with kubectl. The command is `None` until the operator has
    /// materialized the grant and reported its coordinates.
    Kubectl(Option<String>),
    /// No in-cluster approval: a workspace member approves in the dashboard or Slack.
    Elsewhere,
}

impl ApprovalInstructions {
    /// Whether polling can stop: there is a command, or there will never be one.
    pub(crate) fn is_final(&self) -> bool {
        !matches!(self, ApprovalInstructions::Kubectl(None))
    }

    pub(crate) fn kubectl_command(&self) -> Option<&str> {
        match self {
            ApprovalInstructions::Kubectl(command) => command.as_deref(),
            ApprovalInstructions::Elsewhere => None,
        }
    }

    /// One-time message for a waiting caller, once there is something to say.
    pub(crate) fn waiting_message(&self) -> Option<String> {
        match self {
            ApprovalInstructions::Kubectl(Some(command)) => {
                Some(format!("Run this in-cluster to approve:\n  {command}"))
            }
            ApprovalInstructions::Kubectl(None) => None,
            ApprovalInstructions::Elsewhere => Some(ELSEWHERE_APPROVAL.to_string()),
        }
    }
}

const ELSEWHERE_APPROVAL: &str = "A workspace member other than the requester approves this \
     request in the Alien dashboard (Access requests) or in Slack.";

/// Fetch how the customer approves this request via `GET
/// /access-requests/{id}/coordinates`. For in-cluster approval, the
/// `kubectl patch` command is `None` until the operator has materialized the
/// grant CR and reported its namespace/CRD coordinates back (before `queued`,
/// or briefly after, before the operator's next ~5s pull).
pub(crate) async fn fetch_approval_instructions(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    id: &str,
) -> Result<ApprovalInstructions> {
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
    let in_cluster = coordinates
        .approval_channels
        .contains(&alien_platform_api::types::AccessRequestApprovalChannel::Kubectl);
    Ok(if in_cluster {
        ApprovalInstructions::Kubectl(coordinates.kubectl_approve)
    } else {
        ApprovalInstructions::Elsewhere
    })
}

fn print_kubectl_approve(instructions: &ApprovalInstructions, status: AccessRequestStatus) {
    if *instructions == ApprovalInstructions::Elsewhere && status == AccessRequestStatus::Queued {
        println!();
        println!("{}", dim_label(ELSEWHERE_APPROVAL));
        return;
    }
    match (instructions.kubectl_command(), status) {
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
                    "Queued — waiting for the customer's approval. On Kubernetes, the \
                     approve command appears once the operator materializes the grant CR \
                     (usually within a few seconds); run this command again shortly."
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
            let instructions = fetch_approval_instructions(sdk_client, workspace, id).await?;
            if let Some(message) = instructions.waiting_message() {
                println!("{}", dim_label(&message));
                println!();
                printed_kubectl_approve = true;
            }
        }

        match approval_outcome(request.status) {
            ApprovalOutcome::Approved => {
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
            ApprovalOutcome::Closed => return Err(not_approved_error(id, request.status)),
            ApprovalOutcome::Pending => {}
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

/// What a poll of an access request's status means for a caller waiting on
/// approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApprovalOutcome {
    /// The customer approved it; the grant is usable.
    Approved,
    /// It will never be approved: rejected, expired, or revoked.
    Closed,
    /// Still waiting on the customer.
    Pending,
}

pub(crate) fn approval_outcome(status: AccessRequestStatus) -> ApprovalOutcome {
    match status {
        AccessRequestStatus::CustomerApproved => ApprovalOutcome::Approved,
        AccessRequestStatus::Rejected
        | AccessRequestStatus::Expired
        | AccessRequestStatus::Revoked => ApprovalOutcome::Closed,
        AccessRequestStatus::PendingApproval | AccessRequestStatus::Queued => {
            ApprovalOutcome::Pending
        }
    }
}

pub(crate) fn not_approved_error(id: &str, status: AccessRequestStatus) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ApiRequestFailed {
        message: format!("access request '{id}' is '{status}', not approved"),
        url: None,
    })
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
            let instructions = fetch_approval_instructions(sdk_client, workspace, id).await?;
            if let Some(message) = instructions.waiting_message() {
                eprintln!("{}", dim_label(&message));
                printed_kubectl_approve = true;
            }
        }

        match approval_outcome(request.status) {
            ApprovalOutcome::Approved => return Ok(request.id),
            ApprovalOutcome::Closed => return Err(not_approved_error(id, request.status)),
            ApprovalOutcome::Pending => {}
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
    use std::sync::{Arc, Mutex};

    use axum::{
        extract::{RawQuery, State},
        http::StatusCode,
        response::IntoResponse,
        routing::post,
        Json, Router,
    };

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

    #[test]
    fn revoke_parses_with_and_without_a_reason() {
        let args = AccessRequestsArgs::try_parse_from([
            "access-requests",
            "revoke",
            "ar_123",
            "--reason",
            "incident resolved",
            "--json",
        ])
        .expect("revoke with a reason should parse");
        assert!(args.json);
        assert!(matches!(
            args.action,
            AccessRequestsAction::Revoke { ref id, reason: Some(ref reason) }
                if id == "ar_123" && reason == "incident resolved"
        ));

        let args = AccessRequestsArgs::try_parse_from(["access-requests", "revoke", "ar_123"])
            .expect("revoke without a reason should parse");
        assert!(matches!(
            args.action,
            AccessRequestsAction::Revoke { ref id, reason: None } if id == "ar_123"
        ));

        AccessRequestsArgs::try_parse_from(["access-requests", "revoke"])
            .expect_err("revoke needs a request id");
    }

    #[test]
    fn only_a_customer_approval_ends_the_wait_successfully() {
        assert_eq!(
            approval_outcome(AccessRequestStatus::CustomerApproved),
            ApprovalOutcome::Approved
        );
        for closed in [
            AccessRequestStatus::Rejected,
            AccessRequestStatus::Expired,
            AccessRequestStatus::Revoked,
        ] {
            assert_eq!(
                approval_outcome(closed),
                ApprovalOutcome::Closed,
                "{closed}"
            );
            let error = not_approved_error("ar_123", closed);
            assert_eq!(
                error.message,
                format!("API request failed: access request 'ar_123' is '{closed}', not approved")
            );
        }
        for pending in [
            AccessRequestStatus::PendingApproval,
            AccessRequestStatus::Queued,
        ] {
            assert_eq!(
                approval_outcome(pending),
                ApprovalOutcome::Pending,
                "{pending}"
            );
        }
    }

    /// A loopback platform API that answers `POST /v1/access-requests/ar_123/revoke`
    /// with a canned status and body, recording the query string and request body
    /// it received.
    async fn fake_platform(
        status: u16,
        body: Value,
    ) -> (
        alien_platform_api::Client,
        Arc<Mutex<Option<(Option<String>, Value)>>>,
    ) {
        type Received = Arc<Mutex<Option<(Option<String>, Value)>>>;

        async fn revoke(
            State((status, body, received)): State<(u16, Value, Received)>,
            RawQuery(query): RawQuery,
            Json(request_body): Json<Value>,
        ) -> impl IntoResponse {
            *received.lock().expect("record the request") = Some((query, request_body));
            (
                StatusCode::from_u16(status).expect("valid status"),
                Json(body),
            )
        }

        let received: Received = Arc::new(Mutex::new(None));
        let app = Router::new()
            .route("/v1/access-requests/ar_123/revoke", post(revoke))
            .with_state((status, body, received.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (
            alien_platform_api::Client::new(&format!("http://{addr}")),
            received,
        )
    }

    #[tokio::test]
    async fn revoke_sends_the_reason_and_accepts_a_revoked_request() {
        let (client, received) = fake_platform(
            200,
            serde_json::json!({
                "id": "ar_123",
                "deploymentId": "dep_1",
                "title": "Restart api",
                "reason": "api is wedged",
                "status": "revoked",
                "operationPattern": null,
                "maxRisk": null,
                "commands": [],
                "debugGrant": null,
                "approvedUntil": null,
                "agentSessionId": null,
                "createdAt": "2026-10-01T00:00:00Z",
                "queuedBy": null,
                "queuedAt": null,
                "approvedBy": null,
                "deniedBy": null,
                "revokedBy": {
                    "actorKind": "user",
                    "actorId": "usr_1",
                    "at": "2026-10-01T01:00:00Z",
                    "reason": "incident resolved"
                },
                "remediationPlanId": null,
                "requestedExpiresAt": null,
                "requesterId": "usr_1",
                "requesterKind": null
            }),
        )
        .await;

        revoke_task(&client, "acme", "ar_123", Some("incident resolved"), true)
            .await
            .expect("a revoked response should succeed");

        let (query, body) = received
            .lock()
            .expect("read the recorded request")
            .clone()
            .expect("the revoke endpoint should have been called");
        assert_eq!(query.as_deref(), Some("workspace=acme"));
        assert_eq!(body, serde_json::json!({ "reason": "incident resolved" }));
    }

    #[tokio::test]
    async fn revoke_without_a_reason_sends_an_empty_body() {
        let (client, received) = fake_platform(
            404,
            serde_json::json!({
                "code": "ACCESS_REQUEST_NOT_FOUND",
                "message": "Access request not found",
                "retryable": false,
                "internal": false
            }),
        )
        .await;

        revoke_task(&client, "acme", "ar_123", None, true)
            .await
            .expect_err("a 404 must fail the command");

        let (_, body) = received
            .lock()
            .expect("read the recorded request")
            .clone()
            .expect("the revoke endpoint should have been called");
        assert_eq!(body, serde_json::json!({}));
    }

    #[tokio::test]
    async fn revoking_a_closed_request_surfaces_the_api_message() {
        let (client, _) = fake_platform(
            409,
            serde_json::json!({
                "code": "ACCESS_REQUEST_NOT_REVOCABLE",
                "message": "Access request ar_123 is expired and cannot be revoked",
                "retryable": false,
                "internal": false
            }),
        )
        .await;

        let error = revoke_task(&client, "acme", "ar_123", None, false)
            .await
            .expect_err("a 409 must fail the command");

        assert_eq!(error.http_status_code, Some(409));
        let rendered = crate::ui::render_human_error(&error);
        assert!(
            rendered.contains("Access request ar_123 is expired and cannot be revoked"),
            "{rendered}"
        );
        assert!(
            rendered.contains("revoking access request 'ar_123'"),
            "{rendered}"
        );
    }
}

#[cfg(test)]
mod approval_instructions_tests {
    use super::ApprovalInstructions;

    #[test]
    fn push_deployments_stop_waiting_for_a_kubectl_command() {
        let elsewhere = ApprovalInstructions::Elsewhere;
        assert!(elsewhere.is_final());
        assert_eq!(elsewhere.kubectl_command(), None);
        assert!(elsewhere
            .waiting_message()
            .is_some_and(|message| message.contains("dashboard")));
    }

    #[test]
    fn kubernetes_waits_until_the_operator_reports_the_command() {
        let pending = ApprovalInstructions::Kubectl(None);
        assert!(!pending.is_final());
        assert_eq!(pending.waiting_message(), None);

        let ready = ApprovalInstructions::Kubectl(Some("kubectl patch x".to_string()));
        assert!(ready.is_final());
        assert_eq!(ready.kubectl_command(), Some("kubectl patch x"));
        assert!(ready
            .waiting_message()
            .is_some_and(|message| message.contains("kubectl patch x")));
    }
}
