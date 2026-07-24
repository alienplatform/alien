//! Deploy command — sets up and runs a deployment.
//!
//! Push model (AWS, GCP, Azure): runs initial setup locally, then the manager
//! continues reconciliation remotely.
//!
//! Pull model (Local, Kubernetes): installs and starts the alien-operator service.

use crate::deployment_tracking::{DeploymentTracker, TrackedLocalDeployment};
use crate::error::{ErrorData, Result};
use crate::output;
use alien_cli_common::network::{self, NetworkArgs, NetworkMode};
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{
    parse_public_endpoint_assignment, validate_public_endpoint_urls, ClientConfig, ComputeSettings,
    Container, Daemon, DeploymentConfig, DeploymentModel, DeploymentState, DeploymentStatus,
    ManagementConfig, NetworkSettings, Platform, PublicEndpointUrls, ReleaseInfo, Stack,
    StackInputDefinition, StackInputKind, StackInputProvider, StackSettings, TelemetryMode,
    UpdatesMode, Worker,
};
use alien_deployment::{
    loop_contract::{LoopOperation, LoopOutcome, LoopResult, LoopStopReason},
    manager_api_transport::{
        acquire_runtime_delete_deployment, acquire_setup_delete_deployment,
        acquire_setup_run_deployment, final_reconcile, release_deployment, ManagerApiTransport,
        SetupDeleteAcquireOutcome,
    },
    runner::{run_step_loop as shared_run_step_loop, RunnerPolicy, RunnerResult},
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_manager_api::{Client as ServerClient, SdkResultExt as ManagerSdkResultExt};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

mod config;
mod inputs;
mod machines;
mod pull;
mod push;
mod resolve;
#[cfg(test)]
mod tests;

use config::*;
use inputs::*;
use machines::*;
use pull::*;
use push::*;
use resolve::*;

pub use pull::create_manager_client;
pub(crate) use pull::create_manager_http_client;
pub use push::{push_deletion, push_initial_setup};
pub(crate) use resolve::{
    read_token_file, resolve_base_url_option, resolve_manager_url_option, resolve_optional_token,
    resolve_platform_option,
};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Deploy the application to a target environment",
    after_help = "EXAMPLES:
    # Deploy to AWS using a deployment group token
    alien-deploy deploy --token ax_dg_abc123... --platform aws

    # Deploy using a token file so the token is not exposed in argv
    alien-deploy deploy --token-file /run/alien/token --platform local

    # Deploy a local pull-model workload behind customer-managed ingress
    alien-deploy deploy --token-file /run/alien/token --platform local --public-endpoint gateway.api=https://gateway.example.com

    # Deploy with an isolated VPC
    alien-deploy deploy --token ax_dg_abc123... --platform aws --network create

    # Deploy into an existing VPC
    alien-deploy deploy --token ax_dg_abc123... --platform aws --network byo --vpc-id vpc-0abc123 --public-subnet-ids subnet-pub1 --private-subnet-ids subnet-priv1

    # Redeploy an existing tracked deployment
    alien-deploy deploy --name production

    # Deploy to a standalone (OSS) manager
    ALIEN_MANAGER_URL=https://manager.example.com alien-deploy deploy --token ax_dg_abc123... --platform aws"
)]
pub struct UpArgs {
    /// Authentication token (deployment or deployment group token)
    #[arg(long, env = "ALIEN_TOKEN")]
    pub token: Option<String>,

    /// Read authentication token from a file.
    #[arg(long, conflicts_with = "token")]
    pub token_file: Option<PathBuf>,

    /// Manager URL override for pull-model platforms.
    /// Cloud push deployments resolve their manager and install context from
    /// the platform API so setup has the management configuration it needs.
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Platform API base URL.
    /// Used for manager discovery when ALIEN_MANAGER_URL is not set.
    #[arg(long, env = "ALIEN_BASE_URL")]
    pub base_url: Option<String>,

    /// Target platform (aws, gcp, azure, kubernetes, machines, local)
    #[arg(long)]
    pub platform: Option<String>,

    /// Base cloud platform for managed Kubernetes setup (aws, gcp, azure).
    #[arg(long, env = "OPERATOR_BASE_PLATFORM")]
    pub base_platform: Option<String>,

    /// Deployment name (for tracking)
    #[arg(long)]
    pub name: Option<String>,

    /// Encryption key for operator database (required for pull model)
    #[arg(long, env = "OPERATOR_ENCRYPTION_KEY")]
    pub encryption_key: Option<String>,

    /// Skip confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Run the operator in the foreground instead of installing as a service.
    /// Useful for testing — Ctrl+C to stop.
    #[arg(long)]
    pub foreground: bool,

    /// Data directory for operator state (foreground mode only).
    /// Defaults to ~/.alien/operator-data.
    #[arg(long)]
    pub data_dir: Option<String>,

