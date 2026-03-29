//! Platform-only commands for managing private managers.
//!
//! Private managers are alien-manager instances deployed to a user's cloud by
//! the platform. These commands interact with the platform API to create,
//! monitor, and destroy them.

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::interaction::{ConfirmationMode, InteractionMode};
use crate::output::{print_json, prompt_confirm};
use crate::ui::{make_table, print_table, status_cell};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    CreateManagerWorkspace, DeleteManagerWorkspace, GetManagerWorkspace,
    ListManagerEventsWorkspace, ListManagersWorkspace, ManagerId, NewManagerRequest,
    NewManagerRequestPlatform, NewManagerRequestTargetsItem,
};
use alien_platform_api::SdkResultExt as _;
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manager commands",
    long_about = "Manage private managers deployed to your cloud."
)]
pub struct ManagerArgs {
    /// Print machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: ManagerCmd,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagerSummary {
    id: String,
    name: String,
    status: String,
    targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    managed_deployment_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_heartbeat_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deployment_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_system: Option<bool>,
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
struct DestroyManagerOutput {
    destroyed: bool,
    id: String,
    name: String,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ManagerCmd {
    /// Deploy a new private manager
    Deploy {
        /// Manager display name
        #[arg(long)]
        name: String,

        /// Platform to deploy the manager on (aws, gcp, or azure)
        #[arg(long)]
        platform: String,

        /// Target platforms this manager can manage (comma-separated: aws,gcp,azure,kubernetes)
        #[arg(long, value_delimiter = ',')]
        targets: Vec<String>,
    },
    /// Show manager status and details
    Status {
        /// Manager ID (e.g. mgr_...)
        id: String,
    },
    /// List managers
    #[command(alias = "list")]
    Ls,
    /// View manager events
    Events {
        /// Manager ID (e.g. mgr_...)
        id: String,

        /// Follow events (poll every 3 seconds)
        #[arg(long, short)]
        follow: bool,
    },
    /// Destroy a manager
    Destroy {
        /// Manager ID (e.g. mgr_...)
        id: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

pub async fn manager_task(args: ManagerArgs, ctx: ExecutionMode) -> Result<()> {
    if let ManagerCmd::Events { follow: true, .. } = &args.cmd {
        if args.json {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "follow".to_string(),
                message: "`alien manager events --json` does not support `--follow`; rerun without `--json` for streaming output".to_string(),
            }));
        }
    }

    if let ManagerCmd::Destroy { yes, .. } = &args.cmd {
        destroy_confirmation_mode(*yes, args.json)?;
    }

    let client = ctx.sdk_client().await?;
    let workspace_name = ctx.resolve_workspace_with_bootstrap(!args.json).await?;

    match args.cmd {
        ManagerCmd::Deploy {
            name,
            platform,
            targets,
        } => {
            deploy_manager_task(
                &client,
                &workspace_name,
                &name,
                &platform,
                targets,
                args.json,
            )
            .await?;
        }
        ManagerCmd::Status { id } => {
            status_manager_task(&client, &workspace_name, &id, args.json).await?;
        }
        ManagerCmd::Ls => {
            list_managers_task(&client, &workspace_name, args.json).await?;
        }
        ManagerCmd::Events { id, follow } => {
            events_manager_task(&client, &workspace_name, &id, follow, args.json).await?;
        }
        ManagerCmd::Destroy { id, yes } => {
            destroy_manager_task(&client, &workspace_name, &id, yes, args.json).await?;
        }
    }

    Ok(())
}

fn parse_manager_platform(platform_str: &str) -> Result<NewManagerRequestPlatform> {
    match platform_str {
        "aws" => Ok(NewManagerRequestPlatform::Aws),
        "gcp" => Ok(NewManagerRequestPlatform::Gcp),
        "azure" => Ok(NewManagerRequestPlatform::Azure),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: format!(
                "Unknown platform: {}. Managers can be deployed on: aws, gcp, azure",
                platform_str
            ),
        })),
    }
}

fn parse_target_platform(target_str: &str) -> Result<NewManagerRequestTargetsItem> {
    match target_str {
        "aws" => Ok(NewManagerRequestTargetsItem::Aws),
        "gcp" => Ok(NewManagerRequestTargetsItem::Gcp),
        "azure" => Ok(NewManagerRequestTargetsItem::Azure),
        "kubernetes" | "k8s" => Ok(NewManagerRequestTargetsItem::Kubernetes),
        "local" => Ok(NewManagerRequestTargetsItem::Local),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "targets".to_string(),
            message: format!(
                "Unknown target platform: {}. Valid values: aws, gcp, azure, kubernetes (k8s), local",
                target_str
            ),
        })),
    }
}

