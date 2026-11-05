use crate::auth::{force_login, save_workspace};
use crate::commands::platform::workspace::{
    list_workspace_names, prompt_workspace, validate_workspace_name,
};
use crate::error::Result;
use crate::execution_context::ExecutionMode;
use crate::ui::{command, contextual_heading, dim_label, success_line};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Sign in to alien.dev, or connect to a manager you run",
    long_about = "Sign in to alien.dev and choose a default workspace, or connect the CLI to a manager you run with --manager. Later commands use whichever you logged in to.",
    after_help = "EXAMPLES:
    alien login
    alien login --workspace my-workspace
    alien login --manager https://manager.example.com --token ax_admin_..."
)]
pub struct LoginArgs {
    /// URL of a manager you run (instead of alien.dev)
    #[arg(long, value_name = "URL")]
    pub manager: Option<String>,

    /// API key for --manager (prompted for when omitted in a terminal)
    #[arg(
        long,
        requires = "manager",
        env = "ALIEN_API_KEY",
        hide_env_values = true
    )]
    pub token: Option<String>,
}

pub async fn login_task(_args: LoginArgs, ctx: ExecutionMode) -> Result<()> {
    let auth_opts = ctx.auth_opts();
    let http = force_login(&auth_opts).await?;

    if !matches!(
        ctx,
        ExecutionMode::Platform {
            workspace: Some(_),
            ..
        }
    ) && list_workspace_names(&http).await?.is_empty()
    {
        println!("{}", success_line("Logged in."));
        println!("{}", dim_label("Next"));
        println!(
            "  {}  Create a workspace (its name is permanent)",
            command("alien workspaces create <name>")
        );
        return Ok(());
    }

    let workspace = if let ExecutionMode::Platform {
        workspace: Some(ref workspace),
        ..
    } = ctx
    {
        validate_workspace_name(&http, workspace).await?
    } else {
        prompt_workspace(&http, false).await?
    };

    save_workspace(&workspace)?;

    println!("{}", contextual_heading("Logged in to", &workspace, &[]));
    println!("{}", success_line("Workspace ready."));
    println!("{}", dim_label("Next"));
    println!("  {}  Link a project directory", command("alien link"));
    println!(
        "  {}  Set up customer models",
        command("alien --project <name> onboard <customer> --setup-items models")
    );
    println!(
        "  {}  Publish an application",
        command("alien release --project <name>")
    );

    Ok(())
}
