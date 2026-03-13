use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use alien_platform_api::SdkResultExt;
use alien_error::Context;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show current authenticated user information",
    long_about = "Display information about the current authenticated principal."
)]
pub struct WhoamiArgs {}

pub async fn whoami_task(_args: WhoamiArgs, ctx: ExecutionMode) -> Result<()> {
    // Ensure target is ready
    ctx.ensure_ready().await?;

    // Get SDK client
    let client = ctx.sdk_client().await?;

    let response =
        client
            .whoami()
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to get user information".to_string(),
                url: None,
            })?;
    let whoami_response = response.into_inner();

    println!("{:?}", whoami_response);

    Ok(())
}
