//! `alien commands ls | get | stats`: read what happened to commands.
//!
//! These read the platform API (list, get and per-name statistics), so they need a platform
//! login. A standalone manager serves a single command by ID only, and `alien dev` has no
//! history to summarize.

use alien_error::{AlienError, Context};
use alien_platform_api::types::{ListCommandsState, ListCommandsWorkspace};
use alien_platform_api::SdkResultExt as _;
use chrono::{DateTime, Duration, Utc};
use clap::ValueEnum;
use serde::Deserialize;
use serde_json::Value;
use std::num::NonZeroU64;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{dim_label, heading, make_table, print_table, status_cell};

/// Largest page the API serves.
const MAX_PAGE: u64 = 100;
/// Longest error message shown on one line.
const ERROR_LINE_MAX_CHARS: usize = 160;

/// Command outcome as typed on the command line.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusArg {
    Failed,
    Succeeded,
    Pending,
    Dispatched,
    Expired,
    PendingUpload,
}

impl StatusArg {
    fn state(self) -> ListCommandsState {
        match self {
            Self::Failed => ListCommandsState::Failed,
            Self::Succeeded => ListCommandsState::Succeeded,
            Self::Pending => ListCommandsState::Pending,
            Self::Dispatched => ListCommandsState::Dispatched,
            Self::Expired => ListCommandsState::Expired,
            Self::PendingUpload => ListCommandsState::PendingUpload,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ListOptions {
    pub deployment: Option<String>,
    pub name: Option<String>,
    pub status: Option<StatusArg>,
    pub since: Option<Duration>,
    pub limit: u64,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct StatsOptions {
    pub deployment: Option<String>,
    pub since: Duration,
    pub json: bool,
}

/// The fields of a command the tables and the detail view show.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandView {
    id: String,
    name: String,
    state: String,
    deployment_id: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    dispatched_at: Option<DateTime<Utc>>,
    #[serde(default)]
    completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    attempt: Option<f64>,
    #[serde(default)]
    request_size_bytes: Option<f64>,
    #[serde(default)]
    response_size_bytes: Option<f64>,
    #[serde(default)]
    error: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    target: Option<TargetView>,
    #[serde(default)]
    deployment: Option<DeploymentView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TargetView {
    resource_id: String,
    resource_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentView {
    name: String,
    #[serde(default)]
    deployment_group: Option<NamedView>,
}

#[derive(Debug, Deserialize)]
struct NamedView {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatsRow {
    name: String,
    total: u64,
    succeeded: u64,
    failed: u64,
    expired: u64,
    in_flight: u64,
    p50_duration_ms: Option<f64>,
    p95_duration_ms: Option<f64>,
    top_error_codes: Vec<ErrorCodeCount>,
}

#[derive(Debug, Deserialize)]
struct ErrorCodeCount {
    code: String,
    count: u64,
}

impl CommandView {
    fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value(value.clone()).map_err(|error| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("the platform returned a command this CLI cannot read: {error}"),
                url: None,
            })
        })
    }

    /// Time from creation to completion, once the command has finished.
    fn duration(&self) -> Option<Duration> {
        self.completed_at.map(|done| done - self.created_at)
    }

    fn deployment_label(&self) -> String {
        match &self.deployment {
            Some(DeploymentView {
                name,
                deployment_group: Some(group),
            }) => format!("{}/{}", group.name, name),
            Some(DeploymentView { name, .. }) => name.clone(),
            None => self.deployment_id.clone(),
        }
    }

    /// Why the command failed, in one line.
    fn error_line(&self) -> Option<String> {
        self.error.as_ref().map(error_line)
    }
}

/// `CODE: message` on one line, cut at [`ERROR_LINE_MAX_CHARS`].
fn error_line(error: &serde_json::Map<String, Value>) -> String {
    let code = error.get("code").and_then(Value::as_str);
    let message = error.get("message").and_then(Value::as_str);
    let line = match (code, message) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        (Some(code), None) => code.to_string(),
        (None, Some(message)) => message.to_string(),
        (None, None) => "failed without an error message".to_string(),
    };
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > ERROR_LINE_MAX_CHARS {
        let cut: String = line.chars().take(ERROR_LINE_MAX_CHARS - 1).collect();
        format!("{cut}…")
    } else {
        line
    }
}

fn format_duration_ms(ms: f64) -> String {
    if ms < 1000.0 {
        format!("{}ms", ms.round() as u64)
    } else if ms < 60_000.0 {
        format!("{:.1}s", ms / 1000.0)
    } else {
        let seconds = (ms / 1000.0).round() as u64;
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    }
}

fn format_duration(duration: Duration) -> String {
    format_duration_ms(duration.num_milliseconds().max(0) as f64)
}

fn format_bytes(bytes: f64) -> String {
    let bytes = bytes.max(0.0);
    if bytes < 1024.0 {
        format!("{} B", bytes as u64)
    } else if bytes < 1024.0 * 1024.0 {
        format!("{:.1} KiB", bytes / 1024.0)
    } else {
        format!("{:.1} MiB", bytes / (1024.0 * 1024.0))
    }
}

fn require_platform(ctx: &ExecutionMode, subcommand: &str) -> Result<()> {
    if ctx.is_platform() {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::ValidationError {
        field: "command".to_string(),
        message: format!(
            "`alien commands {subcommand}` reads the platform's command history. Log in to the platform (`alien login`) and run it again; a standalone manager or `alien dev` does not keep one."
        ),
    }))
}

