use crate::commands::release::{
    auto_build_settings_for_platform, manager_proxy_push_settings, push_stack_with_cache,
};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::get_current_dir;
use crate::output::print_json;
use crate::ui::{command, dim_label, make_table, print_table, success_line};
use alien_core::{
    Platform, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    Stack, ToolchainConfig,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    ConfigureModelsRequest, ConfigureModelsRequestAllowedProvidersItem,
    ConfigureModelsRequestRequirementsItem, ConfigureModelsRequestRequirementsItemClientApisItem,
    ConfigureModelsRequestRequirementsItemPublicModelId, ConfigureProjectBucketsBody,
    ConfigureProjectBucketsBodyAccess, ConfigureProjectDeploymentsBody,
    ConfigureProjectDeploymentsBodyMethodsItem, ConfigureProjectKeysBody,
    ConfigureProjectRegistryBody, ConfigureProjectRegistryBodyCredentialPolicy,
    ConfigureProjectRegistryBodyRepositoriesItem, ConfigureRemoteSandboxRequest,
    ConfigureRemoteSandboxRequestBaseImage, CreateProjectBody, CreateProjectBodyName,
    CreateProjectWorkspace, ListProjectsWorkspace, SandboxBaseImageRepository,
};
use alien_platform_api::SdkResultExt;
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::BTreeSet;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

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
    alien projects capabilities enable encryption
    alien projects capabilities enable remote-sandbox --base-image public.ecr.aws/example/analysis:v1 --max-session-lifetime-seconds 3600
    alien projects capabilities enable remote-sandbox --src ./sandbox --max-session-lifetime-seconds 3600"
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
        /// Public container image Alien builds the sandbox bundle from.
        #[arg(long, conflicts_with = "src")]
        base_image: Option<String>,
        /// Directory Docker builds the sandbox base image from. The image is pushed to the
        /// project's private repository.
        #[arg(long)]
        src: Option<PathBuf>,
        /// Dockerfile path relative to --src (default: Dockerfile).
        #[arg(long, requires = "src", conflicts_with = "base_image")]
        dockerfile: Option<String>,
        /// Ceiling on a single sandbox session, in seconds.
        #[arg(long)]
        max_session_lifetime_seconds: Option<NonZeroU64>,
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
    RemoteSandbox,
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
    if let ProjectCmd::Capabilities {
        command:
            CapabilityCommand::Enable {
                capability,
                models,
                required_models,
                providers,
                repositories,
                push,
                base_image,
                src,
                max_session_lifetime_seconds,
                ..
            },
    } = &args.cmd
    {
        validate_capability_options(
            *capability,
            models,
            required_models,
            providers,
            repositories,
            *push,
            &RemoteSandboxOptions {
                base_image: base_image.as_deref(),
                src: src.as_deref(),
                max_session_lifetime_seconds: *max_session_lifetime_seconds,
            },
        )?;
    }
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
                &ctx,
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
    ctx: &ExecutionMode,
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
            base_image,
            src,
            dockerfile,
            max_session_lifetime_seconds,
        } => {
            let client = http.sdk_client();
            let result = match capability {
                CapabilityName::Deployments => {
                    let mut request = client
                        .configure_project_deployments()
                        .id_or_name(project)
                        .body(&ConfigureProjectDeploymentsBody {
                            enabled: true,
                            methods: vec![ConfigureProjectDeploymentsBodyMethodsItem::Framework],
                        });
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
                CapabilityName::RemoteSandbox => {
                    let max_lifetime_seconds =
                        remote_sandbox_lifetime(max_session_lifetime_seconds)?;
                    let base_image = match (base_image, src) {
                        (Some(base_image), _) => base_image,
                        (None, Some(src)) => {
                            build_and_push_sandbox_base_image(
                                ctx, http, workspace, project, &src, dockerfile, json,
                            )
                            .await?
                        }
                        (None, None) => return Err(remote_sandbox_image_required()),
                    };
                    let body = remote_sandbox_request(&base_image, max_lifetime_seconds)?;
                    let mut request = client
                        .configure_project_remote_sandbox()
                        .id_or_name(project)
                        .body(&body);
                    if let Some(workspace) = workspace {
                        request = request.workspace(workspace);
                    }
                    serde_json::to_value(
                        request.send().await.into_sdk_error().context(
                            ErrorData::ApiRequestFailed {
                                message: "Failed to enable remote sandbox".to_string(),
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

struct RemoteSandboxOptions<'a> {
    base_image: Option<&'a str>,
    src: Option<&'a Path>,
    max_session_lifetime_seconds: Option<NonZeroU64>,
}

fn remote_sandbox_image_required() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationError {
        field: "base-image".to_string(),
        message: "Remote Sandbox requires --base-image or --src.".to_string(),
    })
}

fn remote_sandbox_lifetime(max_session_lifetime_seconds: Option<NonZeroU64>) -> Result<NonZeroU64> {
    let max_session_lifetime_seconds = max_session_lifetime_seconds.ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "max-session-lifetime-seconds".to_string(),
            message: "Remote Sandbox requires --max-session-lifetime-seconds.".to_string(),
        })
    })?;
    if max_session_lifetime_seconds.get() > 28_800 {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "max-session-lifetime-seconds".to_string(),
            message: "Sandbox sessions must be at most 28800 seconds (8 hours).".to_string(),
        }));
    }
    Ok(max_session_lifetime_seconds)
}

