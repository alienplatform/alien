//! Deploy command — creates or updates a deployment via the manager.
//!
//! Flow:
//! 1. Resolve/create deployment (via platform API for DG tokens, or from tracker)
//! 2. Discover manager URL (resolve_manager for OAuth, DG endpoint for DG tokens)
//! 3. Run step loop via manager (acquire → step → reconcile → release)

use crate::commands::deployments::{parse_resource_prefix, MonitoringMode};

use crate::commands::{
    create_initial_deployment, fetch_dev_deployment_live_state,
    wait_for_dev_deployment_ready_with_progress,
};
use crate::deployment_tracking::{validate_token, DeploymentToken, DeploymentTracker};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_cli_common::network::{self, NetworkArgs, NetworkMode};
use alien_core::{ClientConfig, DeploymentState, DeploymentStatus, NetworkSettings, Platform};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome, LoopStopReason};
use alien_deployment::manager_api_transport::{
    acquire_deployment, acquire_setup_run_deployment, final_reconcile, release_deployment,
    ManagerApiTransport,
};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::Client as SdkClient;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use clap::Parser;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Provision and update a customer deployment",
    long_about = "Provision and update a customer deployment in their cloud account.",
    after_help = "EXAMPLES:
    # Deploy to your own AWS environment
    alien deploy --name production --platform aws --secret-input descopeAccessKey=...

    # Create a Machines deployment and print a host join command
    alien deploy --name eu-prod --machines

    # Set up a deployment from a customer deployment-group token
    alien deploy --token dg_abc123... --name production --platform aws

    # Deploy an existing deployment (uses stored API key)
    alien deploy --name production --platform aws

    # Deploy without heartbeat capability
    alien deploy --token ax_deployment_xyz... --name prod --platform aws --no-heartbeat"
)]
pub struct DeployArgs {
    /// Deployment API key for authentication (optional if deployment is already tracked)
    #[arg(long)]
    pub token: Option<String>,

    /// Deployment name for identification in tracking
    #[arg(long)]
    pub name: Option<String>,

    /// Target platform for the deployment (aws, gcp, azure, machines)
    #[arg(long, conflicts_with = "machines")]
    pub platform: Option<String>,

    /// Create or update a Machines deployment and print a host join command
    #[arg(long)]
    pub machines: bool,

    /// TOML file containing deployment settings.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Stack input value for setup (id=value).
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Secret stack input value for setup (id=value).
    #[arg(long = "secret-input")]
    pub secret_input_values: Vec<String>,

    /// Public subdomain for deployments in your own environment.
    ///
    /// This is only accepted when creating a new deployment without --token.
    #[arg(long)]
    pub public_subdomain: Option<String>,

    /// Physical-name prefix for generated cloud resources.
    /// Omit to let the manager generate one.
    #[arg(long, value_parser = parse_resource_prefix)]
    pub resource_prefix: Option<String>,

    /// Disable heartbeat capability
    #[arg(long)]
    pub no_heartbeat: bool,

    /// Telemetry / monitoring mode.
    /// "auto" (default) uses the parent manager's built-in log store or external OTLP integration.
    /// "off" disables all monitoring.
    #[arg(long, value_enum, default_value_t = MonitoringMode::Auto)]
    pub monitoring: MonitoringMode,

    /// Manager to use for deployment.
    /// Omit for auto-resolve (platform resolves from deployment record).
    /// Use "none" to deploy without a manager (e.g., bootstrapping the manager itself).
    /// Or pass a specific manager ID.
    #[arg(long)]
    pub manager: Option<String>,

    #[command(flatten)]
    pub network: NetworkArgs,
}

#[derive(Debug, Clone)]
struct ResolvedDeployArgs {
    name: String,
    platform: String,
    platform_enum: Platform,
    network_settings: Option<NetworkSettings>,
    input_values: HashMap<String, serde_json::Value>,
    public_subdomain: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeployConfigFile {
    name: Option<String>,
    platform: Option<String>,
    network: Option<DeployConfigNetwork>,
    inputs: Option<HashMap<String, String>>,
    secret_inputs: Option<HashMap<String, String>>,
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
                private_endpoint_subnet_name: None,
            },
        }
    }
}

fn resolve_deploy_args(args: &DeployArgs) -> Result<ResolvedDeployArgs> {
    let config = match args.config.as_ref() {
        Some(path) => Some(read_deploy_config(path)?),
        None => None,
    };

    let platform = if args.machines {
        "machines".to_string()
    } else {
        args.platform
            .clone()
            .or_else(|| config.as_ref().and_then(|config| config.platform.clone()))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ValidationError {
                    field: "platform".to_string(),
                    message: "--platform, --machines, or config field `platform` is required."
                        .to_string(),
                })
            })?
    };

    let platform_enum = Platform::from_str(&platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: e,
        })
    })?;

    let name = args
        .name
        .clone()
        .or_else(|| config.as_ref().and_then(|config| config.name.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "name".to_string(),
                message: "--name or config field `name` is required.".to_string(),
            })
        })?;

    let network_settings = resolve_network_settings(args, config.as_ref(), &platform)?;
    let input_values = collect_raw_input_values(
        config.as_ref(),
        &args.input_values,
        &args.secret_input_values,
    )?;
    if args.token.is_some() && args.public_subdomain.is_some() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "public-subdomain".to_string(),
            message:
                "--public-subdomain is only supported when creating a deployment without --token."
                    .to_string(),
        }));
    }

    Ok(ResolvedDeployArgs {
        name,
        platform,
        platform_enum,
        network_settings,
        input_values,
        public_subdomain: args.public_subdomain.clone(),
    })
}

