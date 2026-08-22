use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{command, dim_label, make_table, print_table, success_line};
use alien_error::{Context, IntoAlienError};
use alien_platform_api::types::{
    ConfigureModelsRequest, ConfigureModelsRequestAllowedProvidersItem,
    ConfigureModelsRequestRequirementsItem, ConfigureModelsRequestRequirementsItemClientApisItem,
    ConfigureModelsRequestRequirementsItemPublicModelId, ConfigureProjectBucketsBody,
    ConfigureProjectBucketsBodyAccess, ConfigureProjectDeploymentsBody, ConfigureProjectKeysBody,
    ConfigureProjectRegistryBody, ConfigureProjectRegistryBodyCredentialPolicy,
    ConfigureProjectRegistryBodyRepositoriesItem, CreateProjectBody, CreateProjectBodyName,
    CreateProjectWorkspace, ListProjectsWorkspace,
};
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::BTreeSet;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Project commands",
    long_about = "Manage projects in the Alien platform.",
    after_help = "EXAMPLES:
    alien projects create my-project
    alien projects get my-project
    alien projects describe my-project --json
    alien projects list
    alien projects ls --json
    alien --workspace my-workspace projects ls
    alien projects capabilities status
    alien projects capabilities enable ai --model byo/claude-opus-5
    alien projects capabilities enable encryption"
)]
pub struct ProjectArgs {
    /// Emit structured JSON output
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: ProjectCmd,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ProjectCmd {
    /// Create an ordinary project
    Create {
        /// Project name
        name: String,
    },
    /// List projects
    #[command(visible_alias = "list")]
    Ls,
    /// Show project configuration and enabled capabilities
    #[command(visible_aliases = ["describe", "show"])]
    Get {
        /// Project ID or name (defaults to the linked project)
        project: Option<String>,
    },
    /// Inspect and enable project capabilities
    Capabilities {
        #[command(subcommand)]
        command: CapabilityCommand,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum CapabilityCommand {
    /// Show configured capabilities and customer readiness
    #[command(visible_aliases = ["get", "describe", "show"])]
    Status,
    /// Enable or replace one capability's configuration
    Enable {
        #[arg(value_enum)]
        capability: CapabilityName,
        /// AI model to offer. Repeat for multiple models.
        #[arg(long = "model")]
        models: Vec<String>,
        /// AI model that every customer must connect. Also enables the model.
        #[arg(long = "required-model")]
        required_models: Vec<String>,
        /// Allowed AI provider. Repeat for multiple providers.
        #[arg(long = "provider", value_enum)]
        providers: Vec<AiProvider>,
        /// Registry repository allowlist entry. Repeat for multiple repositories.
        #[arg(long = "repository")]
        repositories: Vec<String>,
        /// Permit registry pushes in addition to pulls.
        #[arg(long)]
        push: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityName {
    #[value(alias = "application")]
    Deployments,
    #[value(alias = "models")]
    Ai,
    #[value(alias = "keys")]
    Encryption,
    #[value(alias = "storage")]
    Buckets,
    Registry,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    AwsBedrock,
    GcpVertex,
    AzureFoundry,
    Anthropic,
    Databricks,
    Openai,
}

pub async fn project_task(args: ProjectArgs, ctx: ExecutionMode) -> Result<()> {
    let http = ctx.auth_http().await?;
    let workspace_name = ctx
        .resolve_workspace_query_with_bootstrap(!args.json)
        .await?;

    match args.cmd {
        ProjectCmd::Create { name } => {
            create_project_task(&http, workspace_name.as_deref(), &name, args.json).await?
        }
        ProjectCmd::Ls => list_projects_task(&http, workspace_name.as_deref(), args.json).await?,
        ProjectCmd::Get { project } => {
            let (project_id, _) = ctx.resolve_project(project.as_deref(), !args.json).await?;
            get_project_task(&http, workspace_name.as_deref(), &project_id, args.json).await?
        }
        ProjectCmd::Capabilities { command } => {
            let (project_id, _) = ctx.resolve_project(None, !args.json).await?;
            capabilities_task(
                &http,
                workspace_name.as_deref(),
                &project_id,
                command,
                args.json,
            )
            .await?
        }
    }

    Ok(())
}

async fn capabilities_task(
    http: &crate::auth::AuthHttp,
    workspace: Option<&str>,
    project: &str,
    action: CapabilityCommand,
    json: bool,
) -> Result<()> {
    match action {
        CapabilityCommand::Status => {
            let mut request = http
                .sdk_client()
                .get_project_capability_overview()
                .id_or_name(project);
            if let Some(workspace) = workspace {
                request = request.workspace(workspace);
            }
            let overview = request
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: "Failed to get project capability status".to_string(),
                    url: None,
                })?
                .into_inner();
            if json {
                print_json(&overview)?;
            } else {
                let value = serde_json::to_value(&overview).into_alien_error().context(
                    ErrorData::ConfigurationError {
                        message: "Failed to render project capability status".to_string(),
                    },
                )?;
                println!("{} {project}", dim_label("Project"));
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value)
                        .into_alien_error()
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to render project capability status".to_string(),
                        })?
                );
            }
        }
        CapabilityCommand::Enable {
            capability,
            models,
            required_models,
            providers,
            repositories,
            push,
        } => {
            validate_capability_options(
                capability,
                &models,
                &required_models,
                &providers,
                &repositories,
                push,
            )?;
            let client = http.sdk_client();
            let result = match capability {
                CapabilityName::Deployments => {
                    let mut request = client
                        .configure_project_deployments()
                        .id_or_name(project)
                        .body(&ConfigureProjectDeploymentsBody { enabled: true });
                    if let Some(workspace) = workspace {
                        request = request.workspace(workspace);
                    }
                    serde_json::to_value(
                        request.send().await.into_sdk_error().context(
                            ErrorData::ApiRequestFailed {
                                message: "Failed to enable deployments".to_string(),
                                url: None,
                            },
                        )?.into_inner(),
                    )
                }
                CapabilityName::Encryption => {
                    let mut request = client
                        .configure_project_keys()
                        .id_or_name(project)
                        .body(&ConfigureProjectKeysBody {
                            application_encryption: true,
                        });
                    if let Some(workspace) = workspace {
                        request = request.workspace(workspace);
                    }
                    serde_json::to_value(
                        request.send().await.into_sdk_error().context(
                            ErrorData::ApiRequestFailed {
                                message: "Failed to enable Encryption Gateway".to_string(),
                                url: None,
                            },
                        )?.into_inner(),
                    )
                }
                CapabilityName::Buckets => {
                    let mut request = client
                        .configure_project_buckets()
                        .id_or_name(project)
                        .body(&ConfigureProjectBucketsBody {
                            access: ConfigureProjectBucketsBodyAccess::ReadWrite,
                        });
                    if let Some(workspace) = workspace {
                        request = request.workspace(workspace);
                    }
                    serde_json::to_value(
                        request.send().await.into_sdk_error().context(
                            ErrorData::ApiRequestFailed {
                                message: "Failed to enable buckets".to_string(),
                                url: None,
                            },
                        )?.into_inner(),
                    )
                }
                CapabilityName::Ai => {
                    let required = required_models.iter().cloned().collect::<BTreeSet<_>>();
                    let all_models = models
                        .into_iter()
                        .chain(required_models)
                        .collect::<BTreeSet<_>>();
                    let requirements = all_models
                        .into_iter()
                        .map(|model| {
                            Ok(ConfigureModelsRequestRequirementsItem {
                                client_apis: vec![
                                    ConfigureModelsRequestRequirementsItemClientApisItem::OpenaiChat,
                                    ConfigureModelsRequestRequirementsItemClientApisItem::OpenaiResponses,
                                    ConfigureModelsRequestRequirementsItemClientApisItem::AnthropicMessages,
                                ],
                                public_model_id: ConfigureModelsRequestRequirementsItemPublicModelId::try_from(model.clone())
                                    .into_alien_error()
                                    .context(ErrorData::ValidationError {
                                        field: "model".to_string(),
                                        message: format!("Invalid model ID {model}"),
                                    })?,
                                required: required.contains(&model),
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let allowed_providers = providers
                        .into_iter()
                        .map(AiProvider::into_sdk)
                        .collect();
                    let mut request = client
                        .configure_project_models()
                        .id_or_name(project)
                        .body(&ConfigureModelsRequest {
                            allowed_providers,
                            requirements,
                        });
                    if let Some(workspace) = workspace {
                        request = request.workspace(workspace);
                    }
                    serde_json::to_value(
                        request.send().await.into_sdk_error().context(
                            ErrorData::ApiRequestFailed {
                                message: "Failed to enable AI Gateway".to_string(),
                                url: None,
                            },
                        )?.into_inner(),
                    )
                }
                CapabilityName::Registry => {
                    let repositories = repositories
                        .into_iter()
                        .map(|repository| {
                            ConfigureProjectRegistryBodyRepositoriesItem::try_from(repository)
                                .into_alien_error()
                                .context(ErrorData::ValidationError {
                                    field: "repository".to_string(),
                                    message: "Invalid repository allowlist entry".to_string(),
                                })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let mut request = client
                        .configure_project_registry()
                        .id_or_name(project)
                        .body(&ConfigureProjectRegistryBody {
                            credential_policy: if push {
                                ConfigureProjectRegistryBodyCredentialPolicy::PushAndPull
                            } else {
                                ConfigureProjectRegistryBodyCredentialPolicy::PullOnly
                            },
                            repositories,
                        });
                    if let Some(workspace) = workspace {
                        request = request.workspace(workspace);
                    }
                    serde_json::to_value(
                        request.send().await.into_sdk_error().context(
                            ErrorData::ApiRequestFailed {
                                message: "Failed to enable container registry".to_string(),
                                url: None,
                            },
                        )?.into_inner(),
                    )
                }
            }
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize capability response".to_string(),
            })?;

            if json {
                print_json(&result)?;
            } else {
                println!("{}", success_line("Project capability configured."));
                println!(
                    "{} {}",
                    dim_label("Next"),
                    command("alien projects capabilities status")
                );
            }
        }
    }
    Ok(())
}

impl AiProvider {
    fn into_sdk(self) -> ConfigureModelsRequestAllowedProvidersItem {
        match self {
            Self::AwsBedrock => ConfigureModelsRequestAllowedProvidersItem::AwsBedrock,
            Self::GcpVertex => ConfigureModelsRequestAllowedProvidersItem::GcpVertex,
            Self::AzureFoundry => ConfigureModelsRequestAllowedProvidersItem::AzureFoundry,
            Self::Anthropic => ConfigureModelsRequestAllowedProvidersItem::Anthropic,
            Self::Databricks => ConfigureModelsRequestAllowedProvidersItem::Databricks,
            Self::Openai => ConfigureModelsRequestAllowedProvidersItem::Openai,
        }
    }
}

fn validate_capability_options(
    capability: CapabilityName,
    models: &[String],
    required_models: &[String],
    providers: &[AiProvider],
    repositories: &[String],
    push: bool,
) -> Result<()> {
    if capability == CapabilityName::Ai && models.is_empty() && required_models.is_empty() {
        return Err(alien_error::AlienError::new(ErrorData::ValidationError {
            field: "model".to_string(),
            message: "AI Gateway requires at least one --model or --required-model.".to_string(),
        }));
    }
    if capability == CapabilityName::Registry && repositories.is_empty() {
        return Err(alien_error::AlienError::new(ErrorData::ValidationError {
            field: "repository".to_string(),
            message: "Container Registry requires at least one --repository allowlist entry."
                .to_string(),
        }));
    }
    if capability != CapabilityName::Ai
        && (!models.is_empty() || !required_models.is_empty() || !providers.is_empty())
    {
        return Err(alien_error::AlienError::new(ErrorData::ValidationError {
            field: "capability".to_string(),
            message: "--model, --required-model, and --provider are only valid for AI Gateway."
                .to_string(),
        }));
    }
    if capability != CapabilityName::Registry && (!repositories.is_empty() || push) {
        return Err(alien_error::AlienError::new(ErrorData::ValidationError {
            field: "capability".to_string(),
            message: "--repository and --push are only valid for Container Registry.".to_string(),
        }));
    }
    Ok(())
}

async fn get_project_task(
    http: &crate::auth::AuthHttp,
    workspace: Option<&str>,
    project: &str,
    json: bool,
) -> Result<()> {
    let mut request = http.sdk_client().get_project().id_or_name(project);
    if let Some(workspace) = workspace {
        request = request.workspace(workspace);
    }
    let project = request
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("Failed to get project {project}"),
            url: None,
        })?
        .into_inner();

    if json {
        print_json(&project)?;
        return Ok(());
    }

    println!("{} {}", dim_label("Project"), project.name.as_str());
    println!("{} {}", dim_label("ID"), project.id.as_str());
    println!(
        "{} {}",
        dim_label("Workspace"),
        project.workspace_id.as_str()
    );
    println!(
        "{} {}",
        dim_label("Created"),
        project.created_at.to_rfc3339()
    );
    println!();
    println!("{}", dim_label("Capabilities"));
    match project.project_capabilities {
        Some(capabilities) => {
            let value = serde_json::to_value(capabilities)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to render project capabilities".to_string(),
                })?;
            let enabled = value
                .get("capabilities")
                .and_then(serde_json::Value::as_object)
                .map(|items| {
                    let mut names = items.keys().cloned().collect::<Vec<_>>();
                    names.sort();
                    names
                })
                .unwrap_or_default();
            if enabled.is_empty() {
                println!("  {}", dim_label("None enabled"));
            } else {
                for capability in enabled {
                    println!("  {capability}");
                }
            }
        }
        None => println!("  {}", dim_label("None enabled")),
    }
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command("alien onboard <customer-name>")
    );

    Ok(())
}