fn remote_sandbox_request(
    base_image: &str,
    max_lifetime_seconds: NonZeroU64,
) -> Result<ConfigureRemoteSandboxRequest> {
    let base_image = ConfigureRemoteSandboxRequestBaseImage::try_from(base_image)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "base-image".to_string(),
            message: "Invalid base image reference".to_string(),
        })?;
    Ok(ConfigureRemoteSandboxRequest {
        base_image: Some(base_image),
        azure: None,
        max_lifetime_seconds: Some(max_lifetime_seconds),
    })
}

/// Builds `src` into the sandbox base image, pushes it to the project's private repository, and
/// returns the reference to configure. Build runs first so a failed build creates no repository.
async fn build_and_push_sandbox_base_image(
    ctx: &ExecutionMode,
    http: &crate::auth::AuthHttp,
    workspace: Option<&str>,
    project: &str,
    src: &Path,
    dockerfile: Option<String>,
    json: bool,
) -> Result<String> {
    // Kept apart from `alien release`'s output so neither overwrites the other's stack.json;
    // the push cache in here is what keeps an unchanged image's reference stable.
    let output_dir = get_current_dir()?.join(".alien").join("remote-sandbox");
    if !json {
        println!("{} {}", dim_label("Building"), src.display());
    }
    let settings =
        auto_build_settings_for_platform(Platform::Aws.as_str(), &output_dir, None, None, None)?;
    let built = alien_build::build_stack(sandbox_source_stack(src, dockerfile)?, &settings)
        .await
        .context(ErrorData::BuildFailed)?;

    let mut request = http
        .sdk_client()
        .ensure_project_sandbox_base_image_repository()
        .id_or_name(project);
    if let Some(workspace) = workspace {
        request = request.workspace(workspace);
    }
    let destination = request
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to prepare the project's sandbox image repository".to_string(),
            url: None,
        })?
        .into_inner();

    let manager = ctx
        .resolve_manager_metadata_only(project, Platform::Aws.as_str())
        .await?;
    let push_settings = manager_proxy_push_settings(
        &destination.registry_host,
        &destination.repository,
        &manager,
    )?;
    if !json {
        println!("{} {}", dim_label("Pushing"), push_settings.repository);
    }
    let pushed = push_stack_with_cache(built, Platform::Aws, &output_dir, &push_settings)
        .await
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to push the sandbox base image".to_string(),
            url: None,
        })?;

    configured_base_image(
        &pushed_sandbox_image(&pushed)?,
        &push_settings.repository,
        &destination,
    )
}

