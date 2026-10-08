//! Prepare a new installation before creating its deployment record.

use super::{
    api_url, create_platform_http_client, deployment_stack_settings_json, parse_api_response,
    DeployArgs, ResolvedDeployArgs,
};
use crate::error::{ErrorData, Result};
use alien_core::Platform;
use alien_error::{AlienError, Context, IntoAlienError};

pub(super) async fn validate_before_creation(
    base_url: &str,
    token: &str,
    resolved: &ResolvedDeployArgs,
    args: &DeployArgs,
) -> Result<()> {
    if !matches!(
        resolved.platform_enum,
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Machines
    ) {
        return Ok(());
    }
    let client = create_platform_http_client(token)?;
    let body = serde_json::json!({
        "platform": resolved.platform,
        "setupMethod": "cli",
        "stackSettings": deployment_stack_settings_json(resolved, args)?,
    });
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
        parse_api_response::<alien_platform_api::types::PreparedDeploymentStack>(
            response,
            "Invalid prepared installation",
        )
        .await?;
        return Ok(());
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
    Ok(())
}
