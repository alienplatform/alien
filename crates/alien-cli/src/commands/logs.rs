use std::collections::{HashSet, VecDeque};
use std::convert::TryFrom;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types;
use alien_platform_api::SdkResultExt as _;
use chrono::{DateTime, Duration, Utc};
use clap::{Parser, ValueEnum};
use console::style;
use deepstore_client::{DeepstoreClient, SearchParams};
use serde::Serialize;
use serde_json::Value;

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::supports_ansi;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1000;
const SEEN_KEYS_LIMIT: usize = 20_000;
const DEFAULT_LOG_SEARCH_FIELDS: &[&str] = &["body.message"];

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Search deployment and manager logs",
    long_about = "Search logs directly in Deepstore. The platform API is used only to resolve the target manager and mint a scoped query token.",
    after_help = "EXAMPLES:
    alien logs --deployment acme/production --follow
    alien logs --source ai-gateway --status provider-error --provider anthropic
    alien logs --source ai-gateway --model byo/claude-opus-5 --since 24h --json
    alien logs --source encryption-gateway --status failed --operation decrypt"
)]
pub struct LogsArgs {
    /// Deployment ID, or <deployment-group-name>/<deployment-name>.
    #[arg(long, value_name = "DEPLOYMENT")]
    pub deployment: Option<String>,

    /// Manager ID to search when --deployment is not provided.
    #[arg(long, value_name = "MANAGER_ID")]
    pub manager: Option<String>,

    /// Project ID or name. Defaults to the linked project or global --project.
    #[arg(long)]
    pub project: Option<String>,

    /// Cloud default to use when the project has multiple default managers.
    #[arg(long, value_enum)]
    pub cloud: Option<LogsCloud>,

    /// Deepstore query text.
    #[arg(long, short = 'q', default_value = "*")]
    pub query: String,

    /// Severity band to include. Repeat to include several bands.
    #[arg(long, value_enum)]
    pub level: Vec<LogLevel>,

    /// Restrict results to a gateway request-diagnostics source.
    #[arg(long, value_enum, conflicts_with = "deployment")]
    pub source: Option<LogSource>,

    /// Gateway outcome, such as success, provider-error, not-routed, gateway-error, cancelled, or failed.
    #[arg(long, requires = "source")]
    pub status: Option<String>,

    /// Requested AI model. Only valid with --source ai-gateway.
    #[arg(long, requires = "source")]
    pub model: Option<String>,

    /// Selected AI provider. Only valid with --source ai-gateway.
    #[arg(long, requires = "source")]
    pub provider: Option<String>,

    /// Encryption operation, such as encrypt or decrypt. Only valid with --source encryption-gateway.
    #[arg(long, requires = "source")]
    pub operation: Option<String>,

    /// Deployment-group ID associated with a gateway request.
    #[arg(long, requires = "source", value_name = "DG_ID")]
    pub deployment_group: Option<String>,

    /// Relative time window, such as 30m, 2h, or 7d.
    #[arg(long, default_value = "1h", value_parser = parse_duration_arg)]
    pub since: Duration,

    /// Start timestamp in RFC3339 format. Overrides --since.
    #[arg(long, value_parser = parse_rfc3339_arg)]
    pub from: Option<DateTime<Utc>>,

    /// End timestamp in RFC3339 format. Defaults to now.
    #[arg(long, value_parser = parse_rfc3339_arg)]
    pub to: Option<DateTime<Utc>>,

    /// Maximum logs to return per query.
    #[arg(long, default_value_t = DEFAULT_LIMIT)]
    pub limit: usize,

    /// Include logs from Alien system components (hidden by default).
    #[arg(long)]
    pub system: bool,

    /// Keep polling for new logs.
    #[arg(long)]
    pub follow: bool,

    /// Poll interval used with --follow, such as 1s or 5s.
    #[arg(long, default_value = "2s", value_parser = parse_std_duration_arg)]
    pub interval: StdDuration,

