//! Prepare a new installation before creating its deployment record.

use super::{
    api_url, create_platform_http_client, deployment_stack_settings_json, parse_api_response,
    DeployArgs, ResolvedDeployArgs,
};
use crate::error::{ErrorData, Result};
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};
use std::collections::HashSet;

pub(super) fn is_prepared_before_creation(platform: Platform) -> bool {
    matches!(
        platform,
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Machines
    )
}

fn prepare_stack_body(
    resolved: &ResolvedDeployArgs,
    args: &DeployArgs,
) -> Result<serde_json::Value> {
    let mut body = serde_json::json!({
        "platform": resolved.platform,
        "setupMethod": "cli",
        "setupCapabilities": super::SETUP_CAPABILITIES,
        "stackSettings": deployment_stack_settings_json(resolved, args)?,
        // Validate the Release this deployment will install: the one on its channel.
        "releaseChannel": args.channel,
    });
    if let Some(setup_item) = args.setup_item {
        body["setupItem"] = serde_json::Value::String(setup_item.api_name().to_string());
    }
    Ok(body)
}

/// The ids of the release's boolean inputs. These are the inputs a `when` or
/// a frozen-resource gate can read, and a boolean is never a secret. The
/// manager can start the first apply of some deployments as soon as they are
/// created, so these answers must be in the create request. When the release
/// cannot be read, no id is returned and every answer is held back, as for any
/// other input.
pub(super) async fn boolean_input_ids(
    base_url: &str,
    token: &str,
    resolved: &ResolvedDeployArgs,
    args: &DeployArgs,
) -> HashSet<String> {
    if !is_prepared_before_creation(resolved.platform_enum) {
        return HashSet::new();
    }
    let ids = async {
        let body = prepare_stack_body(resolved, args)?;
        let response = create_platform_http_client(token)?
            .post(api_url(
                base_url,
                "/v1/deployment-info/prepare-stack",
                None,
            )?)
            .json(&body)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to read the release inputs".to_string(),
            })?;
        let prepared: alien_platform_api::types::PreparedDeploymentStack =
            parse_api_response(response, "Invalid prepared installation").await?;
        Ok::<_, AlienError<ErrorData>>(boolean_ids(&prepared))
    }
    .await;
    ids.unwrap_or_else(|error| {
        tracing::warn!(%error, "Could not read the release inputs; every answer is sent after the deployment is created");
        HashSet::new()
    })
}

fn boolean_ids(prepared: &alien_platform_api::types::PreparedDeploymentStack) -> HashSet<String> {
    let Ok(prepared) = serde_json::to_value(prepared) else {
        return HashSet::new();
    };
    prepared["stack"]["inputs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|input| input["kind"] == "boolean")
        .filter_map(|input| input["id"].as_str().map(str::to_string))
        .collect()
}

/// Validates the release a new deployment will install and returns its
/// boolean input ids (see [`boolean_input_ids`]).
pub(super) async fn validate_before_creation(
    base_url: &str,
    token: &str,
    resolved: &ResolvedDeployArgs,
    args: &DeployArgs,
) -> Result<HashSet<String>> {
    if !is_prepared_before_creation(resolved.platform_enum) {
        return Ok(HashSet::new());
    }
    let body = prepare_stack_body(resolved, args)?;
    let client = create_platform_http_client(token)?;
    let response = client
        .post(api_url(
            base_url,
            "/v1/deployment-info/prepare-stack",
            None,
        )?)
        .json(&body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to validate installation before deployment creation".to_string(),
        })?;
    if response.status().is_success() {
        // Parse the normal preparation response; malformed responses fail closed.
        let prepared = parse_api_response::<alien_platform_api::types::PreparedDeploymentStack>(
            response,
            "Invalid prepared installation",
        )
        .await?;
        return Ok(boolean_ids(&prepared));
    }

    // Preparation is authoritative: inline compute and adopted resources can
    // succeed without explicit pool selections. Only consult the planner after
    // preparation fails, to explain missing or invalid choices without echoing
    // an untrusted preparation response that might contain binding secrets.
    if resolved.platform_enum != Platform::Machines {
        let plan = client
            .post(api_url(base_url, "/v1/deployment-info/compute-plan", None)?)
            .json(&body)
            .send()
            .await;
        if let Ok(plan) = plan {
            if plan.status().is_success() {
                let plan: alien_platform_api::types::DeploymentComputePlan =
                    parse_api_response(plan, "Invalid compute plan").await?;
                let missing: Vec<_> = plan
                    .pools
                    .iter()
                    .filter(|pool| {
                        !resolved
                            .compute_settings
                            .as_ref()
                            .is_some_and(|settings| settings.pools.contains_key(&pool.pool_id))
                    })
                    .map(|pool| pool.pool_id.as_str())
                    .collect();
                if !missing.is_empty() {
                    return Err(AlienError::new(ErrorData::ValidationError {
                        field: "compute".to_string(),
                        message: format!(
                            "Select compute for {} pools {} before creating this deployment. Add each pool under [compute.pools.<pool>] in your --config file with its mode, machine and machine counts.",
                            resolved.platform, missing.join(", "),
                        ),
                    }));
                }
                let errors: Vec<_> = plan
                    .pools
                    .iter()
                    .flat_map(|pool| pool.errors.iter())
                    .cloned()
                    .collect();
                if !errors.is_empty() {
                    return Err(AlienError::new(ErrorData::ValidationError {
                        field: "compute".to_string(),
                        message: errors.join("; "),
                    }));
                }
            }
        }
    }
    parse_api_response::<serde_json::Value>(
        response,
        "Installation validation failed before deployment creation",
    )
    .await?;
    Ok(HashSet::new())
}
