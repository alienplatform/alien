use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::{self, Write};
use std::time::{Duration as StdDuration, Instant};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::{types, ClientInfo as _, SdkResultExt as _};
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use clap::{Parser, ValueEnum};
use console::style;
use serde::Serialize;
use serde_json::{Map, Value};

use super::logs_data::{DataClient, GrantSource, QueryBody, NEWEST_FIRST, OLDEST_FIRST};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::supports_ansi;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1000;
/// Rows requested per page. A larger `--limit` is collected over several pages.
const PAGE_SIZE: usize = 500;
/// A follow poll starts this far before the newest row seen, so rows indexed
/// slightly late still show up. Duplicates are dropped by `SeenLogKeys`.
const FOLLOW_OVERLAP_SECONDS: i64 = 1;
/// Projects named in the error shown when no project is selected.
const READABLE_PROJECTS_SHOWN: usize = 20;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show and search deployment logs",
    long_about = "Show and search deployment logs.\n\nA manager you run keeps recent logs for each deployment; long-term search lives in the OpenTelemetry backend it forwards to.\n\nLogs are read one project at a time: pass --project or --deployment, or run the command in a linked directory.",
    after_help = "EXAMPLES:
    alien logs --deployment acme/production --follow
    alien logs --source ai-gateway --status provider-error --provider anthropic
    alien logs --source ai-gateway --model byo/claude-opus-5 --since 24h --json
    alien logs --source encryption-gateway --status failed --operation decrypt
    alien logs --source remote-sandbox --since 1h"
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

    /// Search text, such as `timeout`, `body.message:timeout`, or `resource.service.name:api`.
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

    /// Start timestamp in RFC3339 format, or YYYY-MM-DD (midnight UTC). Overrides --since.
    #[arg(long, value_parser = parse_rfc3339_arg)]
    pub from: Option<DateTime<Utc>>,

    /// End timestamp in RFC3339 format, or YYYY-MM-DD (midnight UTC). Defaults to now.
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
    RemoteSandbox,
}

impl LogSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::AiGateway => "ai-gateway",
            Self::EncryptionGateway => "encryption-gateway",
            Self::RemoteSandbox => "sandbox-gateway",
        }
    }

    /// The value a user passes to `--source`.
    fn flag(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::AiGateway => "ai-gateway",
            Self::EncryptionGateway => "encryption-gateway",
            Self::RemoteSandbox => "remote-sandbox",
        }
    }

    /// Request logs for these sources live only on the system manager, regardless of which
    /// manager runs the project's deployments.
    fn is_stored_on_system_manager(self) -> bool {
        matches!(self, Self::AiGateway | Self::RemoteSandbox)
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
    /// The search text sent to the data service.
    query: String,
    /// Exact-match filters sent with the search text.
    filter: Map<String, Value>,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    num_hits: u64,
    logs: Vec<LogEntry>,
    elapsed_time_micros: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogsTarget {
    manager_id: String,
    project_id: String,
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
    project_id: String,
    project_name: Option<String>,
    deployment_id: Option<String>,
    deployment_name: Option<String>,
    deployment_group_name: Option<String>,
    /// The deployment's Operator reported that it sends no logs.
    log_collector_off: bool,
}

pub async fn logs_task(args: LogsArgs, ctx: ExecutionMode) -> Result<()> {
    if ctx.is_dev() || ctx.is_standalone() {
        return super::logs_manager::manager_logs_task(args, ctx).await;
    }

    validate_args(&args)?;

    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let platform_client = ctx.sdk_client().await?;
    let project_override = args.project.as_deref().or(ctx.project_override());
    let target =
        resolve_logs_target(&ctx, &platform_client, &workspace, &args, project_override).await?;

    let mut data = DataClient::new(GrantSource::new(
        platform_client,
        workspace,
        target.manager_id.clone(),
        target.project_id.clone(),
    ))?;
    // Ask for the grant before building the query, so a missing permission or
    // data service is reported before anything else.
    let database_id = data.grant().await?.database_id.clone();

    let rendered_target = LogsTarget {
        manager_id: target.manager_id.clone(),
        project_id: target.project_id.clone(),
        project_name: target.project_name.clone(),
        deployment_id: target.deployment_id.clone(),
        deployment_name: target.deployment_name.clone(),
        deployment_group_name: target.deployment_group_name.clone(),
        database_id,
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
    });
    let (start_time, end_time) = resolve_time_window(&args);

    if args.follow {
        follow_logs(
            &mut data,
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
            &mut data,
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
            render_human_logs(&output.logs, target.log_collector_off, args.source);
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
    if !(1..=MAX_LIMIT).contains(&args.limit) {
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
    if let Some(source) = args
        .source
        .filter(|source| source.is_stored_on_system_manager())
    {
        if args.manager.is_some() || args.cloud.is_some() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "source".to_string(),
                message: format!(
                    "`--source {}` reads the system manager's logs, so it does not take --manager or --cloud.",
                    source.flag()
                ),
            }));
        }
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

    // A data grant covers one project, so the project is settled first.
    let (project_id, project_name) =
        require_project(ctx, client, workspace, project_override, !args.json).await?;
    let manager_id = match args.manager.as_deref() {
        Some(manager_id) => manager_id.to_string(),
        None => select_logs_manager(client, workspace, args.cloud, args.source)
            .await?
            .id
            .to_string(),
    };
    Ok(ResolvedLogsTarget {
        manager_id,
        project_id,
        project_name,
        deployment_id: None,
        deployment_name: None,
        deployment_group_name: None,
        log_collector_off: false,
    })
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
            message: "This manager doesn't index logs for discovery; pass --deployment."
                .to_string(),
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
    let log_collector_off = log_collector_reported_off(
        &deployment.platform.to_string(),
        deployment.capabilities.as_deref(),
    );

    Ok(ResolvedLogsTarget {
        manager_id,
        project_id,
        project_name,
        deployment_id: Some(deployment_id),
        deployment_name: Some(deployment_name),
        deployment_group_name,
        log_collector_off,
    })
}

