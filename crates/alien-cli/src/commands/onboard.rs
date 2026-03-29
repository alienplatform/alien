use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json, prompt_text};
use crate::ui::{accent, command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_error::{AlienError, Context};
use alien_manager_api::types::CreateDeploymentGroupRequest;
use alien_manager_api::SdkResultExt;
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

    if !args.json {
        println!(
            "{}",
            contextual_heading("Creating deployment group", &name, &[])
        );
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

    if let Some(steps) = &steps {
        steps.complete(1, Some("Deployment token ready".to_string()));
    }
    println!("{}", success_line("Deployment token ready."));
    println!("{} {}", dim_label("Group"), deployment_group_id);
    println!("{} {}", dim_label("Deploy link"), accent(&deploy_link));
    println!(
        "{}",
        dim_label("Share this link with your team to open install and deploy instructions.")
    );
    println!("{}", dim_label("CLI"));
    println!(
        "  curl -fsSL {}/install | bash",
        mgr.manager_url.trim_end_matches('/')
    );
    println!("  alien-deploy up \\");
    println!("    --token {} \\", token_response.token);
    println!("    --platform <aws|gcp|azure|kubernetes|local> \\");
    println!(
        "    --manager-url {}",
        mgr.manager_url.trim_end_matches('/')
    );
    println!(
        "{} {} | {} {}",
        dim_label("Name"),
        name,
        dim_label("Max"),
        args.max_deployments
    );
    println!(
        "{} {}",
        dim_label("Next"),
        command("open the deploy link or run alien-deploy up")
    );

    Ok(())
}
