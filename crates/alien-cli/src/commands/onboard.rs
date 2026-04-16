use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json, prompt_text};
use crate::ui::{accent, command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_error::{AlienError, Context};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Onboard a customer and generate a deployment link or token",
    long_about = "Create a deployment group for a customer and generate a deployment link (platform) or CLI command (standalone) to share with their admin."
)]
pub struct OnboardArgs {
    /// Customer name
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Maximum number of deployments for this customer
    #[arg(long, default_value = "100")]
    pub max_deployments: u64,

    /// Output in JSON format (for scripting)
    #[arg(long)]
    pub json: bool,
}

pub async fn onboard_task(args: OnboardArgs, ctx: ExecutionMode) -> Result<()> {
    let name = if let Some(ref name) = args.name {
        name.clone()
    } else if args.json || !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Customer name is required in non-interactive mode. Pass `alien onboard <name>`."
                    .to_string(),
        }));
    } else {
        prompt_text("Customer name", None)?
    };

    match ctx {
        #[cfg(feature = "platform")]
        ExecutionMode::Platform { .. } => onboard_platform(args, ctx, name).await,
        _ => onboard_standalone(args, ctx, name).await,
    }
}

/// Platform mode: use Platform API directly to get deployment link.
#[cfg(feature = "platform")]
async fn onboard_platform(args: OnboardArgs, ctx: ExecutionMode, name: String) -> Result<()> {
    use alien_platform_api::SdkResultExt;

    let (project_id, _project_link) = ctx.resolve_project(None, !args.json).await?;
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let client = ctx.sdk_client().await?;

    if !args.json {
        println!("{}", contextual_heading("Onboarding", &name, &[]));
    }
    let steps = if args.json {
        None
    } else {
        let steps = FixedSteps::new(&["Create deployment group", "Generate deployment link"]);
        steps.activate(0, Some(name.clone()));
        Some(steps)
    };

    // Create deployment group via Platform API
    let workspace_param =
        alien_platform_api::types::CreateDeploymentGroupWorkspace::try_from(workspace.as_str())
            .map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "workspace".to_string(),
                    message: format!("Invalid workspace: {}", e),
                })
            })?;

    let response = client
        .create_deployment_group()
        .workspace(&workspace_param)
        .body(alien_platform_api::types::CreateDeploymentGroupRequest {
            name: name.clone().try_into().map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: format!("{}", e),
                })
            })?,
            project: project_id.clone().try_into().map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "project".to_string(),
                    message: format!("{}", e),
                })
            })?,
            max_deployments: std::num::NonZeroU64::new(args.max_deployments as u64)
                .unwrap_or(std::num::NonZeroU64::new(100).unwrap()),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group".to_string(),
            url: None,
        })?;

    let deployment_group_id = response.id.clone();

    if let Some(steps) = &steps {
        steps.complete(0, Some(deployment_group_id.clone()));
        steps.activate(1, Some("Generating deployment link".to_string()));
    }

    // Create token via Platform API — returns deploymentLink
    let token_workspace_param = alien_platform_api::types::CreateDeploymentGroupTokenWorkspace::try_from(workspace.as_str())
        .map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "workspace".to_string(),
                message: format!("Invalid workspace: {}", e),
            })
        })?;

    let dg_id_param =
        alien_platform_api::types::CreateDeploymentGroupTokenId::try_from(
            deployment_group_id.as_str(),
        )
        .map_err(|e| {
            AlienError::new(ErrorData::ValidationError {
                field: "id".to_string(),
                message: format!("Invalid deployment group ID: {}", e),
            })
        })?;

    let token_response = client
        .create_deployment_group_token()
        .id(&dg_id_param)
        .workspace(&token_workspace_param)
        .body(alien_platform_api::types::CreateDeploymentGroupTokenRequest {
            description: None,
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to generate deployment link".to_string(),
            url: None,
        })?;

    let deployment_link = token_response.deployment_link.clone();

    if args.json {
        print_json(&serde_json::json!({
            "deploymentGroupId": deployment_group_id,
            "name": name,
            "deploymentLink": deployment_link,
            "maxDeployments": args.max_deployments,
        }))?;
        return Ok(());
    }

    if let Some(steps) = &steps {
        steps.complete(1, Some("Deployment link ready".to_string()));
    }
    drop(steps);

    println!("{}", success_line("Ready to deploy."));
    println!("{} {}", dim_label("Customer"), name);
    println!();
    println!(
        "{}",
        dim_label("Share this link with the customer's admin:")
    );
    println!("  {}", accent(&deployment_link));
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command("wait for customer setup, then run alien deployments ls")
    );

    Ok(())
}

/// Standalone/Dev mode: use manager API, show CLI command.
async fn onboard_standalone(args: OnboardArgs, ctx: ExecutionMode, name: String) -> Result<()> {
    use alien_manager_api::types::CreateDeploymentGroupRequest;
    use alien_manager_api::SdkResultExt;

    let (project_id, _project_link) = ctx.resolve_project(None, !args.json).await?;

    // Resolve manager (known in Standalone/Dev mode)
    let mgr = ctx.resolve_manager(&project_id, "local").await?;

    if !args.json {
        println!("{}", contextual_heading("Onboarding", &name, &[]));
    }
    let steps = if args.json {
        None
    } else {
        let steps = FixedSteps::new(&["Create deployment group", "Generate deployment token"]);
        steps.activate(0, Some(name.clone()));
        Some(steps)
    };

    let response = mgr
        .client
        .create_deployment_group()
        .body(CreateDeploymentGroupRequest {
            name: name.clone(),
            max_deployments: Some(args.max_deployments as i64),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group".to_string(),
            url: None,
        })?;

    let deployment_group_id = response.id.clone();

    if let Some(steps) = &steps {
        steps.complete(0, Some(deployment_group_id.clone()));
        steps.activate(1, Some("Creating deployment token".to_string()));
    }

    let token_response = mgr
        .client
        .create_deployment_group_token()
        .id(&deployment_group_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group token".to_string(),
            url: None,
        })?;

    if args.json {
        print_json(&serde_json::json!({
            "deploymentGroupId": deployment_group_id,
            "name": name,
            "token": token_response.token,
            "maxDeployments": args.max_deployments,
        }))?;
        return Ok(());
    }

    if let Some(steps) = &steps {
        steps.complete(1, Some("Deployment token ready".to_string()));
    }
    drop(steps);

    println!("{}", success_line("Ready to deploy."));
    println!("{} {}", dim_label("Customer"), name);
    println!("{} {}", dim_label("Token"), accent(&token_response.token));
    println!();
    println!(
        "{}",
        dim_label("Share with the customer's admin:")
    );
    println!(
        "  curl -fsSL {}/install | bash",
        mgr.manager_url.trim_end_matches('/')
    );
    println!("  alien-deploy up \\");
    println!("    --token {} \\", token_response.token);
    println!("    --name <deployment-name> \\");
    println!("    --platform <aws|gcp|azure> \\");
    println!(
        "    --manager-url {}",
        mgr.manager_url.trim_end_matches('/')
    );
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command("wait for customer setup, then run alien deployments ls")
    );

    Ok(())
}