async fn create_project_task(
    http: &crate::auth::AuthHttp,
    workspace: Option<&str>,
    name: &str,
    json: bool,
) -> Result<()> {
    let workspace = workspace.ok_or_else(|| {
        alien_error::AlienError::new(ErrorData::ConfigurationError {
            message: "Project creation requires a workspace. Pass `--workspace <name>` or run `alien workspaces set`.".to_string(),
        })
    })?;
    let workspace_param = CreateProjectWorkspace::try_from(workspace)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "workspace".to_string(),
            message: "Invalid workspace name".to_string(),
        })?;
    let name_param = CreateProjectBodyName::try_from(name.to_string())
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "name".to_string(),
            message: "Invalid project name".to_string(),
        })?;

    let project = http
        .sdk_client()
        .create_project()
        .workspace(&workspace_param)
        .body(&CreateProjectBody {
            name: name_param,
            git_repository: None,
            root_directory: None,
            packages_config: None,
            enabled_capabilities: Vec::new(),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create project".to_string(),
            url: None,
        })?
        .into_inner();

    if json {
        print_json(&project)?;
    } else {
        println!("{}", success_line("Project created."));
        println!("{} {}", dim_label("Project"), project.name.as_str());
        println!("{} {}", dim_label("ID"), project.id.as_str());
        println!();
        println!(
            "{} {}",
            dim_label("Next"),
            command(&format!(
                "alien --project {} onboard <customer-name>",
                project.name.as_str()
            ))
        );
    }

    Ok(())
}