    /// Output a stable machine-readable format. With --follow, emits JSON lines.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogsCloud {
    Aws,
    Gcp,
    Azure,
    Kubernetes,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogSource {
    Application,
    AiGateway,
    EncryptionGateway,
}

impl LogSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::AiGateway => "ai-gateway",
            Self::EncryptionGateway => "encryption-gateway",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogsOutput {
    target: LogsTarget,
    query: String,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    num_hits: usize,
    logs: Vec<LogEntry>,
    errors: Vec<String>,
    elapsed_time_micros: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogsTarget {
    manager_id: String,
    project_id: Option<String>,
    project_name: Option<String>,
    deployment_id: Option<String>,
    deployment_name: Option<String>,
    deployment_group_name: Option<String>,
    database_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogEntry {
    timestamp: Option<DateTime<Utc>>,
    timestamp_nanos: Option<i64>,
    level: Option<String>,
    severity_number: Option<i64>,
    service: Option<String>,
    deployment_id: Option<String>,
    message: String,
    raw: Value,
}

#[derive(Debug, Clone)]
struct ResolvedLogsTarget {
    manager_id: String,
    project_id: Option<String>,
    project_name: Option<String>,
    deployment_id: Option<String>,
    deployment_name: Option<String>,
    deployment_group_name: Option<String>,
}

pub async fn logs_task(args: LogsArgs, ctx: ExecutionMode) -> Result<()> {
    if ctx.is_dev() || ctx.is_standalone() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "`alien logs` requires Alien platform authentication so it can mint a Deepstore query token."
                    .to_string(),
        }));
    }

    validate_args(&args)?;

    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let platform_client = ctx.sdk_client().await?;
    let project_override = args.project.as_deref().or(ctx.project_override());
    let target =
        resolve_logs_target(&ctx, &platform_client, &workspace, &args, project_override).await?;
    let token_scope_project = target.project_id.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message:
                "A project is required to query logs. Pass --project <name> or run `alien link`."
                    .to_string(),
        })
    })?;
    let token = generate_logs_token(
        &platform_client,
        &workspace,
        &target.manager_id,
        token_scope_project,
    )
    .await?;

    let database_id = token.database_id.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Manager {} does not have Deepstore logs configured.",
                target.manager_id
            ),
        })
    })?;
    let control_plane_url = token.control_plane_url.clone().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Manager {} did not return a Deepstore control plane URL.",
                target.manager_id
            ),
        })
    })?;

    let shared_client = Arc::new(platform_client);
    let provider_workspace = workspace.clone();
    let provider_manager = target.manager_id.clone();
    let provider_project = token_scope_project.to_string();
    let deepstore = DeepstoreClient::builder()
        .control_plane_url(control_plane_url)
        .auth_proxy_url(token.manager_url)
        .query_token_provider(move || {
            let client = Arc::clone(&shared_client);
            let workspace = provider_workspace.clone();
            let manager_id = provider_manager.clone();
            let project = provider_project.clone();
            async move {
                let token = generate_logs_token(&client, &workspace, &manager_id, &project)
                    .await
                    .map_err(|error| {
                        deepstore_client::DeepstoreError::Authentication(error.to_string())
                    })?;
                Ok(token.access_token)
            }
        })
        .build()
        .map_err(|error| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("Failed to configure Deepstore client: {error}"),
            })
        })?;

    let rendered_target = LogsTarget {
        manager_id: target.manager_id.clone(),
        project_id: target.project_id.clone(),
        project_name: target.project_name.clone(),
        deployment_id: target.deployment_id.clone(),
        deployment_name: target.deployment_name.clone(),
        deployment_group_name: target.deployment_group_name.clone(),
        database_id: database_id.clone(),
    };
    let deployment_filter =
        deployment_filter_for_source(args.source, target.deployment_id.as_deref());
    let query = build_logs_query(LogQueryFilters {
        user_query: &args.query,
        levels: &args.level,
        deployment_id: deployment_filter,
        include_system: args.system || args.source.is_some(),
        source: args.source,
        status: args.status.as_deref(),
        model: args.model.as_deref(),
        provider: args.provider.as_deref(),
        operation: args.operation.as_deref(),
        deployment_group: args.deployment_group.as_deref(),
    })?;
    let (start_time, end_time) = resolve_time_window(&args);

    if args.follow {
        follow_logs(
            &deepstore,
            &database_id,
            &query,
            &rendered_target,
            start_time,
            args.limit,
            args.interval,
            args.json,
        )
        .await
    } else {
        let output = fetch_logs(
            &deepstore,
            &database_id,
            &query,
            &rendered_target,
            start_time,
            end_time,
            args.limit,
        )
        .await?;
        if args.json {
            print_json(&output)
        } else {
            render_human_logs(&output.logs);
            Ok(())
        }
    }
}

