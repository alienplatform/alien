use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::{can_prompt, print_json, prompt_text};
use crate::ui::{accent, command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_core::{
    deployer_secret_value_refusal, is_deployer_secret_input, Platform, Stack, StackInputDefinition,
    StackInputKind, StackInputProvider,
};
use alien_error::{AlienError, Context, IntoAlienError};
use clap::{Parser, ValueEnum};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Onboard a customer and get the setup command to send them",
    long_about = "Create a deployment group for a customer and print what their admin runs to set up the deployment: a setup link, a Helm install command for Kubernetes, or a CLI command."
)]
pub struct OnboardArgs {
    /// Customer name
    #[arg(value_name = "NAME")]
    pub name: Option<String>,

    /// Stable customer identifier used by your application. Defaults to a URL-safe form of NAME.
    #[arg(long)]
    pub external_id: Option<String>,

    /// Customer infrastructure to include in the setup link
    #[arg(
        long = "setup-items",
        value_delimiter = ',',
        default_value = "application"
    )]
    pub setup_items: Vec<OnboardSetupItem>,

    /// Maximum number of deployments for this customer
    #[arg(long, default_value = "100")]
    pub max_deployments: u64,

    /// Output in JSON format (for scripting)
    #[arg(long)]
    pub json: bool,

    /// Plain environment variables for deployments created from this link (KEY=VALUE or KEY=VALUE:target1,target2)
    #[arg(long = "env")]
    pub env_vars: Vec<String>,

    /// Secret environment variables for deployments created from this link (KEY=VALUE or KEY=VALUE:target1,target2)
    #[arg(long = "secret")]
    pub secret_vars: Vec<String>,

    /// Stack input value provided before creating the deployment link (id=value)
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Secret stack input value provided before creating the deployment link (id=value)
    #[arg(long = "secret-input")]
    pub secret_input_values: Vec<String>,

    /// Platforms this deployment link can create deployments for (comma-separated). Defaults to every platform in the active release.
    #[arg(long = "platforms", alias = "platform", value_delimiter = ',')]
    pub platforms: Vec<String>,

    /// Public subdomain to reserve for deployments created from this link.
    #[arg(long)]
    pub subdomain: Option<String>,

    /// The environment can't reach your manager: register its deployment now.
    /// The site keeps it up to date with `alien-deploy sync`.
    #[arg(long)]
    pub airgapped: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OnboardSetupItem {
    Application,
    Models,
    Keys,
    Storage,
    Registry,
    Sandbox,
}

pub async fn onboard_task(args: OnboardArgs, ctx: ExecutionMode) -> Result<()> {
    let name = if let Some(ref name) = args.name {
        name.clone()
    } else if args.json || !can_prompt() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Customer name is required in non-interactive mode. Pass `alien onboard <name>`."
                    .to_string(),
        }));
    } else {
        prompt_text("Customer name", None)?
    };

    match ctx {
        #[cfg(feature = "platform")]
        ExecutionMode::Platform { .. } => onboard_platform(args, ctx, name).await,
        _ => onboard_standalone(args, ctx, name).await,
    }
}

/// Platform mode: use Platform API directly to get deployment link.
#[cfg(feature = "platform")]
async fn onboard_platform(args: OnboardArgs, ctx: ExecutionMode, name: String) -> Result<()> {
    use alien_platform_api::SdkResultExt;

    let setup_environment_variables = platform_setup_environment_variables(
        &crate::parse_env_and_secret_vars(&args.env_vars, &args.secret_vars)?,
    )?;

    let (project_id, _project_link) = ctx.resolve_project(None, !args.json).await?;
    let workspace = ctx.resolve_platform_workspace_context(!args.json).await?;
    let client = ctx.sdk_client().await?;
    let available_setup =
        fetch_available_setup(&client, workspace.query.as_deref(), &project_id).await?;
    validate_setup_items(&args.setup_items, &available_setup.items)?;
    let includes_application = args.setup_items.contains(&OnboardSetupItem::Application);
    let release_inputs = if includes_application {
        fetch_active_release_stack_inputs(&client, workspace.query.as_deref(), &project_id).await?
    } else {
        ActiveReleaseStackInputs {
            supported_platforms: available_setup.supported_platforms,
            inputs_by_platform: Vec::new(),
        }
    };
    let selected_platforms = select_onboard_platforms(
        &args.platforms,
        &release_inputs.supported_platforms,
        args.json,
    )?;
    if args.airgapped && selected_platforms != [Platform::Kubernetes] {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "platforms".to_string(),
            message: "Air-gapped environments run on Kubernetes: pass --platforms kubernetes"
                .to_string(),
        }));
    }
    let developer_inputs = developer_inputs_for_platforms(&release_inputs, &selected_platforms);
    let input_args = airgapped_or_group_inputs(&args, &release_inputs, &selected_platforms)?;
    let stack_input_values = collect_stack_input_values(
        &developer_inputs,
        &input_args.group_inputs,
        &input_args.group_secret_inputs,
        &selected_platforms,
        args.json,
    )?;
    let public_subdomain = args
        .subdomain
        .as_deref()
        .map(validate_public_subdomain)
        .transpose()?;
    let deployment_group_name = customer_environment_name(&name);
    let external_id = args
        .external_id
        .clone()
        .unwrap_or_else(|| deployment_group_name.clone());

    if !args.json {
        let platforms_label = selected_platforms
            .iter()
            .map(|platform| platform.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{}",
            contextual_heading("Onboarding", &name, &[("platforms", &platforms_label)])
        );
        print_required_developer_inputs(&developer_inputs);
    }
    let steps = if args.json {
        None
    } else {
        let steps = FixedSteps::new(&["Prepare customer environment", "Generate setup link"]);
        steps.activate(0, Some(name.clone()));
        Some(steps)
    };

    // Ensure the Project + external ID Deployment Group and issue its setup
    // token through one retry-safe API operation. A retry reuses the same
    // customer environment instead of creating duplicate groups.
    let mut create_setup_link = client.create_setup_link();
    if let Some(workspace_query) = workspace.query.as_deref() {
        let workspace_param =
            alien_platform_api::types::CreateSetupLinkWorkspace::try_from(workspace_query)
                .map_err(|e| {
                    AlienError::new(ErrorData::ValidationError {
                        field: "workspace".to_string(),
                        message: format!("Invalid workspace: {}", e),
                    })
                })?;
        create_setup_link = create_setup_link.workspace(&workspace_param);
    }

    let response = create_setup_link
        .body(alien_platform_api::types::CreateSetupLinkRequest {
            deployment_setup_config: Some(platform_onboard_deployment_setup_config(
                setup_environment_variables,
                &selected_platforms,
                public_subdomain.as_deref(),
                &name,
                includes_application,
            )?),
            description: None,
            entry_point: None,
            expires_at: None,
            external_id: external_id.clone().try_into().map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "external-id".to_string(),
                    message: format!("{}", e),
                })
            })?,
            input_values: Some(stack_input_values),
            max_deployments: std::num::NonZeroU64::new(args.max_deployments)
                .unwrap_or(std::num::NonZeroU64::new(100).unwrap()),
            recovery_deployment_group_id: None,
            name: deployment_group_name.try_into().map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: format!("{}", e),
                })
            })?,
            project: project_id.clone().try_into().map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "project".to_string(),
                    message: format!("{}", e),
                })
            })?,
            setup_items: Some(
                args.setup_items
                    .iter()
                    .map(|item| alien_platform_api::types::DeploymentSetupItemSelection {
                    item: match item {
                        OnboardSetupItem::Application => alien_platform_api::types::DeploymentSetupItemSelectionItem::Deployment,
                        OnboardSetupItem::Models => alien_platform_api::types::DeploymentSetupItemSelectionItem::Models,
                        OnboardSetupItem::Keys => alien_platform_api::types::DeploymentSetupItemSelectionItem::Keys,
                        OnboardSetupItem::Storage => alien_platform_api::types::DeploymentSetupItemSelectionItem::Bucket,
                        OnboardSetupItem::Registry => alien_platform_api::types::DeploymentSetupItemSelectionItem::Registry,
                        OnboardSetupItem::Sandbox => alien_platform_api::types::DeploymentSetupItemSelectionItem::Sandbox,
                    },
                    provider_allowlist: Vec::new(),
                    release_channel: None,
                    required: true,
                    })
                    .collect::<Vec<_>>()
                    .into(),
            ),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create customer setup link".to_string(),
            url: None,
        })?;

    let deployment_group_id = response.deployment_group.id.clone();

    if args.airgapped {
        drop(steps);
        let group_name = customer_environment_name(&name);
        let (manager_url, signing_key) =
            airgapped_manager(&ctx.base_url(), &response.token).await?;
        return register_airgapped(
            &args,
            &manager_url,
            &manager_url,
            &name,
            &group_name,
            &response.token,
            &signing_key,
            input_args.deployment_values,
        )
        .await;
    }

    if let Some(steps) = &steps {
        steps.complete(0, Some(deployment_group_id.clone()));
        steps.activate(1, Some("Generating deployment link".to_string()));
    }

    let deployment_link = response.deployment_link.clone();

    if args.json {
        print_json(&serde_json::json!({
            "deploymentGroupId": deployment_group_id,
            "name": name,
            "externalId": external_id,
            "deploymentLink": deployment_link,
            "setupItems": args.setup_items.iter().map(onboard_setup_item_name).collect::<Vec<_>>(),
            "readiness": "setup_pending",
            "nextAction": "Share deploymentLink with the customer's admin, then run `alien deployments ls` to check readiness.",
            "maxDeployments": args.max_deployments,
            "platforms": selected_platforms.iter().map(|platform| platform.as_str()).collect::<Vec<_>>(),
            "subdomain": public_subdomain,
        }))?;
        return Ok(());
    }

    if let Some(steps) = &steps {
        steps.complete(1, Some("Deployment link ready".to_string()));
    }
    drop(steps);

    println!("{}", success_line("Ready to deploy."));
    println!("{} {}", dim_label("Customer"), name);
    println!();
    println!(
        "{}",
        dim_label("Share this link with the customer's admin:")
    );
    println!("  {}", accent(&deployment_link));
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command("wait for customer setup, then run alien deployments ls")
    );

    Ok(())
}

