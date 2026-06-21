//! Deploy command — sets up and runs a deployment.
//!
//! Push model (AWS, GCP, Azure): runs initial setup locally, then the manager
//! continues reconciliation remotely.
//!
//! Pull model (Local, Kubernetes): installs and starts the alien-operator service.

use crate::deployment_tracking::DeploymentTracker;
use crate::error::{ErrorData, Result};
use crate::output;
use alien_cli_common::network::{self, NetworkArgs, NetworkMode};
use alien_core::embedded_config::DeployCliConfig;
use alien_core::{
    ClientConfig, DeploymentConfig, DeploymentModel, DeploymentState, DeploymentStatus,
    ManagementConfig, NetworkSettings, Platform, ReleaseInfo, Stack, StackSettings, TelemetryMode,
    UpdatesMode,
};
use alien_deployment::{
    loop_contract::{LoopOperation, LoopOutcome},
    manager_api_transport::{
        acquire_deployment, acquire_setup_delete_deployment, final_reconcile, release_deployment,
        ManagerApiTransport,
    },
    runner::{run_step_loop as shared_run_step_loop, RunnerPolicy},
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_manager_api::{Client as ServerClient, SdkResultExt as ManagerSdkResultExt};
use clap::Parser;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Deploy the application to a target environment",
    after_help = "EXAMPLES:
    # Deploy to AWS using a deployment group token
    alien-deploy deploy --token ax_dg_abc123... --platform aws

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

    /// Manager URL override for pull-model platforms.
    /// Cloud push deployments resolve their manager and install context from
    /// the platform API so setup has the management configuration it needs.
    #[arg(long, env = "ALIEN_MANAGER_URL")]
    pub manager_url: Option<String>,

    /// Platform API base URL.
    /// Used for manager discovery when ALIEN_MANAGER_URL is not set.
    #[arg(long, env = "ALIEN_BASE_URL")]
    pub base_url: Option<String>,

    /// Target platform (aws, gcp, azure)
    #[arg(long)]
    pub platform: Option<String>,

    /// Base cloud platform for managed Kubernetes setup (aws, gcp, azure).
    #[arg(long, env = "OPERATOR_BASE_PLATFORM")]
    pub base_platform: Option<String>,

    /// Allow experimental platforms (kubernetes, local)
    #[arg(long)]
    pub experimental: bool,

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

    #[command(flatten)]
    pub network: NetworkArgs,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeployConfigFile {
    /// Deployment name.
    name: Option<String>,
    /// Target platform: aws, gcp, azure, kubernetes, or local.
    platform: Option<String>,
    /// Base cloud platform when `platform = "kubernetes"`.
    base_platform: Option<String>,
    /// Network settings for cloud deployments.
    network: Option<DeployConfigNetwork>,
    /// Update delivery mode.
    updates: Option<UpdatesMode>,
    /// Telemetry delivery mode.
    telemetry: Option<TelemetryMode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum DeployConfigNetwork {
    UseDefault,
    Create {
        cidr: Option<String>,
        #[serde(default = "default_config_availability_zones")]
        availability_zones: u8,
    },
    ByoVpcAws {
        vpc_id: String,
        public_subnet_ids: Vec<String>,
        private_subnet_ids: Vec<String>,
        #[serde(default)]
        security_group_ids: Vec<String>,
    },
    ByoVpcGcp {
        network_name: String,
        subnet_name: String,
        region: String,
    },
    ByoVnetAzure {
        vnet_resource_id: String,
        public_subnet_name: String,
        private_subnet_name: String,
    },
}

fn default_config_availability_zones() -> u8 {
    2
}

impl From<DeployConfigNetwork> for NetworkSettings {
    fn from(value: DeployConfigNetwork) -> Self {
        match value {
            DeployConfigNetwork::UseDefault => NetworkSettings::UseDefault,
            DeployConfigNetwork::Create {
                cidr,
                availability_zones,
            } => NetworkSettings::Create {
                cidr,
                availability_zones,
            },
            DeployConfigNetwork::ByoVpcAws {
                vpc_id,
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
            } => NetworkSettings::ByoVpcAws {
                vpc_id,
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
            },
            DeployConfigNetwork::ByoVpcGcp {
                network_name,
                subnet_name,
                region,
            } => NetworkSettings::ByoVpcGcp {
                network_name,
                subnet_name,
                region,
            },
            DeployConfigNetwork::ByoVnetAzure {
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
            } => NetworkSettings::ByoVnetAzure {
                vnet_resource_id,
                public_subnet_name,
                private_subnet_name,
                application_gateway_subnet_name: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_push_platforms_require_install_context() {
        assert!(requires_install_context(Platform::Aws));
        assert!(requires_install_context(Platform::Gcp));
        assert!(requires_install_context(Platform::Azure));
    }

    #[test]
    fn pull_model_platforms_do_not_require_install_context() {
        assert!(!requires_install_context(Platform::Kubernetes));
        assert!(!requires_install_context(Platform::Local));
        assert!(!requires_install_context(Platform::Test));
    }

    #[test]
    fn stack_settings_external_bindings_are_copied_to_deployment_config() {
        let mut external_bindings = alien_core::ExternalBindings::new();
        external_bindings.insert(
            "storage",
            alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("test-bucket")),
        );
        let stack_settings = StackSettings {
            external_bindings: Some(external_bindings),
            ..StackSettings::default()
        };
        let mut config = DeploymentConfig::builder()
            .stack_settings(stack_settings.clone())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(alien_core::ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();

        assert!(!config.external_bindings.has("storage"));
        apply_external_bindings_from_stack_settings(&mut config, &stack_settings);

        assert!(config.external_bindings.has("storage"));
    }

    #[test]
    fn parses_cloud_base_platform_for_kubernetes() {
        assert_eq!(
            parse_base_platform(Platform::Kubernetes, Some("aws")).unwrap(),
            Some(Platform::Aws)
        );
    }

    #[test]
    fn rejects_base_platform_without_kubernetes_runtime() {
        assert!(parse_base_platform(Platform::Aws, Some("gcp")).is_err());
    }

    #[test]
    fn rejects_non_cloud_base_platform_for_kubernetes() {
        assert!(parse_base_platform(Platform::Kubernetes, Some("local")).is_err());
    }

    #[test]
    fn release_stack_for_kubernetes_uses_runtime_platform_not_base_platform() {
        let stack = alien_manager_api::types::StackByPlatform {
            aws: Some(serde_json::json!({ "id": "aws-stack" })),
            gcp: Some(serde_json::json!({ "id": "gcp-stack" })),
            azure: Some(serde_json::json!({ "id": "azure-stack" })),
            kubernetes: Some(serde_json::json!({ "id": "kubernetes-stack" })),
            local: None,
            test: None,
        };

        let selected = release_stack_value_for_platform(stack, Platform::Kubernetes).unwrap();
        assert_eq!(selected["id"], "kubernetes-stack");
    }

    #[test]
    fn split_image_tag_defaults_missing_tag_to_latest() {
        assert_eq!(
            split_image_tag("ghcr.io/alienplatform/alien-operator").unwrap(),
            (
                "ghcr.io/alienplatform/alien-operator".to_string(),
                "latest".to_string()
            )
        );
    }

    #[test]
    fn split_image_tag_preserves_registry_port() {
        assert_eq!(
            split_image_tag("localhost:5000/alien-operator:v1").unwrap(),
            (
                "localhost:5000/alien-operator".to_string(),
                "v1".to_string()
            )
        );
    }

    #[test]
    fn sanitize_kubernetes_dns_label_falls_back_when_empty() {
        assert_eq!(sanitize_kubernetes_dns_label("___"), "alien");
    }
}

pub async fn up_command(args: UpArgs, embedded_config: Option<&DeployCliConfig>) -> Result<()> {
    let deploy_config = load_deploy_config(&args)?;
    // Resolve token and platform from args, embedded config, or tracked deployment
    let resolved = resolve_deployment_info(&args, embedded_config, deploy_config.as_ref())?;
    let token = resolved.token;
    let platform_str = resolved.platform;
    let base_platform_str = resolved.base_platform;
    let name = resolved.name;

    // Check for experimental platforms
    if let Ok(p) = Platform::from_str(&platform_str) {
        if p.is_experimental() && !args.experimental {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!(
                    "Platform '{}' is experimental and not yet production-ready. Pass --experimental to use it anyway.",
                    platform_str
                ),
            }));
        }
    }

    let platform = Platform::from_str(&platform_str).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;
    let base_platform = parse_base_platform(platform, base_platform_str.as_deref())?;

    let display_platform = match platform_str.as_str() {
        "aws" => "AWS",
        "gcp" => "Google Cloud",
        "azure" => "Azure",
        "kubernetes" => "Kubernetes",
        "local" => "Local",
        other => other,
    };

    let install_context_platform = base_platform.unwrap_or(platform);
    let install_context_platform_str = install_context_platform.as_str().to_string();
    let (manager_url, install_management_config) =
        if requires_install_context(install_context_platform) {
            output::info("Resolving deployment install context via platform API...");
            let context = discover_manager_install_context(
                &resolved.base_url,
                &token,
                &install_context_platform_str,
            )
            .await?;
            let management_config = context.management_config.ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                    "Platform API did not return installContext.managementConfig for {} deployment",
                    install_context_platform.as_str()
                ),
                })
            })?;
            (context.manager_url, Some(management_config))
        } else {
            match resolved.manager_url {
                Some(url) => (url, None),
                None => {
                    output::info("Discovering manager via platform API...");
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
    eprintln!();

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
    )
    .await?;
    let deployment_id = init.deployment_id;
    output::success("Connected to manager");

    // Use deployment-scoped token if the manager returned one, otherwise keep the original.
    let effective_token = init.deployment_token.unwrap_or_else(|| token.clone());
    let client = create_manager_client(&effective_token, &manager_url)?;

    // Track the deployment locally
    let mut tracker = DeploymentTracker::new()?;
    tracker.track(
        name.clone(),
        deployment_id.clone(),
        effective_token.clone(),
        manager_url.clone(),
        platform_str.clone(),
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

    if current_deployment.status == "running" {
        eprintln!();
        output::success(&format!("Deployment '{}' is already active.", name));
        return Ok(());
    }

    match (platform, base_platform) {
        (Platform::Local, _) | (Platform::Kubernetes, None) => {
            run_pull_model(
                &client,
                &args,
                &manager_url,
                &effective_token,
                &deployment_id,
                &name,
                &stack_settings,
                platform,
            )
            .await?;
        }
        (Platform::Aws | Platform::Gcp | Platform::Azure, _) | (Platform::Kubernetes, Some(_)) => {
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
        (Platform::Test, _) => {
            output::info("Test platform — no deployment action needed.");
        }
    }

    eprintln!();
    output::success(&format!("Deployment '{}' is active.", name));

    Ok(())
}

/// Resolved deployment info before manager connection.
struct ResolvedInfo {
    token: String,
    /// Manager URL (from override, tracker, or to be discovered via platform API).
    manager_url: Option<String>,
    /// Platform API base URL used when manager URL must be discovered.
    base_url: String,
    platform: String,
    base_platform: Option<String>,
    name: String,
}

fn requires_install_context(platform: Platform) -> bool {
    matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
}

fn release_stack_value_for_platform(
    stack: alien_manager_api::types::StackByPlatform,
    platform: Platform,
) -> Option<serde_json::Value> {
    match platform {
        Platform::Aws => stack.aws,
        Platform::Gcp => stack.gcp,
        Platform::Azure => stack.azure,
        Platform::Kubernetes => stack.kubernetes,
        Platform::Local => stack.local,
        Platform::Test => stack.test,
    }
}

fn parse_base_platform(
    platform: Platform,
    base_platform: Option<&str>,
) -> Result<Option<Platform>> {
    let Some(base_platform) = base_platform else {
        return Ok(None);
    };

    let parsed = Platform::from_str(base_platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: e,
        })
    })?;

    if platform != Platform::Kubernetes {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "base-platform".to_string(),
            message: "--base-platform is only supported with --platform kubernetes".to_string(),
        }));
    }

    match parsed {
        Platform::Aws | Platform::Gcp | Platform::Azure => Ok(Some(parsed)),
        Platform::Kubernetes | Platform::Local | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "base-platform".to_string(),
                message: "--base-platform must be one of: aws, gcp, azure".to_string(),
            }))
        }
    }
}