fn validate_args(args: &LogsArgs) -> Result<()> {
    if args.deployment.is_some() && args.manager.is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "target".to_string(),
            message: "Use either --deployment or --manager, not both.".to_string(),
        }));
    }
    if args.query.trim().is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "query".to_string(),
            message: "Query cannot be empty.".to_string(),
        }));
    }
    if args.limit == 0 || args.limit > MAX_LIMIT {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "limit".to_string(),
            message: format!("Limit must be between 1 and {MAX_LIMIT}."),
        }));
    }
    if args.interval.is_zero() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "interval".to_string(),
            message: "Interval must be greater than zero.".to_string(),
        }));
    }
    if args.source != Some(LogSource::AiGateway)
        && (args.model.is_some() || args.provider.is_some())
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "source".to_string(),
            message: "--model and --provider require `--source ai-gateway`.".to_string(),
        }));
    }
    if args.source != Some(LogSource::EncryptionGateway) && args.operation.is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "source".to_string(),
            message: "--operation requires `--source encryption-gateway`.".to_string(),
        }));
    }
    if let (Some(from), Some(to)) = (args.from, args.to) {
        if from > to {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "time_window".to_string(),
                message: "--from must be before --to.".to_string(),
            }));
        }
    }
    Ok(())
}

async fn resolve_logs_target(
    ctx: &ExecutionMode,
    client: &alien_platform_api::Client,
    workspace: &str,
    args: &LogsArgs,
    project_override: Option<&str>,
) -> Result<ResolvedLogsTarget> {
    if let Some(deployment) = args.deployment.as_deref() {
        return resolve_deployment_target(
            ctx,
            client,
            workspace,
            deployment,
            project_override,
            !args.json,
        )
        .await;
    }

    if let Some(manager_id) = args.manager.as_deref() {
        let (project_id, project_name) =
            resolve_optional_project(ctx, project_override, workspace, !args.json).await?;
        return Ok(ResolvedLogsTarget {
            manager_id: manager_id.to_string(),
            project_id,
            project_name,
            deployment_id: None,
            deployment_name: None,
            deployment_group_name: None,
        });
    }

    resolve_default_manager_target(
        ctx,
        client,
        workspace,
        project_override,
        args.cloud,
        !args.json,
    )
    .await
}

async fn resolve_deployment_target(
    ctx: &ExecutionMode,
    client: &alien_platform_api::Client,
    workspace: &str,
    deployment: &str,
    project_override: Option<&str>,
    allow_prompt: bool,
) -> Result<ResolvedLogsTarget> {
    #[cfg(feature = "platform")]
    {
        let response = crate::platform_deployment_resolver::resolve(
            ctx,
            client,
            workspace,
            deployment,
            project_override,
            allow_prompt,
        )
        .await?;
        return target_from_deployment_detail(response, None).await;
    }

    #[cfg(not(feature = "platform"))]
    {
        let _ = (
            ctx,
            client,
            workspace,
            deployment,
            project_override,
            allow_prompt,
        );
        Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Deployment log discovery requires platform support.".to_string(),
        }))
    }
}

async fn target_from_deployment_detail(
    deployment: types::DeploymentDetailResponse,
    group_name: Option<String>,
) -> Result<ResolvedLogsTarget> {
    let deployment_id: String = deployment.id.clone().into();
    let project_id: String = deployment.project_id.clone().into();
    let manager_id = String::from(deployment.manager_id.clone());
    let deployment_name: String = deployment.name.clone().into();
    let project_name = deployment
        .project
        .as_ref()
        .map(|project| String::from(project.name.clone()));
    let deployment_group_name = group_name.or_else(|| {
        deployment
            .deployment_group
            .as_ref()
            .map(|group| String::from(group.name.clone()))
    });

    Ok(ResolvedLogsTarget {
        manager_id,
        project_id: Some(project_id),
        project_name,
        deployment_id: Some(deployment_id),
        deployment_name: Some(deployment_name),
        deployment_group_name,
    })
}

