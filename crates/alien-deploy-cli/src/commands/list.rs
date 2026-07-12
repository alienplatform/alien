//! List command — shows all tracked deployments.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(about = "List tracked deployments")]
pub struct ListArgs {
    /// Authentication token for remote manager listing.
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Read authentication token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Manager URL. If omitted, uses the platform API to discover the manager.
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Platform API base URL used for manager discovery.
    #[arg(long, env = "ALIEN_BASE_URL")]
    pub base_url: Option<String>,

    /// Platform used only when discovering the manager URL.
    #[arg(long)]
    pub platform: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDeploymentList {
    items: Vec<RemoteDeployment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDeployment {
    id: String,
    name: String,
    platform: String,
    status: String,
}

pub async fn list_command(args: ListArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    let remote_requested = args.token.is_some()
        || args.token_file.is_some()
        || args.manager_url.is_some()
        || args.base_url.is_some()
        || embedded_config
            .and_then(|config| config.token.as_ref())
            .is_some()
        || embedded_config
            .and_then(|config| config.token_env_var.as_ref())
            .and_then(|env_var| std::env::var(env_var).ok())
            .is_some();

    if remote_requested {
        return list_remote_deployments(args, embedded_config).await;
    }

    let tracker = DeploymentTracker::new()?;
    let deployments = tracker.list();

    if deployments.is_empty() {
        output::info("No tracked deployments. Use 'alien-deploy deploy' to create one.");
        return Ok(());
    }

    output::header("Tracked Deployments");

    for dep in deployments {
        eprintln!(
            "  \x1b[1m{}\x1b[0m  ({})  {}  {}",
            dep.name, dep.platform, dep.deployment_id, dep.manager_url
        );
    }

    eprintln!();
    Ok(())
}

async fn list_remote_deployments(
    args: ListArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<()> {
    let token =
        super::up::resolve_optional_token(args.token, args.token_file.as_ref(), embedded_config)?
            .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: "--token or --token-file is required for remote deployment listing."
                    .to_string(),
            })
        })?;
    let base_url = super::up::resolve_base_url_option(args.base_url.as_ref(), embedded_config);
    let platform = if args.manager_url.is_none() {
        Some(super::up::resolve_platform_option(
            args.platform.as_ref(),
            embedded_config,
            "remote deployment listing",
        )?)
    } else {
        args.platform.clone()
    };
    let manager_url = super::up::resolve_manager_url_option(
        args.manager_url,
        &base_url,
        &token,
        platform.as_deref().unwrap_or(""),
    )
    .await?;
    let client = super::up::create_manager_http_client(&token)?;
    let url = format!("{}/v1/deployments", manager_url.trim_end_matches('/'));
    let response = client.get(&url).send().await.into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to list deployments from manager".to_string(),
        },
    )?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to list deployments from manager (HTTP {status}): {body}"),
        }));
    }

    let deployments: RemoteDeploymentList =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse manager deployment list".to_string(),
            })?;

    if deployments.items.is_empty() {
        output::info("No deployments are visible to this token.");
        return Ok(());
    }

    output::header("Deployments");
    for deployment in deployments.items {
        eprintln!(
            "  \x1b[1m{}\x1b[0m  ({})  {}  {}  {}",
            deployment.name, deployment.platform, deployment.id, deployment.status, manager_url
        );
    }

    eprintln!();
    Ok(())
}