fn read_deploy_config(path: &Path) -> Result<DeployConfigFile> {
    let contents = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to read deploy config".to_string(),
        },
    )?;
    toml::from_str(&contents)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse deploy config {}", path.display()),
        })
}

fn resolve_network_settings(
    args: &DeployArgs,
    config: Option<&DeployConfigFile>,
    platform: &str,
) -> Result<Option<NetworkSettings>> {
    let cli_network = network::parse_network_settings(&args.network, platform).map_err(|e| {
        AlienError::new(ErrorData::ValidationError {
            field: "network".to_string(),
            message: e,
        })
    })?;
    if cli_network.is_some() || args.network.network_mode != NetworkMode::Auto {
        return Ok(cli_network);
    }

    Ok(config
        .and_then(|config| config.network.clone())
        .map(NetworkSettings::from))
}

fn collect_raw_input_values(
    config: Option<&DeployConfigFile>,
    input_values: &[String],
    secret_input_values: &[String],
) -> Result<HashMap<String, serde_json::Value>> {
    let mut values = HashMap::new();

    if let Some(config_inputs) = config.and_then(|config| config.inputs.as_ref()) {
        for (id, value) in config_inputs {
            values.insert(id.clone(), serde_json::Value::String(value.clone()));
        }
    }
    if let Some(config_inputs) = config.and_then(|config| config.secret_inputs.as_ref()) {
        for (id, value) in config_inputs {
            values.insert(id.clone(), serde_json::Value::String(value.clone()));
        }
    }
    for input in input_values {
        let (id, value) = parse_stack_input_arg(input, "--input")?;
        values.insert(id, serde_json::Value::String(value));
    }
    for input in secret_input_values {
        let (id, value) = parse_stack_input_arg(input, "--secret-input")?;
        values.insert(id, serde_json::Value::String(value));
    }

    Ok(values)
}

fn parse_stack_input_arg(input: &str, flag: &str) -> Result<(String, String)> {
    let Some((id, value)) = input.split_once('=') else {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: '{input}'. Use id=value"),
        }));
    };
    let id = id.trim();
    if id.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: flag.trim_start_matches("--").to_string(),
            message: format!("Invalid {flag} format: input id cannot be empty"),
        }));
    }
    Ok((id.to_string(), value.to_string()))
}

/// Create authenticated platform client
fn create_platform_client(api_key: &str, base_url: &str) -> Result<SdkClient> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    let http_client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })?;

    Ok(SdkClient::new_with_client(base_url, http_client))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiDeploymentGroup {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FirstPartyDeploymentSession {
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDeploymentApiResponse {
    deployment: CreateDeploymentApiDeployment,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDeploymentApiDeployment {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MachinesJoinTokenResponse {
    join_token: String,
    control_plane_url: Option<String>,
    cluster_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WrappedMachinesJoinToken<'a> {
    join_token: &'a str,
    control_plane_url: &'a str,
    cluster_id: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoResponse {
    packages: Option<DeploymentInfoPackages>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoPackages {
    cli: Option<DeploymentInfoCliPackage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoCliPackage {
    command_name: Option<String>,
    install_scripts: Option<DeploymentInfoInstallScripts>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentInfoInstallScripts {
    linux: Option<String>,
}

async fn create_self_deployment(
    ctx: &ExecutionMode,
    tracker: &mut DeploymentTracker,
    resolved_args: &ResolvedDeployArgs,
    args: &DeployArgs,
) -> Result<crate::deployment_tracking::TrackedDeployment> {
    if ctx.is_standalone() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "token".to_string(),
            message: "--token is required when creating a deployment against a standalone manager."
                .to_string(),
        }));
    }

    let base_url = ctx.base_url();
    let auth = ctx.auth_http().await?;
    let workspace = ctx.resolve_platform_workspace_context(true).await?;
    let (project_id, _project_link) = ctx.resolve_project(None, true).await?;

    let deployment_group = ensure_self_deployment_group(
        &auth.client,
        &base_url,
        workspace.query.as_deref(),
        &resolved_args.name,
        &project_id,
    )
    .await?;
    let session = create_first_party_deployment_session(
        &auth.client,
        &base_url,
        workspace.query.as_deref(),
        &deployment_group.id,
    )
    .await?;

    if !resolved_args.input_values.is_empty() {
        set_first_party_deployment_inputs(
            &base_url,
            &session.token,
            &resolved_args.platform,
            &resolved_args.input_values,
        )
        .await?;
    }

    let create_response = create_deployment_with_group_session(
        &base_url,
        &session.token,
        resolved_args,
        args,
        &project_id,
    )
    .await?;
    let deployment_token = create_response.token.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Server did not return deployment token".to_string(),
        })
    })?;

    info!("   Deployment created: {}", create_response.deployment.id);
    tracker
        .add_deployment(resolved_args.name.clone(), deployment_token, &base_url)
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to track newly created deployment".to_string(),
        })
}