async fn resolve_default_manager_target(
    ctx: &ExecutionMode,
    client: &alien_platform_api::Client,
    workspace: &str,
    project_override: Option<&str>,
    cloud: Option<LogsCloud>,
    allow_prompt: bool,
) -> Result<ResolvedLogsTarget> {
    let (project_id, project_name) =
        resolve_project_scope(ctx, project_override, workspace, allow_prompt).await?;
    let selected = select_logs_manager(client, workspace, cloud).await?;

    Ok(ResolvedLogsTarget {
        manager_id: selected.id.to_string(),
        project_id,
        project_name,
        deployment_id: None,
        deployment_name: None,
        deployment_group_name: None,
    })
}

async fn resolve_optional_project(
    ctx: &ExecutionMode,
    project_override: Option<&str>,
    workspace: &str,
    allow_prompt: bool,
) -> Result<(Option<String>, Option<String>)> {
    resolve_project_scope(ctx, project_override, workspace, allow_prompt).await
}

async fn resolve_project_scope(
    ctx: &ExecutionMode,
    project_override: Option<&str>,
    workspace: &str,
    allow_prompt: bool,
) -> Result<(Option<String>, Option<String>)> {
    if project_override.is_some() {
        let (project_id, link) = ctx.resolve_project(project_override, allow_prompt).await?;
        return Ok((Some(project_id), Some(link.project_name)));
    }

    let current_dir = crate::get_current_dir()?;
    match crate::project_link::get_project_link_status(&current_dir) {
        crate::project_link::ProjectLinkStatus::Linked(link)
            if workspace.is_empty() || link.workspace == workspace =>
        {
            Ok((Some(link.project_id), Some(link.project_name)))
        }
        crate::project_link::ProjectLinkStatus::Error(message) => {
            Err(AlienError::new(ErrorData::ConfigurationError { message }))
        }
        _ => Ok((None, None)),
    }
}

async fn select_logs_manager(
    client: &alien_platform_api::Client,
    workspace: &str,
    cloud: Option<LogsCloud>,
) -> Result<types::Manager> {
    let workspace_param = types::ListManagersWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let managers = client
        .list_managers()
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list managers for logs".to_string(),
            url: None,
        })?
        .into_inner();

    managers
        .into_iter()
        .find(|manager| {
            manager.logs_database_id.is_some()
                && cloud
                    .map(|cloud| {
                        manager
                            .targets
                            .iter()
                            .any(|target| target.to_string() == cloud.as_str())
                    })
                    .unwrap_or(true)
        })
        .ok_or_else(|| {
            let message = if let Some(cloud) = cloud {
                format!(
                    "No manager with a Deepstore logs database is available for {}. Pass --manager <mgr_...> or try without --cloud.",
                    cloud.as_str()
                )
            } else {
                "No managers have Deepstore logs databases configured. Deploy a manager or pass --manager <mgr_...>.".to_string()
            };
            AlienError::new(ErrorData::ConfigurationError { message })
        })
}

impl LogsCloud {
    fn as_str(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Gcp => "gcp",
            Self::Azure => "azure",
            Self::Kubernetes => "kubernetes",
            Self::Local => "local",
        }
    }
}

async fn generate_logs_token(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
    project: &str,
) -> Result<types::GenerateManagerTokenResponse> {
    let project = types::GenerateManagerTokenRequestProject::try_from(project)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "project".to_string(),
            message: "Invalid project scope for logs token".to_string(),
        })?;

    client
        .generate_manager_token()
        .id(manager_id)
        .workspace(workspace)
        .body(types::GenerateManagerTokenRequest { project })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("Failed to generate Deepstore token for manager {manager_id}"),
            url: None,
        })
        .map(|response| response.into_inner())
}

async fn fetch_logs(
    client: &DeepstoreClient,
    database_id: &str,
    query: &str,
    target: &LogsTarget,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    limit: usize,
) -> Result<LogsOutput> {
    let result = client
        .search(SearchParams {
            database_id: database_id.to_string(),
            query: query.to_string(),
            start_time,
            end_time,
            max_hits: Some(limit),
            sort_by: Some("-timestamp_nanos".to_string()),
            search_fields: Some(
                DEFAULT_LOG_SEARCH_FIELDS
                    .iter()
                    .map(|field| field.to_string())
                    .collect(),
            ),
            ..Default::default()
        })
        .await
        .map_err(|error| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("Deepstore search failed: {error}"),
                url: None,
            })
        })?;

    let mut logs = result
        .hits
        .into_iter()
        .map(log_entry_from_hit)
        .collect::<Vec<_>>();
    logs.sort_by_key(|entry| entry.timestamp_nanos.unwrap_or(i64::MIN));

    Ok(LogsOutput {
        target: target.clone(),
        query: query.to_string(),
        start_time,
        end_time,
        num_hits: result.num_hits,
        logs,
        errors: result.errors,
        elapsed_time_micros: result.elapsed_time_micros,
    })
}

