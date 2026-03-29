//! Platform-only commands for managing private managers.
//!
//! Private managers are alien-manager instances deployed to a user's cloud by
//! the platform. These commands interact with the platform API to create,
//! monitor, and destroy them.

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    CreateManagerWorkspace, DeleteManagerWorkspace, GetManagerWorkspace,
    ListManagerEventsWorkspace, ListManagersWorkspace, ManagerId, NewManagerRequest,
    NewManagerRequestPlatform, NewManagerRequestTargetsItem,
};
use alien_platform_api::SdkResultExt as _;
use clap::{Parser, Subcommand};
use std::io::{self, Write};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manager commands",
    long_about = "Manage private managers deployed to your cloud."
)]
pub struct ManagerArgs {
    #[command(subcommand)]
    pub cmd: ManagerCmd,
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
    let client = ctx.sdk_client().await?;
    let workspace_name = ctx.resolve_workspace().await?;

    match args.cmd {
        ManagerCmd::Deploy {
            name,
            platform,
            targets,
        } => {
            deploy_manager_task(&client, &workspace_name, &name, &platform, targets).await?;
        }
        ManagerCmd::Status { id } => {
            status_manager_task(&client, &workspace_name, &id).await?;
        }
        ManagerCmd::Ls => {
            list_managers_task(&client, &workspace_name).await?;
        }
        ManagerCmd::Events { id, follow } => {
            events_manager_task(&client, &workspace_name, &id, follow).await?;
        }
        ManagerCmd::Destroy { id, yes } => {
            destroy_manager_task(&client, &workspace_name, &id, yes).await?;
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

    println!("Manager created successfully!");
    println!("  ID: {}", *manager.id);
    println!("  Name: {}", manager.name);
    println!("  Platform: {}", platform_str);
    println!(
        "  Targets: {}",
        manager
            .targets
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  Status: {}", manager.status);
    println!();
    println!("  Deployment Link: {}", manager.deployment_link);
    println!("  Token: {}", manager.token);
    println!();
    println!(
        "Deploy the manager using: alien deploy --token {}",
        manager.token
    );

    Ok(())
}

async fn status_manager_task(
    client: &alien_platform_api::Client,
    workspace: &str,
    manager_id: &str,
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

    println!("Manager Details:");
    println!("  ID: {}", *manager.id);
    println!("  Name: {}", manager.name);
    println!("  Status: {}", manager.status);
    println!(
        "  Targets: {}",
        manager
            .targets
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  System: {}", manager.is_system);
    println!("  Deployments: {}", manager.managed_deployment_count);

    if let Some(url) = &manager.url {
        println!("  URL: {}", url);
    }

    if let Some(version) = &manager.version {
        println!("  Version: {}", version);
    }

    if let Some(last_heartbeat) = &manager.last_heartbeat_at {
        println!("  Last Heartbeat: {}", last_heartbeat);
    }

    println!("  Created: {}", manager.created_at);

    Ok(())
}

async fn list_managers_task(client: &alien_platform_api::Client, workspace: &str) -> Result<()> {
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

    if managers.is_empty() {
        println!("(no managers)");
    } else {
        for manager in &managers {
            println!("Manager ID: {}", manager.id.as_str());
            println!("  Name: {}", manager.name);
            println!("  Status: {}", manager.status);
            println!(
                "  Targets: {}",
                manager
                    .targets
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!("  Deployments: {}", manager.managed_deployment_count);

            if let Some(version) = &manager.version {
                println!("  Version: {}", version);
            }

            if let Some(last_heartbeat) = &manager.last_heartbeat_at {
                println!("  Last Heartbeat: {}", last_heartbeat);
            }

            println!("  Created: {}", manager.created_at);
            println!();
        }
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

        if events.is_empty() && last_event_id.is_none() {
            println!("(no events)");
        }

        // Find events newer than what we've already printed
        let new_events: Vec<_> = match &last_event_id {
            Some(seen_id) => {
                // Find the position of the last seen event, then take everything after it.
                // Events are assumed to come in chronological order.
                let pos = events.iter().position(|e| *e.id == *seen_id);
                match pos {
                    Some(idx) => events[idx + 1..].to_vec(),
                    None => events.to_vec(), // If not found, print all
                }
            }
            None => events.to_vec(),
        };

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
) -> Result<()> {
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

    // Show what we're about to delete
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

    // Confirm destruction
    if !yes {
        print!("\nAre you sure you want to destroy this manager? [y/N] ");
        io::stdout()
            .flush()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "flush".to_string(),
                file_path: "stdout".to_string(),
                reason: "could not write output".to_string(),
            })
            .ok();

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: "stdin".to_string(),
                reason: "could not read input".to_string(),
            })?;

        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Destruction cancelled.");
            return Ok(());
        }
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

    println!(
        "Manager '{}' ({}) destruction initiated.",
        manager.name, *manager.id
    );

    Ok(())
}