fn onboard_setup_item_name(item: &OnboardSetupItem) -> &'static str {
    match item {
        OnboardSetupItem::Application => "application",
        OnboardSetupItem::Models => "models",
        OnboardSetupItem::Keys => "keys",
        OnboardSetupItem::Storage => "storage",
        OnboardSetupItem::Registry => "registry",
        OnboardSetupItem::Sandbox => "sandbox",
    }
}

struct ActiveReleaseStackInputs {
    supported_platforms: Vec<Platform>,
    inputs_by_platform: Vec<(Platform, Vec<StackInputDefinition>)>,
}

#[cfg(feature = "platform")]
struct AvailableSetup {
    items: Vec<OnboardSetupItem>,
    supported_platforms: Vec<Platform>,
}

#[cfg(feature = "platform")]
async fn fetch_available_setup(
    client: &alien_platform_api::Client,
    workspace_query: Option<&str>,
    project_id: &str,
) -> Result<AvailableSetup> {
    use alien_platform_api::types::DeploymentLinkSetupResponseSetupItemsItem;
    use alien_platform_api::SdkResultExt;

    let mut request = client
        .get_project_deployment_link_setup()
        .id_or_name(project_id);
    if let Some(workspace_query) = workspace_query {
        request = request.workspace(workspace_query);
    }
    let setup = request
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to fetch available customer infrastructure".to_string(),
            url: None,
        })?;

    let items = setup
        .setup_items
        .iter()
        .map(|item| match item {
            DeploymentLinkSetupResponseSetupItemsItem::Deployment => OnboardSetupItem::Application,
            DeploymentLinkSetupResponseSetupItemsItem::Models => OnboardSetupItem::Models,
            DeploymentLinkSetupResponseSetupItemsItem::Keys => OnboardSetupItem::Keys,
            DeploymentLinkSetupResponseSetupItemsItem::Bucket => OnboardSetupItem::Storage,
            DeploymentLinkSetupResponseSetupItemsItem::Registry => OnboardSetupItem::Registry,
            DeploymentLinkSetupResponseSetupItemsItem::Sandbox => OnboardSetupItem::Sandbox,
        })
        .collect();
    let supported_platforms = setup
        .supported_platforms
        .iter()
        .map(|platform| Platform::from_str(&platform.to_string()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|message| {
            AlienError::new(ErrorData::ValidationError {
                field: "project.setup".to_string(),
                message,
            })
        })?;
    Ok(AvailableSetup {
        items,
        supported_platforms,
    })
}

#[cfg(feature = "platform")]
fn validate_setup_items(
    requested: &[OnboardSetupItem],
    available: &[OnboardSetupItem],
) -> Result<()> {
    if requested.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "setup-items".to_string(),
            message: "Select at least one of application, models, keys, storage, or registry."
                .to_string(),
        }));
    }
    for item in requested {
        if !available.contains(item) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "setup-items".to_string(),
                message: format!(
                    "{} is not configured for this Project. Configure it in the Dashboard before creating the setup link.",
                    item.to_possible_value()
                        .expect("ValueEnum variants have names")
                        .get_name()
                ),
            }));
        }
    }
    Ok(())
}

#[cfg(feature = "platform")]
fn platform_onboard_deployment_setup_config(
    environment_variables: Vec<alien_platform_api::types::EnvironmentVariableConfig>,
    platforms: &[Platform],
    public_subdomain: Option<&str>,
    customer_name: &str,
    includes_application: bool,
) -> Result<alien_platform_api::types::DeploymentSetupConfigInput> {
    use alien_platform_api::types;

    let public_subdomain = public_subdomain
        .map(|value| {
            value
                .parse::<types::DeploymentSetupConfigInputPublicSubdomain>()
                .map_err(|error| {
                    AlienError::new(ErrorData::ValidationError {
                        field: "subdomain".to_string(),
                        message: format!("Invalid public subdomain '{value}': {error}"),
                    })
                })
        })
        .transpose()?;

    let allowed_platforms = if platforms.is_empty() {
        vec![
            types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Aws,
            types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Gcp,
            types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Azure,
            types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Kubernetes,
            types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Machines,
            types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Local,
        ]
    } else {
        platforms
            .iter()
            .map(platform_to_setup_policy_platform)
            .collect::<Result<Vec<_>>>()
            .expect("selected onboarding platforms are validated before setup config creation")
    };

    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "customerName".to_string(),
        serde_json::Value::String(customer_name.to_string()),
    );

    Ok(types::DeploymentSetupConfigInput {
        metadata: Some(types::DeploymentSetupMetadata(metadata)),
        public_subdomain,
        validated_release_selection: None,
        policy: Some(types::DeploymentSetupConfigInputPolicy {
            allow_release_pinning: None,
            allowed_ai_providers: Vec::new(),
            allowed_platforms,
            allowed_kubernetes_base_platforms: vec![
                types::DeploymentSetupConfigInputPolicyAllowedKubernetesBasePlatformsItem::Aws,
                types::DeploymentSetupConfigInputPolicyAllowedKubernetesBasePlatformsItem::Gcp,
                types::DeploymentSetupConfigInputPolicyAllowedKubernetesBasePlatformsItem::Azure,
                types::DeploymentSetupConfigInputPolicyAllowedKubernetesBasePlatformsItem::OnPrem,
            ],
            allowed_kubernetes_cluster_sources: vec![
                types::KubernetesClusterSource::Create,
                types::KubernetesClusterSource::Existing,
            ],
            allowed_setup_methods: vec![
                types::DeploymentSetupMethod::Cloudformation,
                types::DeploymentSetupMethod::GoogleOauth,
                types::DeploymentSetupMethod::Terraform,
                types::DeploymentSetupMethod::Helm,
                types::DeploymentSetupMethod::Cli,
                types::DeploymentSetupMethod::Manual,
            ],
            stack_settings: Some(types::DeploymentSetupStackSettingsPolicy {
                allow_custom_registry: Some(true),
                allow_external_bindings: Some(true),
                allowed_deployment_models: vec![
                    types::DeploymentSetupStackSettingsPolicyAllowedDeploymentModelsItem::Push,
                    types::DeploymentSetupStackSettingsPolicyAllowedDeploymentModelsItem::Pull,
                    types::DeploymentSetupStackSettingsPolicyAllowedDeploymentModelsItem::Airgapped,
                ],
                // A link with only capabilities installs a built-in package that registers no
                // network, telemetry off, approval-required updates and heartbeats on. Allowing
                // any other mode lets the setup request one that package does not accept.
                allowed_heartbeats_modes: if includes_application {
                    vec![
                        types::DeploymentSetupStackSettingsPolicyAllowedHeartbeatsModesItem::On,
                        types::DeploymentSetupStackSettingsPolicyAllowedHeartbeatsModesItem::Off,
                    ]
                } else {
                    vec![types::DeploymentSetupStackSettingsPolicyAllowedHeartbeatsModesItem::On]
                },
                allowed_network_modes: if includes_application {
                    vec![
                        types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::None,
                        types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::Create,
                        types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::Default,
                        types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::Byo,
                    ]
                } else {
                    vec![types::DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem::None]
                },
                allowed_telemetry_modes: if includes_application {
                    vec![
                        types::DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem::Off,
                        types::DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem::Auto,
                        types::DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem::ApprovalRequired,
                    ]
                } else {
                    vec![types::DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem::Off]
                },
                allowed_updates_modes: if includes_application {
                    vec![
                        types::DeploymentSetupStackSettingsPolicyAllowedUpdatesModesItem::Auto,
                        types::DeploymentSetupStackSettingsPolicyAllowedUpdatesModesItem::ApprovalRequired,
                    ]
                } else {
                    vec![types::DeploymentSetupStackSettingsPolicyAllowedUpdatesModesItem::ApprovalRequired]
                },
                defaults: None,
            }),
        }),
        environment_variables,
        validated_release_selection: None,
    })
}

