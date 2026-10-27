use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use alien_error::{Context, IntoAlienError};
use alien_platform_api::{types::CreateDomainBody, SdkResultExt as _};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::Value;

#[derive(Parser, Debug, Clone)]
#[command(about = "Manage workspace domains and certificate verification")]
pub struct DomainsArgs {
    /// Print machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: DomainsCommand,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DnsMode {
    Alien,
    Customer,
}

#[derive(Subcommand, Debug, Clone)]
pub enum DomainsCommand {
    /// Register a domain and show its required DNS records
    Add {
        domain: String,
        /// Who manages the DNS zone
        #[arg(long, value_enum, default_value = "alien")]
        dns: DnsMode,
    },
    /// List workspace domains
    #[command(visible_alias = "list")]
    Ls,
    /// Show a domain's verification state
    Get { id: String },
    /// Check the domain's DNS records again
    Refresh { id: String },
    /// Remove a workspace domain
    #[command(visible_alias = "remove")]
    Rm { id: String },
}

fn response_json(value: impl Serialize) -> Result<Value> {
    serde_json::to_value(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialization".to_string(),
            reason: "Could not serialize domain response".to_string(),
        })
}

pub async fn domains_task(args: DomainsArgs, ctx: ExecutionMode) -> Result<()> {
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let client = ctx.sdk_client().await?;
    let context = || ErrorData::ApiRequestFailed {
        message: "Managing workspace domains".to_string(),
        url: None,
    };
    let response = match args.command {
        DomainsCommand::Add { domain, dns } => {
            let body: CreateDomainBody = serde_json::from_value(serde_json::json!({
                "domain": domain,
                "dns": match dns { DnsMode::Alien => "alien", DnsMode::Customer => "customer" }
            }))
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "domain".to_string(),
                message: "Provide a DNS hostname without a scheme or wildcard".to_string(),
            })?;
            response_json(
                client
                    .create_domain()
                    .workspace(workspace.as_str())
                    .body(body)
                    .send()
                    .await
                    .into_sdk_error()
                    .context(context())?
                    .into_inner(),
            )?
        }
        DomainsCommand::Ls => response_json(
            client
                .list_domains()
                .workspace(workspace.as_str())
                .send()
                .await
                .into_sdk_error()
                .context(context())?
                .into_inner(),
        )?,
        DomainsCommand::Get { id } => response_json(
            client
                .get_domain()
                .workspace(workspace.as_str())
                .id(id)
                .send()
                .await
                .into_sdk_error()
                .context(context())?
                .into_inner(),
        )?,
        DomainsCommand::Refresh { id } => response_json(
            client
                .refresh_domain()
                .workspace(workspace.as_str())
                .id(id)
                .send()
                .await
                .into_sdk_error()
                .context(context())?
                .into_inner(),
        )?,
        DomainsCommand::Rm { id } => response_json(
            client
                .delete_domain()
                .workspace(workspace.as_str())
                .id(id)
                .send()
                .await
                .into_sdk_error()
                .context(context())?
                .into_inner(),
        )?,
    };
    if args.json {
        return print_json(&response);
    }
    if let Some(domains) = response.get("domains").and_then(Value::as_array) {
        for domain in domains {
            print_domain(domain);
        }
    } else if response.get("success").and_then(Value::as_bool) == Some(true) {
        println!("Domain removal requested.");
    } else {
        print_domain(&response);
    }
    Ok(())
}

fn print_domain(domain: &Value) {
    let text = |key: &str| domain.get(key).and_then(Value::as_str).unwrap_or("-");
    println!("{}  {}  {}", text("domain"), text("status"), text("id"));
    if let Some(target) = domain.get("acmeChallengeTarget").and_then(Value::as_str) {
        println!("  _acme-challenge.{} CNAME {target}", text("domain"));
    }
    if let Some(servers) = domain.get("nameServers").and_then(Value::as_array) {
        for server in servers.iter().filter_map(Value::as_str) {
            println!("  {} NS {server}", text("domain"));
        }
    }
    if let Some(error) = domain
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        println!("  {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_dns_arguments_preserve_mode_and_json_output() {
        let args = DomainsArgs::try_parse_from([
            "domains",
            "add",
            "app.example.com",
            "--dns",
            "customer",
            "--json",
        ])
        .unwrap();
        assert!(args.json);
        assert!(matches!(
            args.command,
            DomainsCommand::Add {
                dns: DnsMode::Customer,
                ..
            }
        ));
        assert!(DomainsArgs::try_parse_from([
            "domains",
            "add",
            "app.example.com",
            "--dns",
            "invalid"
        ])
        .is_err());
    }
}