async fn ensure_self_deployment_group(
    http_client: &reqwest::Client,
    base_url: &str,
    workspace: Option<&str>,
    name: &str,
    project_id: &str,
) -> Result<ApiDeploymentGroup> {
    let url = api_url(base_url, "/v1/deployment-groups/by-name", workspace)?;
    let response = http_client
        .put(url)
        .json(&serde_json::json!({
            "name": name,
            "project": project_id,
            "maxDeployments": 1,
        }))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to ensure deployment group".to_string(),
        })?;
    parse_api_response(response, "Failed to ensure deployment group").await
}

async fn create_first_party_deployment_session(
    http_client: &reqwest::Client,
    base_url: &str,
    workspace: Option<&str>,
    deployment_group_id: &str,
) -> Result<FirstPartyDeploymentSession> {
    let path = format!(
        "/v1/deployment-groups/{}/first-party-session",
        urlencoding::encode(deployment_group_id)
    );
    let url = api_url(base_url, &path, workspace)?;
    let response = http_client
        .post(url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create first-party deployment session".to_string(),
        })?;
    parse_api_response(response, "Failed to create first-party deployment session").await
}

async fn set_first_party_deployment_inputs(
    base_url: &str,
    session_token: &str,
    platform: &str,
    input_values: &HashMap<String, serde_json::Value>,
) -> Result<()> {
    let http_client = create_platform_http_client(session_token)?;
    let url = api_url(base_url, "/v1/deployments/first-party-inputs", None)?;
    let response = http_client
        .put(url)
        .json(&serde_json::json!({
            "platform": platform,
            "inputValues": input_values,
        }))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to set first-party deployment inputs".to_string(),
        })?;
    parse_empty_api_response(response, "Failed to set first-party deployment inputs").await
}

async fn create_deployment_with_group_session(
    base_url: &str,
    session_token: &str,
    resolved_args: &ResolvedDeployArgs,
    args: &DeployArgs,
    project_id: &str,
) -> Result<CreateDeploymentApiResponse> {
    let http_client = create_platform_http_client(session_token)?;
    let stack_settings = deployment_stack_settings_json(resolved_args, args)?;
    let mut body = serde_json::json!({
        "name": resolved_args.name,
        "project": project_id,
        "platform": resolved_args.platform,
        "stackSettings": stack_settings,
        "inputValues": {},
        "setupMethod": "cli",
    });

    if let Some(resource_prefix) = args.resource_prefix.as_ref() {
        body["resourcePrefix"] = serde_json::Value::String(resource_prefix.clone());
    }
    if let Some(public_subdomain) = resolved_args.public_subdomain.as_ref() {
        body["publicSubdomain"] = serde_json::Value::String(public_subdomain.clone());
    }

    let url = api_url(base_url, "/v1/deployments", None)?;
    let response = http_client
        .post(url)
        .json(&body)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create deployment".to_string(),
        })?;
    parse_api_response(response, "Failed to create deployment").await
}

fn deployment_stack_settings_json(
    resolved_args: &ResolvedDeployArgs,
    args: &DeployArgs,
) -> Result<serde_json::Value> {
    let mut settings = serde_json::json!({
        "deploymentModel": if resolved_args.platform == "aws" { "push" } else { "pull" },
        "heartbeats": if args.no_heartbeat { "off" } else { "on" },
        "telemetry": match args.monitoring {
            MonitoringMode::Off => "off",
            MonitoringMode::Auto => "auto",
        },
        "updates": "auto",
    });

    if let Some(network_settings) = resolved_args.network_settings.as_ref() {
        settings["network"] = serde_json::to_value(network_settings)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize network settings".to_string(),
            })?;
    }

    Ok(settings)
}

async fn create_machines_join_command(
    base_url: &str,
    deployment_token: &str,
    deployment_id: &str,
    platform: Platform,
) -> Result<String> {
    let info = fetch_deployment_info(base_url, deployment_token, platform)
        .await
        .ok();
    let join_token = create_machines_join_token(base_url, deployment_token, deployment_id).await?;
    let cli_name = info
        .as_ref()
        .and_then(|info| info.packages.as_ref())
        .and_then(|packages| packages.cli.as_ref())
        .and_then(|cli| cli.command_name.as_deref())
        .unwrap_or("alien-deploy");
    let install_script_url = info
        .as_ref()
        .and_then(|info| info.packages.as_ref())
        .and_then(|packages| packages.cli.as_ref())
        .and_then(|cli| cli.install_scripts.as_ref())
        .and_then(|scripts| scripts.linux.as_deref());

    Ok(machines_join_command(
        cli_name,
        install_script_url,
        &join_token,
    ))
}