async fn follow_logs(
    client: &DeepstoreClient,
    database_id: &str,
    query: &str,
    target: &LogsTarget,
    start_time: DateTime<Utc>,
    limit: usize,
    interval: StdDuration,
    json: bool,
) -> Result<()> {
    if json {
        emit_json_line(&serde_json::json!({
            "type": "metadata",
            "target": target,
            "query": query,
            "startTime": start_time,
        }))?;
    }

    let mut seen = SeenLogKeys::default();
    let mut cursor_start = start_time;

    loop {
        let output = fetch_logs(
            client,
            database_id,
            query,
            target,
            cursor_start,
            Utc::now(),
            limit,
        )
        .await?;

        let mut newest_timestamp = None;
        for entry in output.logs {
            if let Some(timestamp) = entry.timestamp {
                newest_timestamp = Some(newest_timestamp.map_or(timestamp, |current| {
                    if timestamp > current {
                        timestamp
                    } else {
                        current
                    }
                }));
            }

            if !seen.insert(log_key(&entry)) {
                continue;
            }

            if json {
                emit_json_line(&serde_json::json!({
                    "type": "log",
                    "log": entry,
                }))?;
            } else {
                render_human_log(&entry);
            }
        }

        if let Some(newest) = newest_timestamp {
            cursor_start = newest - Duration::seconds(1);
        }

        tokio::time::sleep(interval).await;
    }
}

#[derive(Default)]
struct SeenLogKeys {
    ordered: VecDeque<String>,
    set: HashSet<String>,
}

impl SeenLogKeys {
    fn insert(&mut self, key: String) -> bool {
        if !self.set.insert(key.clone()) {
            return false;
        }
        self.ordered.push_back(key);
        while self.ordered.len() > SEEN_KEYS_LIMIT {
            if let Some(oldest) = self.ordered.pop_front() {
                self.set.remove(&oldest);
            }
        }
        true
    }
}

fn render_human_logs(logs: &[LogEntry]) {
    if logs.is_empty() {
        println!("(no logs)");
        return;
    }
    for entry in logs {
        render_human_log(entry);
    }
}

fn render_human_log(entry: &LogEntry) {
    let timestamp = entry
        .timestamp
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string())
        .unwrap_or_else(|| "-".to_string());
    let level = entry.level.as_deref().unwrap_or("-");
    let service = entry.service.as_deref().unwrap_or("-");

    let rendered_level = render_level(level);
    println!(
        "{} {} {} {}",
        dim(&timestamp),
        rendered_level,
        dim(service),
        entry.message
    );
}

fn render_level(level: &str) -> String {
    let padded = format!("{level:<5}");
    if !supports_ansi() {
        return padded;
    }
    match level.to_ascii_uppercase().as_str() {
        "TRACE" => style(padded).dim().to_string(),
        "DEBUG" => style(padded).cyan().to_string(),
        "INFO" => style(padded).green().to_string(),
        "WARN" | "WARNING" => style(padded).yellow().to_string(),
        "ERROR" => style(padded).red().to_string(),
        "FATAL" => style(padded).red().bold().to_string(),
        _ => padded,
    }
}

fn dim(value: &str) -> String {
    if supports_ansi() {
        style(value).dim().to_string()
    } else {
        value.to_string()
    }
}

fn emit_json_line(value: &impl Serialize) -> Result<()> {
    let line = serde_json::to_string(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "Failed to serialize JSON line".to_string(),
        })?;
    println!("{line}");
    io::stdout()
        .flush()
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "flush".to_string(),
            file_path: "stdout".to_string(),
            reason: "Failed to flush log output".to_string(),
        })?;
    Ok(())
}

