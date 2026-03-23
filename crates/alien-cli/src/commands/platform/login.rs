use crate::auth::{force_login, save_workspace};
use crate::commands::platform::workspace::prompt_workspace_with_tui;
use crate::error::Result;
use crate::execution_context::ExecutionMode;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Perform login & select default workspace",
    long_about = "Authenticate with the Alien platform and set a default workspace."
)]
pub struct LoginArgs {}

pub async fn login_task(_args: LoginArgs, ctx: ExecutionMode) -> Result<()> {
    let auth_opts = ctx.auth_opts();
    let has_api_key = auth_opts.api_key.is_some();

    // Use force_login which handles logout and OAuth flow with enhanced UI
    let http = force_login(&auth_opts).await?;

    // Login uses --workspace directly (no profile.json fallback since we're logging in fresh)
    let ws = if let ExecutionMode::Platform {
        workspace: Some(ref ws),
        ..
    } = ctx
    {
        ws.clone()
    } else {
        prompt_workspace_with_tui(&http).await?
    };

    save_workspace(&ws)?;

    if has_api_key {
        println!("\n");
        println!("Logged in. Default workspace set to: {ws}");
    } else {
        println!("\n");
        println!("Congratulations! You are now logged in. In order to deploy something, run `alien build` then `alien apply`.");
    }

    Ok(())
}