async fn fetch_deployment_info(
    base_url: &str,
    token: &str,
    platform: Platform,
) -> Result<DeploymentInfoResponse> {
    let http_client = create_platform_http_client(token)?;
    let mut url = api_url(base_url, "/v1/deployment-info", None)?;
    url.query_pairs_mut()
        .append_pair("platform", platform.as_str());
    let response = http_client
        .get(url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to fetch deployment info".to_string(),
        })?;
    parse_api_response(response, "Failed to fetch deployment info").await
}

async fn create_machines_join_token(
    base_url: &str,
    token: &str,
    deployment_id: &str,
) -> Result<String> {
    let http_client = create_platform_http_client(token)?;
    let path = format!(
        "/v1/machines/deployments/{}/join-tokens/rotate",
        urlencoding::encode(deployment_id)
    );
    let response = http_client
        .post(api_url(base_url, &path, None)?)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create Machines join token".to_string(),
        })?;
    let response: MachinesJoinTokenResponse =
        parse_api_response(response, "Failed to create Machines join token").await?;
    normalize_machines_join_token_response(response)
}

fn normalize_machines_join_token_response(response: MachinesJoinTokenResponse) -> Result<String> {
    let join_token = response.join_token.trim();
    if join_token.is_empty() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Platform API returned an empty Machines join token".to_string(),
        }));
    }
    if join_token.starts_with("aj1_") {
        return Ok(join_token.to_string());
    }

    let control_plane_url = response
        .control_plane_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cluster_id = response
        .cluster_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (control_plane_url, cluster_id) {
        (Some(control_plane_url), Some(cluster_id)) => {
            validate_machines_control_plane_url(control_plane_url)?;
            let payload = WrappedMachinesJoinToken {
                join_token,
                control_plane_url,
                cluster_id,
            };
            let json = serde_json::to_vec(&payload).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Failed to encode Machines join token context".to_string(),
                },
            )?;
            Ok(format!("aj1_{}", URL_SAFE_NO_PAD.encode(json)))
        }
        _ => Err(AlienError::new(ErrorData::ConfigurationError {
            message:
                "Platform API returned a raw Machines join token without control plane context"
                    .to_string(),
        })),
    }
}

fn validate_machines_control_plane_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).map_err(|e| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("Platform API returned an invalid Machines control plane URL: {e}"),
        })
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Platform API returned an invalid Machines control plane URL".to_string(),
        }));
    }
    Ok(())
}

fn machines_join_command(
    cli_name: &str,
    install_script_url: Option<&str>,
    join_token: &str,
) -> String {
    if let Some(install_script_url) = install_script_url {
        return format!(
            "curl -fsSL {} | sudo bash -s -- join --token {}",
            shell_single_quote(install_script_url),
            shell_single_quote(join_token)
        );
    }

    format!(
        "sudo {cli_name} join --token {}",
        shell_single_quote(join_token)
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn api_url(base_url: &str, path: &str, workspace: Option<&str>) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(&format!("{}{}", base_url.trim_end_matches('/'), path))
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Invalid platform API base URL".to_string(),
        })?;
    if let Some(workspace) = workspace {
        url.query_pairs_mut().append_pair("workspace", workspace);
    }
    Ok(url)
}

fn create_platform_http_client(token: &str) -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", token))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Invalid authorization header value".to_string(),
            })?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("alien-cli"));

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to create HTTP client".to_string(),
        })
}

async fn parse_api_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    message: &str,
) -> Result<T> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("{message} (HTTP {status}): {body}"),
        }));
    }

    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("{message}: failed to parse response"),
        })
}

async fn parse_empty_api_response(response: reqwest::Response, message: &str) -> Result<()> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("{message} (HTTP {status}): {body}"),
        }));
    }
    Ok(())
}