fn log_entry_from_hit(hit: Value) -> LogEntry {
    let timestamp_nanos = hit.get("timestamp_nanos").and_then(Value::as_i64);
    let timestamp = timestamp_nanos.and_then(timestamp_from_nanos);
    let level = hit
        .get("severity_text")
        .and_then(Value::as_str)
        .map(str::to_string);
    let severity_number = hit.get("severity_number").and_then(Value::as_i64);
    let resource_attrs = hit.get("resource_attributes");
    let service = hit
        .get("service_name")
        .and_then(Value::as_str)
        .filter(|service| *service != "alien-worker-runtime")
        .map(str::to_string)
        .or_else(|| {
            resource_attrs?
                .get("service.name")
                .and_then(Value::as_str)
                .filter(|service| *service != "alien-worker-runtime")
                .map(str::to_string)
        });
    let deployment_id = resource_attrs
        .and_then(|attrs| attrs.get("alien.deployment_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let message = extract_message(&hit);

    LogEntry {
        timestamp,
        timestamp_nanos,
        level,
        severity_number,
        service,
        deployment_id,
        message,
        raw: hit,
    }
}

fn extract_message(hit: &Value) -> String {
    if let Some(message) = hit
        .get("body")
        .and_then(|body| body.get("message"))
        .and_then(Value::as_str)
    {
        return message.to_string();
    }
    if let Some(body) = hit.get("body").and_then(Value::as_str) {
        return body.to_string();
    }
    if let Some(body) = hit.get("body") {
        return body.to_string();
    }
    hit.to_string()
}

fn timestamp_from_nanos(timestamp_nanos: i64) -> Option<DateTime<Utc>> {
    let secs = timestamp_nanos.div_euclid(1_000_000_000);
    let nanos = timestamp_nanos.rem_euclid(1_000_000_000) as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos)
}

fn log_key(entry: &LogEntry) -> String {
    format!(
        "{}:{}:{}:{}",
        entry.timestamp_nanos.unwrap_or_default(),
        entry.level.as_deref().unwrap_or_default(),
        entry.service.as_deref().unwrap_or_default(),
        entry.message
    )
}

fn resolve_time_window(args: &LogsArgs) -> (DateTime<Utc>, DateTime<Utc>) {
    let end = args.to.unwrap_or_else(Utc::now);
    let start = args.from.unwrap_or(end - args.since);
    (start, end)
}

struct LogQueryFilters<'a> {
    user_query: &'a str,
    levels: &'a [LogLevel],
    deployment_id: Option<&'a str>,
    include_system: bool,
    source: Option<LogSource>,
    status: Option<&'a str>,
    model: Option<&'a str>,
    provider: Option<&'a str>,
    operation: Option<&'a str>,
    deployment_group: Option<&'a str>,
}

