//! Platform-only commands for managing workspace private managers.

use crate::auth::AuthHttp;
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::interaction::{ConfirmationMode, InteractionMode};
use crate::output::{print_json, prompt_confirm};
use crate::ui::{make_table, print_table, status_cell};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    DeleteManagerWorkspace, GetManagerWorkspace, ListManagerEventsWorkspace, ListManagersWorkspace,
    ManagerId,
};
use alien_platform_api::SdkResultExt as _;
use clap::{Parser, Subcommand, ValueEnum};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage private managers",
    long_about = "Manage private managers deployed to a workspace cloud account."
)]
pub struct ManagersArgs {
    /// Print machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: ManagersCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ManagersCmd {
    /// Create a new private manager and return its setup action
    Create {
        /// Manager display name
        #[arg(long, default_value = "alien-private-manager")]
        name: String,

        /// Cloud where the manager will run
        #[arg(long)]
        cloud: PrivateManagerCloudArg,

        /// Cloud region for the manager
        #[arg(long)]
        region: String,

        /// Setup method. Defaults by cloud: AWS cloudformation, GCP google-oauth, Azure terraform
        #[arg(long)]
        setup: Option<PrivateManagerSetupMethodArg>,

        /// Network mode. Defaults to create; use default for faster dev/test setup.
        #[arg(long)]
        network: Option<PrivateManagerNetworkArg>,

        /// Write Terraform setup files to this directory
        #[arg(long)]
        output_dir: Option<PathBuf>,

        /// Open the returned browser setup URL
        #[arg(long)]
        open: bool,
    },
    /// List private managers
    List {
        /// Filter by cloud
        #[arg(long)]
        cloud: Option<PrivateManagerCloudArg>,
    },
    /// Show manager status and details
    Status {
        /// Manager ID
        id: String,

        /// Poll until setup reaches a terminal state
        #[arg(long)]
        watch: bool,
    },
    /// Return a fresh setup action for a pending or failed manager
    Setup {
        /// Manager ID
        id: String,

        /// Write Terraform setup files to this directory
        #[arg(long)]
        output_dir: Option<PathBuf>,

        /// Open the returned browser setup URL
        #[arg(long)]
        open: bool,
    },
    /// Cancel an incomplete private-manager setup
    Cancel {
        /// Manager ID
        id: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Delete a private manager
    Delete {
        /// Manager ID
        id: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// View manager deployment events
    Events {
        /// Manager ID
        id: String,

        /// Follow events
        #[arg(long, short)]
        follow: bool,
    },
    /// Manage project default private managers
    Defaults {
        #[command(subcommand)]
        cmd: DefaultsCmd,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum DefaultsCmd {
    /// Show default private managers for a project
    Show {
        /// Project ID or name. Defaults to --project or the linked project
        #[arg(long)]
        project: Option<String>,
    },
    /// Set the default private manager for a cloud
    Set {
        /// Project ID or name. Defaults to --project or the linked project
        #[arg(long)]
        project: Option<String>,

        /// Cloud to configure
        #[arg(long)]
        cloud: PrivateManagerCloudArg,

        /// Manager ID
        #[arg(long)]
        manager: String,
    },
    /// Clear default private managers
    Clear {
        /// Project ID or name. Defaults to --project or the linked project
        #[arg(long)]
        project: Option<String>,

        /// Cloud to clear, or all
        #[arg(long)]
        cloud: DefaultsClearTarget,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivateManagerSetupMethodArg {
    Cloudformation,
    GoogleOauth,
    Terraform,
}

impl PrivateManagerSetupMethodArg {
    fn as_api_str(self) -> &'static str {
        match self {
            Self::Cloudformation => "cloudformation",
            Self::GoogleOauth => "google-oauth",
            Self::Terraform => "terraform",
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivateManagerNetworkArg {
    Create,
    Default,
}

impl PrivateManagerNetworkArg {
    fn as_api_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Default => "default",
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivateManagerCloudArg {
    Aws,
    Gcp,
    Azure,
}

impl PrivateManagerCloudArg {
    fn as_api_str(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Gcp => "gcp",
            Self::Azure => "azure",
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultsClearTarget {
    Aws,
    Gcp,
    Azure,
    All,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagerSummary {
    id: String,
    name: String,
    status: String,
    setup_status: Option<String>,
    cloud: Option<String>,
    region: Option<String>,
    targets: Vec<String>,
    managed_deployment_count: u64,
    default_project_count: u64,
    created_at: String,
    url: Option<String>,
    version: Option<String>,
    last_heartbeat_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagerSetupResponse {
    manager_id: String,
    setup_status: String,
    setup_token: String,
    setup_token_id: String,
    deployment_link: String,
    setup: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagerSetupOutput {
    manager_id: String,
    name: Option<String>,
    cloud: Option<String>,
    region: Option<String>,
    setup_status: String,
    setup_token_id: String,
    deployment_link: String,
    setup: serde_json::Value,
    files_written: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagerEventSummary {
    id: String,
    created_at: String,
    state: String,
    summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagerMutationOutput {
    id: String,
    action: String,
    completed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectDefaultsOutput {
    project_id: String,
    project_name: String,
    default_managers: Option<ProjectDefaultManagersJson>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectDefaultManagersJson {
    aws: Option<String>,
    gcp: Option<String>,
    azure: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DefaultsMutationOutput {
    project_id: String,
    project_name: String,
    default_managers: Option<ProjectDefaultManagersJson>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateManagerRequest<'a> {
    name: &'a str,
    cloud: &'a str,
    region: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    setup_method: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<&'a str>,
}

pub async fn managers_task(args: ManagersArgs, ctx: ExecutionMode) -> Result<()> {
    if let ManagersCmd::Events { follow: true, .. } = &args.cmd {
        if args.json {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "follow".to_string(),
                message:
                    "`alien managers events --json` does not support `--follow`; rerun without `--json` for streaming output"
                        .to_string(),
            }));
        }
    }

    match &args.cmd {
        ManagersCmd::Cancel { yes, .. } => {
            confirmation_mode(*yes, args.json, "cancel")?;
        }
        ManagersCmd::Delete { yes, .. } => {
            confirmation_mode(*yes, args.json, "delete")?;
        }
        _ => {}
    }

    let auth = ctx.auth_http().await?;
    let client = auth.sdk_client().clone();
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;

    match args.cmd {
        ManagersCmd::Create {
            name,
            cloud,
            region,
            setup,
            network,
            output_dir,
            open,
        } => {
            create_manager_task(
                &auth,
                &workspace,
                &name,
                cloud,
                &region,
                setup,
                network,
                output_dir.as_deref(),
                open,
                ctx.no_browser(),
                args.json,
            )
            .await?;
        }
        ManagersCmd::List { cloud } => {
            list_managers_task(&client, &workspace, cloud, args.json).await?;
        }
        ManagersCmd::Status { id, watch } => {
            status_manager_task(&client, &workspace, &id, watch, args.json).await?;
        }
        ManagersCmd::Setup {
            id,
            output_dir,
            open,
        } => {
            setup_manager_task(
                &auth,
                &workspace,
                &id,
                output_dir.as_deref(),
                open,
                ctx.no_browser(),
                args.json,
            )
            .await?;
        }
        ManagersCmd::Cancel { id, yes } => {
            cancel_manager_task(&auth, &workspace, &id, yes, args.json).await?;
        }
        ManagersCmd::Delete { id, yes } => {
            delete_manager_task(&client, &workspace, &id, yes, args.json).await?;
        }
        ManagersCmd::Events { id, follow } => {
            events_manager_task(&client, &workspace, &id, follow, args.json).await?;
        }
        ManagersCmd::Defaults { cmd } => {
            defaults_task(&auth, &ctx, &workspace, cmd, args.json).await?;
        }
    }

    Ok(())
}

async fn create_manager_task(
    auth: &AuthHttp,
    workspace: &str,
    name: &str,
    cloud: PrivateManagerCloudArg,
    region: &str,
    setup: Option<PrivateManagerSetupMethodArg>,
    network: Option<PrivateManagerNetworkArg>,
    output_dir: Option<&Path>,
    open_browser: bool,
    no_browser: bool,
    json: bool,
) -> Result<()> {
    validate_setup_side_effect_flags(open_browser, no_browser, json)?;
    let effective_setup = setup.unwrap_or_else(|| default_setup_method_for_cloud(cloud));
    if output_dir.is_some() && effective_setup != PrivateManagerSetupMethodArg::Terraform {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "output-dir".to_string(),
            message: "--output-dir is only supported for terraform setup methods".to_string(),
        }));
    }

    let body = CreateManagerRequest {
        name,
        cloud: cloud.as_api_str(),
        region,
        setup_method: setup.map(PrivateManagerSetupMethodArg::as_api_str),
        network: network.map(PrivateManagerNetworkArg::as_api_str),
    };
    let url = api_url(&auth.base_url, "/v1/managers", workspace, None)?;
    let response: ManagerSetupResponse = send_json(auth, Method::POST, url, Some(&body)).await?;
    let files_written = write_setup_files(output_dir, &response.setup)?;
    maybe_open_setup(&response.setup, open_browser)?;

    let output = ManagerSetupOutput {
        manager_id: response.manager_id,
        name: Some(name.to_string()),
        cloud: Some(cloud.as_api_str().to_string()),
        region: Some(region.to_string()),
        setup_status: response.setup_status,
        setup_token_id: response.setup_token_id,
        deployment_link: response.deployment_link,
        setup: response.setup,
        files_written,
    };

    render_setup_output(&output, json)
}

async fn setup_manager_task(
    auth: &AuthHttp,
    workspace: &str,
    manager_id: &str,
    output_dir: Option<&Path>,
    open_browser: bool,
    no_browser: bool,
    json: bool,
) -> Result<()> {
    validate_setup_side_effect_flags(open_browser, no_browser, json)?;
    validate_manager_id(manager_id)?;
    let path = format!("/v1/managers/{manager_id}/setup-token");
    let url = api_url(&auth.base_url, &path, workspace, None)?;
    let response: ManagerSetupResponse =
        send_json::<(), _>(auth, Method::POST, url, None::<&()>).await?;
    let files_written = write_setup_files(output_dir, &response.setup)?;
    maybe_open_setup(&response.setup, open_browser)?;

    let output = ManagerSetupOutput {
        manager_id: response.manager_id,
        name: None,
        cloud: None,
        region: None,
        setup_status: response.setup_status,
        setup_token_id: response.setup_token_id,
        deployment_link: response.deployment_link,
        setup: response.setup,
        files_written,
    };

    render_setup_output(&output, json)
}

async fn list_managers_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    cloud: Option<PrivateManagerCloudArg>,
    json: bool,
) -> Result<()> {
    let workspace_param = ListManagersWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;

    let response = client
        .list_managers()
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing managers".to_string(),
            url: None,
        })?;

    let cloud_filter = cloud.map(|value| value.as_api_str().to_string());
    let summaries: Vec<ManagerSummary> = response
        .into_inner()
        .into_iter()
        .filter(|manager| !manager.is_system)
        .filter(|manager| {
            manager
                .setup_status
                .as_ref()
                .map(|status| status.to_string() != "deleted")
                .unwrap_or(true)
        })
        .filter(|manager| {
            cloud_filter
                .as_ref()
                .map(|cloud| {
                    manager
                        .cloud
                        .as_ref()
                        .map(|manager_cloud| manager_cloud.to_string() == *cloud)
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        })
        .map(manager_summary)
        .collect();

    if json {
        return print_json(&summaries);
    }

    if summaries.is_empty() {
        println!("(no private managers)");
        return Ok(());
    }

    let mut table = make_table(&[
        "Name",
        "ID",
        "Cloud",
        "Region",
        "Setup",
        "Health",
        "Deployments",
        "Defaults",
    ]);
    for manager in &summaries {
        table.add_row(vec![
            manager.name.clone().into(),
            manager.id.clone().into(),
            manager
                .cloud
                .clone()
                .unwrap_or_else(|| "-".to_string())
                .into(),
            manager
                .region
                .clone()
                .unwrap_or_else(|| "-".to_string())
                .into(),
            manager
                .setup_status
                .clone()
                .unwrap_or_else(|| "-".to_string())
                .into(),
            status_cell(&manager.status),
            manager.managed_deployment_count.to_string().into(),
            manager.default_project_count.to_string().into(),
        ]);
    }
    print_table(table);
    Ok(())
}

async fn status_manager_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
    watch: bool,
    json: bool,
) -> Result<()> {
    if watch && json {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "watch".to_string(),
            message:
                "`alien managers status --watch --json` is not supported; poll without --watch"
                    .to_string(),
        }));
    }

    loop {
        let summary = get_manager_summary(client, workspace, manager_id).await?;
        if !watch {
            if json {
                return print_json(&summary);
            }
            render_manager_status(&summary);
            return Ok(());
        }

        render_manager_status(&summary);
        if manager_setup_terminal(summary.setup_status.as_deref()) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn get_manager_summary(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
) -> Result<ManagerSummary> {
    let workspace_param = GetManagerWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let id_param = validate_manager_id(manager_id)?;

    let response = client
        .get_manager()
        .id(&id_param)
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "retrieving manager details".to_string(),
            url: None,
        })?;

    Ok(manager_summary(response.into_inner()))
}

fn render_manager_status(summary: &ManagerSummary) {
    println!("Manager:");
    println!("  ID: {}", summary.id);
    println!("  Name: {}", summary.name);
    println!("  Health: {}", summary.status);
    if let Some(setup_status) = &summary.setup_status {
        println!("  Setup: {}", setup_status);
    }
    if let Some(cloud) = &summary.cloud {
        println!("  Cloud: {}", cloud);
    }
    if let Some(region) = &summary.region {
        println!("  Region: {}", region);
    }
    println!("  Targets: {}", summary.targets.join(", "));
    println!("  Deployments: {}", summary.managed_deployment_count);
    println!("  Default Projects: {}", summary.default_project_count);
    if let Some(url) = &summary.url {
        println!("  URL: {}", url);
    }
    if let Some(version) = &summary.version {
        println!("  Version: {}", version);
    }
    if let Some(last_heartbeat) = &summary.last_heartbeat_at {
        println!("  Last Heartbeat: {}", last_heartbeat);
    }
    println!("  Created: {}", summary.created_at);
}

async fn cancel_manager_task(
    auth: &AuthHttp,
    workspace: &str,
    manager_id: &str,
    yes: bool,
    json: bool,
) -> Result<()> {
    validate_manager_id(manager_id)?;
    let mode = confirmation_mode(yes, json, "cancel")?;
    if matches!(mode, ConfirmationMode::Prompt)
        && !prompt_confirm("Cancel this incomplete manager setup?", false)?
    {
        println!("Cancel skipped.");
        return Ok(());
    }

    let path = format!("/v1/managers/{manager_id}/cancel-setup");
    let url = api_url(&auth.base_url, &path, workspace, None)?;
    let _: serde_json::Value = send_json::<(), _>(auth, Method::POST, url, None::<&()>).await?;

    if json {
        return print_json(&ManagerMutationOutput {
            id: manager_id.to_string(),
            action: "cancel".to_string(),
            completed: true,
        });
    }

    println!("Manager setup canceled: {manager_id}");
    Ok(())
}

async fn delete_manager_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
    yes: bool,
    json: bool,
) -> Result<()> {
    let mode = confirmation_mode(yes, json, "delete")?;
    let summary = get_manager_summary(client, workspace, manager_id).await?;

    if !json {
        println!("About to delete manager:");
        println!("  ID: {}", summary.id);
        println!("  Name: {}", summary.name);
        println!(
            "  Managed Deployments: {}",
            summary.managed_deployment_count
        );
    }

    if matches!(mode, ConfirmationMode::Prompt) && !prompt_confirm("Delete this manager?", false)? {
        println!("Delete skipped.");
        return Ok(());
    }

    let workspace_param = DeleteManagerWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    let id_param = validate_manager_id(manager_id)?;

    client
        .delete_manager()
        .id(id_param)
        .workspace(&workspace_param)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "deleting manager".to_string(),
            url: None,
        })?;

    if json {
        return print_json(&ManagerMutationOutput {
            id: summary.id,
            action: "delete".to_string(),
            completed: true,
        });
    }

    println!("Manager delete started: {}", summary.id);
    Ok(())
}

async fn defaults_task(
    auth: &AuthHttp,
    ctx: &ExecutionMode,
    workspace: &str,
    cmd: DefaultsCmd,
    json: bool,
) -> Result<()> {
    match cmd {
        DefaultsCmd::Show { project } => {
            let project = resolve_project_arg(ctx, project.as_deref(), !json).await?;
            let output = get_project_defaults(auth, workspace, &project).await?;
            render_defaults_output(&output, json)
        }
        DefaultsCmd::Set {
            project,
            cloud,
            manager,
        } => {
            validate_manager_id(&manager)?;
            let project = resolve_project_arg(ctx, project.as_deref(), !json).await?;
            let mut output = get_project_defaults(auth, workspace, &project).await?;
            let mut defaults = output.default_managers.unwrap_or_default();
            set_default_manager(&mut defaults, cloud, Some(manager));
            output = patch_project_defaults(auth, workspace, &project, Some(defaults)).await?;
            render_defaults_mutation_output(&output, json)
        }
        DefaultsCmd::Clear { project, cloud } => {
            let project = resolve_project_arg(ctx, project.as_deref(), !json).await?;
            let mut output = get_project_defaults(auth, workspace, &project).await?;
            let mut defaults = output.default_managers.unwrap_or_default();
            match cloud {
                DefaultsClearTarget::Aws => defaults.aws = None,
                DefaultsClearTarget::Gcp => defaults.gcp = None,
                DefaultsClearTarget::Azure => defaults.azure = None,
                DefaultsClearTarget::All => {
                    defaults = ProjectDefaultManagersJson::default();
                }
            }
            let next =
                if defaults.aws.is_none() && defaults.gcp.is_none() && defaults.azure.is_none() {
                    None
                } else {
                    Some(defaults)
                };
            output = patch_project_defaults(auth, workspace, &project, next).await?;
            render_defaults_mutation_output(&output, json)
        }
    }
}

async fn resolve_project_arg(
    ctx: &ExecutionMode,
    project: Option<&str>,
    allow_prompt: bool,
) -> Result<String> {
    let (project_id, _) = ctx.resolve_project(project, allow_prompt).await?;
    Ok(project_id)
}

async fn get_project_defaults(
    auth: &AuthHttp,
    workspace: &str,
    project: &str,
) -> Result<ProjectDefaultsOutput> {
    let path = format!("/v1/projects/{}", urlencoding::encode(project));
    let url = api_url(&auth.base_url, &path, workspace, None)?;
    let value: serde_json::Value = send_json::<(), _>(auth, Method::GET, url, None::<&()>).await?;
    project_defaults_from_value(value)
}

async fn patch_project_defaults(
    auth: &AuthHttp,
    workspace: &str,
    project: &str,
    defaults: Option<ProjectDefaultManagersJson>,
) -> Result<ProjectDefaultsOutput> {
    let path = format!("/v1/projects/{}", urlencoding::encode(project));
    let url = api_url(&auth.base_url, &path, workspace, None)?;
    let value: serde_json::Value = send_json(
        auth,
        Method::PATCH,
        url,
        Some(&json!({ "defaultManagers": defaults })),
    )
    .await?;
    project_defaults_from_value(value)
}

fn project_defaults_from_value(value: serde_json::Value) -> Result<ProjectDefaultsOutput> {
    let project_id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            AlienError::new(ErrorData::JsonError {
                operation: "parse project".to_string(),
                reason: "project response is missing id".to_string(),
            })
        })?
        .to_string();
    let project_name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            AlienError::new(ErrorData::JsonError {
                operation: "parse project".to_string(),
                reason: "project response is missing name".to_string(),
            })
        })?
        .to_string();
    let default_managers = value
        .get("defaultManagers")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse project default managers".to_string(),
            reason: "defaultManagers has an unexpected shape".to_string(),
        })?;

    Ok(ProjectDefaultsOutput {
        project_id,
        project_name,
        default_managers,
    })
}