#[cfg(feature = "platform")]
fn validate_public_subdomain(value: &str) -> Result<String> {
    let valid = !value.is_empty()
        && value.len() <= 63
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');

    if valid {
        Ok(value.to_string())
    } else {
        Err(AlienError::new(ErrorData::ValidationError {
            field: "subdomain".to_string(),
            message: "Subdomain must be 1-63 characters of lowercase letters, numbers, or hyphens, and cannot start or end with a hyphen.".to_string(),
        }))
    }
}

#[cfg(feature = "platform")]
fn platform_to_setup_policy_platform(
    platform: &Platform,
) -> Result<alien_platform_api::types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem> {
    use alien_platform_api::types::DeploymentSetupConfigInputPolicyAllowedPlatformsItem;

    match platform {
        Platform::Aws => Ok(DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Aws),
        Platform::Gcp => Ok(DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Gcp),
        Platform::Azure => Ok(DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Azure),
        Platform::Kubernetes => Ok(DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Kubernetes),
        Platform::Machines => Ok(DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Machines),
        Platform::Local => Ok(DeploymentSetupConfigInputPolicyAllowedPlatformsItem::Local),
        Platform::Test => Err(AlienError::new(ErrorData::ValidationError {
            field: "platforms".to_string(),
            message: "`test` is not a deployment-link platform. Use aws, gcp, azure, kubernetes, machines, or local.".to_string(),
        })),
    }
}

#[cfg(feature = "platform")]
async fn fetch_active_release_stack_inputs(
    client: &alien_platform_api::Client,
    workspace_query: Option<&str>,
    project_id: &str,
) -> Result<ActiveReleaseStackInputs> {
    use alien_platform_api::SdkResultExt;

    let mut request = client.list_releases();
    if let Some(workspace_query) = workspace_query {
        request = request.workspace(workspace_query);
    }

    let releases = request
        .project(project_id)
        .limit(std::num::NonZeroU64::new(1).expect("1 is non-zero"))
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to fetch active release stack inputs".to_string(),
            url: None,
        })?;

    let Some(release) = releases.items.first() else {
        return Ok(ActiveReleaseStackInputs {
            supported_platforms: Vec::new(),
            inputs_by_platform: Vec::new(),
        });
    };

    let Some(stack_by_platform) = release.stack.as_ref() else {
        return Ok(ActiveReleaseStackInputs {
            supported_platforms: Vec::new(),
            inputs_by_platform: Vec::new(),
        });
    };

    let stack_values = [
        (Platform::Aws, stack_by_platform.aws.as_ref()),
        (Platform::Gcp, stack_by_platform.gcp.as_ref()),
        (Platform::Azure, stack_by_platform.azure.as_ref()),
        (Platform::Kubernetes, stack_by_platform.kubernetes.as_ref()),
        (Platform::Machines, stack_by_platform.machines.as_ref()),
        (Platform::Local, stack_by_platform.local.as_ref()),
    ];
    active_release_stack_inputs_from_values(&stack_values)
}

fn active_release_stack_inputs_from_values(
    stack_values: &[(Platform, Option<&serde_json::Value>)],
) -> Result<ActiveReleaseStackInputs> {
    let mut inputs_by_platform = Vec::new();
    for (platform, stack_value) in stack_values
        .iter()
        .filter_map(|(platform, stack)| stack.map(|stack_value| (platform, stack_value)))
    {
        let stack: Stack = serde_json::from_value(stack_value.clone()).map_err(|error| {
            AlienError::new(ErrorData::ValidationError {
                field: "release.stack".to_string(),
                message: format!("Failed to parse release stack input metadata: {error}"),
            })
        })?;
        inputs_by_platform.push((platform.clone(), stack.inputs));
    }

    Ok(ActiveReleaseStackInputs {
        supported_platforms: inputs_by_platform
            .iter()
            .map(|(platform, _)| platform.clone())
            .collect(),
        inputs_by_platform,
    })
}

fn select_onboard_platforms(
    requested: &[String],
    supported: &[Platform],
    json: bool,
) -> Result<Vec<Platform>> {
    if requested.is_empty() && supported.is_empty() {
        return Ok(Vec::new());
    }

    let default = supported
        .iter()
        .map(|platform| platform.as_str())
        .collect::<Vec<_>>()
        .join(",");

    let raw_platforms = if !requested.is_empty() {
        requested.to_vec()
    } else if !json && can_prompt() && supported.len() > 1 {
        prompt_text("Platforms", Some(&default))?
            .split(',')
            .map(str::trim)
            .filter(|platform| !platform.is_empty())
            .map(ToString::to_string)
            .collect()
    } else {
        supported
            .iter()
            .map(|platform| platform.as_str().to_string())
            .collect()
    };

    let mut selected = Vec::new();
    for raw in raw_platforms {
        let platform = Platform::from_str(&raw).map_err(|message| {
            AlienError::new(ErrorData::ValidationError {
                field: "platforms".to_string(),
                message,
            })
        })?;
        platform_to_setup_policy_platform(&platform)?;
        if !supported.is_empty() && !supported.contains(&platform) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platforms".to_string(),
                message: format!(
                    "Platform '{}' is not in the active release. Available platforms: {}.",
                    platform.as_str(),
                    supported
                        .iter()
                        .map(|platform| platform.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }));
        }
        if !selected.contains(&platform) {
            selected.push(platform);
        }
    }

    if selected.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "platforms".to_string(),
            message: "Select at least one platform for the deployment link.".to_string(),
        }));
    }

    Ok(selected)
}

fn developer_inputs_for_platforms(
    release_inputs: &ActiveReleaseStackInputs,
    platforms: &[Platform],
) -> Vec<StackInputDefinition> {
    inputs_for_platforms(release_inputs, platforms, |input| {
        input.provided_by.contains(&StackInputProvider::Developer)
    })
}

/// Inputs only the deployer answers. A connected install answers them on its
/// setup page; an air-gapped site has none, so onboarding answers them.
fn deployer_only_inputs_for_platforms(
    release_inputs: &ActiveReleaseStackInputs,
    platforms: &[Platform],
) -> Vec<StackInputDefinition> {
    inputs_for_platforms(release_inputs, platforms, |input| {
        input.provided_by.contains(&StackInputProvider::Deployer)
            && !input.provided_by.contains(&StackInputProvider::Developer)
    })
}