/// Main entry point for deploy command
pub async fn deploy_task(args: DeployArgs, ctx: ExecutionMode) -> Result<()> {
    let resolved_args = resolve_deploy_args(&args)?;

    if let ExecutionMode::Dev { port } = ctx {
        return deploy_local_dev_task(resolved_args, port).await;
    }

    info!("Starting deploy command");
    println!(
        "{}",
        contextual_heading(
            "Deploying",
            &resolved_args.name,
            &[("to", &resolved_args.platform)]
        )
    );
    let steps = if resolved_args.platform_enum == Platform::Machines {
        FixedSteps::new(&["Resolve deployment", "Create join command"])
    } else {
        FixedSteps::new(&[
            "Resolve deployment",
            "Connect to manager",
            "Provision resources",
            "Activate",
        ])
    };
    steps.activate(0, Some(resolved_args.name.clone()));

    let platform = resolved_args.platform_enum;

    let base_url = ctx.base_url();

    // Step 1: Load or register the deployment (via platform API)
    let mut tracker = DeploymentTracker::new()?;
    let tracked_deployment = match tracker.get_deployment(&resolved_args.name) {
        Some(deployment) => {
            info!("Found tracked deployment '{}'", resolved_args.name);
            if resolved_args.public_subdomain.is_some() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "public-subdomain".to_string(),
                    message: "--public-subdomain can only be set when creating a new deployment."
                        .to_string(),
                }));
            }

            // If a token was provided, check if it's different from the stored one
            if let Some(ref provided_token) = args.token {
                if deployment.api_key != *provided_token {
                    info!(
                        "Updating stored API key for deployment '{}'",
                        resolved_args.name
                    );
                    tracker.remove_deployment(&resolved_args.name)?;
                    tracker
                        .add_deployment(
                            resolved_args.name.clone(),
                            provided_token.clone(),
                            &base_url,
                        )
                        .await
                        .context(ErrorData::ConfigurationError {
                            message: "Failed to update deployment API key".to_string(),
                        })?
                } else {
                    deployment.clone()
                }
            } else {
                deployment.clone()
            }
        }
        None => {
            info!(
                "Deployment '{}' not tracked yet, registering...",
                resolved_args.name
            );

            if let Some(token) = args.token.as_ref() {
                let token_info = validate_token(token, &base_url).await?;

                match token_info {
                    DeploymentToken::Deployment { .. } => {
                        info!("   Using deployment token");
                        tracker
                            .add_deployment(resolved_args.name.clone(), token.clone(), &base_url)
                            .await
                            .context(ErrorData::ConfigurationError {
                                message: "Failed to register deployment".to_string(),
                            })?
                    }
                    DeploymentToken::DeploymentGroup {
                        deployment_group_name,
                        workspace_name,
                        project_id,
                        ..
                    } => {
                        info!(
                            "   Using deployment group token for group '{}'",
                            deployment_group_name
                        );
                        info!("   Creating new deployment '{}'...", resolved_args.name);

                        let sdk_client = create_platform_client(token, &base_url)?;

                        let sdk_network = resolved_args
                            .network_settings
                            .clone()
                            .map(|network_settings| {
                                let json = serde_json::to_value(&network_settings)
                                    .into_alien_error()
                                    .context(ErrorData::ConfigurationError {
                                        message: "Failed to serialize network settings".to_string(),
                                    })?;
                                serde_json::from_value(json).into_alien_error().context(
                                    ErrorData::ConfigurationError {
                                        message: "Failed to convert network settings to SDK type"
                                            .to_string(),
                                    },
                                )
                            })
                            .transpose()?;

                        // AWS managed deployments require push model; pull is for K8s/manager-side.
                        let deployment_model = if resolved_args.platform == "aws" {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Push
                        } else {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Pull
                        };
                        let stack_settings = alien_platform_api::types::NewDeploymentRequestStackSettings {
                        compute: None,
                        deployment_model: Some(deployment_model),
                        heartbeats: Some(if args.no_heartbeat {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsHeartbeats::Off
                        } else {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsHeartbeats::On
                        }),
                        telemetry: Some(match args.monitoring {
                            MonitoringMode::Off => alien_platform_api::types::NewDeploymentRequestStackSettingsTelemetry::Off,
                            MonitoringMode::Auto => alien_platform_api::types::NewDeploymentRequestStackSettingsTelemetry::Auto,
                        }),
                        updates: Some(alien_platform_api::types::NewDeploymentRequestStackSettingsUpdates::Auto),
                        network: sdk_network,
                        domains: None,
                        external_bindings: None,
                        kubernetes: None,
                    };

                        let create_response = sdk_client
                            .create_deployment()
                            .workspace(&workspace_name)
                            .body(alien_platform_api::types::NewDeploymentRequest {
                                name: resolved_args
                                    .name
                                    .clone()
                                    .try_into()
                                    .into_alien_error()
                                    .context(ErrorData::ValidationError {
                                        field: "name".to_string(),
                                        message: "Invalid deployment name".to_string(),
                                    })?,
                                platform: resolved_args
                                    .platform
                                    .as_str()
                                    .try_into()
                                    .into_alien_error()
                                    .context(ErrorData::ValidationError {
                                        field: "platform".to_string(),
                                        message: "Invalid platform value".to_string(),
                                    })?,
                                project: project_id.clone().try_into().into_alien_error().context(
                                    ErrorData::ValidationError {
                                        field: "project".to_string(),
                                        message: "Invalid project".to_string(),
                                    },
                                )?,
                                stack_settings: Some(stack_settings),
                                resource_prefix: args
                                    .resource_prefix
                                    .clone()
                                    .map(TryInto::try_into)
                                    .transpose()
                                    .into_alien_error()
                                    .context(ErrorData::ValidationError {
                                        field: "resource_prefix".to_string(),
                                        message: "Invalid resource prefix".to_string(),
                                    })?,
                                manager_id: None,
                                operator_permission: None,
                                operator_scope: None,
                                pinned_release_id: None,
                                environment_variables: None,
                                deployment_group_id: None,
                                environment_info: None,
                                input_values: HashMap::new(),
                                public_subdomain: None,
                                initial_desired_release: alien_platform_api::types::NewDeploymentRequestInitialDesiredRelease::Active,
                                setup_method: None,
                                setup_metadata: None,
                            })
                            .send()
                            .await
                            .into_alien_error()
                            .context(ErrorData::ConfigurationError {
                                message: "Failed to create deployment with deployment group token"
                                    .to_string(),
                            })?
                            .into_inner();

                        let response_json = serde_json::to_value(&create_response)
                            .into_alien_error()
                            .context(ErrorData::ConfigurationError {
                                message: "Failed to serialize response".to_string(),
                            })?;

                        let deployment_id = response_json
                            .get("deployment")
                            .and_then(|d| d.get("id"))
                            .and_then(|id| id.as_str())
                            .ok_or_else(|| {
                                AlienError::new(ErrorData::ConfigurationError {
                                    message: "Failed to extract deployment ID from response"
                                        .to_string(),
                                })
                            })?
                            .to_string();

                        let deployment_token = response_json
                            .get("token")
                            .and_then(|t| t.as_str())
                            .ok_or_else(|| {
                                AlienError::new(ErrorData::ConfigurationError {
                                    message: "Server did not return deployment token".to_string(),
                                })
                            })?
                            .to_string();

                        info!("   Deployment created: {}", deployment_id);

                        tracker
                            .add_deployment(resolved_args.name.clone(), deployment_token, &base_url)
                            .await
                            .context(ErrorData::ConfigurationError {
                                message: "Failed to track newly created deployment".to_string(),
                            })?
                    }
                }
            } else {
                create_self_deployment(&ctx, &mut tracker, &resolved_args, &args).await?
            }
        }
    };

    steps.complete(
        0,
        Some(format!(
            "{} ({})",
            resolved_args.name, tracked_deployment.deployment_id
        )),
    );

    if platform == Platform::Machines {
        steps.activate(1, Some("Rotating join token".to_string()));
        let join_command = create_machines_join_command(
            &base_url,
            &tracked_deployment.api_key,
            &tracked_deployment.deployment_id,
            resolved_args.platform_enum,
        )
        .await?;
        steps.complete(1, Some("Join command ready".to_string()));
        drop(steps);
        println!(
            "{}",
            success_line("Machines deployment is ready to join hosts.")
        );
        println!();
        println!("{}", command(&join_command));
        return Ok(());
    }

    // Step 2: Resolve manager
    steps.activate(1, Some("Discovering manager...".to_string()));

    let manager_ctx = ctx
        .resolve_manager(&tracked_deployment.project_id, &resolved_args.platform)
        .await?;
    // Provisioning calls the manager's sync endpoints, which require
    // `managers.sync` — held by the deployment's own token, not the install
    // token that resolved the manager. In platform mode (workspace is set),
    // re-authenticate as the deployment for these calls.
    let manager_client = if let Some(workspace) = manager_ctx.workspace.clone() {
        let http_client = crate::auth::client_with_auth_and_workspace(
            &format!("Bearer {}", tracked_deployment.api_key),
            &workspace,
        )?;
        alien_manager_api::Client::new_with_client(&manager_ctx.manager_url, http_client)
    } else {
        manager_ctx.client
    };

    steps.complete(1, Some(format!("Manager: {}", manager_ctx.manager_url)));

    // Step 3: Initialize with manager and run deployment
    steps.activate(2, Some(tracked_deployment.deployment_id.clone()));

    // Get deployment state from manager
    let deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to get deployment '{}' from manager.",
                tracked_deployment.deployment_id
            ),
        })?
        .into_inner();

    let status: DeploymentStatus =
        serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("Unknown deployment status: {}", deployment.status),
            })?;

    // Get cloud credentials from environment
    use alien_infra::ClientConfigExt;
    let client_config =
        ClientConfig::from_std_env(platform)
            .await
            .context(ErrorData::ConfigurationError {
                message: format!("Failed to build client config for platform {:?}", platform),
            })?;

    // Build deployment state
    let mut current = DeploymentState {
        status,
        platform,
        current_release: None,
        target_release: None,
        stack_state: deployment
            .stack_state
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize stack_state".to_string(),
            })?,
        error: None,
        environment_info: deployment
            .environment_info
            .map(serde_json::from_value)
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize environment_info".to_string(),
            })?,
        runtime_metadata: deployment
            .runtime_metadata
            .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
            .transpose()
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize runtime_metadata".to_string(),
            })?,
        retry_requested: deployment.retry_requested,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    };

    // Standalone path: the deployment record carries a `desiredReleaseId` and
    // the platform-mode CLI normally relies on the manager to inject the
    // target_release. The standalone manager doesn't do that injection on
    // get_deployment, so fetch the release directly here and populate
    // `target_release` ourselves — otherwise pending::handle_pending fails
    // immediately with "Target release required for deployment".
    if current.target_release.is_none() {
        if let Some(release_id) = deployment.desired_release_id.as_ref() {
            let url = format!("{}/v1/releases/{}", manager_ctx.manager_url, release_id);
            if let Ok(resp) = manager_ctx
                .http_client
                .get(&url)
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", tracked_deployment.api_key),
                )
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(release_json) = resp.json::<serde_json::Value>().await {
                        let stack_for_platform = release_json
                            .get("stack")
                            .and_then(|s| s.get(resolved_args.platform.as_str()))
                            .cloned();
                        if let Some(stack_json) = stack_for_platform {
                            if let Ok(stack) =
                                serde_json::from_value::<alien_core::Stack>(stack_json)
                            {
                                current.target_release = Some(alien_core::ReleaseInfo {
                                    release_id: Some(release_id.clone()),
                                    version: None,
                                    description: None,
                                    stack,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Running deploy on a failed deployment is an implicit retry request
    if current.status.is_failed() {
        info!(
            "Deployment is in {:?} state, setting retry_requested to proceed",
            current.status
        );
        current.retry_requested = true;
    }

    if let Some(stack_state) = current.stack_state.as_ref() {
        steps.sync_deployment_resources(&stack_state.resources);
    }

    // Build minimal deployment config
    let stack_settings: alien_core::StackSettings = deployment
        .stack_settings
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_settings".to_string(),
        })?
        .unwrap_or_default();

    let mut config: alien_core::DeploymentConfig = serde_json::from_value(serde_json::json!({
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

    let setup_owned_status = matches!(
        current.status,
        DeploymentStatus::Pending
            | DeploymentStatus::PreflightsFailed
            | DeploymentStatus::InitialSetup
            | DeploymentStatus::InitialSetupFailed
    );

    // Acquire → step loop → reconcile → release (all via manager)
    let session = format!("cli-deploy-{}", Uuid::new_v4());
    if setup_owned_status {
        acquire_setup_run_deployment(
            &manager_client,
            &tracked_deployment.deployment_id,
            &session,
            stack_settings.deployment_model,
        )
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to acquire setup deployment lock".to_string(),
        })?;
    } else {
        acquire_deployment(
            &manager_client,
            &tracked_deployment.deployment_id,
            &session,
            stack_settings.deployment_model,
        )
        .await
        .context(ErrorData::ConfigurationError {
            message: "Failed to acquire deployment lock".to_string(),
        })?;
    }

    // Re-fetch under lock (manager may have advanced the state)
    let deployment = manager_client
        .get_deployment()
        .id(&tracked_deployment.deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to re-fetch deployment under lock".to_string(),
        })?
        .into_inner();

    current.status = serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Unknown deployment status: {}", deployment.status),
        })?;
    current.stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize stack_state".to_string(),
        })?;
    current.runtime_metadata = deployment
        .runtime_metadata
        .map(|rm| serde_json::to_value(rm).and_then(serde_json::from_value))
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to deserialize runtime_metadata".to_string(),
        })?;

    let transport = ManagerApiTransport::new(manager_client.clone(), session.clone());
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: if setup_owned_status {
            LoopOperation::InitialSetup
        } else {
            LoopOperation::Deploy
        },
        delay_threshold: None,
    };

    let runner_result = alien_deployment::runner::run_step_loop(
        &mut current,
        &mut config,
        &client_config,
        &tracked_deployment.deployment_id,
        &policy,
        &transport,
        None,
        None,
    )
    .await;

    // Always reconcile + release, even on error
    final_reconcile(
        &manager_client,
        &tracked_deployment.deployment_id,
        &session,
        &current,
    )
    .await;
    release_deployment(&manager_client, &tracked_deployment.deployment_id, &session).await;

    let RunnerResult {
        loop_result,
        steps_executed,
    } = runner_result.context(ErrorData::GenericError {
        message: "deployment step loop failed".to_string(),
    })?;

    info!(
        steps_executed = steps_executed,
        stop_reason = ?loop_result.stop_reason,
        outcome = ?loop_result.outcome,
        final_status = ?loop_result.final_status,
        "Deployment loop finished"
    );

    // Handle runner outcome
    match loop_result.outcome {
        LoopOutcome::Success => {
            steps.complete(2, Some("Resources ready".to_string()));
            steps.complete(3, Some("Running".to_string()));
        }
        LoopOutcome::Failure => {
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: format!(
                    "{} failed",
                    describe_failed_status(&loop_result.final_status)
                ),
            }));
        }
        LoopOutcome::Neutral if loop_result.stop_reason == LoopStopReason::Handoff => {
            steps.complete(2, Some("Resources ready".to_string()));
            steps.complete(3, Some("Running".to_string()));
        }
        LoopOutcome::Neutral => {
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: format!(
                    "deployment loop ended without resolution (stop_reason: {:?}, status: {:?})",
                    loop_result.stop_reason, loop_result.final_status
                ),
            }));
        }
    }
    drop(steps);

    println!("{}", success_line("Deployment is running."));
    println!(
        "{} {} ({})",
        dim_label("Deployment"),
        resolved_args.name,
        tracked_deployment.deployment_id
    );
    println!(
        "{} {}",
        dim_label("Next"),
        command(&format!(
            "alien deployments get {}",
            tracked_deployment.deployment_id
        ))
    );

    Ok(())
}