fn build_logs_query(options: LogQueryFilters<'_>) -> Result<String> {
    let mut filters = Vec::new();
    let query = options.user_query.trim();
    if query != "*" {
        filters.push(format!("({query})"));
    }
    if !options.levels.is_empty() {
        let level_filters = options
            .levels
            .iter()
            .map(|level| {
                let (min, max) = severity_range(*level);
                format!("(severity_number:>={min} AND severity_number:<={max})")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        filters.push(format!("({level_filters})"));
    }
    if let Some(deployment_id) = options.deployment_id {
        filters.push(format!(
            "{}:\"{}\"",
            deployment_id_resource_attribute_field(),
            escape_query_string(deployment_id)
        ));
    }
    if let Some(source) = options.source {
        if source == LogSource::Application {
            filters.push("NOT attributes.alien\\.log\\.source:*".to_string());
        } else {
            filters.push(format!(
                "attributes.alien\\.log\\.source:\"{}\"",
                source.as_str()
            ));
        }
    }
    for (field, value) in [
        ("gateway\\.outcome", options.status),
        ("ai\\.model\\.requested", options.model),
        ("ai\\.provider", options.provider),
        ("encryption\\.operation", options.operation),
        ("alien\\.deployment_group_id", options.deployment_group),
    ] {
        if let Some(value) = value {
            filters.push(format!(
                "attributes.{field}:\"{}\"",
                escape_query_string(value)
            ));
        }
    }
    if !options.include_system {
        // Hide Alien system-component logs (e.g. infrastructure daemons) unless
        // explicitly requested. `NOT field:value` keeps documents that lack the
        // attribute entirely, so user workload logs are always shown.
        filters.push(format!(
            "NOT {}:\"true\" AND NOT {}:\"true\"",
            system_resource_attribute_field(),
            system_log_attribute_field()
        ));
    }

    if filters.is_empty() {
        Ok("*".to_string())
    } else if filters.iter().all(|filter| filter.starts_with("NOT ")) {
        // Quickwit/tantivy rejects a query made up of only negative clauses, so
        // prepend a match-all positive term.
        Ok(format!("* AND {}", filters.join(" AND ")))
    } else {
        Ok(filters.join(" AND "))
    }
}

fn deployment_filter_for_source(
    source: Option<LogSource>,
    deployment_id: Option<&str>,
) -> Option<&str> {
    if source.is_some() {
        None
    } else {
        deployment_id
    }
}

fn severity_range(level: LogLevel) -> (u8, u8) {
    match level {
        LogLevel::Trace => (1, 4),
        LogLevel::Debug => (5, 8),
        LogLevel::Info => (9, 12),
        LogLevel::Warn => (13, 16),
        LogLevel::Error => (17, 20),
        LogLevel::Fatal => (21, 24),
    }
}

fn escape_query_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn deployment_id_resource_attribute_field() -> &'static str {
    // `alien.deployment_id` is a single OpenTelemetry resource attribute key
    // nested under `resource_attributes`, not two nested JSON fields.
    "resource_attributes.alien\\.deployment_id"
}

fn system_resource_attribute_field() -> String {
    // The marker key contains a dot that is part of the attribute name, not a
    // path separator — escape it so Quickwit treats the segment under
    // `resource_attributes.` as a single field name.
    format!(
        "resource_attributes.{}",
        alien_core::ALIEN_SYSTEM_RESOURCE_ATTRIBUTE.replace('.', "\\.")
    )
}

fn system_log_attribute_field() -> String {
    format!(
        "attributes.{}",
        alien_core::ALIEN_SYSTEM_RESOURCE_ATTRIBUTE.replace('.', "\\.")
    )
}

fn parse_rfc3339_arg(value: &str) -> std::result::Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| format!("invalid RFC3339 timestamp: {error}"))
}

fn parse_duration_arg(value: &str) -> std::result::Result<Duration, String> {
    let duration = parse_std_duration_arg(value)?;
    Duration::from_std(duration).map_err(|error| format!("duration is too large: {error}"))
}