fn inputs_for_platforms(
    release_inputs: &ActiveReleaseStackInputs,
    platforms: &[Platform],
    include: impl Fn(&StackInputDefinition) -> bool,
) -> Vec<StackInputDefinition> {
    let selected = if platforms.is_empty() {
        &release_inputs.supported_platforms
    } else {
        platforms
    };
    let mut inputs_by_id = HashMap::new();
    for (platform, inputs) in &release_inputs.inputs_by_platform {
        if !selected.contains(platform) {
            continue;
        }
        for input in inputs {
            if !include(input) {
                continue;
            }
            if input
                .platforms
                .as_ref()
                .is_some_and(|input_platforms| !input_platforms.contains(platform))
            {
                continue;
            }
            inputs_by_id
                .entry(input.id.clone())
                .or_insert(input.clone());
        }
    }
    let mut inputs = inputs_by_id.into_values().collect::<Vec<_>>();
    inputs.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
    inputs
}

/// `--input` / `--secret-input` for an air-gapped site, split by who answers
/// each input: deployer inputs register with the site's deployment, the rest
/// stay with its deployment group like any other link.
#[derive(Debug, Default, PartialEq)]
struct AirgappedInputArgs {
    group_inputs: Vec<String>,
    group_secret_inputs: Vec<String>,
    deployment_values: serde_json::Map<String, serde_json::Value>,
}

fn split_airgapped_inputs(
    deployer_inputs: &[StackInputDefinition],
    input_values: &[String],
    secret_input_values: &[String],
    json: bool,
) -> Result<AirgappedInputArgs> {
    let mut split = AirgappedInputArgs::default();
    let args = input_values.iter().map(|arg| (arg, "--input")).chain(
        secret_input_values
            .iter()
            .map(|arg| (arg, "--secret-input")),
    );
    for (arg, flag) in args {
        let (id, value) = parse_stack_input_arg(arg, flag)?;
        let Some(input) = deployer_inputs.iter().find(|input| input.id == id) else {
            match flag {
                "--input" => split.group_inputs.push(arg.clone()),
                _ => split.group_secret_inputs.push(arg.clone()),
            }
            continue;
        };
        if is_deployer_secret_input(input) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: deployer_secret_value_refusal(&input.label),
            }));
        }
        split
            .deployment_values
            .insert(id, stack_input_json_value(input, &value)?);
    }

    for input in deployer_inputs
        .iter()
        .filter(|input| input.required && !input.is_generated() && !is_deployer_secret_input(input))
    {
        if split.deployment_values.contains_key(&input.id) {
            continue;
        }
        if json || !can_prompt() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!(
                    "Missing deployer input: {}. An air-gapped site has no setup page to \
                     answer it: pass --input {}=...",
                    input.label, input.id
                ),
            }));
        }
        let value = prompt_text(&input.label, input.placeholder.as_deref())?;
        split
            .deployment_values
            .insert(input.id.clone(), stack_input_json_value(input, &value)?);
    }
    Ok(split)
}

fn airgapped_or_group_inputs(
    args: &OnboardArgs,
    release_inputs: &ActiveReleaseStackInputs,
    selected_platforms: &[Platform],
) -> Result<AirgappedInputArgs> {
    if !args.airgapped {
        return Ok(AirgappedInputArgs {
            group_inputs: args.input_values.clone(),
            group_secret_inputs: args.secret_input_values.clone(),
            deployment_values: serde_json::Map::new(),
        });
    }
    split_airgapped_inputs(
        &deployer_only_inputs_for_platforms(release_inputs, selected_platforms),
        &args.input_values,
        &args.secret_input_values,
        args.json,
    )
}

/// The value as the manager's `/v1/initialize` takes it: plain JSON, the same
/// shape a Helm install's `inputValues` carries.
fn stack_input_json_value(input: &StackInputDefinition, value: &str) -> Result<serde_json::Value> {
    serde_json::to_value(parse_stack_input_value(input, value)?)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: input.id.clone(),
            message: format!("{} could not be encoded", input.label),
        })
}

fn collect_stack_input_values(
    inputs: &[StackInputDefinition],
    input_values: &[String],
    secret_input_values: &[String],
    selected_platforms: &[Platform],
    json: bool,
) -> Result<alien_platform_api::types::StackInputValuesRequest> {
    use alien_platform_api::types;

    let mut raw_values = HashMap::<String, String>::new();
    for input in input_values {
        let (id, value) = parse_stack_input_arg(input, "--input")?;
        raw_values.insert(id, value);
    }
    for input in secret_input_values {
        let (id, value) = parse_stack_input_arg(input, "--secret-input")?;
        raw_values.insert(id, value);
    }

    for id in raw_values.keys() {
        let Some(input) = inputs.iter().find(|input| input.id == *id) else {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("Unknown or unavailable developer stack input '{id}'."),
            }));
        };
        if let Some(input_platforms) = &input.platforms {
            if let Some(unavailable_platform) = selected_platforms
                .iter()
                .find(|platform| !input_platforms.contains(platform))
            {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "input".to_string(),
                    message: format!(
                        "Developer stack input '{}' is not available for platform '{}'. Create a separate deployment link or narrow this link with --platforms {}.",
                        id,
                        unavailable_platform.as_str(),
                        input_platforms
                            .iter()
                            .map(|platform| platform.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                }));
            }
        }
    }

    // Alien generates a value for generated inputs when none is passed. A
    // secret the deployer may also provide is optional here: without a
    // developer value, the deployer writes it into their own secret store.
    for input in inputs
        .iter()
        .filter(|input| input.required && !input.is_generated() && !is_deployer_secret_input(input))
    {
        if !raw_values.contains_key(&input.id) {
            if json || !can_prompt() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "input".to_string(),
                    message: format!(
                        "Missing developer input: {}. Pass {} {}=...{}",
                        input.label,
                        if matches!(input.kind, StackInputKind::Secret) {
                            "--secret-input"
                        } else {
                            "--input"
                        },
                        input.id,
                        narrowing_hint(input, selected_platforms)
                    ),
                }));
            }
            let value = prompt_text(&input.label, input.placeholder.as_deref())?;
            raw_values.insert(input.id.clone(), value);
        }
    }

    let mut values = HashMap::<String, types::StackInputValueRequest>::new();
    for input in inputs {
        let Some(value) = raw_values.get(&input.id) else {
            continue;
        };
        values.insert(input.id.clone(), parse_stack_input_value(input, value)?);
    }

    Ok(types::StackInputValuesRequest(values))
}

fn narrowing_hint(input: &StackInputDefinition, selected_platforms: &[Platform]) -> String {
    let Some(input_platforms) = &input.platforms else {
        return ".".to_string();
    };
    let alternatives = selected_platforms
        .iter()
        .filter(|platform| !input_platforms.contains(platform))
        .map(|platform| platform.as_str())
        .collect::<Vec<_>>();
    if alternatives.is_empty() {
        ".".to_string()
    } else {
        format!(
            ", or narrow the link with --platforms {}.",
            alternatives.join(",")
        )
    }
}

fn parse_stack_input_arg(input: &str, flag: &str) -> Result<(String, String)> {
    let Some((id, value)) = input.split_once('=') else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: '{input}'. Use id=value"),
        }));
    };
    if id.trim().is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: input id is required"),
        }));
    }
    Ok((id.trim().to_string(), value.to_string()))
}

fn parse_stack_input_value(
    input: &StackInputDefinition,
    value: &str,
) -> Result<alien_platform_api::types::StackInputValueRequest> {
    use alien_platform_api::types;

    match input.kind {
        StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum => {
            validate_string_stack_input(input, value)?;
            Ok(types::StackInputValueRequest::Variant0(value.to_string()))
        }
        StackInputKind::Number => {
            let number = value.parse::<f64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a number.", input.label),
                })
            })?;
            Ok(types::StackInputValueRequest::Variant1(number))
        }
        StackInputKind::Integer => {
            let number = value.parse::<i64>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be a whole number.", input.label),
                })
            })?;
            Ok(types::StackInputValueRequest::Variant1(number as f64))
        }
        StackInputKind::Boolean => {
            let parsed = value.parse::<bool>().map_err(|_| {
                AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be true or false.", input.label),
                })
            })?;
            Ok(types::StackInputValueRequest::Variant2(parsed))
        }
        StackInputKind::StringList => {
            let values = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            Ok(types::StackInputValueRequest::Variant3(values))
        }
    }
}

fn validate_string_stack_input(input: &StackInputDefinition, value: &str) -> Result<()> {
    if let Some(validation) = &input.validation {
        if let Some(values) = &validation.values {
            if !values.iter().any(|candidate| candidate == value) {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} must be one of: {}.", input.label, values.join(", ")),
                }));
            }
        }
        if let Some(min) = validation.min_length {
            if value.len() < min as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too short.", input.label),
                }));
            }
        }
        if let Some(max) = validation.max_length {
            if value.len() > max as usize {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: input.id.clone(),
                    message: format!("{} is too long.", input.label),
                }));
            }
        }
    }
    Ok(())
}