/// The project whose logs to read: `--project`, else the linked project. Without
/// either, say which projects can be read.
async fn require_project(
    ctx: &ExecutionMode,
    client: &alien_platform_api::Client,
    workspace: &str,
    project_override: Option<&str>,
    allow_prompt: bool,
) -> Result<(String, Option<String>)> {
    match resolve_project_scope(ctx, project_override, workspace, allow_prompt).await? {
        Some((project_id, project_name)) => Ok((project_id, Some(project_name))),
        None => Err(AlienError::new(ErrorData::LogsProjectRequired {
            readable: readable_projects_suffix(client, workspace).await,
        })),
    }
}

/// " Readable projects: name (id), ..." for the no-project error. Empty when the
/// list cannot be fetched, since the error is still useful without it.
async fn readable_projects_suffix(client: &alien_platform_api::Client, workspace: &str) -> String {
    let Ok(workspace) = types::ListProjectsWorkspace::try_from(workspace) else {
        return String::new();
    };
    let Ok(response) = client.list_projects().workspace(&workspace).send().await else {
        return String::new();
    };
    let projects = response.into_inner().items;
    if projects.is_empty() {
        return " This workspace has no projects.".to_string();
    }
    let mut listed = projects
        .iter()
        .take(READABLE_PROJECTS_SHOWN)
        .map(|project| format!("{} ({})", project.name.as_str(), project.id.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    if projects.len() > READABLE_PROJECTS_SHOWN {
        listed.push_str(&format!(
            ", and {} more",
            projects.len() - READABLE_PROJECTS_SHOWN
        ));
    }
    format!(" Readable projects: {listed}.")
}

/// `--project` or the linked project, as `(id, name)`. `None` when neither
/// applies. A project linked in a different workspace does not apply.
async fn resolve_project_scope(
    ctx: &ExecutionMode,
    project_override: Option<&str>,
    workspace: &str,
    allow_prompt: bool,
) -> Result<Option<(String, String)>> {
    if project_override.is_some() {
        let (project_id, link) = ctx.resolve_project(project_override, allow_prompt).await?;
        return Ok(Some((project_id, link.project_name)));
    }

    let current_dir = crate::get_current_dir()?;
    match crate::project_link::get_project_link_status(&current_dir) {
        crate::project_link::ProjectLinkStatus::Linked(link)
            if workspace.is_empty() || link.workspace == workspace =>
        {
            Ok(Some((link.project_id, link.project_name)))
        }
        crate::project_link::ProjectLinkStatus::Error(message) => {
            Err(AlienError::new(ErrorData::ConfigurationError { message }))
        }
        crate::project_link::ProjectLinkStatus::Linked(link) => {
            if allow_prompt {
                eprintln!(
                    "The linked project is in workspace '{}', not '{}', so it is not used.",
                    link.workspace, workspace
                );
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

async fn select_logs_manager(
    client: &alien_platform_api::Client,
    workspace: &str,
    cloud: Option<LogsCloud>,
    source: Option<LogSource>,
) -> Result<types::Manager> {
    let system_source = source.filter(|source| source.is_stored_on_system_manager());
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
            if system_source.is_some() {
                return manager.is_system && manager.logs_database_id.is_some();
            }
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
            let message = if let Some(source) = system_source {
                format!(
                    "No system manager with a log data service is listed at {}, and `--source {}` request logs are stored only on the system manager. Check that ALIEN_BASE_URL points at the Alien API you meant to use.",
                    client.baseurl(),
                    source.flag()
                )
            } else if let Some(cloud) = cloud {
                format!(
                    "No manager with a log data service is available for {}. Pass --manager <mgr_...> or try without --cloud.",
                    cloud.as_str()
                )
            } else {
                "No managers have a log data service configured. Deploy a manager or pass --manager <mgr_...>.".to_string()
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

fn query_body(
    query: &LogsQuery,
    sort_by: &'static str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    page_limit: usize,
) -> QueryBody {
    QueryBody {
        search: query.search.clone(),
        filter: query.filter.clone(),
        sort_by,
        start: start.to_rfc3339_opts(SecondsFormat::Nanos, true),
        end: end.to_rfc3339_opts(SecondsFormat::Nanos, true),
        limit: page_limit,
        cursor: None,
    }
}

/// The newest `limit` logs in the window, oldest first.
async fn fetch_logs(
    data: &mut DataClient,
    query: &LogsQuery,
    target: &LogsTarget,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    limit: usize,
) -> Result<LogsOutput> {
    let started = Instant::now();
    let body = query_body(
        query,
        NEWEST_FIRST,
        start_time,
        end_time,
        limit.min(PAGE_SIZE),
    );
    let result = data.query(&body, limit).await?;
    let mut logs = result
        .rows
        .into_iter()
        .map(log_entry_from_row)
        .collect::<Vec<_>>();
    logs.sort_by_key(|entry| entry.timestamp_nanos.unwrap_or(i64::MIN));

    Ok(LogsOutput {
        target: target.clone(),
        query: query.search.clone(),
        filter: query.filter.clone(),
        start_time,
        end_time,
        num_hits: result.num_hits,
        logs,
        elapsed_time_micros: started.elapsed().as_micros().try_into().unwrap_or(u64::MAX),
    })
}

/// Tracks what `--follow` has already shown, and where the next poll starts.
struct Follower {
    seen: SeenLogKeys,
    start: DateTime<Utc>,
    limit: usize,
    newest_nanos: Option<i64>,
    polled: bool,
}

impl Follower {
    fn new(start: DateTime<Utc>, limit: usize) -> Self {
        Self {
            seen: SeenLogKeys::default(),
            start,
            limit,
            newest_nanos: None,
            polled: false,
        }
    }

    /// Hands `emit` the logs not shown yet, oldest first. The first poll returns
    /// the newest `limit` logs since the start time. Later polls read everything
    /// after the newest log seen, one page at a time, so a burst is neither cut
    /// off by the limit nor held in memory.
    async fn poll(
        &mut self,
        data: &mut DataClient,
        query: &LogsQuery,
        now: DateTime<Utc>,
        emit: &mut dyn FnMut(LogEntry) -> Result<()>,
    ) -> Result<()> {
        let end = now.max(self.start);
        if self.polled {
            let body = query_body(query, OLDEST_FIRST, self.next_start(), end, PAGE_SIZE);
            data.for_each_page(&body, |rows| self.accept(rows, emit))
                .await?;
        } else {
            let body = query_body(
                query,
                NEWEST_FIRST,
                self.start,
                end,
                self.limit.min(PAGE_SIZE),
            );
            let rows = data.query(&body, self.limit).await?.rows;
            self.accept(rows, emit)?;
            self.polled = true;
        }
        if let Some(cutoff) = self.next_start_nanos() {
            self.seen.forget_before(cutoff);
        }
        Ok(())
    }

    fn accept(
        &mut self,
        rows: Vec<Value>,
        emit: &mut dyn FnMut(LogEntry) -> Result<()>,
    ) -> Result<()> {
        let mut entries = rows.into_iter().map(log_entry_from_row).collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.timestamp_nanos.unwrap_or(i64::MIN));
        for entry in entries {
            if let Some(nanos) = entry.timestamp_nanos {
                self.newest_nanos =
                    Some(self.newest_nanos.map_or(nanos, |newest| newest.max(nanos)));
            }
            if self.seen.insert(log_key(&entry), entry.timestamp_nanos) {
                emit(entry)?;
            }
        }
        Ok(())
    }

    fn next_start_nanos(&self) -> Option<i64> {
        self.newest_nanos
            .map(|newest| newest.saturating_sub(FOLLOW_OVERLAP_SECONDS * 1_000_000_000))
    }

    fn next_start(&self) -> DateTime<Utc> {
        self.newest_nanos
            .and_then(timestamp_from_nanos)
            .map_or(self.start, |newest| {
                newest - Duration::seconds(FOLLOW_OVERLAP_SECONDS)
            })
    }
}

async fn follow_logs(
    data: &mut DataClient,
    query: &LogsQuery,
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
            "query": query.search,
            "filter": query.filter,
            "startTime": start_time,
        }))?;
    }

    let mut follower = Follower::new(start_time, limit);
    loop {
        follower
            .poll(data, query, Utc::now(), &mut |entry| {
                if json {
                    emit_json_line(&serde_json::json!({
                        "type": "log",
                        "log": entry,
                    }))?;
                } else {
                    render_human_log(&entry);
                }
                Ok(())
            })
            .await?;
        tokio::time::sleep(interval).await;
    }
}

/// Keys of the logs a follow has shown that a later poll can still read again.
/// A key is kept while its log lies inside the overlap window, however many
/// logs that is, so a burst is never shown twice.
#[derive(Default)]
struct SeenLogKeys {
    timestamps: HashMap<String, i64>,
}

impl SeenLogKeys {
    fn insert(&mut self, key: String, timestamp_nanos: Option<i64>) -> bool {
        match self.timestamps.entry(key) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(timestamp_nanos.unwrap_or(i64::MAX));
                true
            }
        }
    }

    /// Drops the keys of logs older than `cutoff_nanos`, which no poll reads again.
    fn forget_before(&mut self, cutoff_nanos: i64) {
        self.timestamps.retain(|_, nanos| *nanos >= cutoff_nanos);
    }
}

const LOG_COLLECTOR_OFF_MESSAGE: &str = "\
No logs. This installation doesn't send logs: its log collector is off.
Turn the log collector on in the deployment's Helm release:
  helm upgrade <release> <chart> --reuse-values \\
    --set logCollector.enabled=true --set-string logCollector.mode=podApi
Logs need telemetry. If the release sets management.telemetry or \
stackSettings.telemetry to off, set it to auto in the same upgrade, \
or the Operator won't start.
In podApi mode the Operator reads Pod logs through the Kubernetes API. \
logCollector.mode=nodeAgent runs a Fluent Bit DaemonSet on each node instead.";

/// A Kubernetes Operator reports a `logs` capability: granted when a node
/// collector or its own Pod log stream is configured, unavailable with a detail
/// when neither is. Earlier Operators sent the capability without a detail and
/// reported it unavailable in podApi mode even though logs flow there, so an
/// unavailable report without a detail tells us nothing, and neither does an
/// Operator too old to report the capability. Only the Kubernetes chart has a
/// log collector to turn on; earlier cloud and local Operators reported the
/// capability unavailable too, so the platform check stays.
fn log_collector_reported_off(
    platform: &str,
    capabilities: Option<&[types::OperatorCapabilityReport]>,
) -> bool {
    platform == "kubernetes"
        && capabilities.is_some_and(|capabilities| {
            capabilities.iter().any(|capability| {
                capability.key.as_str() == "logs"
                    && capability.state == types::OperatorCapabilityState::Unavailable
                    && capability.detail.is_some()
            })
        })
}

/// A `--source` query reads gateway request diagnostics without the deployment
/// filter, so the deployment's Pod log collector doesn't explain an empty
/// result there.
fn empty_logs_message(log_collector_off: bool, source: Option<LogSource>) -> &'static str {
    if log_collector_off && source.is_none() {
        LOG_COLLECTOR_OFF_MESSAGE
    } else {
        "(no logs)"
    }
}