fn load_deploy_config(args: &UpArgs) -> Result<Option<DeployConfigFile>> {
    let Some(path) = &args.config else {
        return Ok(None);
    };

    let text = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("Failed to read deployment config {}", path.display()),
        },
    )?;
    let config =
        toml::from_str(&text)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to parse deployment config {}", path.display()),
            })?;
    Ok(Some(config))
}

fn resolve_deployment_info(
    args: &UpArgs,
    embedded_config: Option<&DeployCliConfig>,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<ResolvedInfo> {
    // If name is provided, try to load from tracker
    let requested_name = args
        .name
        .as_ref()
        .or_else(|| deploy_config.and_then(|c| c.name.as_ref()));
    if let Some(name) = requested_name {
        let tracker = DeploymentTracker::new()?;
        if let Some(tracked) = tracker.get(name) {
            let token =
                resolve_token(args, embedded_config).unwrap_or_else(|_| tracked.token.clone());
            let manager_url = args
                .manager_url
                .clone()
                .or(Some(tracked.manager_url.clone()));
            let platform = args
                .platform
                .clone()
                .or_else(|| deploy_config.and_then(|c| c.platform.clone()))
                .unwrap_or_else(|| tracked.platform.clone());
            let base_platform = args
                .base_platform
                .clone()
                .or_else(|| deploy_config.and_then(|c| c.base_platform.clone()));
            return Ok(ResolvedInfo {
                token,
                manager_url,
                base_url: resolve_base_url(args, embedded_config),
                platform,
                base_platform,
                name: name.clone(),
            });
        }
    }

    // CLI args override embedded config, which overrides nothing (required)
    let token = resolve_token(args, embedded_config)?;

    // Manager URL: explicit override only. If not set, will be discovered via platform API.
    let manager_url = args.manager_url.clone();

    let platform = args
        .platform
        .clone()
        .or_else(|| deploy_config.and_then(|c| c.platform.clone()))
        .or_else(|| embedded_config.and_then(|c| c.default_platform.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message:
                    "--platform is required for new deployments. Choose from: aws, gcp, azure."
                        .to_string(),
            })
        })?;
    let base_platform = args
        .base_platform
        .clone()
        .or_else(|| deploy_config.and_then(|c| c.base_platform.clone()));

    let name = match args.name.clone() {
        Some(n) => n,
        None => match deploy_config.and_then(|c| c.name.clone()) {
            Some(n) => n,
            None if platform == "local" => hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "default".to_string()),
            None => {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "name".to_string(),
                    message: "--name or config field `name` is required for non-local deployments."
                        .to_string(),
                }));
            }
        },
    };

    Ok(ResolvedInfo {
        token,
        manager_url,
        base_url: resolve_base_url(args, embedded_config),
        platform,
        base_platform,
        name,
    })
}