fn render_defaults_output(output: &ProjectDefaultsOutput, json: bool) -> Result<()> {
    if json {
        return print_json(output);
    }
    println!("Project: {} ({})", output.project_name, output.project_id);
    render_default_managers(output.default_managers.as_ref());
    Ok(())
}

fn render_defaults_mutation_output(output: &ProjectDefaultsOutput, json: bool) -> Result<()> {
    if json {
        return print_json(&DefaultsMutationOutput {
            project_id: output.project_id.clone(),
            project_name: output.project_name.clone(),
            default_managers: output.default_managers.clone(),
        });
    }
    println!(
        "Project defaults updated: {} ({})",
        output.project_name, output.project_id
    );
    render_default_managers(output.default_managers.as_ref());
    Ok(())
}

fn render_default_managers(defaults: Option<&ProjectDefaultManagersJson>) {
    let empty = ProjectDefaultManagersJson::default();
    let defaults = defaults.unwrap_or(&empty);
    println!("  AWS: {}", defaults.aws.as_deref().unwrap_or("-"));
    println!("  GCP: {}", defaults.gcp.as_deref().unwrap_or("-"));
    println!("  Azure: {}", defaults.azure.as_deref().unwrap_or("-"));
}

fn set_default_manager(
    defaults: &mut ProjectDefaultManagersJson,
    cloud: PrivateManagerCloudArg,
    manager_id: Option<String>,
) {
    match cloud {
        PrivateManagerCloudArg::Aws => defaults.aws = manager_id,
        PrivateManagerCloudArg::Gcp => defaults.gcp = manager_id,
        PrivateManagerCloudArg::Azure => defaults.azure = manager_id,
    }
}

