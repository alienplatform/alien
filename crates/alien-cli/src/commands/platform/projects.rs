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
    alien projects capabilities enable remote-sandbox --image public.ecr.aws/example/analysis:v1 --max-session-lifetime-seconds 3600
    alien projects capabilities enable remote-sandbox --src ./sandbox --max-session-lifetime-seconds 3600
    alien projects capabilities enable remote-sandbox --src ./sandbox --dockerfile Sandbox.dockerfile --max-session-lifetime-seconds 3600
    alien projects capabilities enable remote-sandbox --src ./sandbox --rebuild --max-session-lifetime-seconds 3600"
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
        /// Prebuilt container image the sandbox starts from, used as is. For a private image,
        /// use --src.
        #[arg(long = "image", alias = "base-image", conflicts_with = "src")]
        image: Option<String>,
        /// Directory with a Dockerfile. The image is built locally with Docker and pushed to the
        /// project's private repository.
        #[arg(long)]
        src: Option<PathBuf>,
        /// Dockerfile path relative to --src (default: Dockerfile).
        #[arg(long, requires = "src", conflicts_with = "image")]
        dockerfile: Option<String>,
        /// Build and push even when --src is unchanged, for example to pick up a newer image in
        /// FROM.
        #[arg(long, requires = "src", conflicts_with = "image")]
        rebuild: bool,
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
                image,
                src,
                dockerfile,
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
                image: image.as_deref(),
                src: src.as_deref(),
                dockerfile: dockerfile.as_deref(),
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
            image,
            src,
            dockerfile,
            rebuild,
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
                    let source = match (image, src) {
                        (Some(image), _) => SandboxImageSource::Image(image),
                        (None, Some(src)) => SandboxImageSource::Source(SandboxSource {
                            src,
                            dockerfile,
                            rebuild,
                        }),
                        (None, None) => return Err(remote_sandbox_image_required()),
                    };
                    let base_image = match source {
                        SandboxImageSource::Image(image) => image,
                        SandboxImageSource::Source(source) => {
                            build_and_push_sandbox_base_image(
                                ctx, http, workspace, project, &source, json,
                            )
                            .await?
                        }
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
    image: Option<&'a str>,
    src: Option<&'a Path>,
    dockerfile: Option<&'a str>,
    max_session_lifetime_seconds: Option<NonZeroU64>,
}

/// Where the sandbox base image comes from.
enum SandboxImageSource {
    /// A prebuilt image reference, configured as given.
    Image(String),
    /// A Docker build context, built and pushed to the project's repository.
    Source(SandboxSource),
}

struct SandboxSource {
    src: PathBuf,
    dockerfile: Option<String>,
    rebuild: bool,
}

/// Hash of everything a source build reads. Equal hashes mean equal build inputs, so the tag it
/// names lets any machine reuse an image another one already pushed.
struct SourceInputHash(String);

impl SourceInputHash {
    fn tag(&self) -> String {
        format!("remote-sandbox-src-{}", &self.0[..16])
    }
}

fn remote_sandbox_image_required() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationError {
        field: "image".to_string(),
        message: "Remote Sandbox requires --image or --src.".to_string(),
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
            field: "image".to_string(),
            message: "Invalid image reference".to_string(),
        })?;
    Ok(ConfigureRemoteSandboxRequest {
        base_image: Some(base_image),
        azure: None,
        max_lifetime_seconds: Some(max_lifetime_seconds),
    })
}