fn print_required_developer_inputs(inputs: &[StackInputDefinition]) {
    let required = inputs
        .iter()
        .filter(|input| input.required && !input.is_generated() && !is_deployer_secret_input(input))
        .collect::<Vec<_>>();
    if required.is_empty() {
        return;
    }

    println!("{}", dim_label("Required developer inputs"));
    for input in required {
        let kind = if matches!(input.kind, StackInputKind::Secret) {
            "secret"
        } else {
            "plain"
        };
        println!("  {}  required  {}", input.label, kind);
    }
    println!();
}

#[cfg(feature = "platform")]
fn platform_setup_environment_variables(
    variables: &[super::CliEnvVar],
) -> Result<Vec<alien_platform_api::types::EnvironmentVariableConfig>> {
    use alien_platform_api::types;

    variables
        .iter()
        .map(|variable| {
            let target_resources = variable
                .target_resources
                .as_ref()
                .map(|targets| {
                    targets
                        .iter()
                        .map(|target| {
                            types::EnvironmentVariableConfigTargetResourcesItem::try_from(
                                target.clone(),
                            )
                            .into_alien_error()
                            .context(ErrorData::ValidationError {
                                field: if variable.is_secret {
                                    "secret".to_string()
                                } else {
                                    "env".to_string()
                                },
                                message: format!(
                                    "Invalid target resource pattern in {}: '{}'. Must match pattern ^[a-zA-Z0-9_-]+(\\*)?$",
                                    if variable.is_secret { "--secret" } else { "--env" },
                                    target
                                ),
                            })
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?;

            Ok(types::EnvironmentVariableConfig {
                name: types::EnvironmentVariableConfigName::try_from(variable.name.clone())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: if variable.is_secret {
                            "secret".to_string()
                        } else {
                            "env".to_string()
                        },
                        message: format!(
                            "Invalid variable name in {}: '{}'. Must match pattern ^[A-Z_][A-Z0-9_]*$",
                            if variable.is_secret { "--secret" } else { "--env" },
                            variable.name
                        ),
                    })?,
                value: types::EnvironmentVariableConfigValue::try_from(variable.value.clone())
                    .into_alien_error()
                    .context(ErrorData::ValidationError {
                        field: if variable.is_secret {
                            "secret".to_string()
                        } else {
                            "env".to_string()
                        },
                        message: format!(
                            "Invalid variable value for {} '{}'. Must not exceed 10000 characters",
                            if variable.is_secret { "--secret" } else { "--env" },
                            variable.name
                        ),
                    })?,
                type_: if variable.is_secret {
                    types::EnvironmentVariableType::Secret
                } else {
                    types::EnvironmentVariableType::Plain
                },
                target_resources,
            })
        })
        .collect()
}

/// Self-hosted and local managers: create the deployment group on the
/// manager and print the setup command for each selected platform.
async fn onboard_standalone(args: OnboardArgs, ctx: ExecutionMode, name: String) -> Result<()> {
    use alien_manager_api::types::{
        CreateDeploymentGroupRequest, EnvironmentVariable, EnvironmentVariableType,
    };
    use alien_manager_api::SdkResultExt;

    let (project_id, _project_link) = ctx.resolve_project(None, !args.json).await?;
    let mgr = ctx.resolve_manager(&project_id, "local").await?;

    // Developer inputs come from the latest release, like a setup link.
    let latest = mgr
        .client
        .get_latest_release()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to fetch the latest release. Run `alien release` first.".to_string(),
            url: None,
        })?;
    let stack_by_platform = serde_json::to_value(&latest.stack)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "release stacks".to_string(),
        })?;
    let stack_values: Vec<(Platform, Option<&serde_json::Value>)> = [
        Platform::Aws,
        Platform::Gcp,
        Platform::Azure,
        Platform::Kubernetes,
        Platform::Machines,
        Platform::Local,
    ]
    .into_iter()
    .map(|platform| {
        let value = stack_by_platform
            .get(platform.as_str())
            .filter(|value| !value.is_null());
        (platform, value)
    })
    .collect();
    let release_inputs = active_release_stack_inputs_from_values(&stack_values)?;
    let selected_platforms = select_onboard_platforms(
        &args.platforms,
        &release_inputs.supported_platforms,
        args.json,
    )?;
    let developer_inputs = developer_inputs_for_platforms(&release_inputs, &selected_platforms);
    let input_args = airgapped_or_group_inputs(&args, &release_inputs, &selected_platforms)?;
    let input_values = collect_stack_input_values(
        &developer_inputs,
        &input_args.group_inputs,
        &input_args.group_secret_inputs,
        &selected_platforms,
        args.json,
    )?;
    let environment_variables =
        crate::parse_env_and_secret_vars(&args.env_vars, &args.secret_vars)?
            .into_iter()
            .map(|variable| EnvironmentVariable {
                name: variable.name,
                value: variable.value,
                type_: if variable.is_secret {
                    EnvironmentVariableType::Secret
                } else {
                    EnvironmentVariableType::Plain
                },
                target_resources: variable.target_resources,
            })
            .collect::<Vec<_>>();
    let manager = fetch_manager_info(&mgr).await?;

    if !args.json {
        let platforms_label = selected_platforms
            .iter()
            .map(|platform| platform.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{}",
            contextual_heading("Onboarding", &name, &[("platforms", &platforms_label)])
        );
        print_required_developer_inputs(&developer_inputs);
    }
    let steps = if args.json {
        None
    } else {
        let steps = FixedSteps::new(&["Create deployment group", "Generate deployment token"]);
        steps.activate(0, Some(name.clone()));
        Some(steps)
    };

    let deployment_group_name = customer_environment_name(&name);
    let input_values = match serde_json::to_value(&input_values)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "stack input values".to_string(),
        })? {
        serde_json::Value::Object(values) => values,
        other => {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "input".to_string(),
                message: format!("stack input values must be an object, got {other}"),
            }))
        }
    };
    let response = mgr
        .client
        .create_deployment_group()
        .body(CreateDeploymentGroupRequest {
            name: deployment_group_name.clone(),
            max_deployments: Some(args.max_deployments as i64),
            input_values,
            environment_variables,
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group".to_string(),
            url: None,
        })?;
    let deployment_group_id = response.id.clone();

    if let Some(steps) = &steps {
        steps.complete(0, Some(deployment_group_id.clone()));
        steps.activate(1, Some("Creating deployment token".to_string()));
    }

    let token = mgr
        .client
        .create_deployment_group_token()
        .id(&deployment_group_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to create deployment group token".to_string(),
            url: None,
        })?
        .token
        .clone();

    if args.airgapped {
        let signing_key = manager.bundle_signing_key.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "This manager doesn't sign air-gapped bundles".to_string(),
            })
        })?;
        return register_airgapped(
            &args,
            &mgr.manager_url,
            &manager.url,
            &name,
            &deployment_group_name,
            &token,
            &signing_key,
            input_args.deployment_values,
        )
        .await;
    }

    let kubernetes_stack = stack_values
        .iter()
        .find(|(platform, _)| *platform == Platform::Kubernetes)
        .and_then(|(_, value)| *value)
        .filter(|_| selected_platforms.contains(&Platform::Kubernetes))
        .map(|value| serde_json::from_value::<Stack>(value.clone()))
        .transpose()
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "parse".to_string(),
            reason: "release Kubernetes stack".to_string(),
        })?;
    let helm = kubernetes_stack
        .as_ref()
        .filter(|_| manager.capabilities.charts)
        .map(|stack| {
            HelmInstall::new(
                stack,
                &manager.url,
                &manager.registry_host,
                &deployment_group_name,
            )
        });
    let cli_platforms: Vec<&str> = selected_platforms
        .iter()
        .filter(|platform| **platform != Platform::Kubernetes || helm.is_none())
        .map(|platform| platform.as_str())
        .collect();

    if args.json {
        print_json(&serde_json::json!({
            "deploymentGroupId": deployment_group_id,
            "name": name,
            "token": token,
            "maxDeployments": args.max_deployments,
            "managerUrl": manager.url,
            "platforms": selected_platforms.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            "helm": helm.as_ref().map(|helm| serde_json::json!({
                "chart": helm.chart,
                "release": helm.release,
                "namespace": helm.release,
                "command": helm.command(),
                "values": helm.values_example(),
            })),
        }))?;
        return Ok(());
    }

    if let Some(steps) = &steps {
        steps.complete(1, Some("Deployment token ready".to_string()));
    }
    drop(steps);

    println!("{}", success_line("Ready to deploy."));
    println!("{} {}", dim_label("Customer"), name);
    println!("{} {}", dim_label("Token"), accent(&token));

    if let Some(helm) = &helm {
        if helm.needs_https() {
            println!();
            println!(
                "{} {} is served over plain HTTP. Helm and Kubernetes nodes pull charts and images over HTTPS, so put the manager behind TLS before a customer installs.",
                dim_label("Warning"),
                manager.url
            );
        }
        println!();
        println!("{}", dim_label("Send to the customer's Kubernetes admin:"));
        println!();
        for line in helm.command().lines() {
            println!("  {line}");
        }
        println!();
        println!(
            "{}",
            dim_label("values.yaml connects the environment's own infrastructure:")
        );
        println!();
        for line in helm.values_example().lines() {
            println!("  {line}");
        }
    }
    if !cli_platforms.is_empty() {
        println!();
        println!("{}", dim_label("Send to the customer's cloud admin:"));
        println!("  curl -fsSL {}/install | sh -s -- deploy \\", manager.url);
        println!("    --token {} \\", token);
        println!("    --name <deployment-name> \\");
        println!("    --platform <{}> \\", cli_platforms.join("|"));
        println!("    --manager-url {}", manager.url);
    }
    println!();
    println!(
        "{} {}",
        dim_label("Next"),
        command("wait for customer setup, then run alien deployments ls")
    );

    Ok(())
}