fn resolve_token(args: &UpArgs, embedded_config: Option<&DeployCliConfig>) -> Result<String> {
    args.token
        .clone()
        .or_else(|| {
            embedded_config
                .and_then(|c| c.token_env_var.as_ref())
                .and_then(|env_var| std::env::var(env_var).ok())
        })
        .or_else(|| embedded_config.and_then(|c| c.token.clone()))
        .ok_or_else(|| {
            let branded_hint = embedded_config
                .and_then(|c| c.token_env_var.as_deref())
                .map(|env_var| format!(" or set ${env_var}"))
                .unwrap_or_default();
            AlienError::new(ErrorData::ValidationError {
                field: "token".to_string(),
                message: format!(
                    "--token is required for new deployments{branded_hint}. Use the deployment token from the deploy page."
                ),
            })
        })
}

fn resolve_base_url(args: &UpArgs, embedded_config: Option<&DeployCliConfig>) -> String {
    args.base_url
        .clone()
        .or_else(|| embedded_config.and_then(|c| c.api_base_url.clone()))
        .unwrap_or_else(|| "https://api.alien.dev".to_string())
}

fn load_stack_settings(
    args: &UpArgs,
    platform: Platform,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<StackSettings> {
    let mut settings = StackSettings::default();
    settings.deployment_model = match platform {
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test => DeploymentModel::Push,
        Platform::Kubernetes | Platform::Local => DeploymentModel::Pull,
    };

    if let Some(config) = deploy_config {
        if let Some(network) = config.network.clone() {
            settings.network = Some(network.into());
        }
        if let Some(updates) = config.updates {
            settings.updates = updates;
        }
        if let Some(telemetry) = config.telemetry {
            settings.telemetry = telemetry;
        }
    }

    if args.network.network_mode != NetworkMode::Auto {
        let network_override = network::parse_network_settings(&args.network, platform.as_str())
            .map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "network".to_string(),
                    message: e,
                })
            })?;
        if let Some(network) = network_override {
            settings.network = Some(network);
        }
    }

    Ok(settings)
}

struct ManagerInstallContext {
    manager_url: String,
    management_config: Option<ManagementConfig>,
}

