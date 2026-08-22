use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{command, dim_label, make_table, print_table};
use alien_error::{AlienError, Context};
use alien_platform_api::{types, SdkResultExt as _};
use clap::{Parser, Subcommand};
use std::num::NonZeroU64;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage deployment groups",
    long_about = "List and inspect deployment groups in a project. In customer-facing projects, each deployment group commonly represents one customer environment.",
    after_help = "EXAMPLES:
    alien deployment-groups list
    alien deployment-groups get customer_123
    alien deployment-groups describe acme --json
    alien onboard acme --external-id customer_123 --setup-items models,keys"
)]
pub struct CustomersArgs {
    /// Project to manage (defaults to the linked project)
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Emit stable machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: CustomersCommand,
}

impl CustomersArgs {
    pub fn wants_json_output(&self) -> bool {
        self.json
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum CustomersCommand {
    /// List deployment groups in the project
    #[command(visible_alias = "ls")]
    List {
        /// Search deployment-group name or external ID
        #[arg(long)]
        search: Option<String>,
    },
    /// Show deployment-group details
    #[command(visible_aliases = ["describe", "show"])]
    Get {
        /// Deployment-group ID, name, or external ID
        customer: String,
    },
}

pub async fn customers_task(args: CustomersArgs, ctx: ExecutionMode) -> Result<()> {
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    let (project_id, _) = ctx
        .resolve_project(args.project.as_deref(), !args.json)
        .await?;
    let client = ctx.sdk_client().await?;

    match args.command {
        CustomersCommand::List { search } => {
            let customers =
                list_customers(&client, &workspace, &project_id, search.as_deref()).await?;
            if args.json {
                print_json(&customers)?;
            } else if customers.is_empty() {
                println!("{}", dim_label("No deployment groups found."));
                println!(
                    "{} {}",
                    dim_label("Next"),
                    command("alien onboard <customer-name>")
                );
            } else {
                let mut table = make_table(&["Deployment group", "External ID", "Limit", "ID"]);
                for customer in customers {
                    table.add_row(vec![
                        customer.name.as_str().to_string(),
                        customer
                            .external_id
                            .as_ref()
                            .map(|value| value.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        customer.max_deployments.get().to_string(),
                        customer.id.as_str().to_string(),
                    ]);
                }
                print_table(table);
            }
        }
        CustomersCommand::Get { customer } => {
            let customer = resolve_customer(&client, &workspace, &project_id, &customer).await?;
            let response = client
                .get_deployment_group()
                .id(customer.id.as_str())
                .workspace(workspace.as_str())
                .include(vec![types::GetDeploymentGroupIncludeItem::Project])
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("Failed to get customer {}", customer.name.as_str()),
                    url: None,
                })?
                .into_inner();
            if args.json {
                print_json(&response)?;
            } else {
                println!(
                    "{} {}",
                    dim_label("Deployment group"),
                    response.name.as_str()
                );
                println!("{} {}", dim_label("ID"), response.id.as_str());
                println!(
                    "{} {}",
                    dim_label("External ID"),
                    response
                        .external_id
                        .as_ref()
                        .map(|value| value.as_str())
                        .unwrap_or("-")
                );
                println!("{} {}", dim_label("Project"), response.project_id.as_str());
                println!(
                    "{} {}",
                    dim_label("Deployment limit"),
                    response.max_deployments
                );
                println!();
                println!(
                    "{} {}",
                    dim_label("Logs"),
                    command(&format!("alien logs --project {project_id}"))
                );
            }
        }
    }

    Ok(())
}

async fn list_customers(
    client: &alien_platform_api::Client,
    workspace: &str,
    project: &str,
    search: Option<&str>,
) -> Result<Vec<types::ListDeploymentGroupsResponseItemsItem>> {
    let mut customers = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut request = client
            .list_deployment_groups()
            .workspace(workspace)
            .project(project)
            .limit(NonZeroU64::new(100).expect("constant is non-zero"));
        if let Some(search) = search {
            request = request.search(search);
        }
        if let Some(cursor) = cursor.as_deref() {
            request = request.cursor(cursor);
        }
        let page = request
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to list deployment groups".to_string(),
                url: None,
            })?
            .into_inner();
        customers.extend(page.items);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(customers)
}

async fn resolve_customer(
    client: &alien_platform_api::Client,
    workspace: &str,
    project: &str,
    reference: &str,
) -> Result<types::ListDeploymentGroupsResponseItemsItem> {
    let customers = list_customers(client, workspace, project, None).await?;
    let mut matches = customers.into_iter().filter(|customer| {
        customer.id.as_str() == reference
            || customer.name.as_str() == reference
            || customer.external_id.as_ref().map(|value| value.as_str()) == Some(reference)
    });
    let customer = matches.next().ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "customer".to_string(),
            message: format!("Deployment group '{reference}' was not found. Run `alien deployment-groups list` to see available groups."),
        })
    })?;
    if matches.next().is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "customer".to_string(),
            message: format!(
                "Deployment-group reference '{reference}' is ambiguous. Pass the dg_... ID."
            ),
        }));
    }
    Ok(customer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_detail_accepts_common_agent_verbs() {
        for verb in ["get", "describe", "show"] {
            let args = CustomersArgs::try_parse_from([
                "deployment-groups",
                verb,
                "customer_123",
                "--json",
            ])
            .expect("deployment-group detail alias should parse");
            assert!(args.json);
            assert!(matches!(
                args.command,
                CustomersCommand::Get { customer } if customer == "customer_123"
            ));
        }
    }
}