fn render_setup_output(output: &ManagerSetupOutput, json: bool) -> Result<()> {
    if json {
        return print_json(output);
    }

    println!("Manager setup ready:");
    println!("  ID: {}", output.manager_id);
    if let Some(name) = &output.name {
        println!("  Name: {}", name);
    }
    if let Some(cloud) = &output.cloud {
        println!("  Cloud: {}", cloud);
    }
    if let Some(region) = &output.region {
        println!("  Region: {}", region);
    }
    println!("  Setup: {}", output.setup_status);
    println!("  Deployment Portal: {}", output.deployment_link);
    render_setup_action(&output.setup);
    if !output.files_written.is_empty() {
        println!("  Files:");
        for file in &output.files_written {
            println!("    {}", file);
        }
    }
    println!();
    println!("Next: alien managers status {} --watch", output.manager_id);
    Ok(())
}

fn render_setup_action(setup: &serde_json::Value) {
    let method = setup
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    println!("  Method: {}", method);
    match method {
        "cloudformation" => {
            print_json_field(setup, "launchUrl", "  CloudFormation");
            print_json_field(setup, "templateUrl", "  Template");
            print_json_field(setup, "stackName", "  Stack");
        }
        "google-oauth" => {
            print_json_field(setup, "deploymentPortalUrl", "  Google OAuth");
            print_json_field(setup, "oauthStartUrl", "  OAuth API");
        }
        "terraform" => {
            print_json_field(setup, "moduleSource", "  Terraform Module");
            print_json_field(setup, "providerSource", "  Terraform Provider");
            println!("  Terraform: use --output-dir to write main.tf and terraform.tfvars");
        }
        _ => {}
    }
}