async fn resolve_deployment_id(
    ctx: &ExecutionMode,
    client: &alien_platform_api::Client,
    reference: &str,
    json: bool,
) -> Result<String> {
    // An ID needs no lookup: the API filters on it directly.
    if reference.starts_with("dep_") {
        return Ok(reference.to_string());
    }
    let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
    let detail = crate::platform_deployment_resolver::resolve(
        ctx, client, &workspace, reference, None, !json,
    )
    .await?;
    Ok(String::from(detail.id))
}

fn workspace_param(workspace: Option<&str>) -> Result<Option<ListCommandsWorkspace>> {
    workspace
        .map(|workspace| {
            ListCommandsWorkspace::try_from(workspace).map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: "workspace".to_string(),
                    message: format!("'{workspace}' is not a valid workspace"),
                })
            })
        })
        .transpose()
}

pub async fn list_task(ctx: &ExecutionMode, options: ListOptions) -> Result<()> {
    require_platform(ctx, "ls")?;
    let client = ctx.sdk_client().await?;
    let workspace = ctx
        .resolve_workspace_query_with_bootstrap(!options.json)
        .await?;
    let deployment_id = match options.deployment.as_deref() {
        Some(reference) => {
            Some(resolve_deployment_id(ctx, &client, reference, options.json).await?)
        }
        None => None,
    };
    let items = list_commands(
        &client,
        workspace.as_deref(),
        deployment_id.as_deref(),
        &options,
        Utc::now(),
    )
    .await?;
    print_list(&items, options.json)
}

async fn list_commands(
    client: &alien_platform_api::Client,
    workspace: Option<&str>,
    deployment_id: Option<&str>,
    options: &ListOptions,
    now: DateTime<Utc>,
) -> Result<Vec<Value>> {
    let workspace = workspace_param(workspace)?;
    let mut items: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    while (items.len() as u64) < options.limit {
        let page = (options.limit - items.len() as u64).min(MAX_PAGE);
        let mut request = client
            .list_commands()
            .limit(NonZeroU64::new(page).expect("page is at least 1"))
            .include(vec![
                alien_platform_api::types::ListCommandsIncludeItem::Deployment,
            ]);
        if let Some(workspace) = workspace.clone() {
            request = request.workspace(workspace);
        }
        if let Some(id) = deployment_id {
            request = request.deployment_id(id);
        }
        if let Some(name) = options.name.as_deref() {
            request = request.name(name);
        }
        if let Some(status) = options.status {
            request = request.state(status.state());
        }
        if let Some(since) = options.since {
            request = request.created_after(now - since);
        }
        if let Some(cursor) = cursor.as_deref() {
            request = request.cursor(cursor);
        }
        let response = request
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: "listing commands".to_string(),
                url: None,
            })?
            .into_inner();
        let page_items = serde_json::to_value(&response.items).map_err(|error| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("reading the command list: {error}"),
                url: None,
            })
        })?;
        items.extend(page_items.as_array().cloned().unwrap_or_default());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    items.truncate(options.limit as usize);
    Ok(items)
}

fn print_list(items: &[Value], json: bool) -> Result<()> {
    if json {
        return print_json(&items);
    }
    if items.is_empty() {
        println!("(no commands)");
        return Ok(());
    }
    let mut table = make_table(&[
        "ID",
        "Name",
        "Status",
        "Deployment",
        "Created",
        "Duration",
        "Error",
    ]);
    for item in items {
        let command = CommandView::from_value(item)?;
        table.add_row(vec![
            command.id.clone().into(),
            command.name.clone().into(),
            status_cell(&command.state),
            command.deployment_label().into(),
            command
                .created_at
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
                .into(),
            command
                .duration()
                .map(format_duration)
                .unwrap_or_else(|| "—".to_string())
                .into(),
            command.error_line().unwrap_or_default().into(),
        ]);
    }
    print_table(table);
    Ok(())
}

