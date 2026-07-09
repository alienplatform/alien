//! Status command — shows current deployment status.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_core::embedded_config::DeployCliConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::path::PathBuf;

use super::up::{
    create_manager_client, create_manager_http_client, resolve_base_url_option,
    resolve_manager_url_option, resolve_optional_token, resolve_platform_option,
};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show deployment status",
    after_help = "EXAMPLES:
    # Show status of a tracked deployment
    alien-deploy status --name production"
)]
pub struct StatusArgs {
    /// Deployment name
    #[arg(long)]
    pub name: String,

    /// Authentication token (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Read authentication token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Manager URL (optional if deployment is tracked)
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Platform API base URL used for manager discovery when deployment is not tracked.
    #[arg(long, env = "ALIEN_BASE_URL")]
    pub base_url: Option<String>,

    /// Platform used only when discovering the manager URL for an untracked deployment.
    #[arg(long)]
    pub platform: Option<String>,

    /// Output status as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDeploymentList {
    items: Vec<RemoteDeploymentListItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDeploymentListItem {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDeploymentInfo {
    resources: std::collections::HashMap<String, RemoteResourceInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteResourceInfo {
    resource_type: String,
    public_url: Option<String>,
}

pub async fn status_command(
    args: StatusArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<()> {
    let tracker = DeploymentTracker::new()?;

    let tracked = tracker.get(&args.name);
    let token = resolve_optional_token(args.token.clone(), args.token_file.as_ref(), embedded_config)?
        .or_else(|| tracked.map(|deployment| deployment.token.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: format!(
                    "Deployment '{}' is not tracked. Pass --token or --token-file to look it up remotely.",
                    args.name
                ),
            })
        })?;
    let base_url = resolve_base_url_option(args.base_url.as_ref(), embedded_config);
    let manager_url = match (args.manager_url, tracked) {
        (Some(manager_url), _) => manager_url,
        (None, Some(tracked)) => tracked.manager_url.clone(),
        (None, None) => {
            let platform = resolve_platform_option(
                args.platform.as_ref(),
                embedded_config,
                "remote deployment status",
            )?;
            resolve_manager_url_option(None, &base_url, &token, &platform).await?
        }
    };
    let deployment_id = match tracked {
        Some(tracked) => tracked.deployment_id.clone(),
        None => resolve_remote_deployment_id(&token, &manager_url, &args.name).await?,
    };

    let client = create_manager_client(&token, &manager_url)?;

    let deployment = client
        .get_deployment()
        .id(&deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let deployment_info =
        fetch_remote_deployment_info(&token, &manager_url, &deployment_id).await?;

    if args.json {
        let resources = deployment_info
            .resources
            .iter()
            .map(|(id, resource)| {
                (
                    id.clone(),
                    serde_json::json!({
                        "type": resource.resource_type,
                        "publicUrl": resource.public_url,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let output = serde_json::json!({
            "name": args.name,
            "id": deployment_id,
            "platform": deployment.platform,
            "managerUrl": manager_url,
            "status": deployment.status,
            "currentReleaseId": deployment.current_release_id,
            "desiredReleaseId": deployment.desired_release_id,
            "error": deployment.error,
            "createdAt": deployment.created_at,
            "resources": resources,
        });
        let json = serde_json::to_string_pretty(&output)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize status JSON".to_string(),
            })?;
        println!("{json}");
        return Ok(());
    }

    output::header(&format!("Deployment: {}", args.name));
    output::status("ID:", &deployment_id);
    output::status("Platform:", &deployment.platform.to_string());
    output::status("Manager:", &manager_url);
    output::status("Status:", &deployment.status);

    if let Some(ref release_id) = deployment.current_release_id {
        output::status("Current Release:", release_id);
    }
    if let Some(ref release_id) = deployment.desired_release_id {
        output::status("Desired Release:", release_id);
    }
    if let Some(ref error) = deployment.error {
        let lines = format_error_chain(error);
        if let Some((first, rest)) = lines.split_first() {
            output::status("Error:", first);
            for line in rest {
                output::status("Caused by:", line);
            }
        }
    }
    output::status("Created:", &deployment.created_at);
    for (resource_id, resource) in deployment_info
        .resources
        .iter()
        .filter(|(_, resource)| resource.public_url.is_some())
    {
        if let Some(public_url) = resource.public_url.as_ref() {
            output::status(&format!("{resource_id} URL:"), public_url);
        }
    }

    Ok(())
}

async fn fetch_remote_deployment_info(
    token: &str,
    manager_url: &str,
    deployment_id: &str,
) -> Result<RemoteDeploymentInfo> {
    let client = create_manager_http_client(token)?;
    let url = format!(
        "{}/v1/deployments/{}/info",
        manager_url.trim_end_matches('/'),
        urlencoding::encode(deployment_id),
    );
    let response = client.get(&url).send().await.into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to fetch deployment info from manager".to_string(),
        },
    )?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to fetch deployment info (HTTP {status}): {body}"),
        }));
    }

    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to parse deployment info response".to_string(),
        })
}