fn print_json_field(setup: &serde_json::Value, field: &str, label: &str) {
    if let Some(value) = setup.get(field).and_then(serde_json::Value::as_str) {
        println!("{label}: {value}");
    }
}

fn write_setup_files(output_dir: Option<&Path>, setup: &serde_json::Value) -> Result<Vec<String>> {
    let Some(output_dir) = output_dir else {
        return Ok(Vec::new());
    };
    let method = setup
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if method != "terraform" {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "output-dir".to_string(),
            message: "--output-dir is only supported for terraform setup methods".to_string(),
        }));
    }

    fs::create_dir_all(output_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create directory".to_string(),
            file_path: output_dir.display().to_string(),
            reason: "failed to create Terraform setup directory".to_string(),
        })?;

    let files = [
        (
            "main.tf",
            setup.get("mainTf").and_then(serde_json::Value::as_str),
        ),
        (
            "terraform.tfvars",
            setup.get("tfvars").and_then(serde_json::Value::as_str),
        ),
    ];
    let mut written = Vec::new();
    for (name, content) in files {
        let content = content.ok_or_else(|| {
            AlienError::new(ErrorData::JsonError {
                operation: "parse terraform setup".to_string(),
                reason: format!("terraform setup is missing {name}"),
            })
        })?;
        let path = output_dir.join(name);
        fs::write(&path, content)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "write".to_string(),
                file_path: path.display().to_string(),
                reason: "failed to write Terraform setup file".to_string(),
            })?;
        written.push(path.display().to_string());
    }
    Ok(written)
}

