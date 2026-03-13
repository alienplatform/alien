use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_platform_api::Client as SdkClient;
use alien_platform_api::SdkResultExt;
use alien_error::{AlienError, Context, IntoAlienError};
use clap::Parser;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT},
    Client,
};
use tracing::info;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Destroy resources from a deployment",
    long_about = "Destroy resources from a deployment using the Alien Platform API.",
    after_help = "EXAMPLES:
    # Destroy resources from a deployment using the deployment API key
    alien destroy --token ax_deployment_1234abcd... --name production

    # Destroy using tracked deployment
    alien destroy --name production"
)]
pub struct DestroyArgs {
    /// Deployment API key for authentication
    #[arg(long)]
    pub token: String,

    /// Deployment name for identification in tracking
    #[arg(long)]
    pub name: String,
}

/// Create authenticated platform client
fn create_authenticated_client(api_key: &str, base_url: &str) -> Result<SdkClient> {
    let auth_value = format!("Bearer {}", api_key);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth_value)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let reqwest_client = Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(SdkClient::new_with_client(base_url, reqwest_client))
}

/// Core destroy logic
pub async fn destroy_task(args: DestroyArgs, ctx: ExecutionMode) -> Result<()> {
    info!("Starting destroy command");
    info!("🗑️  Destroying resources from deployment '{}'", args.name);

    let base_url = ctx.base_url();

    // Validate deployment (should already be tracked)
    let tracker = DeploymentTracker::new()?;
    let tracked_deployment = tracker
        .get_deployment(&args.name)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: format!(
                    "Deployment '{}' is not tracked. Please add it first using 'alien deploy'",
                    args.name
                ),
            })
        })?
        .clone();

    // Verify the provided token matches the stored token
    if tracked_deployment.api_key != args.token {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "token".to_string(),
            message: format!(
                "Provided API key does not match stored key for deployment '{}'",
                args.name
            ),
        }));
    }

    info!("✅ Found tracked deployment '{}'", args.name);
    info!("   Deployment ID: {}", tracked_deployment.deployment_id);
    info!("   Workspace ID: {}", tracked_deployment.workspace_id);

    // Create authenticated client
    let sdk_client = create_authenticated_client(&args.token, &base_url)?;

    // Get current deployment status and verify it can be destroyed
    let deployment_response = sdk_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment details from platform".to_string(),
        })?;

    let deployment = deployment_response.into_inner();

    info!("📊 Current deployment status: {}", deployment.status);

    // Verify deployment can be destroyed
    use alien_platform_api::types::DeploymentDetailResponseStatus;

    match deployment.status {
        DeploymentDetailResponseStatus::Deleting | DeploymentDetailResponseStatus::Deleted => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "status".to_string(),
                message: format!("Cannot destroy deployment in status '{}'. Deployment must be in a destroyable state", deployment.status),
            }));
        }
        _ => {
            // All other statuses are destroyable
        }
    }

    // Call the platform API delete endpoint
    info!("🚀 Starting deployment destruction...");

    sdk_client
        .delete_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to start deployment destruction".to_string(),
        })?;

    info!("✅ Deployment deletion initiated successfully!");
    info!("   The platform controller will handle the destruction process.");
    info!("   You can monitor the progress via the dashboard.");

    Ok(())
}