/// Discover the manager URL and platform-managed install context via the platform API.
///
/// Calls GET /v1/resolve?platform=X to resolve the manager.
/// The token's scope (DG, project, etc.) provides the project context
/// to the server — no need to call whoami first.
async fn discover_manager_install_context(
    base_url: &str,
    token: &str,
    platform: &str,
) -> Result<ManagerInstallContext> {
    let http_client = {
        use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Invalid token format".to_string(),
                })?,
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to build HTTP client".to_string(),
            })?
    };

    let url = format!(
        "{}/v1/resolve?platform={}",
        base_url,
        urlencoding::encode(platform),
    );

    let resp = http_client
        .get(&url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to call /v1/resolve on platform API".to_string(),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Failed to resolve manager via platform API (HTTP {}): {}",
                status, body
            ),
        }));
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResolveResponse {
        manager_url: String,
        install_context: Option<ResolveInstallContext>,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ResolveInstallContext {
        management_config: ManagementConfig,
    }

    let resolved: ResolveResponse =
        resp.json()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to parse /v1/resolve response".to_string(),
            })?;

    Ok(ManagerInstallContext {
        manager_url: resolved.manager_url,
        management_config: resolved
            .install_context
            .map(|context| context.management_config),
    })
}

pub fn create_manager_client(token: &str, manager_url: &str) -> Result<ServerClient> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid token format".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-deploy-cli"));

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(ServerClient::new_with_client(manager_url, http_client))
}

fn parse_deployment_status(raw_status: &str) -> Result<DeploymentStatus> {
    match raw_status.to_ascii_lowercase().as_str() {
        "pending" => Ok(DeploymentStatus::Pending),
        "preflights-failed" => Ok(DeploymentStatus::PreflightsFailed),
        "initial-setup" => Ok(DeploymentStatus::InitialSetup),
        "initial-setup-failed" => Ok(DeploymentStatus::InitialSetupFailed),
        "provisioning" => Ok(DeploymentStatus::Provisioning),
        "provisioning-failed" => Ok(DeploymentStatus::ProvisioningFailed),
        "running" => Ok(DeploymentStatus::Running),
        "refresh-failed" => Ok(DeploymentStatus::RefreshFailed),
        "update-pending" => Ok(DeploymentStatus::UpdatePending),
        "updating" => Ok(DeploymentStatus::Updating),
        "update-failed" => Ok(DeploymentStatus::UpdateFailed),
        "delete-pending" => Ok(DeploymentStatus::DeletePending),
        "deleting" => Ok(DeploymentStatus::Deleting),
        "delete-failed" => Ok(DeploymentStatus::DeleteFailed),
        "teardown-required" => Ok(DeploymentStatus::TeardownRequired),
        "teardown-failed" => Ok(DeploymentStatus::TeardownFailed),
        "deleted" => Ok(DeploymentStatus::Deleted),
        "error" => Ok(DeploymentStatus::Error),
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unknown deployment status returned by manager: {raw_status}"),
        })),
    }
}

fn deployment_status_str(status: DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Pending => "pending",
        DeploymentStatus::PreflightsFailed => "preflights-failed",
        DeploymentStatus::InitialSetup => "initial-setup",
        DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
        DeploymentStatus::Provisioning => "provisioning",
        DeploymentStatus::ProvisioningFailed => "provisioning-failed",
        DeploymentStatus::Running => "running",
        DeploymentStatus::RefreshFailed => "refresh-failed",
        DeploymentStatus::UpdatePending => "update-pending",
        DeploymentStatus::Updating => "updating",
        DeploymentStatus::UpdateFailed => "update-failed",
        DeploymentStatus::DeletePending => "delete-pending",
        DeploymentStatus::Deleting => "deleting",
        DeploymentStatus::DeleteFailed => "delete-failed",
        DeploymentStatus::TeardownRequired => "teardown-required",
        DeploymentStatus::TeardownFailed => "teardown-failed",
        DeploymentStatus::Deleted => "deleted",
        DeploymentStatus::Error => "error",
    }
}

/// Result of initializing with the manager.
struct InitResult {
    deployment_id: String,
    /// Deployment-scoped token returned by the manager (when using a deployment group token).
    /// If present, this should replace the original token for subsequent requests.
    deployment_token: Option<String>,
}

async fn initialize_deployment(
    client: &ServerClient,
    _token: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    name: &str,
    stack_settings: &StackSettings,
) -> Result<InitResult> {
    let body = alien_manager_api::types::InitializeRequest {
        name: Some(name.to_string()),
        platform: Some(sdk_platform(platform)),
        base_platform: base_platform.map(sdk_platform),
        stack_settings: Some(sdk_stack_settings(stack_settings)?),
        scope: None,
        permission: None,
    };

    let response = client
        .initialize()
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to initialize with manager. Is the manager running? Check that --manager-url is correct.".to_string(),
        })?;

    let init = response.into_inner();
    Ok(InitResult {
        deployment_id: init.deployment_id,
        deployment_token: init.token,
    })
}

fn sdk_platform(platform: Platform) -> alien_manager_api::types::Platform {
    match platform {
        Platform::Aws => alien_manager_api::types::Platform::Aws,
        Platform::Gcp => alien_manager_api::types::Platform::Gcp,
        Platform::Azure => alien_manager_api::types::Platform::Azure,
        Platform::Kubernetes => alien_manager_api::types::Platform::Kubernetes,
        Platform::Local => alien_manager_api::types::Platform::Local,
        Platform::Test => alien_manager_api::types::Platform::Test,
    }
}

fn sdk_stack_settings(
    stack_settings: &StackSettings,
) -> Result<alien_manager_api::types::StackSettings> {
    let value = serde_json::to_value(stack_settings)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize stack settings".to_string(),
        })?;
    serde_json::from_value(value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to convert stack settings for manager API".to_string(),
        })
}