fn maybe_open_setup(setup: &serde_json::Value, open_browser: bool) -> Result<()> {
    if !open_browser {
        return Ok(());
    }

    let url = setup
        .get("launchUrl")
        .or_else(|| setup.get("deploymentPortalUrl"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            AlienError::new(ErrorData::JsonError {
                operation: "open setup URL".to_string(),
                reason: "setup response does not include a browser URL".to_string(),
            })
        })?;

    open::that(url)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("failed to open setup URL: {url}"),
        })?;
    Ok(())
}

fn validate_setup_side_effect_flags(
    open_browser: bool,
    no_browser: bool,
    json: bool,
) -> Result<()> {
    if !open_browser {
        return Ok(());
    }
    if json {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "open".to_string(),
            message: "`--open` is not supported with `--json`".to_string(),
        }));
    }
    if no_browser {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "open".to_string(),
            message: "`--open` cannot be used with `--no-browser`".to_string(),
        }));
    }
    Ok(())
}

fn default_setup_method_for_cloud(cloud: PrivateManagerCloudArg) -> PrivateManagerSetupMethodArg {
    match cloud {
        PrivateManagerCloudArg::Aws => PrivateManagerSetupMethodArg::Cloudformation,
        PrivateManagerCloudArg::Gcp => PrivateManagerSetupMethodArg::GoogleOauth,
        PrivateManagerCloudArg::Azure => PrivateManagerSetupMethodArg::Terraform,
    }
}