/// Register the deployment for an environment that can't reach the manager,
/// the way its Operator would on first contact.
/// The manager the project's Kubernetes deployments use, and its bundle key,
/// as the new customer's setup token sees them.
#[cfg(feature = "platform")]
async fn airgapped_manager(platform_url: &str, group_token: &str) -> Result<(String, String)> {
    let failed =
        |message: String| AlienError::new(ErrorData::ApiRequestFailed { message, url: None });
    let get = |url: String| async move {
        let response = reqwest::Client::new()
            .get(&url)
            .bearer_auth(group_token)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("GET {url}"),
                url: None,
            })?;
        let status = response.status();
        let body: serde_json::Value =
            response
                .json()
                .await
                .into_alien_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("reading {url}"),
                    url: None,
                })?;
        if !status.is_success() {
            return Err(failed(format!(
                "{url} returned {status}: {}",
                body["message"].as_str().unwrap_or("no message")
            )));
        }
        Ok(body)
    };
    let info = get(format!(
        "{}/v1/deployment-info?platform=kubernetes",
        platform_url.trim_end_matches('/')
    ))
    .await?;
    let manager_url = info["installContext"]["targets"]["kubernetes"]["managerUrl"]
        .as_str()
        .ok_or_else(|| failed("The project has no manager for Kubernetes deployments".to_string()))?
        .trim_end_matches('/')
        .to_string();
    let manager = get(format!("{manager_url}/v1/manager")).await?;
    let signing_key = manager["bundleSigningKey"]
        .as_str()
        .ok_or_else(|| failed(format!("{manager_url} doesn't sign air-gapped bundles")))?
        .to_string();
    Ok((manager_url, signing_key))
}

async fn register_airgapped(
    args: &OnboardArgs,
    manager_url: &str,
    public_manager_url: &str,
    name: &str,
    group_name: &str,
    group_token: &str,
    signing_key: &str,
    input_values: serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    let response = reqwest::Client::new()
        .post(format!(
            "{}/v1/initialize",
            manager_url.trim_end_matches('/')
        ))
        .bearer_auth(group_token)
        // The site installs the project's Helm chart, so it registers as the
        // chart's Operator does: the `deployment` setup item, set up with Helm.
        // Without them the platform can't pick the setup or tell who owns the
        // cluster.
        .json(&serde_json::json!({
            "name": group_name,
            "platform": "kubernetes",
            "initialDesiredRelease": "active",
            "setupItem": "deployment",
            "setupMethod": "helm",
            "inputValues": input_values,
        }))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to register the air-gapped deployment".to_string(),
            url: None,
        })?;
    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "Registering the air-gapped deployment failed ({}): {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ),
            url: None,
        }));
    }
    let registered: serde_json::Value =
        response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "Failed to read the registration".to_string(),
                url: None,
            })?;
    let deployment_id = registered["deploymentId"].as_str().unwrap_or_default();
    // The site's token: `alien-deploy sync` uses it to download updates and
    // send reports, and it can do nothing else.
    let site_token = registered["token"].as_str().ok_or_else(|| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: "The manager registered the deployment but returned no token".to_string(),
            url: None,
        })
    })?;
    let start = format!("alien-deploy sync --token {site_token} --manager {public_manager_url}");
    if args.json {
        return print_json(&serde_json::json!({
            "name": name,
            "deploymentId": deployment_id,
            "reference": format!("{group_name}/{group_name}"),
            "airgapped": true,
            "token": site_token,
            "managerUrl": public_manager_url,
            "bundleSigningKey": signing_key,
            "command": start,
        }));
    }
    println!(
        "{}",
        success_line(&format!("Registered {name} for air-gapped updates."))
    );
    println!();
    println!("{}", dim_label(&format!("Send {name}'s admin:")));
    println!(
        "  {} {}   {}",
        dim_label("Token"),
        accent(site_token),
        dim_label("keep on the online machine only")
    );
    println!(
        "  {} {}   {}",
        dim_label("Key  "),
        accent(signing_key),
        dim_label("confirm it with them by phone or email")
    );
    println!("  {} {}", dim_label("Start"), command(&start));
    println!();
    println!(
        "{}",
        dim_label("They run `alien-deploy sync` on both sides of the gap. Every `alien release` reaches the site on its next sync.")
    );
    Ok(())
}

/// Manager identity as deployments see it (`GET /v1/manager`).
struct ManagerInfo {
    url: String,
    registry_host: String,
    capabilities: ManagerCapabilities,
    /// Public key air-gapped sites verify bundles with.
    bundle_signing_key: Option<String>,
}

struct ManagerCapabilities {
    charts: bool,
}

async fn fetch_manager_info(mgr: &crate::execution_context::ManagerContext) -> Result<ManagerInfo> {
    use alien_manager_api::SdkResultExt;

    let info = mgr
        .client
        .manager_info()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to read manager information".to_string(),
            url: None,
        })?;
    Ok(ManagerInfo {
        url: info.url.trim_end_matches('/').to_string(),
        registry_host: info.registry_host.clone(),
        capabilities: ManagerCapabilities {
            charts: info.capabilities.charts,
        },
        bundle_signing_key: info.bundle_signing_key.clone(),
    })
}

/// The Helm command a customer's Kubernetes admin runs once.
struct HelmInstall {
    chart: String,
    /// The manager is served over plain HTTP.
    plain_http: bool,
    release: String,
    customer: String,
    infrastructure: Vec<(String, String)>,
}

impl HelmInstall {
    fn new(stack: &Stack, manager_url: &str, registry_host: &str, customer: &str) -> Self {
        let infrastructure = stack
            .resources()
            .filter(|(_, entry)| {
                matches!(
                    entry.config.resource_type().as_ref(),
                    "storage" | "kv" | "queue" | "vault"
                )
            })
            .map(|(id, entry)| {
                (
                    id.clone(),
                    entry.config.resource_type().as_ref().to_string(),
                )
            })
            .collect();
        Self {
            chart: format!("oci://{registry_host}/charts/{}", stack.id()),
            plain_http: manager_url.starts_with("http://"),
            release: stack.id().to_string(),
            customer: customer.to_string(),
            infrastructure,
        }
    }

    fn registry(&self) -> &str {
        self.chart
            .trim_start_matches("oci://")
            .split('/')
            .next()
            .unwrap_or_default()
    }