async fn deploy_manager_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    name: &str,
    platform_str: &str,
    target_strs: Vec<String>,
    json: bool,
) -> Result<()> {
    let platform = parse_manager_platform(platform_str)?;

    let targets: Vec<NewManagerRequestTargetsItem> = target_strs
        .iter()
        .map(|t| parse_target_platform(t))
        .collect::<Result<Vec<_>>>()?;

    if targets.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "targets".to_string(),
            message: "At least one target platform is required (e.g. --targets aws,gcp)"
                .to_string(),
        }));
    }

    let request = NewManagerRequest {
        name: name.to_string(),
        platform,
        targets,
        otlp_config: None,
    };

    let workspace_param = CreateManagerWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;

    let response = client
        .create_manager()
        .workspace(&workspace_param)
        .body(&request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "creating manager".to_string(),
            url: None,
        })?;

    let manager = response.into_inner();
    let summary = ManagerSummary {
        id: manager.id.as_str().to_string(),
        name: manager.name.clone(),
        status: manager.status.to_string(),
        targets: manager
            .targets
            .iter()
            .map(|target| target.to_string())
            .collect(),
        managed_deployment_count: None,
        created_at: None,
        url: None,
        version: None,
        last_heartbeat_at: None,
        deployment_link: Some(manager.deployment_link.to_string()),
        token: Some(manager.token.to_string()),
        is_system: None,
    };

    if json {
        return print_json(&summary);
    }

    println!("Manager created successfully!");
    println!("  ID: {}", summary.id);
    println!("  Name: {}", summary.name);
    println!("  Platform: {}", platform_str);
    println!("  Targets: {}", summary.targets.join(", "));
    println!("  Status: {}", summary.status);
    println!();
    if let Some(deployment_link) = &summary.deployment_link {
        println!("  Deployment Link: {}", deployment_link);
    }
    if let Some(token) = &summary.token {
        println!("  Token: {}", token);
        println!();
        println!("Next: alien deploy --token {} --name <deployment>", token);
    }
    println!();

    Ok(())
}

async fn status_manager_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
    json: bool,
) -> Result<()> {
    let workspace_param = GetManagerWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;

    let id_param =
        ManagerId::try_from(manager_id)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "id".to_string(),
                message: "manager ID format is invalid (expected mgr_...)".to_string(),
            })?;

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

    let manager = response.into_inner();
    let summary = ManagerSummary {
        id: manager.id.as_str().to_string(),
        name: manager.name.clone(),
        status: manager.status.to_string(),
        targets: manager
            .targets
            .iter()
            .map(|target| target.to_string())
            .collect(),
        managed_deployment_count: Some(manager.managed_deployment_count as i64),
        created_at: Some(manager.created_at.to_string()),
        url: manager.url.as_ref().map(|value| value.to_string()),
        version: manager.version.as_ref().map(|value| value.to_string()),
        last_heartbeat_at: manager.last_heartbeat_at.map(|value| value.to_string()),
        deployment_link: None,
        token: None,
        is_system: Some(manager.is_system),
    };

    if json {
        return print_json(&summary);
    }

    println!("Manager Details:");
    println!("  ID: {}", summary.id);
    println!("  Name: {}", summary.name);
    println!("  Status: {}", summary.status);
    println!("  Targets: {}", summary.targets.join(", "));
    if let Some(is_system) = summary.is_system {
        println!("  System: {}", is_system);
    }
    if let Some(managed_deployment_count) = summary.managed_deployment_count {
        println!("  Deployments: {}", managed_deployment_count);
    }

    if let Some(url) = &summary.url {
        println!("  URL: {}", url);
    }

    if let Some(version) = &summary.version {
        println!("  Version: {}", version);
    }

    if let Some(last_heartbeat) = &summary.last_heartbeat_at {
        println!("  Last Heartbeat: {}", last_heartbeat);
    }

    if let Some(created_at) = &summary.created_at {
        println!("  Created: {}", created_at);
    }

    Ok(())
}

async fn list_managers_task(
    client: &alien_platform_api::Client,
    workspace: &str,
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

    let managers = response.into_inner();
    let summaries: Vec<ManagerSummary> = managers
        .iter()
        .map(|manager| ManagerSummary {
            id: manager.id.as_str().to_string(),
            name: manager.name.clone(),
            status: manager.status.to_string(),
            targets: manager
                .targets
                .iter()
                .map(|target| target.to_string())
                .collect(),
            managed_deployment_count: Some(manager.managed_deployment_count as i64),
            created_at: Some(manager.created_at.to_string()),
            url: manager.url.as_ref().map(|value| value.to_string()),
            version: manager.version.as_ref().map(|value| value.to_string()),
            last_heartbeat_at: manager.last_heartbeat_at.map(|value| value.to_string()),
            deployment_link: None,
            token: None,
            is_system: Some(manager.is_system),
        })
        .collect();

    if json {
        return print_json(&summaries);
    }

    if summaries.is_empty() {
        println!("(no managers)");
    } else {
        let mut table = make_table(&[
            "Name",
            "ID",
            "Status",
            "Targets",
            "Deployments",
            "Version",
            "Last heartbeat",
        ]);
        for manager in &summaries {
            table.add_row(vec![
                manager.name.clone().into(),
                manager.id.clone().into(),
                status_cell(&manager.status),
                manager.targets.join(", ").into(),
                manager
                    .managed_deployment_count
                    .map(|count| count.to_string())
                    .unwrap_or_else(|| "—".to_string())
                    .into(),
                manager
                    .version
                    .clone()
                    .unwrap_or_else(|| "—".to_string())
                    .into(),
                manager
                    .last_heartbeat_at
                    .clone()
                    .or_else(|| manager.created_at.clone())
                    .unwrap_or_else(|| "—".to_string())
                    .into(),
            ]);
        }
        print_table(table);
    }

    Ok(())
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
        } => {
            format!("Building {} '{}'", resource_type, resource_name)
        }
        EventData::BuildingImage { image } => format!("Building image: {}", image),
        EventData::PushingImage { image, .. } => format!("Pushing image: {}", image),
        EventData::PushingStack { stack, platform } => {
            format!("Pushing stack: {} ({})", stack, platform)
        }
        EventData::PushingResource {
            resource_name,
            resource_type,
        } => {
            format!("Pushing {} '{}'", resource_type, resource_name)
        }
        EventData::CreatingRelease { .. } => "Creating release".to_string(),
        // Fall back to debug format for any other variants
        other => format!("{:?}", other),
    }
}