async fn run_pull_model(
    client: &ServerClient,
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
    platform: Platform,
) -> Result<()> {
    match platform {
        Platform::Kubernetes => {
            run_kubernetes_pull_model(
                client,
                args,
                manager_url,
                token,
                deployment_id,
                deployment_name,
                stack_settings,
            )
            .await
        }
        _ => run_local_pull_model(args, manager_url, token, &platform.to_string()).await,
    }
}

async fn run_local_pull_model(
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    platform: &str,
) -> Result<()> {
    let encryption_key = args.encryption_key.clone().unwrap_or_else(|| {
        use super::operator::generate_encryption_key_public;
        generate_encryption_key_public()
    });

    // Find or download the alien-operator binary
    let binary_path = find_or_download_operator_binary().await?;

    output::info(&format!("Operator binary: {}", binary_path.display()));

    if args.foreground {
        return run_operator_foreground(
            &binary_path,
            manager_url,
            token,
            platform,
            &encryption_key,
            args.data_dir.as_deref(),
        )
        .await;
    }

    output::info("Installing alien-operator as a system service...");

    // Delegate to the operator install logic
    let install_args = super::operator::InstallArgs {
        binary: Some(binary_path),
        sync_url: manager_url.to_string(),
        sync_token: token.to_string(),
        platform: platform.to_string(),
        data_dir: None,
        encryption_key: Some(encryption_key),
    };

    super::operator::install_service(install_args)?;

    output::success("alien-operator installed and running as a system service.");
    output::info("The operator will sync with the manager and deploy updates automatically.");
    output::info("Use 'alien-deploy operator status' to check the service.");

    Ok(())
}

/// Run the operator as a foreground child process (for testing).
async fn run_operator_foreground(
    binary_path: &std::path::Path,
    manager_url: &str,
    token: &str,
    platform: &str,
    encryption_key: &str,
    data_dir_override: Option<&str>,
) -> Result<()> {
    use std::io::Write;

    output::info("Running operator in foreground (Ctrl+C to stop)...");

    let data_dir = if let Some(dir) = data_dir_override {
        std::path::PathBuf::from(dir)
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join(".alien")
            .join("operator-data")
    };

    // The operator rejects `--sync-token`/`--encryption-key` because argv is
    // visible in `ps` / `/proc/<pid>/cmdline`. Write each secret to its own
    // tempfile (0o600 on Unix) and pass the path via `--*-file`. The
    // `NamedTempFile`s must outlive the child process — drop deletes them.
    let mut sync_token_file = tempfile::NamedTempFile::new().into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to create temp file for sync token".to_string(),
        },
    )?;
    sync_token_file
        .write_all(token.as_bytes())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write sync token".to_string(),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            sync_token_file.path(),
            std::fs::Permissions::from_mode(0o600),
        );
    }

    let mut encryption_key_file = tempfile::NamedTempFile::new().into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Failed to create temp file for encryption key".to_string(),
        },
    )?;
    encryption_key_file
        .write_all(encryption_key.as_bytes())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to write encryption key".to_string(),
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            encryption_key_file.path(),
            std::fs::Permissions::from_mode(0o600),
        );
    }

    let status = tokio::process::Command::new(binary_path)
        .arg("--platform")
        .arg(platform)
        .arg("--sync-url")
        .arg(manager_url)
        .arg("--sync-token-file")
        .arg(sync_token_file.path())
        .arg("--encryption-key-file")
        .arg(encryption_key_file.path())
        .arg("--data-dir")
        .arg(&data_dir)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to run operator: {}", binary_path.display()),
        })?;

    // Tempfiles drop here, after the child exits.
    drop(sync_token_file);
    drop(encryption_key_file);

    if !status.success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Operator exited with status: {}", status),
        }));
    }

    Ok(())
}

async fn run_kubernetes_pull_model(
    client: &ServerClient,
    args: &UpArgs,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
) -> Result<()> {
    output::info("Kubernetes platform detected — installing alien-operator with Helm.");
    let stack = fetch_kubernetes_release_stack(client, deployment_id).await?;
    let namespace = args
        .namespace
        .clone()
        .unwrap_or_else(|| format!("alien-{}", sanitize_kubernetes_dns_label(deployment_name)));
    let release = args
        .helm_release
        .clone()
        .unwrap_or_else(|| "alien-operator".to_string());
    let operator_image = args
        .operator_image
        .clone()
        .unwrap_or_else(|| "ghcr.io/alienplatform/alien-operator:latest".to_string());

    let chart_dir = render_kubernetes_helm_chart(&stack, stack_settings, deployment_name)?;
    let values_file = write_kubernetes_helm_values(
        chart_dir.path(),
        manager_url,
        token,
        deployment_id,
        deployment_name,
        stack_settings,
        &operator_image,
    )?;

    helm_upgrade_install(
        chart_dir.path(),
        &values_file,
        &release,
        &namespace,
        args.kubeconfig.as_deref(),
        args.kube_context.as_deref(),
    )
    .await?;

    output::success(&format!(
        "alien-operator Helm release '{}' is installed in namespace '{}'.",
        release, namespace
    ));
    output::info(&format!("Deployment ID: {}", deployment_id));

    Ok(())
}

async fn fetch_kubernetes_release_stack(
    client: &ServerClient,
    deployment_id: &str,
) -> Result<Stack> {
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let release_id = deployment
        .desired_release_id
        .or(deployment.current_release_id)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Deployment has no release to install as a Kubernetes Helm chart"
                    .to_string(),
            })
        })?;
    let release = client
        .get_release()
        .id(&release_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch release '{release_id}' from manager"),
        })?
        .into_inner();
    let stack_value = release.stack.kubernetes.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Release '{release_id}' does not contain a Kubernetes stack"),
        })
    })?;

    serde_json::from_value(stack_value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse Kubernetes stack from release '{release_id}'"),
        })
}