fn sandbox_source_stack(src: &Path, dockerfile: Option<String>) -> Result<Stack> {
    // The build cache is keyed on the source path, so `./sandbox` and `sandbox` must agree.
    let src =
        std::fs::canonicalize(src)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "resolve".to_string(),
                file_path: src.display().to_string(),
                reason: "The --src directory could not be found".to_string(),
            })?;
    let sandbox = Sandbox::new("remote-sandbox".to_string())
        .code(SandboxCode::Source {
            src: src.display().to_string(),
            toolchain: ToolchainConfig::Docker {
                dockerfile,
                build_args: None,
                target: None,
            },
        })
        .egress(SandboxEgress::Deny)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    Ok(Stack::new("remote-sandbox".to_string())
        .add(sandbox, ResourceLifecycle::Live)
        .build())
}

fn pushed_sandbox_image(stack: &Stack) -> Result<String> {
    stack
        .resources()
        .find_map(
            |(_, entry)| match &entry.config.downcast_ref::<Sandbox>()?.code {
                SandboxCode::Image { image } => Some(image.clone()),
                SandboxCode::Source { .. } => None,
            },
        )
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "The push did not produce a sandbox base image reference".to_string(),
            })
        })
}

/// Names the pushed image under the host and repository the API returned, which is the only
/// name it accepts as project-owned. The push may address a host rewritten for local access.
fn configured_base_image(
    pushed: &str,
    pushed_repository: &str,
    destination: &SandboxBaseImageRepository,
) -> Result<String> {
    let tag_or_digest = pushed
        .strip_prefix(pushed_repository)
        .filter(|rest| rest.starts_with(':') || rest.starts_with('@'))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Pushed image '{pushed}' is not in the repository '{pushed_repository}'"
                ),
            })
        })?;
    Ok(format!(
        "{}/{}{}",
        alien_core::image_rewrite::strip_url_scheme(&destination.registry_host),
        destination.repository,
        tag_or_digest
    ))
}

