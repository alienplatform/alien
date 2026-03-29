use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use alien_error::Context;
use alien_platform_api::SdkResultExt;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show the current authenticated principal",
    long_about = "Display information about the current authenticated principal for the selected target.",
    after_help = "EXAMPLES:
    alien whoami
    alien whoami --json
    alien dev whoami"
)]
pub struct WhoamiArgs {
    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

pub async fn whoami_task(args: WhoamiArgs, ctx: ExecutionMode) -> Result<()> {
    ctx.ensure_ready().await?;

    let response = ctx
        .sdk_client()
        .await?
        .whoami()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to get user information".to_string(),
            url: None,
        })?
        .into_inner();

    if args.json {
        print_json(&response)?;
    } else {
        println!("{:#?}", response);
    }

    Ok(())
}