fn render_kubernetes_helm_chart(
    stack: &Stack,
    stack_settings: &StackSettings,
    deployment_name: &str,
) -> Result<tempfile::TempDir> {
    let chart_dir =
        tempfile::tempdir()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to create temporary Helm chart directory".to_string(),
            })?;
    let registry = alien_helm::HelmRegistry::built_in();
    let mut helm_settings = stack_settings.clone();
    helm_settings.deployment_model = DeploymentModel::Pull;
    let chart = alien_helm::generate_helm_chart(
        stack,
        alien_helm::HelmOptions {
            registry: &registry,
            stack_settings: helm_settings,
            chart_name: sanitize_kubernetes_dns_label(deployment_name),
        },
    )
    .map_err(|error| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to generate Kubernetes Helm chart: {error}"),
        })
    })?;

    for (relative_path, contents) in chart.files {
        let path = chart_dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "create directory".to_string(),
                    file_path: parent.display().to_string(),
                    reason: "Failed to create Helm chart output directory".to_string(),
                },
            )?;
        }
        std::fs::write(&path, contents).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "write".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to write generated Helm chart file".to_string(),
            },
        )?;
    }

    Ok(chart_dir)
}

fn write_kubernetes_helm_values(
    chart_dir: &Path,
    manager_url: &str,
    token: &str,
    deployment_id: &str,
    deployment_name: &str,
    stack_settings: &StackSettings,
    operator_image: &str,
) -> Result<PathBuf> {
    let (repository, tag) = split_image_tag(operator_image)?;
    let mut helm_settings = stack_settings.clone();
    helm_settings.deployment_model = DeploymentModel::Pull;
    let values = serde_json::json!({
        "management": {
            "token": token,
            "name": deployment_name,
            "url": manager_url,
            "deploymentId": deployment_id,
            "updates": "auto",
            "telemetry": "auto",
            "healthChecks": "on",
        },
        "runtime": {
            "image": {
                "repository": repository,
                "tag": tag,
                "pullPolicy": "IfNotPresent",
            },
            "encryption": {
                "key": super::operator::generate_encryption_key_public(),
            }
        },
        "stackSettings": helm_settings,
        "infrastructure": null,
    });
    let values_path = chart_dir.join("alien-deploy-values.json");
    let contents = serde_json::to_string_pretty(&values)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to serialize Helm values".to_string(),
        })?;
    std::fs::write(&values_path, contents)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: values_path.display().to_string(),
            reason: "Failed to write Helm values file".to_string(),
        })?;
    Ok(values_path)
}

async fn helm_upgrade_install(
    chart_dir: &Path,
    values_file: &Path,
    release: &str,
    namespace: &str,
    kubeconfig: Option<&str>,
    kube_context: Option<&str>,
) -> Result<()> {
    let mut cmd = tokio::process::Command::new("helm");
    cmd.arg("upgrade")
        .arg("--install")
        .arg(release)
        .arg(chart_dir)
        .arg("--namespace")
        .arg(namespace)
        .arg("--create-namespace")
        .arg("-f")
        .arg(values_file)
        .arg("--wait")
        .arg("--timeout")
        .arg("300s");

    if let Some(kubeconfig) = kubeconfig {
        cmd.env("KUBECONFIG", kubeconfig);
    }
    if let Some(context) = kube_context {
        cmd.arg("--kube-context").arg(context);
    }

    let output = cmd
        .output()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to execute helm. Ensure Helm is installed and available on PATH."
                .to_string(),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Helm upgrade/install failed: {stderr}"),
        }));
    }

    Ok(())
}

fn split_image_tag(image: &str) -> Result<(String, String)> {
    if image.contains('@') {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "operator-image".to_string(),
            message: "Kubernetes Helm installs require a tag-based operator image, not a digest"
                .to_string(),
        }));
    }
    let last_slash = image.rfind('/').unwrap_or(0);
    let tag_separator = image[last_slash..].rfind(':').map(|idx| last_slash + idx);
    let Some(separator) = tag_separator else {
        return Ok((image.to_string(), "latest".to_string()));
    };
    Ok((
        image[..separator].to_string(),
        image[separator + 1..].to_string(),
    ))
}

fn sanitize_kubernetes_dns_label(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            last_dash = false;
            ch.to_ascii_lowercase()
        } else if !last_dash {
            last_dash = true;
            '-'
        } else {
            continue;
        };
        out.push(next);
        if out.len() == 63 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "alien".to_string()
    } else {
        out
    }
}

/// Default releases URL for downloading binaries.
const DEFAULT_RELEASES_URL: &str = "https://releases.alien.dev";

/// Find the alien-operator binary locally, or download it from the releases URL.
async fn find_or_download_operator_binary() -> Result<std::path::PathBuf> {
    // Try to find it locally first
    if let Ok(path) = super::operator::which_operator_binary() {
        return Ok(path);
    }

    // Download to ~/.alien/bin/alien-operator
    let home = dirs::home_dir().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Could not determine home directory".to_string(),
        })
    })?;

    let bin_dir = home.join(".alien").join("bin");
    std::fs::create_dir_all(&bin_dir)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "create".to_string(),
            file_path: bin_dir.display().to_string(),
            reason: "Failed to create ~/.alien/bin directory".to_string(),
        })?;

    let binary_path = bin_dir.join("alien-operator");

    let releases_url =
        std::env::var("ALIEN_RELEASES_URL").unwrap_or_else(|_| DEFAULT_RELEASES_URL.to_string());

    let (os, arch) = detect_os_arch()?;
    let url = format!(
        "{}/alien-operator/latest/{}-{}/alien-operator",
        releases_url, os, arch
    );

    output::info(&format!("Downloading alien-operator from {}...", url));

    let response =
        reqwest::get(&url)
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to download alien-operator from {}", url),
            })?;

    if !response.status().is_success() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "Failed to download alien-operator: HTTP {}",
                response.status()
            ),
        }));
    }

    let bytes =
        response
            .bytes()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to read alien-operator download response".to_string(),
            })?;

    std::fs::write(&binary_path, &bytes)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: binary_path.display().to_string(),
            reason: "Failed to write alien-operator binary".to_string(),
        })?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755))
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "chmod".to_string(),
                file_path: binary_path.display().to_string(),
                reason: "Failed to make alien-operator executable".to_string(),
            })?;
    }

    output::success("alien-operator downloaded successfully.");

    Ok(binary_path)
}