fn validate_capability_options(
    capability: CapabilityName,
    models: &[String],
    required_models: &[String],
    providers: &[AiProvider],
    repositories: &[String],
    push: bool,
    sandbox: &RemoteSandboxOptions<'_>,
) -> Result<()> {
    if capability == CapabilityName::Ai && models.is_empty() && required_models.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "model".to_string(),
            message: "AI Gateway requires at least one --model or --required-model.".to_string(),
        }));
    }
    if capability == CapabilityName::Registry && repositories.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "repository".to_string(),
            message: "Container Registry requires at least one --repository allowlist entry."
                .to_string(),
        }));
    }
    if capability != CapabilityName::Ai
        && (!models.is_empty() || !required_models.is_empty() || !providers.is_empty())
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "capability".to_string(),
            message: "--model, --required-model, and --provider are only valid for AI Gateway."
                .to_string(),
        }));
    }
    if capability != CapabilityName::Registry && (!repositories.is_empty() || push) {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "capability".to_string(),
            message: "--repository and --push are only valid for Container Registry.".to_string(),
        }));
    }
    if capability == CapabilityName::RemoteSandbox {
        if sandbox.base_image.is_none() && sandbox.src.is_none() {
            return Err(remote_sandbox_image_required());
        }
        let max_lifetime_seconds = remote_sandbox_lifetime(sandbox.max_session_lifetime_seconds)?;
        if let Some(base_image) = sandbox.base_image {
            remote_sandbox_request(base_image, max_lifetime_seconds)?;
        }
    } else if sandbox.base_image.is_some()
        || sandbox.src.is_some()
        || sandbox.max_session_lifetime_seconds.is_some()
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "capability".to_string(),
            message: "--base-image, --src, and --max-session-lifetime-seconds are only valid for \
                      Remote Sandbox."
                .to_string(),
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

    fn sandbox_options(base_image: Option<&str>, lifetime: Option<NonZeroU64>) -> Result<()> {
        validate_capability_options(
            CapabilityName::RemoteSandbox,
            &[],
            &[],
            &[],
            &[],
            false,
            &RemoteSandboxOptions {
                base_image,
                src: None,
                max_session_lifetime_seconds: lifetime,
            },
        )
    }

    fn enable_remote_sandbox(flags: &[&str]) -> std::result::Result<ProjectArgs, clap::Error> {
        ProjectArgs::try_parse_from(
            ["projects", "capabilities", "enable", "remote-sandbox"]
                .into_iter()
                .chain(flags.iter().copied()),
        )
    }

    #[test]
    fn remote_sandbox_request_serializes_only_the_base_image_source() {
        let request = remote_sandbox_request(
            "public.ecr.aws/example/analysis:v1",
            NonZeroU64::new(28_800).unwrap(),
        )
        .expect("the maximum session lifetime should be accepted");
        assert_eq!(
            serde_json::to_value(request).expect("request should serialize"),
            serde_json::json!({
                "baseImage": "public.ecr.aws/example/analysis:v1",
                "maxLifetimeSeconds": 28_800,
            }),
        );
    }

    #[test]
    fn remote_sandbox_rejects_invalid_base_images() {
        for image in [
            "".to_string(),
            "image with spaces".to_string(),
            "x".repeat(1025),
        ] {
            let error = sandbox_options(Some(&image), NonZeroU64::new(3600))
                .expect_err("invalid image references must be rejected");
            assert!(error.to_string().contains("base-image"), "{error}");
        }
    }

    #[test]
    fn remote_sandbox_rejects_sessions_longer_than_eight_hours() {
        let error = sandbox_options(
            Some("public.ecr.aws/example/analysis:v1"),
            NonZeroU64::new(28_801),
        )
        .expect_err("the API session lifetime limit must be enforced locally");
        assert!(error.to_string().contains("28800"), "{error}");
    }

    #[tokio::test]
    async fn remote_sandbox_validates_before_resolving_auth_or_project() {
        let args = enable_remote_sandbox(&["--json"])
            .expect("the command should parse before capability validation");
        let error = project_task(args, ExecutionMode::Dev { port: 0 })
            .await
            .expect_err("missing flags must fail before connecting to any API");
        assert!(
            error.to_string().contains("--base-image or --src"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn remote_sandbox_source_still_needs_a_session_ceiling_before_building() {
        let args = enable_remote_sandbox(&["--src", "./does-not-exist", "--json"])
            .expect("--src alone should parse");
        let error = project_task(args, ExecutionMode::Dev { port: 0 })
            .await
            .expect_err("a missing session ceiling must fail before any build");
        assert!(
            error.to_string().contains("--max-session-lifetime-seconds"),
            "{error}"
        );
    }

    #[test]
    fn remote_sandbox_parses_its_own_flags() {
        let args = enable_remote_sandbox(&[
            "--base-image",
            "public.ecr.aws/example/analysis:v1",
            "--max-session-lifetime-seconds",
            "3600",
        ])
        .expect("remote sandbox flags should parse");

        let ProjectCmd::Capabilities {
            command:
                CapabilityCommand::Enable {
                    capability,
                    base_image,
                    max_session_lifetime_seconds,
                    ..
                },
        } = args.cmd
        else {
            panic!("expected a capability enable command");
        };
        assert_eq!(capability, CapabilityName::RemoteSandbox);
        assert_eq!(
            base_image.as_deref(),
            Some("public.ecr.aws/example/analysis:v1")
        );
        assert_eq!(
            max_session_lifetime_seconds.map(NonZeroU64::get),
            Some(3600)
        );
    }

    #[test]
    fn remote_sandbox_parses_a_source_build() {
        let args = enable_remote_sandbox(&[
            "--src",
            "./sandbox",
            "--dockerfile",
            "docker/Sandbox.dockerfile",
            "--max-session-lifetime-seconds",
            "3600",
        ])
        .expect("--src with --dockerfile should parse");

        let ProjectCmd::Capabilities {
            command:
                CapabilityCommand::Enable {
                    base_image,
                    src,
                    dockerfile,
                    ..
                },
        } = args.cmd
        else {
            panic!("expected a capability enable command");
        };
        assert_eq!(base_image, None);
        assert_eq!(src, Some(PathBuf::from("./sandbox")));
        assert_eq!(dockerfile.as_deref(), Some("docker/Sandbox.dockerfile"));
    }

    #[test]
    fn src_and_base_image_are_mutually_exclusive() {
        let error = enable_remote_sandbox(&[
            "--src",
            "./sandbox",
            "--base-image",
            "public.ecr.aws/example/analysis:v1",
        ])
        .expect_err("--src and --base-image name two different images");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn dockerfile_requires_src() {
        let error = enable_remote_sandbox(&["--dockerfile", "Dockerfile"])
            .expect_err("--dockerfile means nothing without a build");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );

        let error = enable_remote_sandbox(&[
            "--dockerfile",
            "Dockerfile",
            "--base-image",
            "public.ecr.aws/example/analysis:v1",
        ])
        .expect_err("a prebuilt image has no Dockerfile to choose");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn remote_sandbox_requires_an_image_source() {
        let error = sandbox_options(None, NonZeroU64::new(3600))
            .expect_err("a missing image source must be refused");
        assert!(
            error.to_string().contains("--base-image or --src"),
            "unexpected message: {error}"
        );

        validate_capability_options(
            CapabilityName::RemoteSandbox,
            &[],
            &[],
            &[],
            &[],
            false,
            &RemoteSandboxOptions {
                base_image: None,
                src: Some(Path::new("./sandbox")),
                max_session_lifetime_seconds: NonZeroU64::new(3600),
            },
        )
        .expect("--src is an image source");
    }

    #[test]
    fn remote_sandbox_requires_a_session_ceiling() {
        let error = sandbox_options(Some("public.ecr.aws/x/y:v1"), None)
            .expect_err("a missing session ceiling must be refused");
        assert!(
            error.to_string().contains("--max-session-lifetime-seconds"),
            "unexpected message: {error}"
        );
    }

    #[test]
    fn sandbox_options_are_refused_for_another_capability() {
        for sandbox in [
            RemoteSandboxOptions {
                base_image: Some("public.ecr.aws/x/y:v1"),
                src: None,
                max_session_lifetime_seconds: None,
            },
            RemoteSandboxOptions {
                base_image: None,
                src: Some(Path::new("./sandbox")),
                max_session_lifetime_seconds: None,
            },
        ] {
            let error = validate_capability_options(
                CapabilityName::Registry,
                &[],
                &[],
                &[],
                &["repo".to_string()],
                false,
                &sandbox,
            )
            .expect_err("sandbox options must not apply to another capability");
            assert!(
                error.to_string().contains("only valid for Remote Sandbox"),
                "unexpected message: {error}"
            );
        }
    }

    #[test]
    fn configure_names_the_pushed_image_under_the_returned_repository() {
        let destination = SandboxBaseImageRepository {
            registry_host: "host.docker.internal:8090".to_string(),
            repository: "acme-sandbox".to_string(),
        };
        let base_image = configured_base_image(
            "localhost:8090/acme-sandbox:remote-sandbox-k3j9x2ab",
            "localhost:8090/acme-sandbox",
            &destination,
        )
        .expect("the pushed image is in the pushed repository");
        assert_eq!(
            base_image,
            "host.docker.internal:8090/acme-sandbox:remote-sandbox-k3j9x2ab"
        );

        let request = remote_sandbox_request(&base_image, NonZeroU64::new(3600).unwrap())
            .expect("the configured reference is a valid base image");
        assert_eq!(
            serde_json::to_value(request).expect("request should serialize")["baseImage"],
            serde_json::json!(base_image),
        );
    }

    #[test]
    fn configure_refuses_an_image_outside_the_pushed_repository() {
        let destination = SandboxBaseImageRepository {
            registry_host: "registry.example.com".to_string(),
            repository: "acme-sandbox".to_string(),
        };
        for pushed in [
            "registry.example.com/acme-sandbox-other:v1",
            "registry.example.com/other:v1",
        ] {
            configured_base_image(pushed, "registry.example.com/acme-sandbox", &destination)
                .expect_err("a sibling repository must not be renamed into the project's");
        }
    }

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
