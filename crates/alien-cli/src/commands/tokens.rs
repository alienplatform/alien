//! `alien tokens` — scoped tokens on your manager.

use alien_error::Context;
use alien_manager_api::SdkResultExt;
use clap::{Parser, Subcommand};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{make_table, print_table, success_line};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "List and revoke tokens on your manager",
    after_help = "EXAMPLES:
    alien tokens ls
    alien tokens revoke <token-id>"
)]
pub struct TokensArgs {
    #[command(subcommand)]
    pub command: TokensCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TokensCommand {
    /// List tokens
    #[command(alias = "list")]
    Ls {
        /// Emit structured JSON output
        #[arg(long)]
        json: bool,
    },
    /// Revoke a token
    Revoke {
        /// Token ID (from `alien tokens ls`)
        id: String,
    },
}

impl TokensArgs {
    pub fn wants_json_output(&self) -> bool {
        match &self.command {
            TokensCommand::Ls { json } => *json,
            TokensCommand::Revoke { .. } => false,
        }
    }
}

pub async fn tokens_task(args: TokensArgs, ctx: ExecutionMode) -> Result<()> {
    let (project_id, _) = ctx.resolve_project(None, false).await?;
    let mgr = ctx.resolve_manager(&project_id, "local").await?;
    let client = &mgr.client;

    match args.command {
        TokensCommand::Ls { json } => {
            let tokens = client
                .list_tokens()
                .send()
                .await
                .into_sdk_error()
                .await
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to list tokens".to_string(),
                    url: None,
                })?
                .into_inner();
            if json {
                return print_json(&tokens);
            }
            let mut table = make_table(&["ID", "Type", "Prefix", "Customer", "Created"]);
            for token in tokens.items {
                table.add_row(vec![
                    token.id,
                    token.token_type,
                    token.key_prefix,
                    token.deployment_group_id.unwrap_or_else(|| "—".to_string()),
                    token.created_at,
                ]);
            }
            print_table(table);
        }
        TokensCommand::Revoke { id } => {
            client
                .delete_token()
                .id(&id)
                .send()
                .await
                .into_sdk_error()
                .await
                .context(ErrorData::ApiRequestFailed {
                    message: format!("Failed to revoke token '{id}'"),
                    url: None,
                })?;
            println!("{}", success_line(&format!("Token {id} revoked.")));
        }
    }
    Ok(())
}