fn detect_os_arch() -> Result<(&'static str, &'static str)> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported OS: {}", std::env::consts::OS),
        }));
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Unsupported architecture: {}", std::env::consts::ARCH),
        }));
    };

    Ok((os, arch))
}

async fn run_push_model(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    manager_url: &str,
    deployment_token: &str,
    management_config: Option<ManagementConfig>,
    network_args: &NetworkArgs,
    on_progress: Option<alien_deployment::runner::ProgressCallback>,
) -> Result<()> {
    let credential_platform = base_platform.unwrap_or(platform);
    let client_config = ClientConfig::from_std_env(credential_platform)
        .await
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to load {} credentials from environment. Ensure the required environment variables are set.",
                credential_platform
            ),
        })?;

    push_initial_setup(
        client,
        deployment_id,
        platform,
        base_platform,
        client_config,
        management_config,
        manager_url,
        deployment_token,
        Some(network_args),
        on_progress,
    )
    .await
}

fn apply_external_bindings_from_stack_settings(
    config: &mut DeploymentConfig,
    stack_settings: &StackSettings,
) {
    if let Some(ref external_bindings) = stack_settings.external_bindings {
        config.external_bindings = external_bindings.clone();
    }
}

/// Run the push-model initial setup flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through InitialSetup until it reaches Provisioning (or a
/// terminal state), reconciles state back to the manager, and releases the lock.
///
/// This is used by both `alien-deploy deploy` (push model) and `alien-test` (e2e setup).
pub async fn push_initial_setup(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    base_platform: Option<Platform>,
    client_config: ClientConfig,
    management_config: Option<alien_core::ManagementConfig>,
    manager_base_url: &str,
    deployment_token: &str,
    network_args: Option<&NetworkArgs>,
    on_progress: Option<alien_deployment::runner::ProgressCallback>,
) -> Result<()> {
    // Get deployment from manager
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    // Reconstruct DeploymentState from flat API response
    let status = parse_deployment_status(&deployment.status)?;

    let stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    let environment_info = deployment
        .environment_info
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize environment_info from manager".to_string(),
        })?;

    // If there's a desired release, fetch the full release info
    let target_release = if let Some(ref release_id) = deployment.desired_release_id {
        match client.get_release().id(release_id).send().await {
            Ok(resp) => {
                let rel = resp.into_inner();
                let platform_stack_value = release_stack_value_for_platform(rel.stack, platform)
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::ConfigurationError {
                            message: format!(
                                "Release {} has no stack for platform {}",
                                release_id,
                                platform.as_str()
                            ),
                        })
                    })?;

                // No stack rewriting — release already stores proxy URIs.
                // Controllers use image URIs as-is.
                let stack = serde_json::from_value(platform_stack_value)
                    .into_alien_error()
                    .context(ErrorData::ConfigurationError {
                        message: "Failed to parse release stack".to_string(),
                    })?;

                Some(ReleaseInfo {
                    release_id: rel.id,
                    version: None,
                    description: None,
                    stack,
                })
            }
            Err(e) => {
                output::warn(&format!("Could not fetch release {}: {}", release_id, e));
                None
            }
        }
    } else {
        None
    };

    let mut state = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release,
        stack_state,
        error: None,
        environment_info,
        runtime_metadata: None,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    // Always override environment_info with the target client_config.
    // The manager may have already run the Pending step with management
    // credentials, setting environment_info to the management project.
    // push_initial_setup runs with *target* credentials, so re-collecting
    // ensures the environment_info reflects the actual target project.
    let environment_platform = base_platform.unwrap_or(platform);
    match alien_deployment::collect_environment_info(environment_platform, &client_config).await {
        Ok(env_info) => {
            state.environment_info = Some(env_info);
        }
        Err(e) => {
            tracing::warn!("Failed to collect target environment info: {e}");
        }
    }

    // Reconstruct DeploymentConfig from stack_settings
    let mut stack_settings: StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings from manager".to_string(),
        })?
        .unwrap_or_default();

    // Override network settings if the customer provided CLI flags
    if let Some(net_args) = network_args {
        let network_platform = base_platform.unwrap_or(platform);
        let network_override = network::parse_network_settings(net_args, network_platform.as_str())
            .map_err(|e| {
                AlienError::new(ErrorData::ValidationError {
                    field: "network".to_string(),
                    message: e,
                })
            })?;
        if let Some(ns) = network_override {
            stack_settings.network = Some(ns);
        }
    }

    // Build a minimal config JSON and deserialize to get proper defaults
    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "managementConfig": serde_json::to_value(&management_config).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    // Set manager URL and deployment token so controllers can configure
    // pull auth (RegistryCredentials, imagePullSecrets) for the manager's registry.
    config.manager_url = Some(manager_base_url.to_string());
    config.deployment_token = Some(deployment_token.to_string());
    config.base_platform = base_platform;

    apply_external_bindings_from_stack_settings(&mut config, &stack_settings);

    // Acquire sync lock — retry until the specific deployment is locked by us.
    // The manager's deployment loop may already hold the lock; we must wait for
    // it to release before proceeding. 2 minutes (60 × 2s) is sufficient because
    // the manager skips Pending/InitialSetup for push-mode deployments — if it
    // holds the lock, it checks push-mode + Pending and releases immediately.
    // Acquire sync lock — retry until the specific deployment is locked by us.
    let session = format!("push-setup-{}", uuid::Uuid::new_v4());
    acquire_deployment(client, deployment_id, &session)
        .await
        .context(ErrorData::DeploymentFailed {
            operation: "acquire sync lock".to_string(),
        })?;

    // Re-fetch the deployment state now that we hold the lock.
    // The manager may have advanced the state while we were waiting.
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;

    state.status = status;
    state.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    state.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    tracing::info!(
        has_runtime_metadata = state.runtime_metadata.is_some(),
        "push_initial_setup: state after re-fetch (before step loop)"
    );

    // Run the shared step loop with per-step reconciliation via the manager API
    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        // Push model: run initial setup only, then hand off to the manager.
        // The CLI drives Pending → InitialSetup → Provisioning, then stops.
        // The manager picks up from Provisioning and drives to Running.
        operation: LoopOperation::InitialSetup,
        delay_threshold: None,
    };

    let runner_result = shared_run_step_loop(
        &mut state,
        &mut config,
        &client_config,
        deployment_id,
        &policy,
        &transport,
        None,
        on_progress.as_ref(),
    )
    .await;

    // Always reconcile + release, even on error.
    final_reconcile(client, deployment_id, &session, &state).await;
    release_deployment(client, deployment_id, &session).await;

    // Handle runner result after lock release
    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "initial setup".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => {
            output::success("Deployment is running.");
            Ok(())
        }
        LoopOutcome::Failure => Err(AlienError::new(ErrorData::DeploymentFailed {
            operation: format!(
                "deployment failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            ),
        })),
        LoopOutcome::Neutral => {
            output::success(
                "Setup complete. Your deployment is being provisioned and will be ready shortly.",
            );
            Ok(())
        }
    }
}