fn manager_summary(manager: alien_platform_api::types::Manager) -> ManagerSummary {
    ManagerSummary {
        id: manager.id.as_str().to_string(),
        name: manager.name,
        status: manager.status.to_string(),
        setup_status: manager.setup_status.map(|status| status.to_string()),
        cloud: manager.cloud.map(|cloud| cloud.to_string()),
        region: manager.region,
        targets: manager
            .targets
            .into_iter()
            .map(|target| target.to_string())
            .collect(),
        managed_deployment_count: manager.managed_deployment_count,
        default_project_count: manager.default_project_count,
        created_at: manager.created_at.to_string(),
        url: manager.url,
        version: manager.version,
        last_heartbeat_at: manager.last_heartbeat_at.map(|value| value.to_string()),
    }
}

fn manager_setup_terminal(setup_status: Option<&str>) -> bool {
    matches!(
        setup_status,
        Some("active" | "failed" | "deleted" | "canceled" | "cancelled")
    )
}

fn validate_manager_id(manager_id: &str) -> Result<ManagerId> {
    ManagerId::try_from(manager_id)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "id".to_string(),
            message: "manager ID format is invalid (expected mgr_...)".to_string(),
        })
}

fn confirmation_mode(action_yes: bool, json: bool, action: &str) -> Result<ConfirmationMode> {
    InteractionMode::current(json).confirmation_mode(
        action_yes,
        &format!(
            "`alien managers {action} --json` requires `--yes`; confirmation prompts are disabled in machine mode"
        ),
    )
}