fn describe_failed_status(status: &alien_deployment::DeploymentStatus) -> &'static str {
    match status {
        alien_deployment::DeploymentStatus::PreflightsFailed => "preflights",
        alien_deployment::DeploymentStatus::InitialSetupFailed => "initial setup",
        alien_deployment::DeploymentStatus::ProvisioningFailed => "provisioning",
        alien_deployment::DeploymentStatus::UpdateFailed => "update",
        alien_deployment::DeploymentStatus::DeleteFailed => "deletion",
        alien_deployment::DeploymentStatus::TeardownFailed => "setup teardown",
        alien_deployment::DeploymentStatus::RefreshFailed => "refresh",
        _ => "deployment",
    }
}

async fn deploy_local_dev_task(args: ResolvedDeployArgs, port: u16) -> Result<()> {
    if args.platform != "local" {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "platform".to_string(),
            message: "alien dev deploy only supports --platform local".to_string(),
        }));
    }

    println!(
        "{}",
        contextual_heading("Creating local deployment", &args.name, &[])
    );

    let steps = FixedSteps::new(&["Prepare deployment", "Wait for deployment"]);
    steps.activate(0, Some(args.name.clone()));
    let deployment_id = create_initial_deployment(&args.name, port, None).await?;
    steps.complete(0, Some(format!("{} ({})", args.name, deployment_id)));

    steps.activate(1, Some(format!("{} ({})", args.name, "queued")));
    let snapshot = wait_for_dev_deployment_ready_with_progress(port, &args.name, None, |status| {
        steps.activate(
            1,
            Some(format!(
                "{} ({})",
                args.name,
                crate::ui::format_deployment_status(status).to_ascii_lowercase()
            )),
        );
    })
    .await?;
    steps.complete(1, Some(format!("{} ready", args.name)));
    drop(steps);

    println!("{}", success_line("Deployment ready."));
    println!(
        "{} {} ({})",
        dim_label("Deployment"),
        snapshot.deployment_name,
        snapshot.deployment_id
    );
    let live_state = fetch_dev_deployment_live_state(port, &snapshot.deployment_name).await?;
    let stack_state = live_state
        .as_ref()
        .and_then(|state| state.stack_state.as_ref());
    if snapshot.resources.is_empty() && stack_state.is_none() {
        println!("{}", dim_label("No resources were reported yet."));
    } else {
        println!("{}", dim_label("Resources"));
        let mut resource_names = std::collections::BTreeSet::new();
        resource_names.extend(snapshot.resources.keys().cloned());
        if let Some(stack_state) = stack_state {
            resource_names.extend(stack_state.resources.keys().cloned());
        }

        for name in resource_names {
            let public_resource = snapshot.resources.get(&name);
            let stack_resource = stack_state.and_then(|state| state.resources.get(&name));
            let rendered_value =
                format_local_dev_resource_value(&name, public_resource, stack_resource);
            let resource_type = public_resource
                .and_then(|resource| resource.resource_type.as_ref().map(|value| value.as_str()))
                .or_else(|| stack_resource.map(|resource| resource.resource_type.as_str()));
            println!(
                "  - {}{}{}",
                name,
                resource_type
                    .map(|resource_type| format!(" ({resource_type})"))
                    .unwrap_or_default(),
                format!(": {}", rendered_value)
            );
        }
    }
    println!(
        "{} inspect it with {}",
        dim_label("Next"),
        command(&format!(
            "alien dev deployments get {}",
            snapshot.deployment_name
        ))
    );

    Ok(())
}