    /// Enable Local runtime debug commands and shells on the installed operator service.
    #[arg(long)]
    pub enable_local_debug: bool,

    /// Override the shell command used by Local runtime debug shells.
    #[arg(long)]
    pub local_debug_shell_command: Option<String>,

    /// Kubernetes namespace for Helm installs.
    #[arg(long, env = "ALIEN_KUBERNETES_NAMESPACE")]
    pub namespace: Option<String>,

    /// Helm release name for Kubernetes installs.
    #[arg(long, env = "ALIEN_HELM_RELEASE")]
    pub helm_release: Option<String>,

    /// Kubeconfig path for Kubernetes installs. Defaults to KUBECONFIG or kubectl defaults.
    #[arg(long, env = "KUBECONFIG")]
    pub kubeconfig: Option<String>,

    /// Kubernetes context for Helm installs.
    #[arg(long, env = "ALIEN_KUBE_CONTEXT")]
    pub kube_context: Option<String>,

    /// alien-operator image for Kubernetes Helm installs.
    #[arg(long, env = "ALIEN_OPERATOR_IMAGE")]
    pub operator_image: Option<String>,

    /// TOML file containing deployment settings.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Stack input value for setup (id=value).
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Secret stack input value for setup (id=value).
    #[arg(long = "secret-input")]
    pub secret_input_values: Vec<String>,

    /// Public URL for an exposed endpoint in <resource-id>.<endpoint-name>=<absolute-url> form.
    ///
    /// Intended for pull-model deployments where DNS, TLS, and ingress are
    /// owned outside Alien. Repeat this flag for multiple endpoints.
    #[arg(long = "public-endpoint")]
    pub public_endpoints: Vec<String>,

    #[command(flatten)]
    pub network: NetworkArgs,
}