    /// Helm reaches plain-HTTP registries only on the local machine; anywhere
    /// else it (and every Kubernetes node) needs the manager behind HTTPS.
    fn needs_https(&self) -> bool {
        let host = self
            .registry()
            .rsplit_once(':')
            .map_or(self.registry(), |(host, _)| host);
        self.plain_http && !matches!(host, "localhost" | "127.0.0.1")
    }

    /// The commands for the customer's admin. The token is read once with
    /// `read -s` and piped into Helm, so it stays out of shell history and
    /// process arguments.
    fn command(&self) -> String {
        let (login_flags, install_flags) = if self.plain_http {
            (" --insecure", " --plain-http")
        } else {
            ("", "")
        };
        let values = if self.infrastructure.is_empty() {
            ""
        } else {
            " \\\n  --values values.yaml"
        };
        format!(
            "read -rs ALIEN_TOKEN  # paste {customer}'s token, then Enter\n\
             printf '%s' \"$ALIEN_TOKEN\" | helm registry login {registry}{login_flags} --username {customer} --password-stdin\n\n\
             printf 'management:\\n  token: %s\\n' \"$ALIEN_TOKEN\" | \\\n\
             helm install {release} {chart}{install_flags} \\\n  \
             --namespace {release} --create-namespace \\\n  \
             --set management.name={customer}{values} \\\n  \
             --values -",
            registry = self.registry(),
            release = self.release,
            chart = self.chart,
            customer = self.customer,
        )
    }

    fn values_example(&self) -> String {
        if self.infrastructure.is_empty() {
            return String::new();
        }
        let mut yaml = String::from("infrastructure:\n");
        for (id, resource_type) in &self.infrastructure {
            match resource_type.as_str() {
                "storage" => yaml.push_str(&format!(
                    "  {id}:\n    type: storage\n    service: s3\n    bucketName: <bucket>\n    # S3-compatible stores (MinIO, Ceph, ...): endpoint and keys.\n    # Omit them to use the pod's AWS identity with Amazon S3.\n    endpoint: https://<s3-endpoint>\n    accessKeyId: <access-key-id>\n    secretAccessKey: <secret-access-key>\n"
                )),
                "kv" => yaml.push_str(&format!(
                    "  {id}:\n    type: kv\n    service: redis\n    connectionUrl: redis://<host>:6379\n"
                )),
                other => yaml.push_str(&format!(
                    "  {id}:\n    type: {other}\n    # See `helm show values` for this resource's fields.\n"
                )),
            }
        }
        yaml
    }
}

/// Turn a customer-facing name into the stable internal Deployment Group name.
///
/// The CLI intentionally accepts friendly names such as `Acme Corp`. Platform
/// Deployment Group names are URL-safe identifiers, so exposing that storage
/// constraint in the command's primary argument would make onboarding needlessly
/// awkward. The explicit `--external-id` remains untouched.
fn customer_environment_name(display_name: &str) -> String {
    let mut name = display_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .fold(String::new(), |mut value, character| {
            if character.is_ascii_alphanumeric() {
                value.push(character);
            } else if !value.ends_with('-') {
                value.push('-');
            }
            value
        })
        .trim_matches('-')
        .chars()
        .take(100)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string();

    if name.is_empty() {
        let digest = Sha256::digest(display_name.as_bytes());
        name = format!("customer-{}", hex::encode(&digest[..6]));
    } else if name.len() == 1 {
        name.push_str("-customer");
    } else if name.starts_with("dg-") || name.starts_with("dg_") {
        name = format!("customer-{name}");
        name.truncate(100);
        name = name.trim_end_matches('-').to_string();
    }

    name
}

