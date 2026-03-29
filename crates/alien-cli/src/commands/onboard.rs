use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json, prompt_text};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_manager_api::types::CreateDeploymentGroupRequest;
use clap::Parser;

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
                "Deployment group name is required in non-interactive mode. Pass `alien onboard <name>`."
                    .to_string(),
        }));
    } else {
        prompt_text("Deployment group name", None)?
    };

    let (project_id, _project_link) = ctx.resolve_project(None, !args.json).await?;

    // Resolve manager (discovers URL in Platform mode, known in Standalone/Dev)
    let mgr = ctx.resolve_manager(&project_id, "local").await?;

    println!("Creating deployment group '{}'...", name);

    let response = mgr
        .client
        .create_deployment_group()
        .body(CreateDeploymentGroupRequest {
            name: name.clone(),
            max_deployments: Some(args.max_deployments as i64),
        })
        .send()
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("Failed to create deployment group: {}", e),
                url: None,
            })
        })?;

    let deployment_group_id = response.id.clone();

    println!(
        "Deployment group '{}' created successfully (ID: {})",
        name, deployment_group_id
    );

    println!("Generating deployment token...");

    let token_response = mgr
        .client
        .create_deployment_group_token()
        .id(&deployment_group_id)
        .send()
        .await
        .map_err(|e| {
            AlienError::new(ErrorData::ApiRequestFailed {
                message: format!("Failed to create deployment group token: {}", e),
                url: None,
            })
        })?;

    let deploy_link = format!(
        "{}/deploy#token={}",
        mgr.manager_url.trim_end_matches('/'),
        token_response.token
    );

    if args.json {
        print_json(&serde_json::json!({
            "deploymentGroupId": deployment_group_id,
            "name": name,
            "deployLink": deploy_link,
            "token": token_response.token,
            "maxDeployments": args.max_deployments,
        }))?;
        return Ok(());
    }

    println!();
    println!("  \x1b[1;32mDeployment group created successfully!\x1b[0m");
    println!();
    println!("  \x1b[1;4mDeploy Link\x1b[0m");
    println!();
    println!("    \x1b[36m{}\x1b[0m", deploy_link);
    println!();
    println!("  Share this link with your team. They can open it in a browser");
    println!("  to see install and deploy instructions for their platform.");
    println!();
    println!("  \x1b[1;4mDirect CLI Usage\x1b[0m");
    println!();
    println!(
        "    curl -fsSL {}/install | bash",
        mgr.manager_url.trim_end_matches('/')
    );
    println!("    alien-deploy up \\");
    println!("      --token {} \\", token_response.token);
    println!("      --platform <aws|gcp|azure|kubernetes|local> \\");
    println!(
        "      --manager-url {}",
        mgr.manager_url.trim_end_matches('/')
    );
    println!();
    println!(
        "  \x1b[2mGroup: {} | Max deployments: {}\x1b[0m",
        name, args.max_deployments
    );

    Ok(())
}