/// Resolves the sandbox base image for `source` in the project's private repository and returns
/// the digest reference to configure. An unchanged source reuses the image already tagged with
/// its input hash, from any machine, without building.
async fn build_and_push_sandbox_base_image(
    ctx: &ExecutionMode,
    http: &crate::auth::AuthHttp,
    workspace: Option<&str>,
    project: &str,
    source: &SandboxSource,
    json: bool,
) -> Result<String> {
    // Kept apart from `alien release`'s output so neither overwrites the other's stack.json.
    let output_dir = get_current_dir()?.join(".alien").join("remote-sandbox");
    let src = source_directory(&source.src)?;
    let toolchain = ToolchainConfig::Docker {
        dockerfile: source.dockerfile.clone(),
        build_args: None,
        target: None,
    };
    let mut settings =
        auto_build_settings_for_platform(Platform::Aws.as_str(), &output_dir, None, None, None)?;
    settings.rebuild = source.rebuild;
    let input_hash = SourceInputHash(
        alien_build::docker_source_input_hash(&src, &toolchain, &settings.get_targets())
            .await
            .context(ErrorData::BuildFailed)?,
    );

    let manager = ctx
        .resolve_manager_metadata_only(project, Platform::Aws.as_str())
        .await?;
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
    let push_settings = manager_proxy_push_settings(
        &destination.registry_host,
        &destination.repository,
        &manager,
    )?;
    let push_failed = || ErrorData::SandboxImagePushFailed {
        repository: push_settings.repository.clone(),
    };

    let source_image = format!("{}:{}", push_settings.repository, input_hash.tag());
    let existing = if source.rebuild {
        None
    } else {
        alien_build::registry::manifest_digest(&source_image, &push_settings.options)
            .await
            .context(push_failed())?
    };
    let digest = match existing {
        Some(digest) => {
            if !json {
                println!("{} {}", dim_label("Unchanged"), source_image);
            }
            digest
        }
        None => {
            if !json {
                println!("{} {}", dim_label("Building"), src.display());
            }
            let built = alien_build::build_stack(sandbox_source_stack(&src, toolchain), &settings)
                .await
                .context(ErrorData::BuildFailed)?;
            if !json {
                println!("{} {}", dim_label("Pushing"), push_settings.repository);
            }
            let pushed = push_stack_with_cache(built, Platform::Aws, &output_dir, &push_settings)
                .await
                .context(push_failed())?;
            alien_build::registry::tag_manifest(
                &pushed_sandbox_image(&pushed)?,
                &source_image,
                &push_settings.options,
            )
            .await
            .context(push_failed())?
        }
    };

    configured_base_image(
        &format!("{}@{}", push_settings.repository, digest),
        &push_settings.repository,
        &destination,
    )
}

fn source_directory(src: &Path) -> Result<PathBuf> {
    // The build cache is keyed on the source path, so `./sandbox` and `sandbox` must agree.
    std::fs::canonicalize(src)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "src".to_string(),
            message: format!("Could not resolve the --src directory '{}'", src.display()),
        })
}

