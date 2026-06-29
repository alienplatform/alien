//! Status command — shows current deployment status.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use serde_json::Value as JsonValue;
use std::path::PathBuf;

use super::up::{create_manager_client, read_token_file};

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
}

pub async fn status_command(args: StatusArgs) -> Result<()> {
    let tracker = DeploymentTracker::new()?;

    let tracked = tracker.get(&args.name).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "name".to_string(),
            message: format!(
                "Deployment '{}' is not tracked. Use 'alien-deploy deploy' first.",
                args.name
            ),
        })
    })?;

    let token = args
        .token
        .map(Ok)
        .or_else(|| args.token_file.as_ref().map(|path| read_token_file(path)))
        .transpose()?
        .unwrap_or_else(|| tracked.token.clone());
    let manager_url = args
        .manager_url
        .unwrap_or_else(|| tracked.manager_url.clone());

    let client = create_manager_client(&token, &manager_url)?;

    let deployment = client
        .get_deployment()
        .id(&tracked.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    output::header(&format!("Deployment: {}", args.name));
    output::status("ID:", &tracked.deployment_id);
    output::status("Platform:", &tracked.platform);
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

    Ok(())
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