fn api_url(
    base_url: &str,
    path: &str,
    workspace: &str,
    extra_query: Option<&[(&str, &str)]>,
) -> Result<Url> {
    let mut url =
        Url::parse(base_url)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "platform base URL is invalid".to_string(),
            })?;
    url.set_path(path);
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("workspace", workspace);
        if let Some(extra_query) = extra_query {
            for (key, value) in extra_query {
                query.append_pair(key, value);
            }
        }
    }
    Ok(url)
}

async fn send_json<B, T>(auth: &AuthHttp, method: Method, url: Url, body: Option<&B>) -> Result<T>
where
    B: Serialize + ?Sized,
    T: for<'de> Deserialize<'de>,
{
    let mut request = auth.reqwest_client().request(method, url.clone());
    if let Some(body) = body {
        request = request.json(body);
    }
    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "sending platform API request".to_string(),
                url: Some(url.to_string()),
            })?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "reading platform API response".to_string(),
            url: Some(url.to_string()),
        })?;

    if !status.is_success() {
        let message = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&bytes).to_string());
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("platform API returned {status}: {message}"),
            url: Some(url.to_string()),
        }));
    }

    serde_json::from_slice(&bytes)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse platform API response".to_string(),
            reason: "response JSON did not match the expected shape".to_string(),
        })
}

/// Format an event's data as a human-readable summary.
fn format_event_data(data: &alien_platform_api::types::EventData) -> String {
    use alien_platform_api::types::EventData;

    match data {
        EventData::LoadingConfiguration => "Loading configuration".to_string(),
        EventData::Finished => "Finished".to_string(),
        EventData::BuildingStack { stack } => format!("Building stack: {}", stack),
        EventData::RunningPreflights { platform, stack } => {
            format!("Running preflights: {} ({})", stack, platform)
        }
        EventData::DownloadingAlienRuntime { target_triple, .. } => {
            format!("Downloading alien-runtime ({})", target_triple)
        }
        EventData::BuildingResource {
            resource_name,
            resource_type,
            ..
        } => format!("Building {} '{}'", resource_type, resource_name),
        EventData::BuildingImage { image } => format!("Building image: {}", image),
        EventData::PushingImage { image, .. } => format!("Pushing image: {}", image),
        EventData::PushingStack {
            stack, platform, ..
        } => {
            format!("Pushing stack: {} ({})", stack, platform)
        }
        EventData::PushingResource {
            resource_name,
            resource_type,
        } => format!("Pushing {} '{}'", resource_type, resource_name),
        EventData::CreatingRelease { .. } => "Creating release".to_string(),
        other => format!("{:?}", other),
    }
}