/// Format an event's state as a human-readable string.
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

    // Validate the manager ID format
    let _id_validate =
        ManagerId::try_from(manager_id)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "id".to_string(),
                message: "manager ID format is invalid (expected mgr_...)".to_string(),
            })?;

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
                let pos = events.iter().position(|e| *e.id == *seen_id);
                match pos {
                    Some(idx) => events[idx + 1..].to_vec(),
                    None => events.to_vec(),
                }
            }
            None => events.to_vec(),
        };

        if events.is_empty() && last_event_id.is_none() && !json {
            println!("(no events)");
        }

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

        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }

    Ok(())
}

async fn destroy_manager_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
    yes: bool,
    json: bool,
) -> Result<()> {
    let confirmation_mode = destroy_confirmation_mode(yes, json)?;

    // Get manager details first for confirmation
    let workspace_param_get = GetManagerWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;

    let id_param =
        ManagerId::try_from(manager_id)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "id".to_string(),
                message: "manager ID format is invalid (expected mgr_...)".to_string(),
            })?;

    let response = client
        .get_manager()
        .id(&id_param)
        .workspace(&workspace_param_get)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "retrieving manager details".to_string(),
            url: None,
        })?;

    let manager = response.into_inner();

    if !json {
        println!("About to destroy manager:");
        println!("  ID: {}", *manager.id);
        println!("  Name: {}", manager.name);
        println!("  Status: {}", manager.status);
        println!("  Deployments: {}", manager.managed_deployment_count);

        if manager.managed_deployment_count > 0 {
            println!();
            println!(
                "  WARNING: This manager is currently managing {} deployment(s).",
                manager.managed_deployment_count
            );
            println!("  Destroying it will leave those deployments unmanaged.");
        }
    }

    // Confirm destruction
    if matches!(confirmation_mode, ConfirmationMode::Prompt)
        && !prompt_confirm("Destroy this manager?", false)?
    {
        println!("Destruction cancelled.");
        return Ok(());
    }

    // Delete the manager
    let workspace_param_delete = DeleteManagerWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "workspace name format is invalid".to_string(),
        })?;

    let id_param_delete =
        ManagerId::try_from(manager_id)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "id".to_string(),
                message: "manager ID format is invalid (expected mgr_...)".to_string(),
            })?;

    client
        .delete_manager()
        .id(id_param_delete)
        .workspace(&workspace_param_delete)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "deleting manager".to_string(),
            url: None,
        })?;

    if json {
        return print_json(&DestroyManagerOutput {
            destroyed: true,
            id: manager.id.as_str().to_string(),
            name: manager.name.clone(),
        });
    }

    println!(
        "Manager '{}' ({}) destruction initiated.",
        manager.name, *manager.id
    );

    Ok(())
}

fn destroy_confirmation_mode(yes: bool, json: bool) -> Result<ConfirmationMode> {
    InteractionMode::current(json).confirmation_mode(
        yes,
        "`alien manager destroy --json` requires `--yes`; confirmation prompts are disabled in machine mode",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manager_platform_rejects_unknown_values() {
        let err = parse_manager_platform("local").unwrap_err();
        assert!(err.to_string().contains("Managers can be deployed on"));
    }

    #[test]
    fn parse_target_platform_accepts_aliases() {
        assert_eq!(
            parse_target_platform("k8s").unwrap(),
            NewManagerRequestTargetsItem::Kubernetes
        );
        assert_eq!(
            parse_target_platform("local").unwrap(),
            NewManagerRequestTargetsItem::Local
        );
    }

    #[test]
    fn format_event_state_maps_variants() {
        use alien_platform_api::types::EventState;

        assert_eq!(format_event_state(&EventState::Started), "started");
        assert_eq!(
            format_event_state(&EventState::Failed { error: None }),
            "FAILED"
        );
    }

    #[test]
    fn destroy_confirmation_mode_requires_yes_in_machine_mode() {
        let err = InteractionMode::new(true, false)
            .confirmation_mode(
                false,
                "`alien manager destroy --json` requires `--yes`; confirmation prompts are disabled in machine mode",
            )
            .unwrap_err();
        assert!(err.to_string().contains("requires `--yes`"));
    }
}