fn render_human_logs(logs: &[LogEntry], log_collector_off: bool, source: Option<LogSource>) {
    if logs.is_empty() {
        println!("{}", empty_logs_message(log_collector_off, source));
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

/// A log row as the data service returns it: `timestampNanos` (a string),
/// `severityText`, `severityNumber`, `body`, and `resource` and `attributes`
/// maps keyed by the OpenTelemetry attribute names.
fn log_entry_from_row(row: Value) -> LogEntry {
    let timestamp_nanos = row.get("timestampNanos").and_then(nanos_value);
    let timestamp = timestamp_nanos.and_then(timestamp_from_nanos);
    let level = row
        .get("severityText")
        .and_then(Value::as_str)
        .map(str::to_string);
    let severity_number = row.get("severityNumber").and_then(Value::as_i64);
    let service = resource_attribute(&row, "service.name")
        .filter(|service| *service != "alien-worker-runtime")
        .map(str::to_string);
    let deployment_id = resource_attribute(&row, "alien.deployment_id").map(str::to_string);
    let message = extract_message(&row);

    LogEntry {
        timestamp,
        timestamp_nanos,
        level,
        severity_number,
        service,
        deployment_id,
        message,
        raw: row,
    }
}

/// Nanosecond timestamps arrive as strings, since they exceed what JSON numbers
/// hold exactly.
fn nanos_value(value: &Value) -> Option<i64> {
    match value {
        Value::String(text) => text.parse().ok(),
        other => other.as_i64(),
    }
}

/// An attribute of the log's resource. Dotted names such as `service.name` are
/// single keys, so a nested path is only a fallback.
fn resource_attribute<'a>(row: &'a Value, name: &str) -> Option<&'a str> {
    let resource = row.get("resource")?;
    resource
        .get(name)
        .or_else(|| {
            name.split('.')
                .try_fold(resource, |value, part| value.get(part))
        })
        .and_then(Value::as_str)
}

