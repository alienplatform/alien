use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::interaction::{ConfirmationMode, InteractionMode};
use crate::output::{print_json, prompt_confirm};
use crate::ui::{command, dim_label, make_table, print_table, success_line};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::{types, SdkResultExt as _};
use clap::{Parser, Subcommand};
use std::num::NonZeroU64;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage deployment groups",
    long_about = "List, inspect, and change deployment groups in a project. In customer-facing projects, each deployment group commonly represents one customer environment.",
    after_help = "EXAMPLES:
    alien deployment-groups list
    alien deployment-groups get customer_123
    alien deployment-groups describe acme --json
    alien deployment-groups update acme --max-deployments 5
    alien deployment-groups set-external-id acme customer_123
    alien deployment-groups delete acme --yes
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
    /// Rename a deployment group or change its deployment limit
    Update {
        /// Deployment-group ID, name, or external ID
        customer: String,
        /// New name: lowercase letters, numbers, and hyphens
        #[arg(long)]
        name: Option<String>,
        /// Most deployments the group may hold
        #[arg(long)]
        max_deployments: Option<u64>,
    },
    /// Set or clear the ID your own systems use for a deployment group
    SetExternalId {
        /// Deployment-group ID, name, or external ID
        customer: String,
        /// External ID, unique within the project
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        external_id: Option<String>,
        /// Remove the external ID
        #[arg(long)]
        clear: bool,
    },
    /// Delete an empty deployment group and its API keys
    #[command(visible_alias = "rm")]
    Delete {
        /// Deployment-group ID, name, or external ID
        customer: String,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

pub async fn customers_task(args: CustomersArgs, ctx: ExecutionMode) -> Result<()> {
    let confirmation = match &args.command {
        CustomersCommand::Delete { yes, .. } => InteractionMode::current(args.json)
            .confirmation_mode(
                *yes,
                "Deleting a deployment group needs confirmation. Re-run with `--yes`.",
            )?,
        _ => ConfirmationMode::Skip,
    };
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
        CustomersCommand::Update {
            customer,
            name,
            max_deployments,
        } => {
            if name.is_none() && max_deployments.is_none() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: "Nothing to change. Pass --name or --max-deployments.".to_string(),
                }));
            }
            let customer = resolve_customer(&client, &workspace, &project_id, &customer).await?;
            let mut body = serde_json::Map::new();
            if let Some(name) = name {
                body.insert("name".to_string(), serde_json::json!(name));
            }
            if let Some(max_deployments) = max_deployments {
                body.insert(
                    "maxDeployments".to_string(),
                    serde_json::json!(max_deployments),
                );
            }
            let body: types::UpdateDeploymentGroupRequest =
                serde_json::from_value(serde_json::Value::Object(body))
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "name".to_string(),
                        message: "Names are 2 to 100 lowercase letters, numbers, and single hyphens, and can't start with dg-. The deployment limit is at least 1.".to_string(),
                    })?;
            let response = client
                .update_deployment_group()
                .id(customer.id.as_str())
                .workspace(workspace.as_str())
                .body(body)
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!(
                        "Failed to update deployment group {}",
                        customer.name.as_str()
                    ),
                    url: None,
                })?
                .into_inner();
            if args.json {
                print_json(&response)?;
            } else {
                println!("{}", success_line("Deployment group updated."));
                println!(
                    "{} {}",
                    dim_label("Deployment group"),
                    response.name.as_str()
                );
                println!(
                    "{} {}",
                    dim_label("Deployment limit"),
                    response.max_deployments
                );
            }
        }
        CustomersCommand::SetExternalId {
            customer,
            external_id,
            clear: _,
        } => {
            let customer = resolve_customer(&client, &workspace, &project_id, &customer).await?;
            let body: types::SetDeploymentGroupExternalIdRequest =
                serde_json::from_value(serde_json::json!({ "externalId": external_id }))
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: "external-id".to_string(),
                        message: "External IDs are 1 to 128 characters: letters, numbers, and . _ ~ -, starting with a letter or number.".to_string(),
                    })?;
            let response = client
                .set_deployment_group_external_id()
                .id(customer.id.as_str())
                .workspace(workspace.as_str())
                .body(body)
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!(
                        "Failed to set the external ID of deployment group {}",
                        customer.name.as_str()
                    ),
                    url: None,
                })?
                .into_inner();
            if args.json {
                print_json(&response)?;
            } else {
                match response.external_id.as_ref() {
                    Some(external_id) => println!(
                        "{}",
                        success_line(&format!(
                            "External ID of {} set to {}.",
                            response.name.as_str(),
                            external_id.as_str()
                        ))
                    ),
                    None => println!(
                        "{}",
                        success_line(&format!(
                            "External ID of {} removed.",
                            response.name.as_str()
                        ))
                    ),
                }
            }
        }
        CustomersCommand::Delete { customer, yes: _ } => {
            let customer = resolve_customer(&client, &workspace, &project_id, &customer).await?;
            if confirmation == ConfirmationMode::Prompt
                && !prompt_confirm(
                    &format!(
                        "Delete deployment group {}? Its API keys are deleted too, so anything using them stops working.",
                        customer.name.as_str()
                    ),
                    false,
                )?
            {
                println!("{}", dim_label("Nothing changed."));
                return Ok(());
            }
            client
                .delete_deployment_group()
                .id(customer.id.as_str())
                .workspace(workspace.as_str())
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!(
                        "Failed to delete deployment group {}",
                        customer.name.as_str()
                    ),
                    url: None,
                })?;
            if args.json {
                print_json(&serde_json::json!({
                    "id": customer.id.as_str(),
                    "name": customer.name.as_str(),
                    "deleted": true,
                }))?;
            } else {
                println!(
                    "{}",
                    success_line(&format!(
                        "Deployment group {} deleted.",
                        customer.name.as_str()
                    ))
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