pub async fn up_command(args: UpArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    let deploy_config = load_deploy_config(&args)?;
    // Resolve token and platform from args, embedded config, or tracked deployment
    let resolved = resolve_deployment_info(&args, embedded_config, deploy_config.as_ref())?;
    let token = resolved.token;
    let platform_str = resolved.platform;
    let base_platform_str = resolved.base_platform;
    let name = resolved.name;

    let platform = Platform::from_str(&platform_str).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;
    let print_progress = should_print_deploy_progress(platform);
    let base_platform = parse_base_platform(platform, base_platform_str.as_deref())?;
    let public_endpoints = load_public_endpoints(&args, platform, deploy_config.as_ref())?;
    let deployer_inputs = match fetch_deployment_info(&resolved.base_url, &token, platform).await {
        Ok(info) => {
            validate_deployment_readiness(&info, platform)?;
            deployer_inputs_from_info(&info, platform)
        }
        Err(error) => {
            if !args.input_values.is_empty() || !args.secret_input_values.is_empty() {
                output::warn(&format!(
                    "Could not load stack input metadata; the platform API will validate supplied inputs: {error}"
                ));
            }
            Vec::new()
        }
    };
    let stack_input_values = collect_deployer_input_values(
        &deployer_inputs,
        &args.input_values,
        &args.secret_input_values,
        deploy_config.as_ref(),
    )?;

    let display_platform = match platform_str.as_str() {
        "aws" => "AWS",
        "gcp" => "Google Cloud",
        "azure" => "Azure",
        "kubernetes" => "Kubernetes",
        "machines" => "Your machines",
        "local" => "Local",
        other => other,
    };

    let install_context_platform = base_platform.unwrap_or(platform);
    let install_context_platform_str = install_context_platform.as_str().to_string();
    let (manager_url, install_management_config) =
        if requires_install_context(install_context_platform) {
            if print_progress {
                output::info("Resolving deployment install context via platform API...");
            }
            let context = discover_manager_install_context(
                &resolved.base_url,
                &token,
                &install_context_platform_str,
            )
            .await?;
            // `management_config` is required by the production SaaS API to
            // describe the cross-account role used at provisioning time. The
            // standalone manager returns it as `None` when it runs in a
            // single-account setup (where the deployment account *is* the
            // managing account and no cross-account access is involved);
            // downstream code is already `Option<ManagementConfig>`-aware.
            (context.manager_url, context.management_config)
        } else {
            match resolved.manager_url {
                Some(url) => (url, None),
                None => {
                    if print_progress {
                        output::info("Discovering manager via platform API...");
                    }
                    let context = discover_manager_install_context(
                        &resolved.base_url,
                        &token,
                        &install_context_platform_str,
                    )
                    .await?;
                    (context.manager_url, context.management_config)
                }
            }
        };

    if print_progress {
        let banner_title = embedded_config
            .and_then(|c| c.display_name.as_deref())
            .unwrap_or("Alien Deploy");
        output::banner(banner_title);
        output::label_value("Platform", display_platform);
        if let Some(base_platform) = base_platform {
            output::label_value("Base platform", base_platform.as_str());
        }
        output::label_value("Manager", &manager_url);
        output::label_value("Name", &name);
        if let Some(public_endpoints) = public_endpoints.as_ref() {
            let endpoint_count: usize = public_endpoints.values().map(HashMap::len).sum();
            output::label_value("Public endpoints", &endpoint_count.to_string());
        }
        eprintln!();
    }

    let stack_settings = load_stack_settings(&args, platform, deploy_config.as_ref())?;

    // Create authenticated manager client
    let client = create_manager_client(&token, &manager_url)?;

    // Initialize with manager
    let init = initialize_deployment(
        &client,
        &token,
        platform,
        base_platform,
        &name,
        &stack_settings,
        stack_input_values,
    )
    .await?;
    let deployment_id = init.deployment_id;
    if print_progress {
        output::success("Connected to manager");
    }

    // Use deployment-scoped token if the manager returned one, otherwise keep the original.
    let effective_token = init.deployment_token.unwrap_or_else(|| token.clone());
    let client = create_manager_client(&effective_token, &manager_url)?;
    let local_tracking = local_tracking_metadata(&args, platform);

    // Track the deployment locally
    let mut tracker = DeploymentTracker::new()?;
    tracker.track(
        name.clone(),
        deployment_id.clone(),
        effective_token.clone(),
        manager_url.clone(),
        platform_str.clone(),
        local_tracking,
    )?;

    // Check if the deployment is already active — nothing to do.
    let current_deployment = client
        .get_deployment()
        .id(&deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    if let Some(public_endpoints) = public_endpoints.as_ref() {
        let release_id = current_deployment
            .desired_release_id
            .as_deref()
            .or(current_deployment.current_release_id.as_deref())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "public-endpoint".to_string(),
                    message:
                        "Cannot validate public endpoints because the deployment has no release"
                            .to_string(),
                })
            })?;
        let stack = fetch_release_stack_by_id(&client, release_id, platform).await?;
        validate_public_endpoint_names(public_endpoints, &stack)?;
    }

    if current_deployment.status == "running"
        && public_endpoints.is_none()
        && platform != Platform::Machines
    {
        eprintln!();
        output::success(&format!("Deployment '{}' is already active.", name));
        return Ok(());
    } else if current_deployment.status == "running" {
        output::info(
            "Deployment is already active; updating local operator public endpoint config.",
        );
    }

    match init.deployment_model {
        DeploymentModel::Pull => {
            run_pull_model(
                &client,
                &args,
                &manager_url,
                &effective_token,
                &deployment_id,
                &name,
                &stack_settings,
                platform,
                embedded_config,
                public_endpoints.as_ref(),
            )
            .await?;
        }
        DeploymentModel::Push => match platform {
            Platform::Machines => {
                push_initial_setup(
                    &client,
                    &deployment_id,
                    platform,
                    base_platform,
                    ClientConfig::Machines,
                    install_management_config,
                    &manager_url,
                    &effective_token,
                    None,
                    None,
                )
                .await?;

                let join_token = create_machines_join_token(
                    &resolved.base_url,
                    &effective_token,
                    &deployment_id,
                )
                .await?;
                let cli_name = embedded_config
                    .and_then(|config| config.name.as_deref())
                    .unwrap_or("alien-deploy");
                let install_script_url =
                    embedded_config.and_then(|config| config.install_script_url.as_deref());
                println!(
                    "{}",
                    machines_join_command(cli_name, install_script_url, &join_token)
                );
                return Ok(());
            }
            Platform::Test => {
                output::info("Test platform — no deployment action needed.");
            }
            Platform::Aws
            | Platform::Gcp
            | Platform::Azure
            | Platform::Kubernetes
            | Platform::Local => {
                // Build progress callback
                let progress =
                    std::sync::Arc::new(std::sync::Mutex::new(output::DeployProgress::new()));
                let progress_clone = progress.clone();
                let on_progress: alien_deployment::runner::ProgressCallback =
                    Box::new(move |step_progress| {
                        let mut p = progress_clone.lock().unwrap_or_else(|e| e.into_inner());
                        p.update(step_progress);
                    });

                run_push_model(
                    &client,
                    &deployment_id,
                    platform,
                    base_platform,
                    &manager_url,
                    &effective_token,
                    install_management_config,
                    &args.network,
                    Some(on_progress),
                )
                .await?;

                // Clear the live progress display
                let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                p.finish();
            }
        },
    }

    eprintln!();
    output::success(&format!("Deployment '{}' is active.", name));

    Ok(())
}

fn should_print_deploy_progress(platform: Platform) -> bool {
    platform != Platform::Machines
}