/// Run the push-model deletion flow for a deployment.
///
/// Fetches deployment and release state from the manager, acquires a sync lock,
/// steps the deployment through DeletePending → Deleting → Deleted (or DeleteFailed),
/// reconciles state back to the manager, and releases the lock.
pub async fn push_deletion(
    client: &ServerClient,
    deployment_id: &str,
    platform: Platform,
    client_config: ClientConfig,
) -> Result<()> {
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;

    let stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    let environment_info = deployment
        .environment_info
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize environment_info from manager".to_string(),
        })?;
    let runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    let current_release = if let Some(ref release_id) = deployment.current_release_id {
        match client.get_release().id(release_id).send().await {
            Ok(resp) => {
                let rel = resp.into_inner();
                let platform_stack_value = release_stack_value_for_platform(rel.stack, platform);
                platform_stack_value
                    .and_then(|v| serde_json::from_value(v).ok())
                    .map(|stack| ReleaseInfo {
                        release_id: rel.id,
                        version: None,
                        description: None,
                        stack,
                    })
            }
            Err(_) => None,
        }
    } else {
        None
    };

    let mut state = DeploymentState {
        status,
        platform,
        current_release: current_release.clone(),
        target_release: current_release,
        stack_state,
        error: None,
        environment_info,
        runtime_metadata,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    let stack_settings: StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings from manager".to_string(),
        })?
        .unwrap_or_default();

    let mut config: DeploymentConfig = serde_json::from_value(serde_json::json!({
        "stackSettings": serde_json::to_value(&stack_settings).unwrap_or_default(),
        "environmentVariables": {
            "variables": [],
            "hash": "",
            "createdAt": ""
        }
    }))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Failed to construct deployment config".to_string(),
    })?;

    apply_external_bindings_from_stack_settings(&mut config, &stack_settings);

    // Acquire sync lock with retry
    let session = format!("push-deletion-{}", uuid::Uuid::new_v4());
    acquire_setup_delete_deployment(client, deployment_id, &session)
        .await
        .context(ErrorData::DeploymentFailed {
            operation: "acquire sync lock for deletion".to_string(),
        })?;

    // Re-fetch deployment under lock
    let deployment = client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to get deployment from manager".to_string(),
        })?
        .into_inner();

    let status = parse_deployment_status(&deployment.status)?;

    state.status = status;
    state.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state from manager".to_string(),
        })?;
    state.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata from manager".to_string(),
        })?;

    // Run the shared step loop with per-step reconciliation via the manager API
    let transport = ManagerApiTransport::new(client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: LoopOperation::Delete,
        delay_threshold: None,
    };

    let runner_result = match shared_run_step_loop(
        &mut state,
        &mut config,
        &client_config,
        deployment_id,
        &policy,
        &transport,
        None,
        None,
    )
    .await
    {
        Ok(result)
            if result.loop_result.outcome == LoopOutcome::Success
                && state.status == DeploymentStatus::TeardownRequired =>
        {
            alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
                &mut state,
                &mut config,
                &client_config,
                deployment_id,
                &policy,
                &transport,
                None,
            )
            .await
            .map(|setup_result| setup_result.unwrap_or(result))
        }
        other => other,
    };

    // Always reconcile + release, even on error
    final_reconcile(client, deployment_id, &session, &state).await;
    release_deployment(client, deployment_id, &session).await;

    // Handle runner result after lock release
    let result = runner_result.context(ErrorData::DeploymentFailed {
        operation: "deletion".to_string(),
    })?;

    match result.loop_result.outcome {
        LoopOutcome::Success => {
            output::success("Deployment deleted successfully.");
            Ok(())
        }
        LoopOutcome::Failure => Err(AlienError::new(ErrorData::DeploymentFailed {
            operation: format!(
                "deletion failed at status {}",
                deployment_status_str(result.loop_result.final_status)
            ),
        })),
        LoopOutcome::Neutral => Ok(()),
    }
}