pub async fn get_task(ctx: &ExecutionMode, id: &str, json: bool) -> Result<()> {
    require_platform(ctx, "get")?;
    let client = ctx.sdk_client().await?;
    let workspace = ctx.resolve_workspace_query_with_bootstrap(!json).await?;
    let command = get_command(&client, workspace.as_deref(), id).await?;
    if json {
        return print_json(&command);
    }
    print_command(&CommandView::from_value(&command)?);
    Ok(())
}

async fn get_command(
    client: &alien_platform_api::Client,
    workspace: Option<&str>,
    id: &str,
) -> Result<Value> {
    let mut request = client.get_command().id(id);
    if let Some(workspace) = workspace {
        request = request.workspace(workspace);
    }
    let response = request
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("getting command {id}"),
            url: None,
        })?
        .into_inner();
    serde_json::to_value(&response).map_err(|error| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("reading command {id}: {error}"),
            url: None,
        })
    })
}

fn print_command(command: &CommandView) {
    let field = |label: &str, value: String| println!("{:<12} {}", dim_label(label), value);
    println!("{}", heading(&format!("Command {}", command.id)));
    field("Name", command.name.clone());
    field("Status", command.state.clone());
    field("Deployment", command.deployment_label());
    if let Some(target) = &command.target {
        field(
            "Target",
            format!("{} ({})", target.resource_id, target.resource_type),
        );
    }
    field("Created", command.created_at.to_rfc3339());
    if let Some(dispatched) = command.dispatched_at {
        field("Dispatched", dispatched.to_rfc3339());
    }
    if let Some(completed) = command.completed_at {
        field("Completed", completed.to_rfc3339());
    }
    if let Some(duration) = command.duration() {
        field("Duration", format_duration(duration));
    }
    if let Some(deadline) = command.deadline {
        field("Deadline", deadline.to_rfc3339());
    }
    if let Some(attempt) = command.attempt {
        field("Attempts", (attempt as u64).to_string());
    }
    if let Some(bytes) = command.request_size_bytes {
        field("Request", format_bytes(bytes));
    }
    if let Some(bytes) = command.response_size_bytes {
        field("Response", format_bytes(bytes));
    }
    if let Some(line) = command.error_line() {
        field("Error", line);
    }
}

pub async fn stats_task(ctx: &ExecutionMode, options: StatsOptions) -> Result<()> {
    require_platform(ctx, "stats")?;
    let client = ctx.sdk_client().await?;
    let workspace = ctx
        .resolve_workspace_query_with_bootstrap(!options.json)
        .await?;
    let deployment_id = match options.deployment.as_deref() {
        Some(reference) => {
            Some(resolve_deployment_id(ctx, &client, reference, options.json).await?)
        }
        None => None,
    };
    let items = command_stats(
        &client,
        workspace.as_deref(),
        deployment_id.as_deref(),
        options.since,
        Utc::now(),
    )
    .await?;
    if options.json {
        return print_json(&items);
    }
    print_stats(&items)
}

async fn command_stats(
    client: &alien_platform_api::Client,
    workspace: Option<&str>,
    deployment_id: Option<&str>,
    since: Duration,
    now: DateTime<Utc>,
) -> Result<Vec<Value>> {
    let mut request = client.get_command_stats().created_after(now - since);
    if let Some(workspace) = workspace {
        request = request.workspace(workspace);
    }
    if let Some(id) = deployment_id {
        request = request.deployment_id(id);
    }
    let response = match request.send().await.into_sdk_error() {
        Ok(response) => response.into_inner(),
        // An older platform has no `/commands/stats`; its `/commands/{id}` route answers the path
        // with 422 (`stats` is not a command ID); other gateways answer 404 or 400.
        Err(error) if matches!(error.http_status_code, Some(404 | 400 | 422)) => {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: "this platform does not serve command statistics yet. Update the platform to a version that has `GET /v1/commands/stats`; `alien commands ls` still works.".to_string(),
                url: None,
            }));
        }
        Err(error) => {
            return Err(error).context(ErrorData::ApiRequestFailed {
                message: "getting command statistics".to_string(),
                url: None,
            });
        }
    };
    let items = serde_json::to_value(&response.items).map_err(|error| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("reading command statistics: {error}"),
            url: None,
        })
    })?;
    Ok(items.as_array().cloned().unwrap_or_default())
}

