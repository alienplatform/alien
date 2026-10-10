//! `alien tokens` — scoped tokens on your manager.

use alien_error::{AlienError, Context};
use alien_manager_api::types::{CreatableTokenType, CreateTokenRequest};
use alien_manager_api::SdkResultExt;
use clap::{Parser, Subcommand};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{accent, command, dim_label, make_table, print_table, success_line};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Create and revoke scoped tokens on your manager",
    long_about = "Create and revoke scoped tokens on your manager.\n\nA tunnel token lets a backend send requests into deployments through the manager and nothing else: it can't read deployments, create releases or change anything.",
    after_help = "EXAMPLES:
    alien tokens create --tunnel
    alien tokens create --tunnel --customer customer-1
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
    /// Create a token
    Create {
        /// A token that only calls deployment tunnels
        #[arg(long, required = true)]
        tunnel: bool,
        /// Limit the token to one customer's deployments (deployment group name or ID)
        #[arg(long)]
        customer: Option<String>,
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
        TokensCommand::Create {
            tunnel: _,
            customer,
            json,
        } => {
            let deployment_group_id = match customer {
                Some(customer) => Some(resolve_group(client, &customer).await?),
                None => None,
            };
            let created = client
                .create_token()
                .body(CreateTokenRequest {
                    type_: CreatableTokenType::Tunnel,
                    deployment_group_id,
                })
                .send()
                .await
                .into_sdk_error()
                .await
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to create token".to_string(),
                    url: None,
                })?
                .into_inner();
            if json {
                return print_json(&created);
            }
            println!("{}", success_line("Tunnel token created."));
            println!("{} {}", dim_label("Token"), accent(&created.token));
            println!(
                "{} {}",
                dim_label("Use"),
                command(&format!(
                    "curl -H 'Proxy-Authorization: Bearer {}' {}/v1/deployments/<deployment>/tunnels/<container>/",
                    created.token,
                    mgr.manager_url.trim_end_matches('/')
                ))
            );
            println!("{}", dim_label("Store it now; it isn't shown again."));
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

/// Accept a deployment group ID or name.
async fn resolve_group(client: &alien_manager_api::Client, customer: &str) -> Result<String> {
    let groups = client
        .list_deployment_groups()
        .send()
        .await
        .into_sdk_error()
        .await
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list customers".to_string(),
            url: None,
        })?
        .into_inner();
    groups
        .items
        .iter()
        .find(|group| group.id == customer || group.name == customer)
        .map(|group| group.id.clone())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "customer".to_string(),
                message: format!(
                    "No customer named '{customer}'. Run `alien onboard {customer}` first."
                ),
            })
        })
}