async fn resolve_remote_deployment_id(
    token: &str,
    manager_url: &str,
    name: &str,
) -> Result<String> {
    let client = create_manager_http_client(token)?;
    let url = format!(
        "{}/v1/deployments?name={}",
        manager_url.trim_end_matches('/'),
        urlencoding::encode(name),
    );
    let response = client.get(&url).send().await.into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to list deployments from manager".to_string(),
        },
    )?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to look up deployment by name (HTTP {status}): {body}"),
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

    match deployments.items.as_slice() {
        [deployment] => Ok(deployment.id.clone()),
        [] => Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("Deployment '{name}' was not found for this token."),
        })),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!("Deployment name '{name}' matched multiple deployments."),
        })),
    }
}

fn format_error_chain(error: &JsonValue) -> Vec<String> {
    let mut lines = Vec::new();
    collect_error_chain(error, &mut lines);
    if lines.is_empty() {
        lines.push(format_json_fallback(error));
    }
    lines
}

fn collect_error_chain(error: &JsonValue, lines: &mut Vec<String>) {
    let JsonValue::Object(object) = error else {
        lines.push(format_json_fallback(error));
        return;
    };

    if let Some(summary) = format_error_object_summary(object) {
        lines.push(summary);
    }

    for key in ["source", "cause"] {
        if let Some(source) = object.get(key) {
            collect_error_chain(source, lines);
        }
    }
}

fn format_error_object_summary(object: &serde_json::Map<String, JsonValue>) -> Option<String> {
    let message = json_string_field(object, "message")
        .or_else(|| json_string_field(object, "error"))
        .or_else(|| json_string_field(object, "detail"))?;

    match json_string_field(object, "code") {
        Some(code) => Some(format!("{code}: {message}")),
        None => Some(message.to_string()),
    }
}

fn json_string_field<'a>(
    object: &'a serde_json::Map<String, JsonValue>,
    field: &str,
) -> Option<&'a str> {
    object.get(field).and_then(JsonValue::as_str)
}

fn format_json_fallback(value: &JsonValue) -> String {
    match serde_json::to_string_pretty(value) {
        Ok(rendered) => rendered,
        Err(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_error_code_and_message() {
        let error = serde_json::json!({
            "code": "PREFLIGHT_CHECKS_FAILED",
            "message": "Preflight checks failed"
        });

        assert_eq!(
            format_error_chain(&error),
            vec!["PREFLIGHT_CHECKS_FAILED: Preflight checks failed"]
        );
    }

    #[test]
    fn formats_nested_error_sources() {
        let error = serde_json::json!({
            "code": "DEPLOYMENT_FAILED",
            "message": "Deployment failed",
            "source": {
                "code": "PREFLIGHT_CHECKS_FAILED",
                "message": "Compute pool general requires at least 4 vCPU"
            }
        });

        assert_eq!(
            format_error_chain(&error),
            vec![
                "DEPLOYMENT_FAILED: Deployment failed",
                "PREFLIGHT_CHECKS_FAILED: Compute pool general requires at least 4 vCPU"
            ]
        );
    }

    #[test]
    fn falls_back_to_json_for_unknown_error_shape() {
        let error = serde_json::json!({ "unexpected": true });

        assert_eq!(
            format_error_chain(&error),
            vec![format_json_fallback(&error)]
        );
    }
}