fn sandbox_source_stack(src: &Path, toolchain: ToolchainConfig) -> Stack {
    let sandbox = Sandbox::new("remote-sandbox".to_string())
        .code(SandboxCode::Source {
            src: src.display().to_string(),
            toolchain,
        })
        .egress(SandboxEgress::Deny)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    Stack::new("remote-sandbox".to_string())
        .add(sandbox, ResourceLifecycle::Live)
        .build()
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
            AlienError::new(ErrorData::GenericError {
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
            AlienError::new(ErrorData::GenericError {
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
        if sandbox.image.is_none() && sandbox.src.is_none() {
            return Err(remote_sandbox_image_required());
        }
        let max_lifetime_seconds = remote_sandbox_lifetime(sandbox.max_session_lifetime_seconds)?;
        if let Some(image) = sandbox.image {
            remote_sandbox_request(image, max_lifetime_seconds)?;
        }
        if let Some(src) = sandbox.src {
            let dockerfile = src.join(sandbox.dockerfile.unwrap_or("Dockerfile"));
            if !dockerfile.is_file() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "dockerfile".to_string(),
                    message: format!(
                        "No Dockerfile at '{}'. Pass --dockerfile with its path relative to --src.",
                        dockerfile.display()
                    ),
                }));
            }
        }
    } else if sandbox.image.is_some()
        || sandbox.src.is_some()
        || sandbox.max_session_lifetime_seconds.is_some()
    {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "capability".to_string(),
            message: "--image, --src, and --max-session-lifetime-seconds are only valid for \
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

    fn sandbox_options(image: Option<&str>, lifetime: Option<NonZeroU64>) -> Result<()> {
        source_options(image, None, None, lifetime)
    }

    fn source_options(
        image: Option<&str>,
        src: Option<&Path>,
        dockerfile: Option<&str>,
        lifetime: Option<NonZeroU64>,
    ) -> Result<()> {
        validate_capability_options(
            CapabilityName::RemoteSandbox,
            &[],
            &[],
            &[],
            &[],
            false,
            &RemoteSandboxOptions {
                image,
                src,
                dockerfile,
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
            assert!(error.to_string().contains("image"), "{error}");
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
        assert!(error.to_string().contains("--image or --src"), "{error}");
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
        for flag in ["--image", "--base-image"] {
            let args = enable_remote_sandbox(&[
                flag,
                "public.ecr.aws/example/analysis:v1",
                "--max-session-lifetime-seconds",
                "3600",
            ])
            .expect("remote sandbox flags should parse");

            let ProjectCmd::Capabilities {
                command:
                    CapabilityCommand::Enable {
                        capability,
                        image,
                        rebuild,
                        max_session_lifetime_seconds,
                        ..
                    },
            } = args.cmd
            else {
                panic!("expected a capability enable command");
            };
            assert_eq!(capability, CapabilityName::RemoteSandbox);
            assert_eq!(image.as_deref(), Some("public.ecr.aws/example/analysis:v1"));
            assert!(!rebuild);
            assert_eq!(
                max_session_lifetime_seconds.map(NonZeroU64::get),
                Some(3600)
            );
        }
    }

    #[test]
    fn the_old_image_flag_stays_out_of_the_help() {
        let mut command = <ProjectArgs as clap::CommandFactory>::command();
        let enable = command
            .find_subcommand_mut("capabilities")
            .and_then(|command| command.find_subcommand_mut("enable"))
            .expect("the enable command exists");
        let help = enable.render_long_help().to_string();
        assert!(help.contains("--image"), "{help}");
        assert!(!help.contains("--base-image"), "{help}");
    }

    #[test]
    fn remote_sandbox_parses_a_source_build() {
        let args = enable_remote_sandbox(&[
            "--src",
            "./sandbox",
            "--dockerfile",
            "docker/Sandbox.dockerfile",
            "--rebuild",
            "--max-session-lifetime-seconds",
            "3600",
        ])
        .expect("--src with --dockerfile and --rebuild should parse");

        let ProjectCmd::Capabilities {
            command:
                CapabilityCommand::Enable {
                    image,
                    src,
                    dockerfile,
                    rebuild,
                    ..
                },
        } = args.cmd
        else {
            panic!("expected a capability enable command");
        };
        assert_eq!(image, None);
        assert_eq!(src, Some(PathBuf::from("./sandbox")));
        assert_eq!(dockerfile.as_deref(), Some("docker/Sandbox.dockerfile"));
        assert!(rebuild);
    }

    #[test]
    fn src_and_image_are_mutually_exclusive() {
        for flag in ["--image", "--base-image"] {
            let error = enable_remote_sandbox(&[
                "--src",
                "./sandbox",
                flag,
                "public.ecr.aws/example/analysis:v1",
            ])
            .expect_err("--src and --image name two different images");
            assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        }
    }

    #[test]
    fn build_flags_require_src() {
        for flags in [&["--dockerfile", "Dockerfile"][..], &["--rebuild"][..]] {
            let error = enable_remote_sandbox(flags)
                .expect_err("a build flag means nothing without a build");
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::MissingRequiredArgument,
                "{flags:?}"
            );

            let with_image = [
                flags,
                &["--image", "public.ecr.aws/example/analysis:v1"][..],
            ]
            .concat();
            let error =
                enable_remote_sandbox(&with_image).expect_err("a prebuilt image is never built");
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::ArgumentConflict,
                "{flags:?}"
            );
        }
    }

    #[test]
    fn remote_sandbox_requires_an_image_source() {
        let error = sandbox_options(None, NonZeroU64::new(3600))
            .expect_err("a missing image source must be refused");
        assert!(
            error.to_string().contains("--image or --src"),
            "unexpected message: {error}"
        );

        let src = tempfile::tempdir().expect("temp dir");
        std::fs::write(src.path().join("Dockerfile"), "FROM alpine:3.20\n").unwrap();
        source_options(None, Some(src.path()), None, NonZeroU64::new(3600))
            .expect("--src is an image source");
    }

    #[test]
    fn a_source_without_its_dockerfile_is_refused_before_any_request() {
        let src = tempfile::tempdir().expect("temp dir");
        std::fs::write(src.path().join("Sandbox.dockerfile"), "FROM alpine:3.20\n").unwrap();

        let error = source_options(None, Some(src.path()), None, NonZeroU64::new(3600))
            .expect_err("the default Dockerfile is missing");
        assert!(error.to_string().contains("--dockerfile"), "{error}");

        source_options(
            None,
            Some(src.path()),
            Some("Sandbox.dockerfile"),
            NonZeroU64::new(3600),
        )
        .expect("the named Dockerfile exists");
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
                image: Some("public.ecr.aws/x/y:v1"),
                src: None,
                dockerfile: None,
                max_session_lifetime_seconds: None,
            },
            RemoteSandboxOptions {
                image: None,
                src: Some(Path::new("./sandbox")),
                dockerfile: None,
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
        let digest = "sha256:7d5463a6f1c2b3e4d5c6b7a8f9e0d1c2b3a4f5e6d7c8b9a0f1e2d3c4b5a69788";
        let base_image = configured_base_image(
            &format!("localhost:8090/acme-sandbox@{digest}"),
            "localhost:8090/acme-sandbox",
            &destination,
        )
        .expect("the pushed image is in the pushed repository");
        assert_eq!(
            base_image,
            format!("host.docker.internal:8090/acme-sandbox@{digest}")
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