fn print_stats(items: &[Value]) -> Result<()> {
    if items.is_empty() {
        println!("(no commands in this window)");
        return Ok(());
    }
    let mut table = make_table(&[
        "Name",
        "Total",
        "Succeeded",
        "Failed",
        "Expired",
        "In flight",
        "p50",
        "p95",
        "Top errors",
    ]);
    for item in items {
        let row: StatsRow = serde_json::from_value(item.clone()).map_err(|error| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("the platform returned statistics this CLI cannot read: {error}"),
                url: None,
            })
        })?;
        let duration = |ms: Option<f64>| ms.map(format_duration_ms).unwrap_or_else(|| "—".into());
        let errors = row
            .top_error_codes
            .iter()
            .map(|e| format!("{} ×{}", e.code, e.count))
            .collect::<Vec<_>>()
            .join(", ");
        table.add_row(vec![
            row.name,
            row.total.to_string(),
            row.succeeded.to_string(),
            row.failed.to_string(),
            row.expired.to_string(),
            row.in_flight.to_string(),
            duration(row.p50_duration_ms),
            duration(row.p95_duration_ms),
            errors,
        ]);
    }
    print_table(table);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{Method::GET, MockServer};
    use serde_json::json;

    const DEP: &str = "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f";

    fn command_json(id: &str, state: &str, error: Value) -> Value {
        json!({
            "id": id, "deploymentId": DEP,
            "projectId": "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
            "workspaceId": "ws_It13CUaGEhLLAB87simX07Xb",
            "name": "compact", "state": state, "deploymentModel": "push",
            "target": {"resourceId": "worker-a", "resourceType": "worker"},
            "attempt": 1, "deadline": null,
            "requestSizeBytes": 20, "responseSizeBytes": 2048,
            "createdAt": "2026-01-01T00:00:00Z",
            "dispatchedAt": "2026-01-01T00:00:01Z",
            "completedAt": "2026-01-01T00:00:04.200Z",
            "error": error, "accessRequestId": null, "verification": null,
        })
    }

    fn options() -> ListOptions {
        ListOptions {
            deployment: None,
            name: None,
            status: None,
            since: None,
            limit: 50,
            json: true,
        }
    }

    #[test]
    fn error_line_is_one_short_line() {
        let error = json!({"code": "TIMEOUT", "message": "took too\nlong\n  to answer"});
        assert_eq!(
            error_line(error.as_object().unwrap()),
            "TIMEOUT: took too long to answer"
        );
        let long = json!({"code": "X", "message": "m".repeat(500)});
        let line = error_line(long.as_object().unwrap());
        assert_eq!(line.chars().count(), ERROR_LINE_MAX_CHARS);
        assert!(line.ends_with('…'));
        let bare = json!({});
        assert_eq!(
            error_line(bare.as_object().unwrap()),
            "failed without an error message"
        );
    }

    #[test]
    fn history_needs_the_platform() {
        let ctx = ExecutionMode::Standalone {
            server_url: "http://localhost:1".to_string(),
            api_key: "key".to_string(),
        };
        let error = require_platform(&ctx, "stats").expect_err("standalone has no history");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("alien login"), "{}", error.message);
    }

    #[test]
    fn durations_are_readable() {
        assert_eq!(format_duration_ms(850.0), "850ms");
        assert_eq!(format_duration_ms(2300.0), "2.3s");
        assert_eq!(format_duration_ms(252_000.0), "4m12s");
    }

    #[tokio::test]
    async fn ls_sends_filters_and_returns_items() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/commands")
                    .query_param("deploymentId", DEP)
                    .query_param("name", "compact")
                    .query_param("state", "FAILED")
                    .query_param("createdAfter", "2025-12-31T00:00:00Z")
                    .query_param("limit", "50");
                then.json_body(json!({
                    "items": [command_json(
                        "cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb", "FAILED",
                        json!({"code": "TIMEOUT", "message": "no answer"}),
                    )],
                    "nextCursor": null,
                }));
            })
            .await;
        let client = alien_platform_api::Client::new(&server.base_url());
        let mut opts = options();
        opts.name = Some("compact".into());
        opts.status = Some(StatusArg::Failed);
        opts.since = Some(Duration::hours(24));
        let now = "2026-01-01T00:00:00Z".parse().unwrap();
        let items = list_commands(&client, None, Some(DEP), &opts, now)
            .await
            .expect("lists");
        mock.assert_async().await;
        assert_eq!(items.len(), 1);
        let view = CommandView::from_value(&items[0]).unwrap();
        assert_eq!(view.state, "FAILED");
        assert_eq!(view.error_line().as_deref(), Some("TIMEOUT: no answer"));
        assert_eq!(format_duration(view.duration().unwrap()), "4.2s");
    }

    #[tokio::test]
    async fn ls_follows_cursors_up_to_the_limit() {
        let server = MockServer::start_async().await;
        let first = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/commands")
                    .query_param("limit", "3");
                then.json_body(json!({
                    "items": [
                        command_json("cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb", "SUCCEEDED", Value::Null),
                        command_json("cmd_2sxjXxvOYct7IohT3ukliAzf7Nzc", "SUCCEEDED", Value::Null),
                    ],
                    "nextCursor": "next-page",
                }));
            })
            .await;
        let second = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/commands")
                    .query_param("cursor", "next-page")
                    .query_param("limit", "1");
                then.json_body(json!({
                    "items": [
                        command_json("cmd_2sxjXxvOYct7IohT3ukliAzf7Nzd", "SUCCEEDED", Value::Null),
                    ],
                    "nextCursor": "more",
                }));
            })
            .await;
        let client = alien_platform_api::Client::new(&server.base_url());
        let mut opts = options();
        opts.limit = 3;
        let items = list_commands(&client, None, None, &opts, Utc::now())
            .await
            .expect("lists");
        first.assert_async().await;
        second.assert_async().await;
        assert_eq!(items.len(), 3);
    }

    #[tokio::test]
    async fn get_returns_the_failure_and_response_size() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/commands/cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb");
                then.json_body(command_json(
                    "cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb",
                    "FAILED",
                    json!({"code": "HANDLER_ERROR", "message": "index is locked"}),
                ));
            })
            .await;
        let client = alien_platform_api::Client::new(&server.base_url());
        let value = get_command(&client, None, "cmd_2sxjXxvOYct7IohT3ukliAzf7Nzb")
            .await
            .expect("gets");
        let view = CommandView::from_value(&value).unwrap();
        assert_eq!(
            view.error_line().as_deref(),
            Some("HANDLER_ERROR: index is locked")
        );
        assert_eq!(view.response_size_bytes, Some(2048.0));
        assert_eq!(format_bytes(2048.0), "2.0 KiB");
        assert_eq!(view.target.unwrap().resource_id, "worker-a");
    }

    #[tokio::test]
    async fn stats_reads_rows_and_sends_the_window() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/commands/stats")
                    .query_param("createdAfter", "2025-12-25T00:00:00Z");
                then.json_body(json!({"items": [{
                    "name": "compact", "total": 10, "succeeded": 7, "failed": 2,
                    "expired": 1, "inFlight": 0, "p50DurationMs": 120.0,
                    "p95DurationMs": 4300.0,
                    "topErrorCodes": [{"code": "TIMEOUT", "count": 2}],
                }]}));
            })
            .await;
        let client = alien_platform_api::Client::new(&server.base_url());
        let now = "2026-01-01T00:00:00Z".parse().unwrap();
        let items = command_stats(&client, None, None, Duration::days(7), now)
            .await
            .expect("stats");
        mock.assert_async().await;
        let row: StatsRow = serde_json::from_value(items[0].clone()).unwrap();
        assert_eq!((row.total, row.failed, row.expired), (10, 2, 1));
        assert_eq!(row.top_error_codes[0].code, "TIMEOUT");
        print_stats(&items).expect("prints");
    }

    #[tokio::test]
    async fn stats_on_an_older_platform_says_to_update_it() {
        for status in [404, 400, 422] {
            let server = MockServer::start_async().await;
            server
                .mock_async(|when, then| {
                    when.method(GET).path("/v1/commands/stats");
                    then.status(status).json_body(json!({
                        "code": "NOT_FOUND", "message": "not found", "retryable": false,
                        "internal": false,
                    }));
                })
                .await;
            let client = alien_platform_api::Client::new(&server.base_url());
            let error = command_stats(&client, None, None, Duration::days(7), Utc::now())
                .await
                .expect_err("an older platform has no stats");
            assert!(
                error.message.contains("Update the platform"),
                "status {status}: {}",
                error.message
            );
        }
    }
}