#[cfg(all(test, feature = "platform"))]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(id: &str, kind: StackInputKind, required: bool) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind,
            provided_by: vec![StackInputProvider::Developer],
            required,
            label: "Control plane API key".to_string(),
            description: "API key issued by the control plane.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            generate: None,
            env: vec![],
        }
    }

    fn deployer_input(id: &str, kind: StackInputKind, required: bool) -> StackInputDefinition {
        StackInputDefinition {
            provided_by: vec![StackInputProvider::Deployer],
            label: format!("Deployer {id}"),
            ..input(id, kind, required)
        }
    }

    #[test]
    fn airgapped_deployer_inputs_register_with_the_deployment_as_plain_json() {
        let deployer = [
            deployer_input("siteLabel", StackInputKind::String, true),
            deployer_input("replicas", StackInputKind::Integer, false),
        ];
        let split = split_airgapped_inputs(
            &deployer,
            &[
                "siteLabel=plant-7".to_string(),
                "replicas=3".to_string(),
                "apiKey=k".to_string(),
            ],
            &["token=t".to_string()],
            true,
        )
        .expect("split");

        // `/v1/initialize` validates these raw values; a tagged enum would fail it.
        assert_eq!(
            serde_json::Value::Object(split.deployment_values),
            json!({ "siteLabel": "plant-7", "replicas": 3.0 })
        );
        assert_eq!(split.group_inputs, vec!["apiKey=k".to_string()]);
        assert_eq!(split.group_secret_inputs, vec!["token=t".to_string()]);
    }

    #[test]
    fn airgapped_onboarding_requires_deployer_inputs_up_front() {
        let deployer = [deployer_input("siteLabel", StackInputKind::String, true)];
        let error = split_airgapped_inputs(&deployer, &[], &[], true)
            .expect_err("a required deployer input has no other place to be answered");
        assert!(
            error.message.contains("--input siteLabel="),
            "{}",
            error.message
        );
    }

    #[test]
    fn airgapped_onboarding_refuses_deployer_secret_values() {
        let deployer = [deployer_input("dbPassword", StackInputKind::Secret, true)];
        let error =
            split_airgapped_inputs(&deployer, &[], &["dbPassword=hunter2".to_string()], true)
                .expect_err("deployer secrets never pass through Alien");
        assert!(
            error.message.contains("deployer secret"),
            "{}",
            error.message
        );
        // Without a value it is not required here: the site writes it into its own store.
        let split = split_airgapped_inputs(&deployer, &[], &[], true).expect("split");
        assert!(split.deployment_values.is_empty());
    }

    fn platform_input(
        id: &str,
        kind: StackInputKind,
        required: bool,
        platforms: Vec<Platform>,
    ) -> StackInputDefinition {
        StackInputDefinition {
            platforms: Some(platforms),
            ..input(id, kind, required)
        }
    }

    #[test]
    fn helm_command_follows_the_manager_scheme() {
        let stack: Stack = serde_json::from_value(minimal_stack()).unwrap();

        let https = HelmInstall::new(&stack, "https://m.example.com", "m.example.com", "c1");
        let command = https.command();
        assert!(!https.needs_https());
        assert!(
            command.contains("helm registry login m.example.com --username c1 --password-stdin")
        );
        assert!(
            command.contains("helm install test-stack oci://m.example.com/charts/test-stack \\")
        );
        assert!(command.contains("--set management.name=c1"));
        assert!(command.ends_with("--values -"));
        assert!(!command.contains("--plain-http"));
        assert!(
            !command.contains("ax_dg_"),
            "the token must not appear in the commands"
        );

        let local = HelmInstall::new(&stack, "http://localhost:5050", "localhost:5050", "c1");
        let command = local.command();
        assert!(!local.needs_https());
        assert!(command.contains("helm registry login localhost:5050 --insecure --username c1"));
        assert!(command.contains("oci://localhost:5050/charts/test-stack --plain-http \\"));

        let remote = HelmInstall::new(&stack, "http://m.example.com", "m.example.com", "c1");
        assert!(remote.needs_https());
    }

    #[test]
    fn helm_command_runs_in_a_shell() {
        // The generated commands must install with the token from stdin.
        let stack: Stack = serde_json::from_value(minimal_stack()).unwrap();
        let command =
            HelmInstall::new(&stack, "https://m.example.com", "m.example.com", "c1").command();
        let install = command
            .split("\n\n")
            .nth(1)
            .expect("login and install are separate paragraphs");
        let values = install
            .split(" | ")
            .next()
            .expect("the install reads values from printf");
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("ALIEN_TOKEN=ax_dg_secret; {values}"))
            .output()
            .expect("sh runs");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "management:\n  token: ax_dg_secret\n"
        );
    }

    fn minimal_stack() -> serde_json::Value {
        json!({
            "id": "test-stack",
            "resources": {},
            "inputs": []
        })
    }

    #[test]
    fn active_release_stack_inputs_include_machines_platform() {
        let machines_stack = minimal_stack();
        let stack_values = [(Platform::Machines, Some(&machines_stack))];

        let inputs = active_release_stack_inputs_from_values(&stack_values).unwrap();

        assert_eq!(inputs.supported_platforms, vec![Platform::Machines]);
    }

    #[test]
    fn select_onboard_platforms_accepts_machines_when_supported() {
        let requested = vec!["machines".to_string()];
        let supported = vec![Platform::Machines];

        let selected = select_onboard_platforms(&requested, &supported, true).unwrap();

        assert_eq!(selected, vec![Platform::Machines]);
    }

    #[test]
    fn setup_items_default_to_application_and_accept_composition() {
        let default = OnboardArgs::try_parse_from(["onboard", "customer"]).unwrap();
        assert_eq!(default.setup_items, vec![OnboardSetupItem::Application]);

        let composed = OnboardArgs::try_parse_from([
            "onboard",
            "customer",
            "--setup-items",
            "models,keys,storage,registry",
        ])
        .unwrap();
        assert_eq!(
            composed.setup_items,
            vec![
                OnboardSetupItem::Models,
                OnboardSetupItem::Keys,
                OnboardSetupItem::Storage,
                OnboardSetupItem::Registry,
            ]
        );
    }

    #[test]
    fn setup_item_names_are_stable_for_json_output() {
        assert_eq!(
            [
                OnboardSetupItem::Application,
                OnboardSetupItem::Models,
                OnboardSetupItem::Keys,
                OnboardSetupItem::Storage,
                OnboardSetupItem::Registry,
            ]
            .iter()
            .map(onboard_setup_item_name)
            .collect::<Vec<_>>(),
            ["application", "models", "keys", "storage", "registry"]
        );
    }

    #[test]
    fn customer_names_become_safe_internal_environment_names() {
        assert_eq!(customer_environment_name("Acme Corp"), "acme-corp");
        assert_eq!(customer_environment_name("  A  "), "a-customer");
        assert_eq!(customer_environment_name("dg-admin"), "customer-dg-admin");
        assert_eq!(
            customer_environment_name("客户"),
            customer_environment_name("客户")
        );
        assert!(customer_environment_name("客户").starts_with("customer-"));
    }

    #[test]
    fn setup_portal_keeps_the_customer_facing_name() {
        let config = platform_onboard_deployment_setup_config(
            Vec::new(),
            &[Platform::Aws],
            None,
            "Acme Corp",
            true,
        )
        .expect("setup config should be valid");

        assert_eq!(
            config
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.0.get("customerName")),
            Some(&serde_json::Value::String("Acme Corp".to_string()))
        );
    }

    #[test]
    fn a_capability_only_link_allows_the_settings_its_package_registers() {
        use alien_platform_api::types::{
            DeploymentSetupStackSettingsPolicyAllowedHeartbeatsModesItem as Heartbeats,
            DeploymentSetupStackSettingsPolicyAllowedNetworkModesItem as Network,
            DeploymentSetupStackSettingsPolicyAllowedTelemetryModesItem as Telemetry,
            DeploymentSetupStackSettingsPolicyAllowedUpdatesModesItem as Updates,
        };

        let config = platform_onboard_deployment_setup_config(
            Vec::new(),
            &[Platform::Aws],
            None,
            "Acme Corp",
            false,
        )
        .expect("setup config should be valid");
        let stack = config
            .policy
            .and_then(|policy| policy.stack_settings)
            .expect("the link carries a stack-settings policy");

        // Allowing a mode the built-in capability package does not register lets the setup
        // request it, and that setup cannot complete.
        assert_eq!(stack.allowed_telemetry_modes, vec![Telemetry::Off]);
        assert_eq!(stack.allowed_network_modes, vec![Network::None]);
        assert_eq!(stack.allowed_updates_modes, vec![Updates::ApprovalRequired]);
        assert_eq!(stack.allowed_heartbeats_modes, vec![Heartbeats::On]);
    }

    #[test]
    fn setup_items_must_be_configured_for_the_project() {
        let err = validate_setup_items(
            &[OnboardSetupItem::Models, OnboardSetupItem::Keys],
            &[OnboardSetupItem::Models],
        )
        .expect_err("an unavailable setup item must fail before link creation");

        assert!(err.to_string().contains("keys is not configured"));
    }

    #[test]
    fn parse_stack_input_arg_requires_id_value() {
        let parsed = parse_stack_input_arg("serviceToken=secret", "--secret-input")
            .expect("valid input should parse");
        assert_eq!(parsed, ("serviceToken".to_string(), "secret".to_string()));

        let err = parse_stack_input_arg("serviceToken", "--secret-input")
            .expect_err("missing equals should fail");
        assert!(err.to_string().contains("Invalid --secret-input format"));
    }

    #[test]
    fn collect_stack_input_values_rejects_missing_required_in_json_mode() {
        let err = collect_stack_input_values(
            &[input("serviceToken", StackInputKind::Secret, true)],
            &[],
            &[],
            &[Platform::Aws],
            true,
        )
        .expect_err("missing required input should fail");

        assert!(err.to_string().contains("Missing developer input"));
        assert!(err.to_string().contains("--secret-input serviceToken=..."));
    }

    #[test]
    fn generated_inputs_are_not_required_but_can_be_overridden() {
        let generated = StackInputDefinition {
            generate: Some(alien_core::StackInputGenerate { length: 64 }),
            ..input("signingKey", StackInputKind::Secret, true)
        };

        let values = collect_stack_input_values(
            std::slice::from_ref(&generated),
            &[],
            &[],
            &[Platform::Aws],
            true,
        )
        .expect("a generated input needs no value");
        assert!(values.is_empty(), "Alien generates the value later");

        let values = collect_stack_input_values(
            &[generated],
            &[],
            &["signingKey=0123456789abcdef0123".to_string()],
            &[Platform::Aws],
            true,
        )
        .expect("an explicit value overrides generation");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn collect_stack_input_values_parses_typed_values() {
        let values = collect_stack_input_values(
            &[
                input("region", StackInputKind::String, true),
                input("replicas", StackInputKind::Integer, true),
                input("enabled", StackInputKind::Boolean, true),
            ],
            &[
                "region=us-east-1".to_string(),
                "replicas=3".to_string(),
                "enabled=true".to_string(),
            ],
            &[],
            &[Platform::Aws],
            true,
        )
        .expect("typed values should parse");

        assert_eq!(values.len(), 3);
    }

    #[test]
    fn a_secret_the_deployer_may_provide_is_optional_for_the_developer() {
        let mut shared = input("apiKey", StackInputKind::Secret, true);
        shared.provided_by = vec![StackInputProvider::Developer, StackInputProvider::Deployer];

        let values =
            collect_stack_input_values(&[shared.clone()], &[], &[], &[Platform::Aws], true)
                .expect("the deployer writes it into their secret store instead");
        assert!(values.is_empty());

        // A developer value still takes today's path.
        let values = collect_stack_input_values(
            &[shared],
            &[],
            &["apiKey=developer-value".to_string()],
            &[Platform::Aws],
            true,
        )
        .expect("developer value");
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn missing_platform_scoped_input_suggests_narrowing() {
        let err = collect_stack_input_values(
            &[platform_input(
                "localAccessToken",
                StackInputKind::Secret,
                true,
                vec![Platform::Local],
            )],
            &[],
            &[],
            &[Platform::Aws, Platform::Local],
            true,
        )
        .expect_err("missing required local input should fail");

        assert!(err
            .to_string()
            .contains("--secret-input localAccessToken=..."));
        assert!(err.to_string().contains("--platforms aws"));
    }

    #[test]
    fn provided_platform_scoped_input_rejects_mixed_platform_link() {
        let err = collect_stack_input_values(
            &[platform_input(
                "localAccessToken",
                StackInputKind::Secret,
                true,
                vec![Platform::Local],
            )],
            &[],
            &["localAccessToken=local-test-token".to_string()],
            &[Platform::Aws, Platform::Local],
            true,
        )
        .expect_err("local-only input should fail for mixed platform link");

        assert!(err.to_string().contains("not available for platform 'aws'"));
        assert!(err.to_string().contains("separate deployment link"));
        assert!(err.to_string().contains("--platforms local"));
    }
}