fn format_local_dev_resource_value(
    name: &str,
    public_resource: Option<&alien_core::DevResourceInfo>,
    stack_resource: Option<&alien_core::StackResourceState>,
) -> String {
    if let Some(public_resource) = public_resource {
        if is_local_private_url(&public_resource.url) {
            if name == "worker"
                || public_resource
                    .resource_type
                    .as_deref()
                    .is_some_and(|resource_type| resource_type.eq_ignore_ascii_case("worker"))
            {
                return "running (private)".to_string();
            }
            if public_resource
                .resource_type
                .as_deref()
                .is_some_and(|resource_type| resource_type.eq_ignore_ascii_case("storage"))
            {
                return "local filesystem".to_string();
            }
        }
        return public_resource.url.clone();
    }

    let Some(stack_resource) = stack_resource else {
        return "running".to_string();
    };

    match stack_resource.status {
        alien_core::ResourceStatus::Running
            if stack_resource.resource_type.eq_ignore_ascii_case("storage") =>
        {
            "local filesystem".to_string()
        }
        alien_core::ResourceStatus::Running => "running (private)".to_string(),
        _ => crate::ui::format_resource_status(stack_resource.status)
            .to_ascii_lowercase()
            .replace(' ', "-"),
    }
}

fn is_local_private_url(url: &str) -> bool {
    url.starts_with("http://localhost:")
        || url.starts_with("https://localhost:")
        || url.starts_with("http://127.0.0.1:")
        || url.starts_with("https://127.0.0.1:")
}
