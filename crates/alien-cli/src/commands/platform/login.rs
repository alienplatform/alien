use crate::auth::{force_login, save_workspace};
use crate::commands::platform::workspace::{prompt_workspace, validate_workspace_name};
use crate::error::Result;
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{command, contextual_heading, dim_label, success_line};
use clap::Parser;
use serde::Serialize;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Authenticate with Alien and choose a default workspace",
    long_about = "Authenticate with the Alien platform and set the default workspace used by platform-managed commands.",
    after_help = "EXAMPLES:
    alien login
    alien login --workspace my-workspace
    alien login --workspace my-workspace --json"
)]
pub struct LoginArgs {
    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginOutput {
    workspace: String,
    used_api_key: bool,
}

pub async fn login_task(args: LoginArgs, ctx: ExecutionMode) -> Result<()> {
    let auth_opts = ctx.auth_opts();
    let used_api_key = auth_opts.api_key.is_some();
    let http = force_login(&auth_opts).await?;

    let workspace = if let ExecutionMode::Platform {
        workspace: Some(ref workspace),
        ..
    } = ctx
    {
        validate_workspace_name(&http, workspace).await?
    } else {
        prompt_workspace(&http, args.json).await?
    };

    save_workspace(&workspace)?;

    if args.json {
        print_json(&LoginOutput {
            workspace,
            used_api_key,
        })?;
    } else {
        println!("{}", contextual_heading("Logged in to", &workspace, &[]));
        println!("{}", success_line("Workspace ready."));
        println!(
            "{} run {} in a project directory or {}.",
            dim_label("Next"),
            command("alien link"),
            command("alien release --project <name>")
        );
    }

    Ok(())
}