fn parse_std_duration_arg(value: &str) -> std::result::Result<StdDuration, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    let unit_start = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .ok_or_else(|| "duration must include a unit: s, m, h, or d".to_string())?;
    let (amount, unit) = trimmed.split_at(unit_start);
    let amount = amount
        .parse::<u64>()
        .map_err(|error| format!("invalid duration amount: {error}"))?;
    let seconds = match unit {
        "s" => amount,
        "m" => amount.saturating_mul(60),
        "h" => amount.saturating_mul(60 * 60),
        "d" => amount.saturating_mul(24 * 60 * 60),
        _ => return Err("duration unit must be one of: s, m, h, d".to_string()),
    };
    Ok(StdDuration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_query<'a>(
        user_query: &'a str,
        levels: &'a [LogLevel],
        deployment_id: Option<&'a str>,
        include_system: bool,
        source: Option<LogSource>,
    ) -> Result<String> {
        build_logs_query(LogQueryFilters {
            user_query,
            levels,
            deployment_id,
            include_system,
            source,
            status: None,
            model: None,
            provider: None,
            operation: None,
            deployment_group: None,
        })
    }

    #[test]
    fn builds_query_with_levels_and_deployment() {
        // `include_system = true` keeps the system-exclusion clause out of the
        // way so this asserts only the level + deployment composition.
        let query = test_query(
            "service_name:api",
            &[LogLevel::Warn, LogLevel::Error],
            Some("dep_123"),
            true,
            None,
        )
        .unwrap();

        assert_eq!(
            query,
            "(service_name:api) AND ((severity_number:>=13 AND severity_number:<=16) OR (severity_number:>=17 AND severity_number:<=20)) AND resource_attributes.alien\\.deployment_id:\"dep_123\""
        );
    }

    #[test]
    fn deployment_filter_escapes_resource_attribute_key_dot() {
        let query = test_query("*", &[], Some("dep_123"), true, None).unwrap();

        assert_eq!(
            query,
            "resource_attributes.alien\\.deployment_id:\"dep_123\""
        );
    }

    #[test]
    fn hides_system_components_by_default_with_match_all_base() {
        // Default (no query, levels, or deployment) must not produce a
        // pure-negative query — it needs a positive `*` base.
        let query = test_query("*", &[], None, false, None).unwrap();

        assert_eq!(
            query,
            "* AND NOT resource_attributes.alien\\.system:\"true\" AND NOT attributes.alien\\.system:\"true\""
        );
    }

    #[test]
    fn system_flag_omits_exclusion() {
        let query = test_query("*", &[], None, true, None).unwrap();

        assert_eq!(query, "*");
    }

    #[test]
    fn system_exclusion_appends_after_positive_clauses() {
        let query = test_query(
            "service_name:api",
            &[LogLevel::Error],
            Some("dep_123"),
            false,
            None,
        )
        .unwrap();

        assert_eq!(
            query,
            "(service_name:api) AND ((severity_number:>=17 AND severity_number:<=20)) AND resource_attributes.alien\\.deployment_id:\"dep_123\" AND NOT resource_attributes.alien\\.system:\"true\" AND NOT attributes.alien\\.system:\"true\""
        );
    }

    #[test]
    fn gateway_source_filter_uses_the_trusted_record_attribute() {
        let query = test_query("*", &[], None, true, Some(LogSource::AiGateway)).unwrap();

        assert_eq!(query, "attributes.alien\\.log\\.source:\"ai-gateway\"");
    }

    #[test]
    fn gateway_filters_use_structured_attributes() {
        let query = build_logs_query(LogQueryFilters {
            user_query: "*",
            levels: &[],
            deployment_id: None,
            include_system: true,
            source: Some(LogSource::AiGateway),
            status: Some("provider-error"),
            model: Some("byo/claude-opus-5"),
            provider: Some("anthropic"),
            operation: None,
            deployment_group: Some("dg_123"),
        })
        .unwrap();

        assert_eq!(
            query,
            "attributes.alien\\.log\\.source:\"ai-gateway\" AND attributes.gateway\\.outcome:\"provider-error\" AND attributes.ai\\.model\\.requested:\"byo/claude-opus-5\" AND attributes.ai\\.provider:\"anthropic\" AND attributes.alien\\.deployment_group_id:\"dg_123\""
        );
    }

    #[test]
    fn gateway_source_rejects_a_deployment_filter() {
        let error =
            LogsArgs::try_parse_from(["logs", "--source", "ai-gateway", "--deployment", "dep_123"])
                .expect_err("gateway diagnostics are not deployment-scoped");

        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn gateway_source_ignores_a_deployment_inferred_from_local_context() {
        assert_eq!(
            deployment_filter_for_source(Some(LogSource::AiGateway), Some("dep_linked")),
            None
        );
        assert_eq!(
            deployment_filter_for_source(None, Some("dep_linked")),
            Some("dep_linked")
        );
    }

    #[test]
    fn parses_duration_units() {
        assert_eq!(
            parse_std_duration_arg("30s").unwrap(),
            StdDuration::from_secs(30)
        );
        assert_eq!(
            parse_std_duration_arg("5m").unwrap(),
            StdDuration::from_secs(300)
        );
        assert_eq!(
            parse_std_duration_arg("2h").unwrap(),
            StdDuration::from_secs(7200)
        );
        assert_eq!(
            parse_std_duration_arg("1d").unwrap(),
            StdDuration::from_secs(86400)
        );
    }

    #[test]
    fn extracts_log_entry_fields() {
        let entry = log_entry_from_hit(serde_json::json!({
            "timestamp_nanos": 1_700_000_000_123_000_000i64,
            "severity_text": "INFO",
            "severity_number": 9,
            "service_name": "api",
            "body": { "message": "ready" },
            "resource_attributes": {
                "alien.deployment_id": "dep_abc"
            }
        }));

        assert_eq!(entry.message, "ready");
        assert_eq!(entry.level.as_deref(), Some("INFO"));
        assert_eq!(entry.service.as_deref(), Some("api"));
        assert_eq!(entry.deployment_id.as_deref(), Some("dep_abc"));
        assert_eq!(
            entry.timestamp.unwrap().to_rfc3339(),
            "2023-11-14T22:13:20.123+00:00"
        );
    }
}