fn extract_message(row: &Value) -> String {
    if let Some(message) = row
        .get("body")
        .and_then(|body| body.get("message"))
        .and_then(Value::as_str)
    {
        return tracing_json_message(message).unwrap_or_else(|| message.to_string());
    }
    if let Some(body) = row.get("body").and_then(Value::as_str) {
        return tracing_json_message(body).unwrap_or_else(|| body.to_string());
    }
    if let Some(body) = row.get("body") {
        return body.to_string();
    }
    row.to_string()
}

fn tracing_json_message(input: &str) -> Option<String> {
    let record = serde_json::from_str::<Value>(input).ok()?;
    let record = record.as_object()?;
    record.get("timestamp")?.as_str()?;
    record.get("level")?.as_str()?;
    record.get("target")?.as_str()?;
    record
        .get("fields")?
        .as_object()?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

fn timestamp_from_nanos(timestamp_nanos: i64) -> Option<DateTime<Utc>> {
    let secs = timestamp_nanos.div_euclid(1_000_000_000);
    let nanos = timestamp_nanos.rem_euclid(1_000_000_000) as u32;
    DateTime::<Utc>::from_timestamp(secs, nanos)
}

/// Identifies a row across follow polls: the row's own id when the data service
/// gives one, else its timestamp and content.
fn log_key(entry: &LogEntry) -> String {
    if let Some(id) = entry.raw.get("id").and_then(Value::as_str) {
        return id.to_string();
    }
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

/// What to ask the data service: search text plus exact-match filters on
/// attributes. Filter keys are plain dotted paths into the log row.
#[derive(Debug, Clone, PartialEq)]
struct LogsQuery {
    search: String,
    filter: Map<String, Value>,
}

fn build_logs_query(options: LogQueryFilters<'_>) -> LogsQuery {
    let mut clauses = Vec::new();
    let mut filter = Map::new();
    let query = options.user_query.trim();
    if query != "*" {
        clauses.push(format!("({query})"));
    }
    if !options.levels.is_empty() {
        let level_filters = options
            .levels
            .iter()
            .map(|level| {
                let (min, max) = severity_range(*level);
                format!("severityNumber:[{min} TO {max}]")
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        clauses.push(format!("({level_filters})"));
    }
    if let Some(deployment_id) = options.deployment_id {
        filter.insert(
            DEPLOYMENT_ID_FIELD.to_string(),
            Value::String(deployment_id.to_string()),
        );
    }
    if let Some(source) = options.source {
        if source == LogSource::Application {
            clauses.push(format!("NOT {SOURCE_FIELD}:*"));
        } else {
            filter.insert(
                SOURCE_FIELD.to_string(),
                Value::String(source.as_str().to_string()),
            );
        }
    }
    for (field, value) in [
        ("attributes.gateway.outcome", options.status),
        ("attributes.ai.model.requested", options.model),
        ("attributes.ai.provider", options.provider),
        ("attributes.encryption.operation", options.operation),
        (
            "attributes.alien.deployment_group_id",
            options.deployment_group,
        ),
    ] {
        if let Some(value) = value {
            filter.insert(field.to_string(), Value::String(value.to_string()));
        }
    }
    if !options.include_system {
        // Hide Alien system-component logs (e.g. infrastructure daemons) unless
        // explicitly requested. `NOT field:value` keeps documents that lack the
        // attribute entirely, so user workload logs are always shown.
        clauses.push(format!(
            "NOT resource.{marker}:\"true\" AND NOT attributes.{marker}:\"true\"",
            marker = alien_core::ALIEN_SYSTEM_RESOURCE_ATTRIBUTE
        ));
    }

    let search = if clauses.is_empty() {
        "*".to_string()
    } else if clauses.iter().all(|clause| clause.starts_with("NOT ")) {
        // A query made up of only negative clauses matches nothing, so prepend a
        // match-all positive term.
        format!("* AND {}", clauses.join(" AND "))
    } else {
        clauses.join(" AND ")
    };
    LogsQuery { search, filter }
}

const DEPLOYMENT_ID_FIELD: &str = "resource.alien.deployment_id";
const SOURCE_FIELD: &str = "attributes.alien.log.source";

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

fn parse_rfc3339_arg(value: &str) -> std::result::Result<DateTime<Utc>, String> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Ok(timestamp.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|timestamp| timestamp.and_utc())
        .ok_or_else(|| "expected an RFC3339 timestamp or YYYY-MM-DD (midnight UTC)".to_string())
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
    use super::super::logs_data::test_server::TestServer;
    use super::*;
    use httpmock::{Method::GET, MockServer};
    use serde_json::json;

    fn target() -> LogsTarget {
        LogsTarget {
            manager_id: "mgr_test".to_string(),
            project_id: "prj_test".to_string(),
            project_name: None,
            deployment_id: None,
            deployment_name: None,
            deployment_group_name: None,
            database_id: "prj_test".to_string(),
        }
    }

    fn match_all() -> LogsQuery {
        LogsQuery {
            search: "*".to_string(),
            filter: Map::new(),
        }
    }

    /// A row as the data service returns it.
    fn native_row(nanos: i64, message: &str) -> Value {
        json!({
            "id": format!("auto_{nanos}"),
            "timestampNanos": nanos.to_string(),
            "observedTimestampNanos": nanos.to_string(),
            "severityText": "INFO",
            "severityNumber": 9,
            "body": message,
            "resource": {"service.name": "api", "alien.deployment_id": "dep_abc"},
            "attributes": {},
        })
    }

    fn page(rows: Vec<Value>, cursor: Option<&str>) -> (u16, Value) {
        (
            200,
            json!({
                "rows": rows, "rowMetadata": [], "coverage": {"complete": true},
                "numHits": rows.len(), "nextCursor": cursor,
            }),
        )
    }

    const T0: i64 = 1_700_000_000_000_000_000;
    const SECOND: i64 = 1_000_000_000;

    #[tokio::test]
    async fn fetch_returns_the_newest_logs_oldest_first_with_native_rows_parsed() {
        // The data service answers newest first.
        let server = TestServer::start(|_, _| {
            page(
                vec![
                    native_row(T0 + 2 * SECOND, "third"),
                    native_row(T0 + SECOND, "second"),
                    native_row(T0, "first"),
                ],
                None,
            )
        })
        .await;
        let mut data = server.data_client();
        let end = Utc::now();
        let start = end - Duration::hours(1);

        let output = fetch_logs(&mut data, &match_all(), &target(), start, end, 10)
            .await
            .unwrap();

        let messages: Vec<_> = output.logs.iter().map(|log| log.message.as_str()).collect();
        assert_eq!(messages, ["first", "second", "third"]);
        assert_eq!(output.num_hits, 3);
        assert_eq!(output.logs[0].level.as_deref(), Some("INFO"));
        assert_eq!(output.logs[0].service.as_deref(), Some("api"));
        assert_eq!(output.logs[0].deployment_id.as_deref(), Some("dep_abc"));
        assert_eq!(output.logs[0].timestamp_nanos, Some(T0));

        let body = &server.query_requests()[0].body;
        assert_eq!(body["limit"], 10);
        assert_eq!(body["sortBy"], "-timestampNanos");
        assert_eq!(
            body["start"],
            start.to_rfc3339_opts(SecondsFormat::Nanos, true)
        );
        assert_eq!(body["end"], end.to_rfc3339_opts(SecondsFormat::Nanos, true));
    }

    #[tokio::test]
    async fn fetch_collects_a_limit_larger_than_a_page_over_cursor_pages() {
        // 1,200 rows exist; the limit is 1,000 and a page holds 500.
        let server = TestServer::start(|number, body| {
            assert_eq!(body["limit"], 500, "pages are fetched at the page size");
            let rows = (0..500)
                .map(|index| native_row(T0 - (number as i64 * 500 + index), "row"))
                .collect();
            page(rows, Some("next"))
        })
        .await;
        let mut data = server.data_client();
        let end = Utc::now();

        let output = fetch_logs(
            &mut data,
            &match_all(),
            &target(),
            end - Duration::hours(1),
            end,
            1000,
        )
        .await
        .unwrap();

        assert_eq!(output.logs.len(), 1000);
        assert_eq!(server.query_requests().len(), 2);
        let timestamps: Vec<_> = output
            .logs
            .iter()
            .filter_map(|log| log.timestamp_nanos)
            .collect();
        assert!(
            timestamps.windows(2).all(|pair| pair[0] <= pair[1]),
            "oldest first"
        );
        assert_eq!(timestamps.last(), Some(&T0), "the newest row is kept");
    }

    #[tokio::test]
    async fn follow_polls_after_the_newest_log_without_repeating_any() {
        let server = TestServer::start(|number, body| match number {
            0 => {
                assert_eq!(body["sortBy"], "-timestampNanos");
                page(
                    vec![native_row(T0 + SECOND, "b"), native_row(T0, "a")],
                    None,
                )
            }
            1 => {
                // Asks for logs after the newest one, oldest first. The overlap
                // returns "b" again together with a new row.
                assert_eq!(body["sortBy"], "timestampNanos");
                page(
                    vec![
                        native_row(T0 + SECOND, "b"),
                        native_row(T0 + 2 * SECOND, "c"),
                    ],
                    None,
                )
            }
            2 => page(vec![native_row(T0 + 2 * SECOND, "c")], None),
            _ => page(vec![], None),
        })
        .await;
        let mut data = server.data_client();
        let start = DateTime::<Utc>::from_timestamp(1_699_999_000, 0).unwrap();
        let mut follower = Follower::new(start, 100);
        let query = match_all();

        let mut shown = Vec::new();
        for _ in 0..4 {
            let fresh = poll_all(&mut follower, &mut data, &query).await;
            shown.push(fresh.into_iter().map(|log| log.message).collect::<Vec<_>>());
        }

        assert_eq!(shown, [vec!["a", "b"], vec!["c"], vec![], vec![]]);
        let requests = server.query_requests();
        assert_eq!(requests[0].body["start"], "2023-11-14T21:56:40.000000000Z");
        // One second before the newest log seen (T0 + 1s), then (T0 + 2s).
        assert_eq!(requests[1].body["start"], "2023-11-14T22:13:20.000000000Z");
        assert_eq!(requests[2].body["start"], "2023-11-14T22:13:21.000000000Z");
        assert_eq!(requests[3].body["start"], "2023-11-14T22:13:21.000000000Z");
        for request in &requests[1..] {
            assert_eq!(request.body["sortBy"], "timestampNanos");
        }
    }

    async fn poll_all(
        follower: &mut Follower,
        data: &mut DataClient,
        query: &LogsQuery,
    ) -> Vec<LogEntry> {
        let mut shown = Vec::new();
        follower
            .poll(data, query, Utc::now(), &mut |entry| {
                shown.push(entry);
                Ok(())
            })
            .await
            .unwrap();
        shown
    }

    #[tokio::test]
    async fn follow_shows_each_page_before_reading_the_next() {
        let server = TestServer::start(|number, _| match number {
            0 => page(vec![], None),
            1 => page(
                (0..500).map(|index| native_row(T0 + index, "a")).collect(),
                Some("more"),
            ),
            _ => page(vec![native_row(T0 + 500, "b")], None),
        })
        .await;
        let mut data = server.data_client();
        let start = DateTime::<Utc>::from_timestamp(1_699_999_000, 0).unwrap();
        let mut follower = Follower::new(start, 10);
        let query = match_all();
        poll_all(&mut follower, &mut data, &query).await;

        let mut queries_when_shown = Vec::new();
        follower
            .poll(&mut data, &query, Utc::now(), &mut |_| {
                queries_when_shown.push(server.query_requests().len());
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(queries_when_shown.len(), 501);
        assert_eq!(
            queries_when_shown[0], 2,
            "the first page is shown before the second is read"
        );
        assert_eq!(queries_when_shown[500], 3);
    }

    #[tokio::test]
    async fn follow_does_not_repeat_a_burst_larger_than_any_cache() {
        const BURST: i64 = 25_000;
        // Every row lies inside the one-second overlap of the next poll.
        let server = TestServer::start(|number, body| {
            if number == 0 {
                return page(vec![], None);
            }
            let offset: i64 = body["cursor"]
                .as_str()
                .map_or(0, |cursor| cursor.parse().unwrap());
            let end = (offset + 500).min(BURST);
            let next = (end < BURST).then(|| end.to_string());
            page(
                (offset..end)
                    .map(|index| native_row(T0 + index, "burst"))
                    .collect(),
                next.as_deref(),
            )
        })
        .await;
        let mut data = server.data_client();
        let start = DateTime::<Utc>::from_timestamp(1_699_999_000, 0).unwrap();
        let mut follower = Follower::new(start, 10);
        let query = match_all();
        poll_all(&mut follower, &mut data, &query).await;

        let first = poll_all(&mut follower, &mut data, &query).await;
        let second = poll_all(&mut follower, &mut data, &query).await;
        let third = poll_all(&mut follower, &mut data, &query).await;

        assert_eq!(first.len(), BURST as usize);
        assert!(second.is_empty(), "{} rows shown again", second.len());
        assert!(third.is_empty());
    }

    #[tokio::test]
    async fn follow_reads_a_burst_larger_than_a_page_in_full() {
        let server = TestServer::start(|number, body| match number {
            0 => page(vec![], None),
            1 => page(
                (0..500)
                    .map(|index| native_row(T0 + index, "burst"))
                    .collect(),
                Some("more"),
            ),
            2 => {
                assert_eq!(body["cursor"], "more");
                page(
                    (500..650)
                        .map(|index| native_row(T0 + index, "burst"))
                        .collect(),
                    None,
                )
            }
            _ => unreachable!(),
        })
        .await;
        let mut data = server.data_client();
        let start = DateTime::<Utc>::from_timestamp(1_699_999_000, 0).unwrap();
        let mut follower = Follower::new(start, 10);
        let query = match_all();

        assert!(poll_all(&mut follower, &mut data, &query).await.is_empty());
        let fresh = poll_all(&mut follower, &mut data, &query).await;

        assert_eq!(fresh.len(), 650, "the limit only bounds the first fetch");
    }

    #[tokio::test]
    async fn follow_replaces_a_grant_that_is_about_to_expire() {
        let server = TestServer::start(|_, _| page(vec![], None)).await;
        server.set_grant_ttl(20);
        let mut data = server.data_client();
        let mut follower = Follower::new(Utc::now() - Duration::hours(1), 10);
        let query = match_all();

        for _ in 0..3 {
            poll_all(&mut follower, &mut data, &query).await;
        }

        assert_eq!(server.grant_requests().len(), 3);
        let tokens: Vec<_> = server
            .query_requests()
            .into_iter()
            .map(|request| request.authorization.unwrap())
            .collect();
        assert_eq!(
            tokens,
            ["Bearer grant-1", "Bearer grant-2", "Bearer grant-3"]
        );
    }

    #[tokio::test]
    async fn rejected_search_keeps_bad_request_status_and_plain_message() {
        let server = TestServer::start(|_, _| {
            (
                400,
                json!({"code": "INVALID_REQUEST", "error": "Numeric ranges require TO between bounds", "status": "error"}),
            )
        })
        .await;
        let mut data = server.data_client();
        let end = Utc::now();

        let error = fetch_logs(
            &mut data,
            &match_all(),
            &target(),
            end - Duration::hours(1),
            end,
            10,
        )
        .await
        .expect_err("query is refused over HTTP");

        assert_eq!(error.http_status_code, Some(400));
        assert_eq!(error.code, "INVALID_REQUEST");
        assert_eq!(
            error.message,
            "Log search rejected: Numeric ranges require TO between bounds"
        );
        assert!(!error.retryable);
        assert!(!error.internal);
    }

    fn manager_json(id: &str, is_system: bool) -> serde_json::Value {
        json!({
            "id": id, "name": id, "workspaceId": "ws_000000000000000000000000",
            "createdAt": "2026-01-01T00:00:00Z", "defaultProjectCount": 0,
            "managedDeploymentCount": 0, "managementConfigs": {}, "isSystem": is_system,
            "status": "healthy", "targets": ["aws"], "logsDatabaseId": format!("db_{id}")
        })
    }

    #[tokio::test]
    async fn gateway_sources_read_the_system_managers_logs() {
        const OWN: &str = "mgr_0000000000000000000000000001";
        const SYSTEM: &str = "mgr_0000000000000000000000000002";
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/managers");
                then.status(200).json_body(json!([
                    manager_json(OWN, false),
                    manager_json(SYSTEM, true)
                ]));
            })
            .await;
        let client = alien_platform_api::Client::new(&server.base_url());

        for (source, expected) in [
            (Some(LogSource::RemoteSandbox), SYSTEM),
            (Some(LogSource::AiGateway), SYSTEM),
            (Some(LogSource::Application), OWN),
            (None, OWN),
        ] {
            let selected = select_logs_manager(&client, "example", None, source)
                .await
                .expect("a manager with logs is listed");
            assert_eq!(selected.id.to_string(), expected, "{source:?}");
        }
    }

    #[test]
    fn source_flags_match_what_clap_accepts() {
        for source in LogSource::value_variants() {
            let value = source.to_possible_value().expect("no LogSource is skipped");
            assert_eq!(source.flag(), value.get_name());
        }
    }

    #[test]
    fn gateway_sources_refuse_a_manager_or_cloud_they_would_ignore() {
        for source in ["remote-sandbox", "ai-gateway"] {
            for extra in [["--manager", "mgr_own"], ["--cloud", "gcp"]] {
                let args =
                    LogsArgs::try_parse_from(["logs", "--source", source].into_iter().chain(extra))
                        .expect("flags parse before semantic validation");
                let error = validate_args(&args).expect_err("the system manager holds these logs");
                assert_eq!(error.code, "VALIDATION_ERROR");
                for expected in [source, "--manager or --cloud"] {
                    assert!(error.message.contains(expected), "{}", error.message);
                }
            }
        }
        let args =
            LogsArgs::try_parse_from(["logs", "--source", "encryption-gateway", "--cloud", "gcp"])
                .expect("flags parse");
        validate_args(&args).expect("encryption gateway logs can live on any manager");
        let args =
            LogsArgs::try_parse_from(["logs", "--source", "application", "--manager", "mgr_own"])
                .expect("flags parse");
        validate_args(&args).expect("application logs can live on any manager");
    }

    #[tokio::test]
    async fn gateway_sources_never_fall_back_to_another_manager() {
        const OWN: &str = "mgr_0000000000000000000000000001";
        const SYSTEM: &str = "mgr_0000000000000000000000000002";
        let mut system = manager_json(SYSTEM, true);
        system["logsDatabaseId"] = serde_json::Value::Null;
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/managers");
                then.status(200)
                    .json_body(json!([manager_json(OWN, false), system]));
            })
            .await;
        let client = alien_platform_api::Client::new(&server.base_url());

        let error = select_logs_manager(&client, "example", None, Some(LogSource::RemoteSandbox))
            .await
            .expect_err("only the system manager holds gateway request logs");
        assert_eq!(error.code, "CONFIGURATION_ERROR");
        for expected in ["--source remote-sandbox", &server.base_url()] {
            assert!(error.message.contains(expected), "{}", error.message);
        }
    }

    #[test]
    fn invalid_log_limits_are_client_validation_errors() {
        for limit in [0, 1001] {
            let args = LogsArgs::try_parse_from(["logs", "--limit", &limit.to_string()])
                .expect("numeric limit parses before semantic validation");
            let error = validate_args(&args).expect_err("limit outside supported range");
            assert_eq!(error.code, "VALIDATION_ERROR");
            assert_eq!(error.http_status_code, Some(400));
            assert!(!error.retryable);
            assert!(!error.internal);
        }
        let args =
            LogsArgs::try_parse_from(["logs", "--limit", "1000"]).expect("maximum limit parses");
        validate_args(&args).expect("maximum limit remains valid");
    }

    fn test_query<'a>(
        user_query: &'a str,
        levels: &'a [LogLevel],
        deployment_id: Option<&'a str>,
        include_system: bool,
        source: Option<LogSource>,
    ) -> LogsQuery {
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

    fn filter(pairs: &[(&str, &str)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), json!(value)))
            .collect()
    }

    #[test]
    fn builds_query_with_levels_and_deployment() {
        // `include_system = true` keeps the system-exclusion clause out of the
        // way so this asserts only the level + deployment composition.
        let query = test_query(
            "resource.service.name:api",
            &[LogLevel::Warn, LogLevel::Error],
            Some("dep_123"),
            true,
            None,
        );

        assert_eq!(
            query.search,
            "(resource.service.name:api) AND (severityNumber:[13 TO 16] OR severityNumber:[17 TO 20])"
        );
        assert_eq!(
            query.filter,
            filter(&[("resource.alien.deployment_id", "dep_123")])
        );
    }

    #[test]
    fn a_deployment_alone_is_only_a_filter() {
        let query = test_query("*", &[], Some("dep_123"), true, None);

        assert_eq!(query.search, "*");
        assert_eq!(
            query.filter,
            filter(&[("resource.alien.deployment_id", "dep_123")])
        );
    }

    #[test]
    fn hides_system_components_by_default_with_match_all_base() {
        // Default (no query, levels, or deployment) must not produce a
        // pure-negative query: it needs a positive `*` base.
        let query = test_query("*", &[], None, false, None);

        assert_eq!(
            query.search,
            "* AND NOT resource.alien.system:\"true\" AND NOT attributes.alien.system:\"true\""
        );
        assert!(query.filter.is_empty());
    }

    #[test]
    fn system_flag_omits_exclusion() {
        let query = test_query("*", &[], None, true, None);

        assert_eq!(query.search, "*");
        assert!(query.filter.is_empty());
    }

    #[test]
    fn system_exclusion_appends_after_positive_clauses() {
        let query = test_query(
            "resource.service.name:api",
            &[LogLevel::Error],
            Some("dep_123"),
            false,
            None,
        );

        assert_eq!(
            query.search,
            "(resource.service.name:api) AND (severityNumber:[17 TO 20]) AND NOT resource.alien.system:\"true\" AND NOT attributes.alien.system:\"true\""
        );
        assert_eq!(
            query.filter,
            filter(&[("resource.alien.deployment_id", "dep_123")])
        );
    }

    #[test]
    fn gateway_source_filters_on_the_trusted_record_attribute() {
        let query = test_query("*", &[], None, true, Some(LogSource::AiGateway));

        assert_eq!(query.search, "*");
        assert_eq!(
            query.filter,
            filter(&[("attributes.alien.log.source", "ai-gateway")])
        );
    }

    #[test]
    fn remote_sandbox_source_reads_the_records_its_gateway_writes() {
        let query = test_query("*", &[], None, true, Some(LogSource::RemoteSandbox));

        assert_eq!(
            query.filter,
            filter(&[("attributes.alien.log.source", "sandbox-gateway")])
        );
    }

    #[test]
    fn application_source_is_logs_without_a_source_attribute() {
        let query = test_query("*", &[], None, true, Some(LogSource::Application));

        assert_eq!(query.search, "* AND NOT attributes.alien.log.source:*");
        assert!(query.filter.is_empty());
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
            operation: Some("decrypt"),
            deployment_group: Some("dg_123"),
        });

        assert_eq!(query.search, "*");
        assert_eq!(
            query.filter,
            filter(&[
                ("attributes.alien.log.source", "ai-gateway"),
                ("attributes.gateway.outcome", "provider-error"),
                ("attributes.ai.model.requested", "byo/claude-opus-5"),
                ("attributes.ai.provider", "anthropic"),
                ("attributes.encryption.operation", "decrypt"),
                ("attributes.alien.deployment_group_id", "dg_123"),
            ])
        );
    }

    #[test]
    fn filter_values_with_quotes_stay_literal() {
        // Filters are JSON values, not query syntax, so nothing needs escaping.
        let query = test_query("*", &[], Some("dep\" OR \"x"), true, None);

        assert_eq!(
            query.filter["resource.alien.deployment_id"],
            json!("dep\" OR \"x")
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

    fn capability_reports(value: Value) -> Vec<types::OperatorCapabilityReport> {
        serde_json::from_value(value).expect("capability reports should match the API schema")
    }

    #[test]
    fn empty_result_explains_a_log_collector_the_operator_reported_off() {
        let unavailable = capability_reports(json!([
            { "key": "k8s-workloads", "state": "granted", "detail": "namespace customer" },
            { "key": "logs", "state": "unavailable", "detail": "No log collector configured" }
        ]));
        let message = empty_logs_message(
            log_collector_reported_off("kubernetes", Some(&unavailable)),
            None,
        );
        assert_eq!(message, LOG_COLLECTOR_OFF_MESSAGE);
        assert!(message
            .contains("--set logCollector.enabled=true --set-string logCollector.mode=podApi"));

        let granted = capability_reports(json!([
            { "key": "logs", "state": "granted", "detail": "Pod logs read through the Kubernetes API" }
        ]));
        assert_eq!(
            empty_logs_message(
                log_collector_reported_off("kubernetes", Some(&granted)),
                None
            ),
            "(no logs)"
        );

        // Earlier Operators send no detail and report podApi mode, where logs
        // flow, as unavailable. An empty result there is just an empty result.
        let undetailed = capability_reports(json!([
            { "key": "k8s-workloads", "state": "granted" },
            { "key": "logs", "state": "unavailable", "detail": null }
        ]));
        assert_eq!(
            empty_logs_message(
                log_collector_reported_off("kubernetes", Some(&undetailed)),
                None
            ),
            "(no logs)"
        );
        let detail_omitted = capability_reports(json!([
            { "key": "logs", "state": "unavailable" }
        ]));
        assert_eq!(
            empty_logs_message(
                log_collector_reported_off("kubernetes", Some(&detail_omitted)),
                None
            ),
            "(no logs)"
        );

        // Operators that predate the capability report nothing about logs.
        let without_logs = capability_reports(json!([
            { "key": "k8s-workloads", "state": "granted" }
        ]));
        assert_eq!(
            empty_logs_message(
                log_collector_reported_off("kubernetes", Some(&without_logs)),
                None
            ),
            "(no logs)"
        );
        assert_eq!(
            empty_logs_message(log_collector_reported_off("kubernetes", None), None),
            "(no logs)"
        );

        // Gateway request diagnostics aren't the deployment's Pod logs.
        for source in [
            LogSource::Application,
            LogSource::AiGateway,
            LogSource::EncryptionGateway,
            LogSource::RemoteSandbox,
        ] {
            assert_eq!(
                deployment_filter_for_source(Some(source), Some("dep_1")),
                None
            );
            assert_eq!(
                empty_logs_message(
                    log_collector_reported_off("kubernetes", Some(&unavailable)),
                    Some(source)
                ),
                "(no logs)"
            );
        }

        // Only the Kubernetes chart has a log collector to turn on.
        assert_eq!(
            empty_logs_message(
                log_collector_reported_off("machines", Some(&unavailable)),
                None
            ),
            "(no logs)"
        );
    }

    #[test]
    fn extracts_log_entry_fields() {
        let entry = log_entry_from_row(json!({
            "timestampNanos": "1700000000123000000",
            "severityText": "INFO",
            "severityNumber": 9,
            "body": "ready",
            "resource": {
                "service.name": "api",
                "alien.deployment_id": "dep_abc"
            }
        }));

        assert_eq!(entry.message, "ready");
        assert_eq!(entry.level.as_deref(), Some("INFO"));
        assert_eq!(entry.severity_number, Some(9));
        assert_eq!(entry.service.as_deref(), Some("api"));
        assert_eq!(entry.deployment_id.as_deref(), Some("dep_abc"));
        assert_eq!(entry.timestamp_nanos, Some(1_700_000_000_123_000_000));
        assert_eq!(
            entry.timestamp.unwrap().to_rfc3339(),
            "2023-11-14T22:13:20.123+00:00"
        );
    }

    #[test]
    fn reads_nested_resource_attributes_and_hides_the_runtime_service() {
        let entry = log_entry_from_row(json!({
            "timestampNanos": 1_700_000_000_000_000_000i64,
            "resource": {"service": {"name": "worker"}, "alien": {"deployment_id": "dep_n"}},
        }));
        assert_eq!(entry.service.as_deref(), Some("worker"));
        assert_eq!(entry.deployment_id.as_deref(), Some("dep_n"));
        assert_eq!(entry.timestamp_nanos, Some(1_700_000_000_000_000_000));

        let entry =
            log_entry_from_row(json!({"resource": {"service.name": "alien-worker-runtime"}}));
        assert_eq!(entry.service, None);
    }

    #[test]
    fn structured_bodies_show_their_message() {
        let entry = log_entry_from_row(json!({"body": {"message": "from object"}}));
        assert_eq!(entry.message, "from object");

        let entry = log_entry_from_row(json!({"body": {"code": 7}}));
        assert_eq!(entry.message, r#"{"code":7}"#);
    }

    #[test]
    fn rows_are_told_apart_by_id_else_by_content() {
        let a = log_entry_from_row(native_row(T0, "same"));
        let mut b = log_entry_from_row(native_row(T0, "same"));
        assert_eq!(log_key(&a), log_key(&b));
        b.raw["id"] = json!("auto_other");
        assert_ne!(log_key(&a), log_key(&b));

        let a = log_entry_from_row(json!({"timestampNanos": "5", "body": "x"}));
        let b = log_entry_from_row(json!({"timestampNanos": "5", "body": "y"}));
        assert_ne!(log_key(&a), log_key(&b));
    }

    #[test]
    fn extracts_message_from_pre_normalization_tracing_record() {
        let original = serde_json::json!({
            "timestamp": "2026-08-24T09:12:53.647763Z",
            "level": "DEBUG",
            "fields": {
                "message": "Processed pending jobs",
                "job_count": 2
            },
            "target": "example_service::worker"
        })
        .to_string();
        let hit = serde_json::json!({ "body": { "message": original } });

        let entry = log_entry_from_row(hit.clone());

        assert_eq!(entry.message, "Processed pending jobs");
        assert_eq!(entry.raw, hit);
    }

    #[test]
    fn does_not_guess_a_message_from_generic_json() {
        let original =
            r#"{"timestamp":"2026-08-24T09:12:53Z","level":"INFO","fields":{"message":"ready"}}"#;
        let hit = serde_json::json!({ "body": { "message": original } });

        let entry = log_entry_from_row(hit.clone());

        assert_eq!(entry.message, original);
        assert_eq!(entry.raw, hit);
    }
}
