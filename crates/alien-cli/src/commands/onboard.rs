use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::project_link::{ensure_project_linked, get_project_by_name};
use alien_platform_api::types::{
    CreateDeploymentGroupRequest, CreateDeploymentGroupRequestName,
    CreateDeploymentGroupRequestProject, CreateDeploymentGroupTokenId,
    CreateDeploymentGroupTokenWorkspace, CreateDeploymentGroupWorkspace,
};
use alien_platform_api::SdkResultExt as _;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use std::io::{self, Write};
use std::num::NonZeroU64;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Create a deployment group and generate a deployment link",
    long_about = "Create a deployment group for fleet deployments and generate a deployment link that can be shared with your team."
)]
pub struct OnboardArgs {
    /// Name of the deployment group
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Maximum number of deployments in this deployment group
    #[arg(long, default_value = "100")]
    pub max_deployments: u64,
}

pub async fn onboard_task(args: OnboardArgs, ctx: ExecutionMode) -> Result<()> {
    let http = ctx.auth_http().await?;
    let client = http.sdk_client();

    let workspace_name = ctx.resolve_workspace().await?;

    // Get project: use global --project if provided, otherwise link interactively
    let project_id = if let Some(project) = ctx.project_override() {
        // If it looks like a project ID (starts with prj_), use it directly
        if project.starts_with("prj_") {
            println!("Using project: {}", project);
            project.to_string()
        } else {
            // Treat as project name, resolve to ID
            let link = get_project_by_name(&http, &workspace_name, project).await?;
            println!("Using project: {} ({})", link.project_name, link.project_id);
            link.project_id
        }
    } else {
        let current_dir =
            std::env::current_dir()
                .into_alien_error()
                .context(ErrorData::FileOperationFailed {
                    operation: "get current directory".to_string(),
                    file_path: ".".to_string(),
                    reason: "Failed to get current directory".to_string(),
                })?;
        let project_link = ensure_project_linked(&current_dir, &http, &workspace_name).await?;
        println!("Using linked project: {}", project_link.project_id);
        project_link.project_id
    };

    // Determine deployment group name
    let name = if let Some(name) = args.name {
        name
    } else {
        print!("Enter deployment group name: ");
        io::stdout()
            .flush()
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "flush stdout".to_string(),
                file_path: "stdout".to_string(),
                reason: "Failed to flush stdout".to_string(),
            })?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read line from stdin".to_string(),
                file_path: "stdin".to_string(),
                reason: "Failed to read from stdin".to_string(),
            })?;
        input.trim().to_string()
    };

    println!("Creating deployment group '{}'...", name);

    // Create deployment group using SDK
    let workspace_param = CreateDeploymentGroupWorkspace::try_from(workspace_name.clone())
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "Invalid workspace name".to_string(),
        })?;

    let name_param = CreateDeploymentGroupRequestName::try_from(name.clone())
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "name".to_string(),
            message: "Invalid deployment group name".to_string(),
        })?;

    let max_deployments = NonZeroU64::new(args.max_deployments).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "max_deployments".to_string(),
            message: "max_deployments must be greater than 0".to_string(),
        })
    })?;

    let request = CreateDeploymentGroupRequest {
        name: name_param,
        project: CreateDeploymentGroupRequestProject::try_from(project_id.as_str())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "project".to_string(),
                message: "project ID format is invalid".to_string(),
            })?,
        max_deployments,
    };

    let response = client
        .create_deployment_group()
        .workspace(&workspace_param)
        .body(&request)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group".to_string(),
            url: None,
        })?;

    let deployment_group = response.into_inner();
    let deployment_group_id = deployment_group.id.to_string();

    println!(
        "✓ Deployment group '{}' created successfully (ID: {})",
        name, deployment_group_id
    );

    println!("Generating deployment token...");

    // Create deployment group token using SDK
    let token_workspace_param =
        CreateDeploymentGroupTokenWorkspace::try_from(workspace_name.clone())
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "workspace".to_string(),
                message: "Invalid workspace name".to_string(),
            })?;

    let dg_id_param = CreateDeploymentGroupTokenId::try_from(deployment_group_id.clone())
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "deployment_group_id".to_string(),
            message: "Invalid deployment group ID".to_string(),
        })?;

    let token_request = alien_platform_api::types::CreateDeploymentGroupTokenRequest {
        description: Some(format!("Deployment token for {}", name)),
    };

    let token_response = client
        .create_deployment_group_token()
        .workspace(&token_workspace_param)
        .id(&dg_id_param)
        .body(&token_request)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group token".to_string(),
            url: None,
        })?;

    let token_data = token_response.into_inner();

    println!();
    println!("✓ Deployment link generated successfully!");
    println!();
    println!("🔗 Deployment Link:");
    println!("   {}", token_data.deployment_link);
    println!();
    println!("📋 Token (save securely):");
    println!("   {}", token_data.token);
    println!();
    println!("Share the deployment link with your team to deploy in this group.");
    println!("Max deployments: {}", args.max_deployments);

    Ok(())
}