fn format_event_state(state: &alien_platform_api::types::EventState) -> &'static str {
    use alien_platform_api::types::EventState;

    match state {
        EventState::Started => "started",
        EventState::Success => "success",
        EventState::Failed { .. } => "FAILED",
        EventState::None => "",
    }
}

async fn events_manager_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
    follow: bool,
    json: bool,
) -> Result<()> {
    let workspace_param = ListManagerEventsWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;
    validate_manager_id(manager_id)?;

    let mut last_event_id: Option<String> = None;
    loop {
        let response = client
            .list_manager_events()
            .id(manager_id.to_string())
            .workspace(&workspace_param)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: "listing manager events".to_string(),
                url: None,
            })?;

        let events_response = response.into_inner();
        let events = &events_response.items;
        let new_events: Vec<_> = match &last_event_id {
            Some(seen_id) => {
                let pos = events.iter().position(|event| *event.id == *seen_id);
                match pos {
                    Some(idx) => events[idx + 1..].to_vec(),
                    None => events.to_vec(),
                }
            }
            None => events.to_vec(),
        };

        if json {
            let payload: Vec<ManagerEventSummary> = new_events
                .iter()
                .map(|event| ManagerEventSummary {
                    id: (*event.id).clone(),
                    created_at: event.created_at.to_string(),
                    state: format_event_state(&event.state).to_string(),
                    summary: format_event_data(&event.data),
                })
                .collect();
            return print_json(&payload);
        }

        if events.is_empty() && last_event_id.is_none() {
            println!("(no events)");
        }

        for event in &new_events {
            let state = format_event_state(&event.state);
            let data = format_event_data(&event.data);
            let timestamp = event.created_at.format("%H:%M:%S");
            if state.is_empty() {
                println!("[{}] {}", timestamp, data);
            } else {
                println!("[{}] {} ({})", timestamp, data, state);
            }
            last_event_id = Some((*event.id).clone());
        }

        if !follow {
            break;
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_terminal_statuses_are_terminal() {
        assert!(manager_setup_terminal(Some("active")));
        assert!(manager_setup_terminal(Some("failed")));
        assert!(!manager_setup_terminal(Some("pending")));
    }

    #[test]
    fn default_manager_setter_updates_requested_cloud_only() {
        let mut defaults = ProjectDefaultManagersJson {
            aws: Some("mgr_aaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
            gcp: None,
            azure: None,
        };

        set_default_manager(
            &mut defaults,
            PrivateManagerCloudArg::Gcp,
            Some("mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
        );

        assert_eq!(
            defaults.aws.as_deref(),
            Some("mgr_aaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            defaults.gcp.as_deref(),
            Some("mgr_bbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
    }

    #[test]
    fn cloud_and_setup_args_match_api_values() {
        assert_eq!(PrivateManagerCloudArg::Aws.as_api_str(), "aws");
        assert_eq!(PrivateManagerCloudArg::Gcp.as_api_str(), "gcp");
        assert_eq!(
            PrivateManagerSetupMethodArg::GoogleOauth.as_api_str(),
            "google-oauth"
        );
    }

    #[test]
    fn open_is_rejected_in_machine_mode_before_api_calls() {
        let err = validate_setup_side_effect_flags(true, false, true).unwrap_err();
        assert!(err.to_string().contains("not supported with `--json`"));
    }

    #[test]
    fn default_setup_method_matches_cloud_defaults() {
        assert_eq!(
            default_setup_method_for_cloud(PrivateManagerCloudArg::Aws),
            PrivateManagerSetupMethodArg::Cloudformation
        );
        assert_eq!(
            default_setup_method_for_cloud(PrivateManagerCloudArg::Gcp),
            PrivateManagerSetupMethodArg::GoogleOauth
        );
        assert_eq!(
            default_setup_method_for_cloud(PrivateManagerCloudArg::Azure),
            PrivateManagerSetupMethodArg::Terraform
        );
    }
}