async fn list_projects_task(
    http: &crate::auth::AuthHttp,
    workspace: Option<&str>,
    json: bool,
) -> Result<()> {
    let mut request = http.sdk_client().list_projects();
    if let Some(workspace) = workspace {
        let workspace_param = ListProjectsWorkspace::try_from(workspace)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Workspace name is not valid".to_string(),
            })?;
        request = request.workspace(&workspace_param);
    }

    let response = request
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to list projects".to_string(),
            url: None,
        })?;

    let items = response.into_inner().items;
    if json {
        print_json(&items)?;
    } else if items.is_empty() {
        println!("{}", dim_label("No projects found."));
    } else {
        let mut table = make_table(&["Project", "ID"]);
        for project in items {
            table.add_row(vec![project.name.as_str(), project.id.as_str()]);
        }
        print_table(table);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_project_has_a_complete_non_interactive_form() {
        let args = ProjectArgs::try_parse_from(["projects", "create", "example-project", "--json"])
            .expect("create command should parse");

        assert!(args.json);
        assert!(matches!(
            args.cmd,
            ProjectCmd::Create { name } if name == "example-project"
        ));
    }

    #[test]
    fn get_accepts_agent_friendly_aliases() {
        for verb in ["get", "describe", "show"] {
            let args = ProjectArgs::try_parse_from(["projects", verb, "example-project", "--json"])
                .expect("project detail alias should parse");
            assert!(args.json);
            assert!(matches!(
                args.cmd,
                ProjectCmd::Get { project: Some(project) } if project == "example-project"
            ));
        }
    }

    #[test]
    fn capability_aliases_are_agent_friendly() {
        let args = ProjectArgs::try_parse_from([
            "projects",
            "capabilities",
            "enable",
            "models",
            "--model",
            "byo/claude-opus-5",
            "--provider",
            "anthropic",
            "--json",
        ])
        .expect("AI capability aliases should parse");

        assert!(args.json);
        assert!(matches!(
            args.cmd,
            ProjectCmd::Capabilities {
                command: CapabilityCommand::Enable {
                    capability: CapabilityName::Ai,
                    ..
                }
            }
        ));
    }
}
