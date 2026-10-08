//! Deploy command — creates or updates a deployment via the manager.
//!
//! Flow:
//! 1. Resolve/create deployment (via platform API for DG tokens, or from tracker)
//! 2. Discover manager URL (resolve_manager for OAuth, DG endpoint for DG tokens)
//! 3. Run step loop via manager (acquire → step → reconcile → release)

#[path = "deploy_setup_validation.rs"]
mod setup_validation;

use crate::commands::deployments::{parse_resource_prefix, MonitoringMode};

use crate::commands::{
    create_initial_deployment, fetch_dev_deployment_live_state,
    wait_for_dev_deployment_ready_with_progress,
};
use crate::deployment_tracking::{
    validate_token, DeploymentToken, DeploymentTracker, ValidatedDeploymentInfo,
};
use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::ui::{command, contextual_heading, dim_label, success_line, FixedSteps};
use alien_cli_common::network::{self, NetworkArgs, NetworkMode};
use alien_core::{
    ClientConfig, ComputeSettings, DeploymentState, DeploymentStatus, NetworkSettings, Platform,
};
use alien_deployment::loop_contract::{LoopOperation, LoopOutcome, LoopStopReason};
use alien_deployment::manager_api_transport::{
    acquire_deployment_with_payload, acquire_setup_run_deployment,
    combine_operation_and_finalization, final_reconcile, finalize_step_loop, ManagerApiTransport,
};
use alien_deployment::runner::{RunnerPolicy, RunnerResult};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_infra::ClientConfigExt;
use alien_platform_api::types::DeploymentUpdateOperationStatus;
use alien_platform_api::Client as SdkClient;
use alien_platform_api::SdkResultExt as _;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use clap::Parser;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU64;
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

    # Deploy into an existing customer deployment group
    alien deploy --deployment-group customer-123 --name production --platform aws

    # Create a Machines deployment and print a host join command
    alien deploy --name eu-prod --machines

    # Set up a deployment from a customer deployment-group token
    alien deploy --token dg_abc123... --name production --platform aws

    # Deploy an existing deployment (uses stored API key)
    alien deploy --name production --platform aws

    # Read a deployment token from a protected file
    alien deploy --token-file deployment-token.txt --name prod --platform aws

    # Deploy without heartbeat capability
    alien deploy --token ax_deployment_xyz... --name prod --platform aws --no-heartbeat"
)]
pub struct DeployArgs {
    /// Deployment API key for authentication (optional if deployment is already tracked)
    #[arg(long, conflicts_with = "token_file")]
    pub token: Option<String>,

    /// Read the deployment API key from a file instead of exposing it in command arguments.
    #[arg(long)]
    pub token_file: Option<PathBuf>,

    /// Deployment name for identification in tracking
    #[arg(long)]
    pub name: Option<String>,

    /// Existing deployment group ID, name, or external ID for a new deployment.
    /// The group is inferred when --token contains a deployment-group key.
    #[arg(long, conflicts_with_all = ["token", "token_file"])]
    pub deployment_group: Option<String>,

    /// Target platform for the deployment (aws, gcp, azure, machines)
    #[arg(long, conflicts_with = "machines")]
    pub platform: Option<String>,

    /// Create or update a Machines deployment and print a host join command
    #[arg(long)]
    pub machines: bool,

    /// TOML file containing deployment settings.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Validate the config locally without authentication, API calls, or deployment tracking.
    #[arg(long, requires = "config")]
    pub validate_only: bool,

    /// Stack input value for setup (id=value).
    #[arg(long = "input")]
    pub input_values: Vec<String>,

    /// Secret stack input value for setup (id=value).
    #[arg(long = "secret-input")]
    pub secret_input_values: Vec<String>,

    /// Customer setup item to install, for a deployment-group token whose group
    /// offers more than one. The API rejects the request without it.
    #[arg(long = "setup-item")]
    pub setup_item: Option<String>,

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

    /// Release channel followed by a newly created deployment.
    #[arg(long, default_value = "production")]
    pub channel: String,

    #[command(flatten)]
    pub network: NetworkArgs,
}

impl DeployArgs {
    fn resolve_token_file(&mut self) -> Result<()> {
        let Some(path) = self.token_file.as_deref() else {
            return Ok(());
        };
        let raw = std::fs::read_to_string(path).into_alien_error().context(
            ErrorData::ConfigurationError {
                message: format!("Failed to read token file {}", path.display()),
            },
        )?;
        let token = raw.trim();
        if token.is_empty() {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "token-file".to_string(),
                message: format!("Token file {} is empty", path.display()),
            }));
        }
        self.token = Some(token.to_owned());
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ResolvedDeployArgs {
    name: String,
    platform: String,
    platform_enum: Platform,
    network_settings: Option<NetworkSettings>,
    compute_settings: Option<ComputeSettings>,
    domain_settings: Option<alien_core::DomainSettings>,
    input_values: HashMap<String, serde_json::Value>,
    public_subdomain: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeployConfigFile {
    name: Option<String>,
    platform: Option<String>,
    network: Option<DeployConfigNetwork>,
    compute: Option<ComputeSettings>,
    domains: Option<alien_core::DomainSettings>,
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
    if let Some(settings) = network_settings.as_ref() {
        network::validate_network_settings_for_platform(settings, platform_enum).map_err(
            |message| {
                AlienError::new(ErrorData::ValidationError {
                    field: "network".to_string(),
                    message,
                })
            },
        )?;
    }
    let compute_settings = config.as_ref().and_then(|config| config.compute.clone());
    validate_compute_settings(compute_settings.as_ref())?;
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
        compute_settings,
        domain_settings: config.as_ref().and_then(|config| config.domains.clone()),
        input_values,
        public_subdomain: args.public_subdomain.clone(),
    })
}

fn validate_compute_settings(compute: Option<&ComputeSettings>) -> Result<()> {
    let Some(compute) = compute else {
        return Ok(());
    };
    for (pool, selection) in &compute.pools {
        selection.validate().map_err(|message| {
            AlienError::new(ErrorData::ValidationError {
                field: format!("compute.pools.{pool}"),
                message,
            })
        })?;
    }
    Ok(())
}

fn to_sdk_compute_settings(
    compute: Option<ComputeSettings>,
) -> Result<Option<alien_platform_api::types::NewDeploymentRequestStackSettingsCompute>> {
    compute
        .map(|compute_settings| {
            let json = serde_json::to_value(&compute_settings)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to serialize compute settings".to_string(),
                })?;
            serde_json::from_value(json)
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Failed to convert compute settings to SDK type".to_string(),
                })
        })
        .transpose()
}

fn read_deploy_config(path: &Path) -> Result<DeployConfigFile> {
    let resolved_path = resolved_config_path(path);
    let contents = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::FileOperationFailed {
            operation: "read".to_string(),
            file_path: resolved_path.display().to_string(),
            reason: format!(
                "Failed to read deploy config '{}' (relative paths are resolved from the current working directory)",
                path.display()
            ),
        },
    )?;
    toml::from_str(&contents)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!(
                "Failed to parse deploy config '{}' (resolved as '{}')",
                path.display(),
                resolved_path.display()
            ),
        })
}

fn resolved_config_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
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
        values.insert(id, parse_raw_stack_input_value(&value));
    }
    for input in secret_input_values {
        let (id, value) = parse_stack_input_arg(input, "--secret-input")?;
        values.insert(id, serde_json::Value::String(value));
    }

    Ok(values)
}

fn parse_raw_stack_input_value(value: &str) -> serde_json::Value {
    match serde_json::from_str::<Vec<String>>(value) {
        Ok(values) => {
            serde_json::Value::Array(values.into_iter().map(serde_json::Value::String).collect())
        }
        Err(_) => serde_json::Value::String(value.to_string()),
    }
}

fn to_sdk_stack_input_values(
    values: &HashMap<String, serde_json::Value>,
) -> Result<HashMap<String, alien_platform_api::types::StackInputValueRequest>> {
    values
        .iter()
        .map(|(id, value)| {
            let value = match value {
                serde_json::Value::String(value) => {
                    alien_platform_api::types::StackInputValueRequest::Variant0(value.clone())
                }
                serde_json::Value::Number(value) => {
                    let value = value.as_f64().ok_or_else(|| {
                        AlienError::new(ErrorData::ValidationError {
                            field: id.clone(),
                            message: "Stack input number must be finite.".to_string(),
                        })
                    })?;
                    alien_platform_api::types::StackInputValueRequest::Variant1(value)
                }
                serde_json::Value::Bool(value) => {
                    alien_platform_api::types::StackInputValueRequest::Variant2(*value)
                }
                serde_json::Value::Array(values)
                    if values.iter().all(serde_json::Value::is_string) =>
                {
                    alien_platform_api::types::StackInputValueRequest::Variant3(
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(ToString::to_string))
                            .collect(),
                    )
                }
                _ => {
                    return Err(AlienError::new(ErrorData::ValidationError {
                        field: id.clone(),
                        message: "Stack input must be a string, number, boolean, or string list."
                            .to_string(),
                    }));
                }
            };
            Ok((id.clone(), value))
        })
        .collect()
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
    project_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiDeploymentGroupList {
    items: Vec<ApiDeploymentGroupListItem>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiDeploymentGroupListItem {
    id: String,
    name: String,
    external_id: Option<String>,
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
            message: "Pass --token: this manager creates deployments with a deployment-group token (from `alien onboard`)."
                .to_string(),
        }));
    }

    let base_url = ctx.base_url();
    let auth = ctx.auth_http().await?;
    let workspace = ctx.resolve_platform_workspace_context(true).await?;
    let (project_id, _project_link) = ctx.resolve_project(None, true).await?;

    let deployment_group = match args.deployment_group.as_deref() {
        Some(reference) => {
            resolve_self_deployment_group(
                &auth.client,
                &base_url,
                workspace.query.as_deref(),
                &project_id,
                reference,
            )
            .await?
        }
        None => {
            ensure_self_deployment_group(
                &auth.client,
                &base_url,
                workspace.query.as_deref(),
                &resolved_args.name,
                &project_id,
            )
            .await?
        }
    };
    let session = create_first_party_deployment_session(
        &auth.client,
        &base_url,
        workspace.query.as_deref(),
        &deployment_group.id,
    )
    .await?;

    // Bind the preparation request to this exact channel even without inputs.
    set_first_party_deployment_inputs(
        &base_url,
        &session.token,
        &resolved_args.platform,
        &resolved_args.input_values,
        &args.channel,
    )
    .await?;

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

async fn resolve_self_deployment_group(
    http_client: &reqwest::Client,
    base_url: &str,
    workspace: Option<&str>,
    project_id: &str,
    reference: &str,
) -> Result<ApiDeploymentGroup> {
    if reference.starts_with("dg_") {
        let path = format!("/v1/deployment-groups/{}", urlencoding::encode(reference));
        let url = api_url(base_url, &path, workspace)?;
        let response = http_client
            .get(url)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("Failed to resolve deployment group {reference}"),
                url: None,
            })?;
        let group: ApiDeploymentGroup =
            parse_api_response(response, "Failed to resolve deployment group").await?;
        if group.project_id != project_id {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "deployment-group".to_string(),
                message: format!(
                    "Deployment group '{reference}' belongs to a different project. Pass a group from the selected project."
                ),
            }));
        }
        return Ok(group);
    }

    let mut matches = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut url = api_url(base_url, "/v1/deployment-groups", workspace)?;
        url.query_pairs_mut()
            .append_pair("project", project_id)
            .append_pair(
                "limit",
                &NonZeroU64::new(100)
                    .expect("constant is non-zero")
                    .to_string(),
            );
        if let Some(cursor) = cursor.as_deref() {
            url.query_pairs_mut().append_pair("cursor", cursor);
        }
        let response = http_client
            .get(url)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("Failed to resolve deployment group {reference}"),
                url: None,
            })?;
        let groups: ApiDeploymentGroupList =
            parse_api_response(response, "Failed to list deployment groups").await?;
        matches.extend(groups.items.into_iter().filter(|group| {
            group.name == reference || group.external_id.as_deref() == Some(reference)
        }));
        cursor = groups.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    match matches.as_slice() {
        [group] => Ok(ApiDeploymentGroup {
            id: group.id.clone(),
            project_id: project_id.to_string(),
        }),
        [] => Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment-group".to_string(),
            message: format!(
                "Deployment group '{reference}' was not found in this project. Run `alien onboard <customer-name>` or pass an existing group ID, name, or external ID."
            ),
        })),
        _ => Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment-group".to_string(),
            message: format!(
                "Deployment group reference '{reference}' is ambiguous. Pass the group ID instead."
            ),
        })),
    }
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
    release_channel: &str,
) -> Result<()> {
    let http_client = create_platform_http_client(session_token)?;
    let url = api_url(base_url, "/v1/deployments/first-party-inputs", None)?;
    let response = http_client
        .put(url)
        .json(&first_party_inputs_request_body(
            platform,
            input_values,
            release_channel,
        ))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to set first-party deployment inputs".to_string(),
        })?;
    parse_empty_api_response(response, "Failed to set first-party deployment inputs").await
}

fn first_party_inputs_request_body(
    platform: &str,
    input_values: &HashMap<String, serde_json::Value>,
    release_channel: &str,
) -> serde_json::Value {
    serde_json::json!({
        "platform": platform,
        "inputValues": input_values,
        "releaseChannel": release_channel,
    })
}

async fn create_deployment_with_group_session(
    base_url: &str,
    session_token: &str,
    resolved_args: &ResolvedDeployArgs,
    args: &DeployArgs,
    project_id: &str,
) -> Result<CreateDeploymentApiResponse> {
    setup_validation::validate_before_creation(base_url, session_token, resolved_args, args)
        .await?;
    let http_client = create_platform_http_client(session_token)?;
    let body = deployment_create_request_body(resolved_args, args, project_id)?;

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

pub(crate) fn deployment_manager_http_client(
    deployment_token: &str,
    workspace: Option<&str>,
) -> Result<reqwest::Client> {
    let deployment_auth = format!("Bearer {deployment_token}");
    match workspace {
        Some(workspace) => crate::auth::client_with_auth_and_workspace(&deployment_auth, workspace),
        None => crate::auth::client_with_header(&deployment_auth),
    }
}

fn deployment_create_request_body(
    resolved_args: &ResolvedDeployArgs,
    args: &DeployArgs,
    project_id: &str,
) -> Result<serde_json::Value> {
    let stack_settings = deployment_stack_settings_json(resolved_args, args)?;
    let mut body = serde_json::json!({
        "name": resolved_args.name,
        "project": project_id,
        "platform": resolved_args.platform,
        "stackSettings": stack_settings,
        "inputValues": {},
        "releaseChannel": args.channel,
        "setupMethod": "cli",
    });

    if let Some(resource_prefix) = args.resource_prefix.as_ref() {
        body["resourcePrefix"] = serde_json::Value::String(resource_prefix.clone());
    }
    if let Some(public_subdomain) = resolved_args.public_subdomain.as_ref() {
        body["publicSubdomain"] = serde_json::Value::String(public_subdomain.clone());
    }
    Ok(body)
}

fn deployment_stack_settings_json(
    resolved_args: &ResolvedDeployArgs,
    args: &DeployArgs,
) -> Result<serde_json::Value> {
    let mut settings = serde_json::json!({
        "deploymentModel": if uses_push_deployment_model(resolved_args.platform_enum) {
            "push"
        } else {
            "pull"
        },
        "heartbeats": if args.no_heartbeat { "off" } else { "on" },
        "telemetry": match args.monitoring {
            MonitoringMode::Off => "off",
            MonitoringMode::Auto => "auto",
        },
        "updates": "auto",
    });

    if let Some(access) = args.network.endpoint_access {
        settings["endpointAccess"] = serde_json::json!(access);
    }

    if let Some(network_settings) = resolved_args.network_settings.as_ref() {
        settings["network"] = serde_json::to_value(network_settings)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize network settings".to_string(),
            })?;
    }

    if let Some(compute_settings) = resolved_args.compute_settings.as_ref() {
        settings["compute"] = serde_json::to_value(compute_settings)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to serialize compute settings".to_string(),
            })?;
    }

    if let Some(domains) = resolved_args.domain_settings.as_ref() {
        settings["domains"] = serde_json::to_value(domains).into_alien_error().context(
            ErrorData::ConfigurationError {
                message: "Failed to serialize domain settings".to_string(),
            },
        )?;
    }

    Ok(settings)
}

fn uses_push_deployment_model(platform: Platform) -> bool {
    matches!(
        platform,
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Machines | Platform::Test
    )
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
    let environment = std::env::vars().collect();
    deploy_task_with_environment(args, ctx, &environment).await
}

async fn deploy_task_with_environment(
    mut args: DeployArgs,
    ctx: ExecutionMode,
    environment: &HashMap<String, String>,
) -> Result<()> {
    args.resolve_token_file()?;

    #[cfg(not(feature = "platform"))]
    if args.channel != "production" {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "This manager doesn't support release channels: every release goes to every deployment.".to_string(),
        }));
    }

    let resolved_args = resolve_deploy_args(&args)?;

    if args.validate_only {
        println!("Deployment config is valid.");
        return Ok(());
    }

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

    // Validate runner-local provider configuration before creating any durable
    // deployment record or token. Machines does not use a local cloud client.
    let client_config = if platform == Platform::Machines {
        None
    } else {
        Some(
            ClientConfig::from_env(platform, environment)
                .await
                .context(ErrorData::ConfigurationError {
                    message: format!("Failed to build client config for platform {:?}", platform),
                })?,
        )
    };

    let base_url = ctx.base_url();

    // Step 1: Load or register the deployment (via platform API)
    let mut tracker = DeploymentTracker::new()?;
    // Only the platform API is asked whether a stored key is still live; in dev and
    // standalone mode `base_url` is a manager, whose deployments it cannot speak for.
    let existing_deployment = if ctx.is_platform() {
        tracker
            .resolve_live_deployment(&resolved_args.name, &base_url)
            .await?
    } else {
        tracker.get_deployment(&resolved_args.name).cloned()
    };
    // A deployment group token passed for an existing deployment, with its group.
    let mut supplied_group_token: Option<(String, String)> = None;
    let resuming_existing = existing_deployment.is_some();
    let tracked_deployment = match existing_deployment {
        Some(deployment) => {
            info!("Found tracked deployment '{}'", resolved_args.name);
            match args.token.as_ref() {
                Some(provided_token) if deployment.api_key != *provided_token => {
                    match validate_token(provided_token, &base_url).await? {
                        DeploymentToken::Deployment {
                            deployment_id,
                            project_id,
                            workspace_id,
                        } => {
                            info!(
                                "Updating stored API key for deployment '{}'",
                                resolved_args.name
                            );
                            tracker.track(
                                resolved_args.name.clone(),
                                provided_token.clone(),
                                ValidatedDeploymentInfo {
                                    deployment_id,
                                    workspace_id,
                                    project_id,
                                },
                            )?
                        }
                        // A group token authorizes creating deployments and running
                        // setup, not re-keying one that already exists: keep the
                        // stored deployment key and hold the group token as setup
                        // authority.
                        DeploymentToken::DeploymentGroup {
                            deployment_group_id,
                            ..
                        } => {
                            supplied_group_token =
                                Some((deployment_group_id, provided_token.clone()));
                            deployment
                        }
                    }
                }
                _ => deployment,
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

                        let sdk_compute =
                            to_sdk_compute_settings(resolved_args.compute_settings.clone())?;

                        let deployment_model = if uses_push_deployment_model(
                            resolved_args.platform_enum,
                        ) {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Push
                        } else {
                            alien_platform_api::types::NewDeploymentRequestStackSettingsDeploymentModel::Pull
                        };
                        let stack_settings = alien_platform_api::types::NewDeploymentRequestStackSettings {
                        endpoint_access: args.network.endpoint_access
                            .map(|access| serde_json::from_value(serde_json::json!(access)))
                            .transpose()
                            .into_alien_error()
                            .context(ErrorData::ConfigurationError {
                                message: "Failed to convert endpoint access to SDK type".to_string(),
                            })?,
                        compute: sdk_compute,
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
                        domains: resolved_args.domain_settings.clone().map(|domains| {
                            let value = serde_json::to_value(domains).into_alien_error()
                                .context(ErrorData::ConfigurationError { message: "Failed to serialize domain settings".to_string() })?;
                            serde_json::from_value(value).into_alien_error()
                                .context(ErrorData::ConfigurationError { message: "Failed to convert domain settings to SDK type".to_string() })
                        }).transpose()?,
                        external_bindings: None,
                        kubernetes: None,
                        public_endpoints: None,
                    };

                        let parsed_setup_item = match args.setup_item.as_deref() {
                            Some(item) => Some(
                                serde_json::from_value(serde_json::Value::String(item.to_string()))
                                    .into_alien_error()
                                    .context(ErrorData::ValidationError {
                                        field: "setup-item".to_string(),
                                        message: format!("Unknown setup item '{item}'"),
                                    })?,
                            ),
                            None => None,
                        };

                        if ctx.is_platform() {
                            setup_validation::validate_before_creation(
                                &base_url,
                                token,
                                &resolved_args,
                                &args,
                            )
                            .await?;
                        }

                        let create_response = sdk_client
                            .create_deployment()
                            .workspace(&workspace_name)
                            .body(alien_platform_api::types::NewDeploymentRequest {
                                setup_item: parsed_setup_item,
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
                                release_channel: args.channel.clone().try_into().into_alien_error().context(
                                    ErrorData::ValidationError {
                                        field: "channel".to_string(),
                                        message: "Channel names must start with a letter and contain only lowercase letters, numbers, and hyphens.".to_string(),
                                    },
                                )?,
                                environment_variables: None,
                                deployment_group_id: None,
                                environment_info: None,
                                input_values: to_sdk_stack_input_values(
                                    &resolved_args.input_values,
                                )?,
                                public_subdomain: None,
                                initial_desired_release: alien_platform_api::types::NewDeploymentRequestInitialDesiredRelease::Active,
                                setup_method: None,
                                setup_metadata: None,
                                setup_handoff: ::std::default::Default::default(),
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

    if resuming_existing {
        if let Some(requested) = resolved_args.public_subdomain.as_deref() {
            if !ctx.is_platform() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "public-subdomain".to_string(),
                    message: "An existing standalone deployment cannot change its public subdomain. Omit --public-subdomain when resuming it.".to_string(),
                }));
            }
            let existing = create_platform_client(&tracked_deployment.api_key, &base_url)?
                .get_deployment()
                .id(&tracked_deployment.deployment_id)
                .send()
                .await
                .into_sdk_error()
                .context(ErrorData::ApiRequestFailed {
                    message: format!("reading deployment {}", tracked_deployment.deployment_id),
                    url: None,
                })?
                .into_inner();
            validate_existing_public_subdomain(
                requested,
                existing
                    .public_subdomain
                    .as_ref()
                    .map(|value| value.as_str()),
            )?;
        }
    }

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
    // deployment-scoped authorization. Manager discovery may use a user or
    // project credential, but that credential must never leak into setup.
    let manager_http_client = deployment_manager_http_client(
        &tracked_deployment.api_key,
        manager_ctx.workspace.as_deref(),
    )?;
    let manager_client =
        alien_manager_api::Client::new_with_client(&manager_ctx.manager_url, manager_http_client);

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

    let client_config = client_config.expect("non-Machines deploys validate client config");

    // An installed deployment with nothing pending has nothing to deploy, and
    // one whose update waits for setup needs setup authority this run must
    // bring: say so now instead of waiting for a lock nobody grants.
    let has_installed_release = deployment.current_release_id.is_some();
    let initial_setup =
        existing_deployment_plan(&status, None, ctx.is_platform(), has_installed_release)
            == ExistingDeploymentPlan::InitialSetup;
    let (deployment_group_id, active_update) = if ctx.is_platform() && !initial_setup {
        let (group, active_update) = platform_deployment_progress(
            &base_url,
            &tracked_deployment.api_key,
            &tracked_deployment.deployment_id,
        )
        .await?;
        (Some(group), active_update)
    } else {
        (None, None)
    };
    let plan = existing_deployment_plan(
        &status,
        active_update,
        ctx.is_platform(),
        has_installed_release,
    );
    if plan == ExistingDeploymentPlan::WaitForManager {
        return Err(AlienError::new(ErrorData::DeploymentFailed {
            message: format!(
                "Deployment '{}' is {status:?} with an update waiting for setup. Setup can run once the manager returns it to running; check `alien deployments get {}`.",
                resolved_args.name, tracked_deployment.deployment_id
            ),
        }));
    }
    if plan == ExistingDeploymentPlan::NothingToDo {
        steps.complete(2, Some("Nothing to deploy".to_string()));
        steps.skip(3, Some("Already running".to_string()));
        drop(steps);
        println!(
            "{}",
            success_line("Deployment is running with no pending update. Nothing to do.")
        );
        println!(
            "{} {}",
            dim_label("Next"),
            command(&format!(
                "alien deployments redeploy {}",
                tracked_deployment.deployment_id
            ))
        );
        return Ok(());
    }

    // Setup runs with setup authority for the deployment group; the
    // deployment token keeps configuring the runtime.
    let (setup_client, setup_token) = match plan {
        ExistingDeploymentPlan::SetupUpdate { .. } => {
            let deployment_group_id = deployment_group_id.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "A setup update needs the deployment's group from the platform"
                        .to_string(),
                })
            })?;
            let supplied_group_token = match supplied_group_token.as_ref() {
                Some((group, _)) if group != deployment_group_id => {
                    return Err(AlienError::new(ErrorData::ValidationError {
                        field: "token".to_string(),
                        message: format!(
                            "The deployment group token is for group {group}, but deployment '{}' belongs to group {deployment_group_id}.",
                            resolved_args.name
                        ),
                    }));
                }
                Some((_, token)) => Some(token.as_str()),
                None => None,
            };
            let setup_token = setup_authority_token(
                supplied_group_token,
                || async {
                    let auth = ctx.auth_http().await?;
                    let workspace = ctx.resolve_platform_workspace_context(true).await?;
                    Ok(LoginSession {
                        client: auth.client,
                        workspace: workspace.query,
                    })
                },
                &base_url,
                deployment_group_id,
                &resolved_args.name,
                &plan.setup_reason(&status),
            )
            .await?;
            let client = alien_manager_api::Client::new_with_client(
                &manager_ctx.manager_url,
                deployment_manager_http_client(&setup_token, manager_ctx.workspace.as_deref())?,
            );
            (Some(client), Some(setup_token))
        }
        _ => (None, None),
    };
    // The client that holds the lock: setup authority for a setup update.
    let lock_client = setup_client.as_ref().unwrap_or(&manager_client);

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

    // Deployment records intentionally omit the release stack. Resolve it
    // before entering the step loop; a missing or incompatible target is a
    // configuration error, not an observe-only deployment.
    let deployment_token = tracked_deployment.api_key.as_str();
    let manager_ctx_ref = &manager_ctx;
    (current.target_release, current.current_release) = load_run_releases(
        deployment.desired_release_id.as_deref(),
        deployment.current_release_id.as_deref(),
        matches!(plan, ExistingDeploymentPlan::SetupUpdate { .. }),
        move |release_id: String| async move {
            load_release(manager_ctx_ref, deployment_token, &release_id, platform).await
        },
    )
    .await?;

    // Running deploy on a failed deployment is an implicit retry request
    if current.status.is_failed() {
        info!(
            "Deployment is in {:?} state, setting retry_requested to proceed",
            current.status
        );
        current.retry_requested = true;
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

    if let Some(requested_compute) = resolved_args.compute_settings.as_ref() {
        if stack_settings.compute.as_ref() != Some(requested_compute) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "compute".to_string(),
                message: "Compute settings cannot be changed while resuming an existing deployment. Use the deployment setup flow to change compute, or retry with the deployment's current compute settings.".to_string(),
            }));
        }
    }

    if let Some(requested_domains) = resolved_args.domain_settings.as_ref() {
        if stack_settings.domains.as_ref() != Some(requested_domains) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "domains".to_string(),
                message: "Domain settings differ from the existing deployment. Update its stack settings before resuming setup.".to_string(),
            }));
        }
    }

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

    // The manager persists bindings in stack settings, but the executor and preflights
    // read them off the deployment config. Losing them here presents every adopted
    // resource as unbound, which reads as a release that dropped its bindings.
    if let Some(external_bindings) = stack_settings.external_bindings.clone() {
        config.external_bindings = external_bindings;
    }

    let setup_owned_status = matches!(
        plan,
        ExistingDeploymentPlan::InitialSetup | ExistingDeploymentPlan::SetupUpdate { .. }
    );

    // A local retry flag does not make a failed platform operation claimable.
    // Validate the resume configuration first, then requeue with the same
    // authority that will acquire execution. The API rejects a live contender.
    if ctx.is_platform()
        && matches!(
            current.status,
            DeploymentStatus::PreflightsFailed
                | DeploymentStatus::InitialSetupFailed
                | DeploymentStatus::ProvisioningFailed
                | DeploymentStatus::UpdateFailed
                | DeploymentStatus::RefreshFailed
        )
    {
        request_deployment_retry(
            &base_url,
            setup_token.as_deref().unwrap_or(deployment_token),
            &tracked_deployment.deployment_id,
        )
        .await?;
    }
    steps.activate(2, Some("Waiting for execution ownership".to_string()));

    // Acquire → step loop → reconcile → release (all via manager)
    let session = format!("cli-deploy-{}", Uuid::new_v4());
    let acquisition = if setup_owned_status {
        acquire_setup_run_deployment(
            lock_client,
            &tracked_deployment.deployment_id,
            &session,
            stack_settings.deployment_model,
        )
        .await
    } else {
        acquire_deployment_with_payload(
            &manager_client,
            &tracked_deployment.deployment_id,
            &session,
            stack_settings.deployment_model,
        )
        .await
    };
    let acquired_deployment = match acquisition {
        Ok(deployment) => deployment,
        Err(error) => {
            if ctx.is_platform()
                && completed_after_acquisition_miss(
                    &error,
                    &base_url,
                    deployment_token,
                    &tracked_deployment.deployment_id,
                )
                .await?
            {
                steps.complete(2, Some("Completed by manager".to_string()));
                steps.skip(3, Some("Already running".to_string()));
                drop(steps);
                println!(
                    "{}",
                    success_line("Deployment completed while waiting for execution ownership.")
                );
                return Ok(());
            }
            return Err(error).context(ErrorData::ConfigurationError {
                message: if setup_owned_status {
                    "Failed to acquire setup deployment lock"
                } else {
                    "Failed to acquire deployment lock"
                }
                .to_string(),
            });
        }
    };

    if let Some(acquired_config) = acquired_deployment
        .deployment
        .get("deploymentConfig")
        .cloned()
    {
        config = serde_json::from_value(acquired_config)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Failed to deserialize deploymentConfig from acquired deployment"
                    .to_string(),
            })?;
    } else if manager_ctx.workspace.is_some() {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "Setup acquisition did not return deploymentConfig".to_string(),
        }));
    }
    config.manager_url = Some(manager_ctx.manager_url.clone());
    config.deployment_token = Some(tracked_deployment.api_key.clone());

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

    let transport = ManagerApiTransport::with_execution_claim(
        lock_client.clone(),
        session.clone(),
        acquired_deployment.execution_claim.clone(),
    );
    let policy = RunnerPolicy {
        max_steps: 400,
        operation: if setup_owned_status {
            LoopOperation::InitialSetup
        } else {
            LoopOperation::Deploy
        },
        delay_strategy: alien_deployment::runner::DelayStrategy::Inline,
    };

    // A setup update re-runs setup on an installed deployment: prepare the
    // target release's setup-owned resources, then hand off like initial setup.
    // A failed preparation releases the lock with the state as it was read.
    if matches!(plan, ExistingDeploymentPlan::SetupUpdate { .. }) {
        let mut prepared = current.clone();
        if let Err(error) = prepare_setup_update(&mut prepared, &config, &client_config).await {
            let finalized = final_reconcile(
                lock_client,
                &tracked_deployment.deployment_id,
                &session,
                acquired_deployment.execution_claim.as_ref(),
                &current,
            )
            .await;
            return combine_operation_and_finalization(Err::<(), _>(error), finalized).context(
                ErrorData::GenericError {
                    message: "setup update failed".to_string(),
                },
            );
        }
        current = prepared;
    }

    steps.activate(2, Some(tracked_deployment.deployment_id.clone()));
    let progress_steps = steps.clone();
    let on_progress: alien_deployment::runner::ProgressCallback = Box::new(move |progress| {
        if let Some(stack_state) = progress.stack_state {
            progress_steps.sync_deployment_resources(&stack_state.resources, progress.status);
        }
    });
    let runner_result = alien_deployment::runner::run_step_loop(
        &mut current,
        &mut config,
        &client_config,
        &tracked_deployment.deployment_id,
        &policy,
        &transport,
        None,
        Some(&on_progress),
    )
    .await;
    drop(on_progress);
    let semantic_failure_status = runner_result.as_ref().ok().and_then(|result| {
        (result.loop_result.outcome == LoopOutcome::Failure)
            .then(|| result.loop_result.final_status.clone())
    });

    let runner_result = finalize_step_loop(
        lock_client,
        &tracked_deployment.deployment_id,
        &session,
        acquired_deployment.execution_claim.as_ref(),
        &current,
        runner_result,
    )
    .await;

    // Semantic failures are checkpointed as a successful runner return. Mark
    // the visible step failed after finalization, but before converting that
    // outcome back into the detailed operation error returned to the caller.
    if let Some(status) = semantic_failure_status {
        steps.fail(2, Some(format!("{status:?}")));
    }

    let RunnerResult {
        loop_result,
        steps_executed,
        ..
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

    // Handle runner outcome. `handed_off` means this run finished setup and the
    // manager now provisions the deployment, so it is not running yet.
    let handed_off = match loop_result.outcome {
        LoopOutcome::Success => {
            steps.complete(2, Some("Resources ready".to_string()));
            steps.complete(3, Some("Running".to_string()));
            false
        }
        LoopOutcome::Failure => {
            steps.fail(2, Some(format!("{:?}", loop_result.final_status)));
            let failed = ErrorData::DeploymentFailed {
                message: format!(
                    "{} failed",
                    describe_failed_status(&loop_result.final_status)
                ),
            };
            // The final state's headline error names each failed resource and its cause.
            return Err(
                match alien_deployment::deployment_headline_error_from_state(&current) {
                    Some(cause) => cause.context(failed),
                    None => AlienError::new(failed),
                },
            );
        }
        LoopOutcome::Neutral if loop_result.stop_reason == LoopStopReason::Handoff => {
            // Provisioning can still block after the handoff (for example on a
            // deployer secret that is not written yet), so do not report running.
            steps.complete(2, Some("Handed off to the manager".to_string()));
            steps.activate(3, Some("Waiting for the manager".to_string()));
            true
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
    };

    if handed_off {
        // Setup only gets the deployment to the handoff. Report what the manager
        // makes of it: running, failed, or blocked on the deployer.
        steps.println(&dim_label(
            "Setup complete. Waiting for the manager to provision the deployment...",
        ));
        let deployment_id = tracked_deployment.deployment_id.clone();
        let activation = wait_for_handed_off_deployment(
            || async {
                let observed = observe_deployment(&manager_client, &deployment_id).await?;
                if let Some(stack_state) = &observed.stack_state {
                    steps.sync_deployment_resources(&stack_state.resources, observed.status);
                }
                Ok(observed)
            },
            HANDOFF_POLL_INTERVAL,
            HANDOFF_TIMEOUT,
            |observed| {
                if matches!(
                    observed.status,
                    DeploymentStatus::WaitingForSecrets | DeploymentStatus::WaitingForMachines
                ) {
                    steps.println(&format!(
                        "{} {}",
                        dim_label("Blocked:"),
                        observed
                            .error_message
                            .as_deref()
                            .unwrap_or(describe_waiting_status(&observed.status))
                    ));
                }
            },
        )
        .await;
        if let Err(error) = activation {
            steps.fail(3, Some(error.message.clone()));
            return Err(error);
        }
        steps.complete(3, Some("Running".to_string()));
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

/// How often `alien deploy` polls the deployment after handing it to the manager.
const HANDOFF_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// How long `alien deploy` waits for the manager after the handoff. A deployment
/// waiting for a deployer secret stays blocked until someone writes it.
const HANDOFF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// A deployment's status and headline error, as observed after the handoff.
#[derive(Debug, Clone)]
struct ObservedDeployment {
    status: DeploymentStatus,
    error_message: Option<String>,
    stack_state: Option<alien_core::StackState>,
}

async fn observe_deployment(
    manager_client: &alien_manager_api::Client,
    deployment_id: &str,
) -> Result<ObservedDeployment> {
    let deployment = manager_client
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to read deployment '{deployment_id}' after setup"),
        })?
        .into_inner();
    let status = serde_json::from_value(serde_json::Value::String(deployment.status.clone()))
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Unknown deployment status: {}", deployment.status),
        })?;
    let error_message = deployment
        .error
        .map(serde_json::from_value::<AlienError>)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to decode the error of deployment '{deployment_id}'"),
        })?
        .map(|error| error.message);
    let stack_state = deployment
        .stack_state
        .map(serde_json::from_value)
        .transpose()
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to decode the resource state of deployment '{deployment_id}'"),
        })?;
    Ok(ObservedDeployment {
        status,
        error_message,
        stack_state,
    })
}

/// Wait until the manager reports a handed-off deployment running.
///
/// A failed or deleted deployment, or the timeout, is an error, so `alien deploy`
/// never exits successfully for a deployment that is not running. `on_change`
/// sees each newly observed status and error, for example to say what a blocked
/// deployment needs from the deployer.
async fn wait_for_handed_off_deployment<F, Fut>(
    mut observe: F,
    interval: std::time::Duration,
    timeout: std::time::Duration,
    mut on_change: impl FnMut(&ObservedDeployment),
) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<ObservedDeployment>>,
{
    let started = tokio::time::Instant::now();
    let mut last: Option<(DeploymentStatus, Option<String>)> = None;
    loop {
        let observed = observe().await?;
        let key = (observed.status, observed.error_message.clone());
        if last.as_ref() != Some(&key) {
            on_change(&observed);
            last = Some(key);
        }

        if observed.status == DeploymentStatus::Running {
            return Ok(());
        }
        if observed.status.is_failed()
            || matches!(
                observed.status,
                DeploymentStatus::DeletePending
                    | DeploymentStatus::Deleting
                    | DeploymentStatus::Deleted
                    | DeploymentStatus::TeardownRequired
                    | DeploymentStatus::Error
            )
        {
            let phase = describe_failed_status(&observed.status);
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: match observed.error_message {
                    Some(cause) => format!("{phase} failed ({:?}): {cause}", observed.status),
                    None => format!("{phase} failed ({:?})", observed.status),
                },
            }));
        }
        if started.elapsed() >= timeout {
            let state = match observed.error_message {
                Some(cause) => format!("{:?}: {cause}", observed.status),
                None => format!("{:?}", observed.status),
            };
            return Err(AlienError::new(ErrorData::DeploymentFailed {
                message: format!(
                    "the deployment is still not running after {}s ({state})",
                    timeout.as_secs()
                ),
            }));
        }
        tokio::time::sleep(interval).await;
    }
}

fn describe_waiting_status(status: &DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::WaitingForSecrets => "waiting for deployer secrets",
        DeploymentStatus::WaitingForMachines => "waiting for machines to join",
        _ => "waiting",
    }
}

/// Validate a deployment file without constructing an authenticated execution context.
///
/// This is deliberately separate from [`deploy_task`]: callers of `--validate-only`
/// must not need manager credentials, a platform session, or network access merely to
/// parse and validate a local file.
pub fn validate_deploy_config(args: &DeployArgs) -> Result<()> {
    let mut args = args.clone();
    args.resolve_token_file()?;
    #[cfg(not(feature = "platform"))]
    if args.channel != "production" {
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: "This manager doesn't support release channels: every release goes to every deployment.".to_string(),
        }));
    }

    resolve_deploy_args(&args)?;
    println!("Deployment config is valid.");
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
    let deployment_id =
        create_initial_deployment(&args.name, port, None, args.input_values.clone()).await?;
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

/// Loads a release's stack from the manager with the deployment token.
async fn load_release(
    manager_ctx: &crate::execution_context::ManagerContext,
    deployment_token: &str,
    release_id: &str,
    platform: Platform,
) -> Result<alien_core::ReleaseInfo> {
    let url = format!("{}/v1/releases/{}", manager_ctx.manager_url, release_id);
    let response = manager_ctx
        .http_client
        .get(&url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {deployment_token}"),
        )
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("loading release '{release_id}'"),
            url: Some(url.clone()),
        })?;
    let response =
        response
            .error_for_status()
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("loading release '{release_id}'"),
                url: Some(url),
            })?;
    let release_json = response
        .json::<serde_json::Value>()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to decode release '{release_id}'"),
        })?;
    target_release_from_json(release_id, platform, release_json)
}

/// Loads the target and installed releases a run starts from. The target is
/// the desired release. A setup update also needs the installed release: it
/// reconciles state with the manager at every step, and without the installed
/// release that would clear it. A failed refresh has no update in flight, so
/// the deployment has no desired release; its setup retry applies the
/// installed release again.
async fn load_run_releases<L, LF>(
    desired_release_id: Option<&str>,
    current_release_id: Option<&str>,
    setup_update: bool,
    load: L,
) -> Result<(
    Option<alien_core::ReleaseInfo>,
    Option<alien_core::ReleaseInfo>,
)>
where
    L: Fn(String) -> LF,
    LF: std::future::Future<Output = Result<alien_core::ReleaseInfo>>,
{
    let target = match desired_release_id {
        Some(release_id) => Some(load(release_id.to_string()).await?),
        None => None,
    };
    if !setup_update {
        return Ok((target, None));
    }
    let installed = match current_release_id {
        Some(release_id) => Some(load(release_id.to_string()).await?),
        None => None,
    };
    let target = target.or_else(|| installed.clone());
    Ok((target, installed))
}

fn target_release_from_json(
    release_id: &str,
    platform: Platform,
    release_json: serde_json::Value,
) -> Result<alien_core::ReleaseInfo> {
    let stack_json = release_json
        .get("stack")
        .and_then(|stacks| stacks.get(platform.as_str()))
        .cloned()
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Target release '{release_id}' does not contain a stack for {platform}"
                ),
            })
        })?;
    let stack = serde_json::from_value(stack_json)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Target release '{release_id}' contains an invalid {platform} stack"),
        })?;

    Ok(alien_core::ReleaseInfo {
        release_id: Some(release_id.to_string()),
        version: None,
        description: None,
        stack,
    })
}

/// The deployment's active update, as far as taking a lock is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveUpdate {
    /// Queued or applying: a runtime lock picks it up.
    Pending,
    /// Blocked until setup runs again.
    WaitingForSetup,
}

impl ActiveUpdate {
    fn from_status(status: DeploymentUpdateOperationStatus) -> Option<Self> {
        match status {
            DeploymentUpdateOperationStatus::Queued | DeploymentUpdateOperationStatus::Applying => {
                Some(Self::Pending)
            }
            DeploymentUpdateOperationStatus::Blocked => Some(Self::WaitingForSetup),
            DeploymentUpdateOperationStatus::Succeeded
            | DeploymentUpdateOperationStatus::Failed
            | DeploymentUpdateOperationStatus::Superseded => None,
        }
    }
}

/// How `alien deploy` continues an existing deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingDeploymentPlan {
    /// Setup has not handed the deployment off yet; its deployment token runs setup.
    InitialSetup,
    /// Pending work the deployment token's runtime lock picks up.
    Runtime,
    /// Installed deployment whose update waits for setup, or whose last update
    /// or refresh failed. Running setup again needs setup authority for the
    /// deployment group; `retry` first moves the failed operation back to ready.
    SetupUpdate { retry: bool },
    /// Running with nothing pending.
    NothingToDo,
    /// An update waits for setup while the deployment is in a status the
    /// setup lock does not accept; the manager has to move it first.
    WaitForManager,
}

impl ExistingDeploymentPlan {
    /// What the deployment is waiting for, for a person.
    fn setup_reason(self, status: &DeploymentStatus) -> String {
        match self {
            Self::SetupUpdate { retry: true } => {
                format!("had its last {} fail", describe_failed_status(status))
            }
            _ => "has an update waiting for setup".to_string(),
        }
    }
}

/// Decides how to continue an existing deployment. `active_update` comes from
/// the platform; a standalone manager has no blocked updates, so a running
/// deployment there has nothing pending.
fn existing_deployment_plan(
    status: &DeploymentStatus,
    active_update: Option<ActiveUpdate>,
    platform_mode: bool,
    has_installed_release: bool,
) -> ExistingDeploymentPlan {
    // Setup can fail after a release is installed. Status alone cannot grant
    // the deployment token authority to resume setup on that installation.
    if platform_mode && has_installed_release {
        match status {
            DeploymentStatus::InitialSetup => {
                return ExistingDeploymentPlan::SetupUpdate { retry: false };
            }
            DeploymentStatus::InitialSetupFailed => {
                return ExistingDeploymentPlan::SetupUpdate { retry: true };
            }
            _ => {}
        }
    }
    if matches!(
        status,
        DeploymentStatus::Pending
            | DeploymentStatus::PreflightsFailed
            | DeploymentStatus::InitialSetup
            | DeploymentStatus::InitialSetupFailed
    ) {
        return ExistingDeploymentPlan::InitialSetup;
    }
    if platform_mode {
        if matches!(
            status,
            DeploymentStatus::UpdateFailed | DeploymentStatus::RefreshFailed
        ) {
            return ExistingDeploymentPlan::SetupUpdate { retry: true };
        }
        if active_update == Some(ActiveUpdate::WaitingForSetup) {
            // The setup lock takes an installed deployment only while it
            // runs or after a failed provisioning, update or refresh.
            return if matches!(
                status,
                DeploymentStatus::Running | DeploymentStatus::ProvisioningFailed
            ) {
                ExistingDeploymentPlan::SetupUpdate { retry: false }
            } else {
                ExistingDeploymentPlan::WaitForManager
            };
        }
    }
    if *status == DeploymentStatus::Running && active_update.is_none() {
        return ExistingDeploymentPlan::NothingToDo;
    }
    ExistingDeploymentPlan::Runtime
}

/// A platform user session able to mint a first-party deployment group
/// session: its HTTP client and workspace query.
struct LoginSession {
    client: reqwest::Client,
    workspace: Option<String>,
}

/// Returns a token with setup authority for the deployment group: the group
/// token passed with `--token`, else a first-party session minted from the
/// user's login. The deployment token never has it; it keeps configuring the
/// runtime.
async fn setup_authority_token<L, LF>(
    supplied_group_token: Option<&str>,
    login: L,
    base_url: &str,
    deployment_group_id: &str,
    deployment_name: &str,
    reason: &str,
) -> Result<String>
where
    L: FnOnce() -> LF,
    LF: std::future::Future<Output = Result<LoginSession>>,
{
    if let Some(token) = supplied_group_token {
        return Ok(token.to_string());
    }
    let login = login().await.map_err(|error| {
        if error.code == "LOGIN_REQUIRED" {
            AlienError::new(ErrorData::DeploymentSetupAuthorityRequired {
                deployment: deployment_name.to_string(),
                reason: reason.to_string(),
            })
        } else {
            error
        }
    })?;
    let session = create_first_party_deployment_session(
        &login.client,
        base_url,
        login.workspace.as_deref(),
        deployment_group_id,
    )
    .await?;
    Ok(session.token)
}

fn validate_existing_public_subdomain(requested: &str, existing: Option<&str>) -> Result<()> {
    if existing == Some(requested) {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::ValidationError {
        field: "public-subdomain".to_string(),
        message: match existing {
            Some(existing) => format!(
                "This deployment already uses public subdomain '{existing}'. It cannot be changed by alien deploy; retry with --public-subdomain {existing} or omit the flag."
            ),
            None => "This deployment has no public subdomain. Omit --public-subdomain when resuming it; alien deploy cannot change an existing deployment's routing.".to_string(),
        },
    }))
}

/// The deployment's group and active update, read from the platform with the
/// deployment token.
async fn platform_deployment_progress(
    base_url: &str,
    deployment_token: &str,
    deployment_id: &str,
) -> Result<(String, Option<ActiveUpdate>)> {
    let deployment = create_platform_client(deployment_token, base_url)?
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("reading deployment {deployment_id}"),
            url: None,
        })?
        .into_inner();
    let active_update = deployment
        .update_state
        .as_ref()
        .and_then(|update_state| update_state.active.0.as_ref())
        .and_then(|operation| ActiveUpdate::from_status(operation.status));
    Ok((deployment.deployment_group_id.to_string(), active_update))
}

/// A status rejection can mean the manager completed between our read and
/// acquisition. Confirm convergence from one authoritative snapshot. Other
/// rejection reasons and transport errors must retain their original failure.
async fn completed_after_acquisition_miss(
    error: &AlienError,
    base_url: &str,
    deployment_token: &str,
    deployment_id: &str,
) -> Result<bool> {
    let Some(error) = std::iter::successors(Some(error), |error| error.source.as_deref())
        .find(|error| error.code == "DEPLOYMENT_ACQUIRE_UNAVAILABLE")
    else {
        return Ok(false);
    };
    let reason = error
        .context
        .as_ref()
        .and_then(|context| context["reason"].as_str());
    if !matches!(reason, Some("statusMismatch" | "acquireModeMismatch")) {
        return Ok(false);
    }
    let deployment = create_platform_client(deployment_token, base_url)?
        .get_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!(
                "checking whether deployment {deployment_id} completed during acquisition"
            ),
            url: None,
        })?
        .into_inner();
    Ok(
        deployment.status == alien_platform_api::types::DeploymentDetailResponseStatus::Running
            && deployment.current_release_id.is_some()
            && deployment.desired_release_id.is_none()
            && deployment
                .update_state
                .as_ref()
                .is_some_and(|state| state.active.0.is_none() && state.next.0.is_none()),
    )
}

/// Prepares an installed deployment for a setup run: the target release's
/// setup-owned (frozen) resources become the prepared stack, failed frozen
/// resources are retried, and the deployment re-enters initial setup, which
/// hands it back to the manager once setup is applied.
async fn prepare_setup_update(
    current: &mut DeploymentState,
    config: &alien_core::DeploymentConfig,
    client_config: &ClientConfig,
) -> Result<()> {
    if !matches!(
        current.status,
        DeploymentStatus::Running
            | DeploymentStatus::UpdatePending
            | DeploymentStatus::UpdateFailed
            | DeploymentStatus::RefreshFailed
            | DeploymentStatus::ProvisioningFailed
    ) {
        return Ok(());
    }
    let target_stack = current
        .target_release
        .as_ref()
        .map(|release| release.stack.clone())
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "A setup update requires the deployment's desired release".to_string(),
            })
        })?;
    let stack_state = current.stack_state.as_mut().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "A setup update requires the deployment's stack state".to_string(),
        })
    })?;
    let existing_metadata = current.runtime_metadata.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "An installed deployment has no prepared setup metadata".to_string(),
        })
    })?;
    let runtime_metadata = alien_deployment::prepare_direct_setup_update(
        target_stack,
        stack_state,
        config,
        client_config,
        existing_metadata,
    )
    .await
    .context(ErrorData::ConfigurationError {
        message: "Failed to prepare the setup update".to_string(),
    })?;
    alien_deployment::retry_failed_setup_resources(stack_state, &runtime_metadata, config)
        .context(ErrorData::ConfigurationError {
            message: "Failed to retry failed setup-owned resources".to_string(),
        })?;
    current.runtime_metadata = Some(runtime_metadata);
    current.status = DeploymentStatus::InitialSetup;
    Ok(())
}

/// Moves a failed deployment's operation back to ready so setup can take it.
async fn request_deployment_retry(
    base_url: &str,
    setup_token: &str,
    deployment_id: &str,
) -> Result<()> {
    create_platform_client(setup_token, base_url)?
        .retry_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("requesting a retry of deployment {deployment_id}"),
            url: None,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::{timeout, Duration};

    fn acquisition_miss(reason: &str) -> AlienError {
        serde_json::from_value(serde_json::json!({
            "code": "DEPLOYMENT_ACQUIRE_UNAVAILABLE",
            "message": "The deployment cannot be acquired",
            "context": {"reason": reason},
            "retryable": false,
            "internal": false,
        }))
        .expect("wire acquisition error")
    }

    fn completed_deployment_response() -> serde_json::Value {
        serde_json::json!({
            "id": format!("dep_{}", "a".repeat(28)),
            "name": "test-deployment",
            "status": "running",
            "projectId": format!("prj_{}", "a".repeat(28)),
            "platform": "aws",
            "deploymentProtocolVersion": 1,
            "deploymentGroupId": format!("dg_{}", "a".repeat(28)),
            "purpose": "application",
            "stackSettings": {},
            "releaseChannel": "production",
            "retryRequested": false,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "managerId": format!("mgr_{}", "a".repeat(28)),
            "workspaceId": format!("ws_{}", "a".repeat(24)),
            "currentReleaseId": format!("rel_{}", "a".repeat(28)),
            "desiredReleaseId": null,
            "updateState": {"active": null, "next": null, "latest": null},
        })
    }

    #[tokio::test]
    async fn acquisition_miss_confirms_completion_through_acquisition_helpers() {
        for (setup, structured_response) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let server = MockServer::start_async().await;
            let acquisition = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/sync/acquire");
                    let reason = if setup {
                        "acquireModeMismatch"
                    } else {
                        "statusMismatch"
                    };
                    if structured_response {
                        then.status(200).json_body(serde_json::json!({
                            "deployments": [],
                            "notAcquired": [{
                                "deploymentId": "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                                "reason": reason,
                            }],
                        }));
                    } else {
                        then.status(409)
                            .json_body(serde_json::to_value(acquisition_miss(reason)).unwrap());
                    }
                })
                .await;
            let completion = server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path("/v1/deployments/dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa");
                    then.status(200).json_body(completed_deployment_response());
                })
                .await;
            let client = alien_manager_api::Client::new(&server.base_url());
            let id = "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
            let error = if setup {
                acquire_setup_run_deployment(
                    &client,
                    id,
                    "test-session",
                    alien_core::DeploymentModel::Push,
                )
                .await
            } else {
                acquire_deployment_with_payload(
                    &client,
                    id,
                    "test-session",
                    alien_core::DeploymentModel::Push,
                )
                .await
            }
            .expect_err("manager rejects acquisition after completion");
            assert_eq!(
                error.code == "DEPLOYMENT_ACQUIRE_UNAVAILABLE",
                structured_response
            );
            assert!(
                completed_after_acquisition_miss(&error, &server.base_url(), "test-token", id)
                    .await
                    .unwrap(),
                "setup={setup}, structured_response={structured_response}: {error:?}"
            );
            acquisition.assert_hits_async(1).await;
            completion.assert_hits_async(1).await;
        }
    }

    #[tokio::test]
    async fn acquisition_miss_confirms_completion_without_hiding_pending_work() {
        let operation = serde_json::json!({
            "id": format!("duop_{}", "a".repeat(28)),
            "status": "queued",
            "reasons": [],
            "targetReleaseId": format!("rel_{}", "b".repeat(28)),
            "changedKeys": [],
            "requestedAt": "2026-01-01T00:00:00Z",
        });
        let mut cases = vec![("completed", completed_deployment_response(), true)];
        for status in ["updating", "update-failed", "deleted"] {
            let mut body = completed_deployment_response();
            body["status"] = serde_json::json!(status);
            cases.push((status, body, false));
        }
        for field in ["active", "next"] {
            let mut body = completed_deployment_response();
            body["updateState"][field] = operation.clone();
            cases.push((field, body, false));
        }
        let mut desired = completed_deployment_response();
        desired["desiredReleaseId"] = serde_json::json!(format!("rel_{}", "b".repeat(28)));
        cases.push(("desired release", desired, false));
        let mut uninstalled = completed_deployment_response();
        uninstalled["currentReleaseId"] = serde_json::Value::Null;
        cases.push(("uninstalled", uninstalled, false));
        let mut incomplete = completed_deployment_response();
        incomplete.as_object_mut().unwrap().remove("updateState");
        cases.push(("old response without update state", incomplete, false));
        for reason in ["statusMismatch", "acquireModeMismatch"] {
            for (label, body, expected) in &cases {
                let server = MockServer::start_async().await;
                let response = server
                    .mock_async(|when, then| {
                        when.method(GET)
                            .path("/v1/deployments/dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa");
                        then.status(200).json_body(body.clone());
                    })
                    .await;
                let completed = completed_after_acquisition_miss(
                    &acquisition_miss(reason),
                    &server.base_url(),
                    "test-token",
                    "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
                .await
                .expect("authoritative completion read");
                assert_eq!(completed, *expected, "{reason}: {label}");
                response.assert_hits_async(1).await;
            }
        }
    }

    #[tokio::test]
    async fn acquisition_miss_preserves_other_rejections_and_transport_errors() {
        let server = MockServer::start_async().await;
        let response = server
            .mock_async(|when, then| {
                when.method(GET);
                then.status(200).json_body(completed_deployment_response());
            })
            .await;
        for reason in ["notFound", "platformMismatch", "contended", "deferred"] {
            assert!(!completed_after_acquisition_miss(
                &acquisition_miss(reason),
                &server.base_url(),
                "test-token",
                "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .await
            .unwrap());
        }
        let mut transport = acquisition_miss("statusMismatch");
        transport.code = "HTTP_RESPONSE_ERROR".to_string();
        assert!(!completed_after_acquisition_miss(
            &transport,
            &server.base_url(),
            "test-token",
            "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .unwrap());
        response.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn acquisition_miss_requires_a_successful_completion_read() {
        let server = MockServer::start_async().await;
        let response = server
            .mock_async(|when, then| {
                when.method(GET);
                then.status(503);
            })
            .await;
        completed_after_acquisition_miss(
            &acquisition_miss("statusMismatch"),
            &server.base_url(),
            "test-token",
            "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .expect_err("failed read cannot establish successful deployment");
        response.assert_hits_async(1).await;
    }

    #[test]
    fn an_existing_public_subdomain_can_be_repeated_but_not_changed() {
        validate_existing_public_subdomain("release", Some("release")).unwrap();
        let changed = validate_existing_public_subdomain("other", Some("release")).unwrap_err();
        assert_eq!(changed.code, "VALIDATION_ERROR");
        assert!(changed.message.contains("--public-subdomain release"));
        let absent = validate_existing_public_subdomain("release", None).unwrap_err();
        assert!(absent.message.contains("Omit --public-subdomain"));
    }

    #[test]
    fn token_file_uses_the_existing_token_path_and_trims_whitespace() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("token");
        std::fs::write(&path, "  ax_test\n").expect("write token fixture");
        let mut args =
            DeployArgs::try_parse_from(["deploy", "--token-file", path.to_str().expect("path")])
                .expect("file argument");
        args.resolve_token_file().expect("read token");
        assert_eq!(args.token.as_deref(), Some("ax_test"));

        let mut inline = DeployArgs::try_parse_from(["deploy", "--token", "ax_inline"])
            .expect("inline argument");
        inline
            .resolve_token_file()
            .expect("inline token remains supported");
        assert_eq!(inline.token.as_deref(), Some("ax_inline"));
    }

    #[test]
    fn token_file_rejects_conflicting_authentication_selectors() {
        for selector in ["--token", "--deployment-group"] {
            let error = DeployArgs::try_parse_from([
                "deploy",
                "--token-file",
                "token.txt",
                selector,
                "value",
            ])
            .expect_err("conflicting selector");
            assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        }
    }

    #[tokio::test]
    async fn token_file_errors_fail_before_deployment_resolution() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("token");
        for contents in [None, Some(" \n\t")] {
            if let Some(contents) = contents {
                std::fs::write(&path, contents).expect("write empty fixture");
            }
            let args = DeployArgs::try_parse_from([
                "deploy",
                "--token-file",
                path.to_str().expect("path"),
            ])
            .expect("file argument");
            let error = deploy_task_with_environment(
                args,
                ExecutionMode::Standalone {
                    server_url: "http://127.0.0.1:1".to_string(),
                    api_key: "unused".to_string(),
                },
                &HashMap::new(),
            )
            .await
            .expect_err("file must fail before deployment resolution");
            assert_eq!(
                error.code,
                if contents.is_some() {
                    "VALIDATION_ERROR"
                } else {
                    "CONFIGURATION_ERROR"
                }
            );
            assert!(error.message.contains("token file") || error.message.contains("Token file"));
        }
    }

    #[test]
    fn existing_deployment_plan_routes_each_state_to_the_lock_that_can_take_it() {
        use ExistingDeploymentPlan::*;

        for status in [
            DeploymentStatus::Pending,
            DeploymentStatus::PreflightsFailed,
            DeploymentStatus::InitialSetup,
            DeploymentStatus::InitialSetupFailed,
        ] {
            for platform_mode in [true, false] {
                assert_eq!(
                    existing_deployment_plan(&status, None, platform_mode, false),
                    InitialSetup,
                    "{status:?} is still in initial setup"
                );
            }
        }

        let cases = [
            // Platform: installed deployments.
            (DeploymentStatus::Running, None, true, NothingToDo),
            (
                DeploymentStatus::Running,
                Some(ActiveUpdate::Pending),
                true,
                Runtime,
            ),
            (
                DeploymentStatus::Running,
                Some(ActiveUpdate::WaitingForSetup),
                true,
                SetupUpdate { retry: false },
            ),
            (
                DeploymentStatus::UpdatePending,
                Some(ActiveUpdate::WaitingForSetup),
                true,
                WaitForManager,
            ),
            (
                DeploymentStatus::ProvisioningFailed,
                Some(ActiveUpdate::WaitingForSetup),
                true,
                SetupUpdate { retry: false },
            ),
            (
                DeploymentStatus::UpdatePending,
                Some(ActiveUpdate::Pending),
                true,
                Runtime,
            ),
            (
                DeploymentStatus::UpdateFailed,
                None,
                true,
                SetupUpdate { retry: true },
            ),
            (
                DeploymentStatus::RefreshFailed,
                None,
                true,
                SetupUpdate { retry: true },
            ),
            (DeploymentStatus::Provisioning, None, true, Runtime),
            (DeploymentStatus::WaitingForSecrets, None, true, Runtime),
            // A standalone manager has no blocked updates or setup authority.
            (DeploymentStatus::Running, None, false, NothingToDo),
            (DeploymentStatus::UpdateFailed, None, false, Runtime),
            (DeploymentStatus::UpdatePending, None, false, Runtime),
        ];
        for (status, active_update, platform_mode, expected) in cases {
            assert_eq!(
                existing_deployment_plan(&status, active_update, platform_mode, true),
                expected,
                "{status:?} with {active_update:?} (platform: {platform_mode})"
            );
        }
    }

    #[test]
    fn installed_setup_resume_uses_setup_authority_and_retries_failed_operations() {
        for active in [
            None,
            Some(ActiveUpdate::WaitingForSetup),
            Some(ActiveUpdate::Pending),
        ] {
            for (status, expected) in [
                (
                    DeploymentStatus::InitialSetup,
                    ExistingDeploymentPlan::SetupUpdate { retry: false },
                ),
                (
                    DeploymentStatus::InitialSetupFailed,
                    ExistingDeploymentPlan::SetupUpdate { retry: true },
                ),
            ] {
                assert_eq!(
                    existing_deployment_plan(&status, active, true, true),
                    expected
                );
                assert_eq!(
                    existing_deployment_plan(&status, active, true, false),
                    ExistingDeploymentPlan::InitialSetup
                );
                assert_eq!(
                    existing_deployment_plan(&status, active, false, true),
                    ExistingDeploymentPlan::InitialSetup
                );
            }
        }
    }

    #[test]
    fn only_queued_applying_and_blocked_updates_are_active() {
        assert_eq!(
            ActiveUpdate::from_status(DeploymentUpdateOperationStatus::Blocked),
            Some(ActiveUpdate::WaitingForSetup)
        );
        for status in [
            DeploymentUpdateOperationStatus::Queued,
            DeploymentUpdateOperationStatus::Applying,
        ] {
            assert_eq!(
                ActiveUpdate::from_status(status),
                Some(ActiveUpdate::Pending)
            );
        }
        for status in [
            DeploymentUpdateOperationStatus::Succeeded,
            DeploymentUpdateOperationStatus::Failed,
            DeploymentUpdateOperationStatus::Superseded,
        ] {
            assert_eq!(ActiveUpdate::from_status(status), None);
        }
    }

    /// A platform API that mints first-party sessions and records who asked.
    async fn first_party_session_api() -> (String, Arc<std::sync::Mutex<Vec<(String, String)>>>) {
        use axum::{extract::State, http::HeaderMap as AxumHeaders, routing::post, Json, Router};

        type Calls = Arc<std::sync::Mutex<Vec<(String, String)>>>;
        async fn mint(
            State(calls): State<Calls>,
            axum::extract::Path(group): axum::extract::Path<String>,
            headers: AxumHeaders,
        ) -> Json<serde_json::Value> {
            let authorization = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            calls.lock().unwrap().push((group, authorization));
            Json(serde_json::json!({ "token": "minted-group-session" }))
        }

        let calls: Calls = Arc::default();
        let app = Router::new()
            .route(
                "/v1/deployment-groups/{group}/first-party-session",
                post(mint),
            )
            .with_state(calls.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (format!("http://{addr}"), calls)
    }

    #[tokio::test]
    async fn setup_authority_prefers_the_supplied_group_token() {
        let (base_url, calls) = first_party_session_api().await;

        let token = setup_authority_token(
            Some("group-token"),
            || async { panic!("a supplied group token must not need a login") },
            &base_url,
            "dg_1",
            "production",
            "has an update waiting for setup",
        )
        .await
        .expect("supplied token");

        assert_eq!(token, "group-token");
        assert!(calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn setup_authority_mints_a_group_session_from_the_login() {
        let (base_url, calls) = first_party_session_api().await;

        let token = setup_authority_token(
            None,
            || async {
                Ok(LoginSession {
                    client: crate::auth::client_with_header("Bearer user-session")?,
                    workspace: Some("acme".to_string()),
                })
            },
            &base_url,
            "dg_1",
            "production",
            "has an update waiting for setup",
        )
        .await
        .expect("session token");

        assert_eq!(token, "minted-group-session");
        assert_eq!(
            *calls.lock().unwrap(),
            vec![("dg_1".to_string(), "Bearer user-session".to_string())],
            "the session is minted for the deployment's group with the user's login"
        );
    }

    #[tokio::test]
    async fn setup_authority_without_login_or_token_names_both_ways_to_get_it() {
        let (base_url, calls) = first_party_session_api().await;

        let error = setup_authority_token(
            None,
            || async {
                Err(AlienError::new(ErrorData::LoginRequired {
                    reason: "no usable login session on this machine".to_string(),
                }))
            },
            &base_url,
            "dg_1",
            "production",
            "has an update waiting for setup",
        )
        .await
        .expect_err("no setup authority");

        assert_eq!(error.code, "DEPLOYMENT_SETUP_AUTHORITY_REQUIRED");
        assert!(error
            .message
            .contains("'production' has an update waiting for setup"));
        let hint = error.hint.as_deref().unwrap_or_default();
        assert!(
            hint.contains("alien login") && hint.contains("--token"),
            "{hint}"
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn deployment_group_selector_is_available_without_a_token() {
        let args = DeployArgs::try_parse_from([
            "deploy",
            "--deployment-group",
            "customer_123",
            "--name",
            "production",
            "--platform",
            "aws",
        ])
        .expect("authenticated deployment-group selection should parse");

        assert_eq!(args.deployment_group.as_deref(), Some("customer_123"));
    }

    #[test]
    fn deployment_group_selector_cannot_override_token_scope() {
        DeployArgs::try_parse_from([
            "deploy",
            "--deployment-group",
            "customer_123",
            "--token",
            "ax_test",
            "--name",
            "production",
            "--platform",
            "aws",
        ])
        .expect_err("deployment-group selector and scoped token must conflict");
    }

    #[tokio::test]
    async fn invalid_provider_config_fails_before_any_deployment_api_request() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test API listener");
        let server_url = format!(
            "http://{}",
            listener.local_addr().expect("read listener address")
        );
        let args = DeployArgs::try_parse_from([
            "deploy",
            "--name",
            "must-not-be-created",
            "--platform",
            "azure",
            "--token",
            "deployment-token-must-not-be-sent",
        ])
        .expect("valid deploy arguments");

        let error = deploy_task_with_environment(
            args,
            ExecutionMode::Standalone {
                server_url,
                api_key: "deployment-token-must-not-be-sent".to_string(),
            },
            &HashMap::new(),
        )
        .await
        .expect_err("unsupported provider configuration must fail preflight");

        assert_eq!(error.code, "CONFIGURATION_ERROR");
        assert!(
            error.message.contains("Azure"),
            "unexpected provider error: {error:?}"
        );
        assert!(
            timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "provider preflight failure must not contact the deployment API"
        );
    }

    #[test]
    fn validate_only_requires_a_config_file() {
        DeployArgs::try_parse_from([
            "deploy",
            "--name",
            "production",
            "--platform",
            "aws",
            "--validate-only",
        ])
        .expect_err("validation must name the config being validated");
    }

    #[test]
    fn missing_relative_config_error_shows_resolved_path_rule() {
        let path = Path::new("definitely-missing/deployment.toml");
        let error = read_deploy_config(path).expect_err("missing config should fail");
        assert!(error.message.contains("current working directory"));
        assert!(error.message.contains("definitely-missing/deployment.toml"));
        assert!(error.message.contains(
            &std::env::current_dir()
                .expect("current directory")
                .display()
                .to_string()
        ));
    }

    #[test]
    fn target_release_requires_a_stack_for_the_deployment_platform() {
        let error = target_release_from_json(
            "rel_test",
            Platform::Aws,
            serde_json::json!({ "stack": { "gcp": {} } }),
        )
        .expect_err("an AWS deployment must not continue without an AWS stack");

        assert_eq!(error.code, "CONFIGURATION_ERROR");
        assert!(error.message.contains("rel_test"));
        assert!(error.message.contains("aws"));
    }

    /// Runs `load_run_releases` with a loader that records each release id it
    /// is asked for and returns a release whose stack is named after it.
    async fn run_releases(
        desired: Option<&str>,
        installed: Option<&str>,
        setup_update: bool,
    ) -> (Option<String>, Option<String>, Vec<String>) {
        let loaded = std::sync::Mutex::new(Vec::new());
        let (target, current) =
            load_run_releases(desired, installed, setup_update, |release_id: String| {
                loaded
                    .lock()
                    .expect("loader log lock")
                    .push(release_id.clone());
                let release: Result<alien_core::ReleaseInfo> = Ok(alien_core::ReleaseInfo {
                    stack: alien_core::Stack::new(format!("stack-{release_id}")).build(),
                    release_id: Some(release_id),
                    version: None,
                    description: None,
                });
                async move { release }
            })
            .await
            .expect("releases load");
        for release in target.iter().chain(current.iter()) {
            let release_id = release.release_id.as_deref().expect("release id");
            assert_eq!(release.stack.id, format!("stack-{release_id}"));
        }
        let id = |release: Option<alien_core::ReleaseInfo>| release.and_then(|r| r.release_id);
        (
            id(target),
            id(current),
            loaded.into_inner().expect("loader log"),
        )
    }

    #[tokio::test]
    async fn refresh_retry_without_a_desired_release_reapplies_the_installed_release() {
        // A failed refresh has no update in flight, so the deployment carries
        // only its installed release.
        let (target, current, loaded) = run_releases(None, Some("rel_installed"), true).await;
        assert_eq!(target.as_deref(), Some("rel_installed"));
        assert_eq!(current.as_deref(), Some("rel_installed"));
        assert_eq!(loaded, ["rel_installed"]);
    }

    #[tokio::test]
    async fn setup_update_targets_the_desired_release_over_the_installed_one() {
        let (target, current, loaded) =
            run_releases(Some("rel_new"), Some("rel_installed"), true).await;
        assert_eq!(target.as_deref(), Some("rel_new"));
        assert_eq!(current.as_deref(), Some("rel_installed"));
        assert_eq!(loaded, ["rel_new", "rel_installed"]);
    }

    #[tokio::test]
    async fn runtime_runs_load_only_the_desired_release() {
        let (target, current, loaded) =
            run_releases(Some("rel_new"), Some("rel_installed"), false).await;
        assert_eq!(target.as_deref(), Some("rel_new"));
        assert_eq!(current, None);
        assert_eq!(loaded, ["rel_new"]);

        let (target, current, loaded) = run_releases(None, Some("rel_installed"), false).await;
        assert_eq!((target, current), (None, None));
        assert!(loaded.is_empty());
    }

    #[test]
    fn deployment_models_match_platform_delivery() {
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Machines,
            Platform::Test,
        ] {
            assert!(uses_push_deployment_model(platform));
        }

        assert!(!uses_push_deployment_model(Platform::Kubernetes));
        assert!(!uses_push_deployment_model(Platform::Local));
    }

    #[tokio::test]
    async fn new_deployment_creation_fails_closed_on_invalid_preparation() {
        for (status, body, should_create) in [
            (200, serde_json::json!({}), false),
            (
                403,
                serde_json::json!({"code":"FORBIDDEN", "message":"Not authorized"}),
                false,
            ),
            (
                503,
                serde_json::json!({"code":"UNAVAILABLE", "message":"Preparation unavailable"}),
                false,
            ),
            (
                200,
                serde_json::json!({
                    "platform":"aws", "stack":{"id":"test", "resources":{}},
                    "setup":{"target":"aws/us-east-2", "fingerprint":"test", "version":1}
                }),
                true,
            ),
        ] {
            let server = httpmock::MockServer::start_async().await;
            let preparation = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST)
                        .path("/v1/deployment-info/prepare-stack")
                        .header("authorization", "Bearer test-group-token");
                    then.status(status).json_body(body);
                })
                .await;
            let plan = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::POST)
                        .path("/v1/deployment-info/compute-plan");
                    then.status(503)
                        .json_body(serde_json::json!({"message":"Planner unavailable"}));
                })
                .await;
            let create = server.mock_async(|when, then| {
                when.method(httpmock::Method::POST).path("/v1/deployments");
                // The deliberately rejected creation proves validation let a valid
                // empty stack proceed without fabricating a deployment response.
                then.status(409).json_body(serde_json::json!({"code":"CONFLICT", "message":"Synthetic creation rejection"}));
            }).await;
            let resolved = ResolvedDeployArgs {
                name: "test".to_string(),
                platform: "aws".to_string(),
                platform_enum: Platform::Aws,
                network_settings: None,
                compute_settings: None,
                domain_settings: None,
                input_values: HashMap::new(),
                public_subdomain: None,
            };
            let args =
                DeployArgs::try_parse_from(["deploy", "--name", "test", "--platform", "aws"])
                    .expect("deploy args");
            create_deployment_with_group_session(
                &server.base_url(),
                "test-group-token",
                &resolved,
                &args,
                "test-project",
            )
            .await
            .expect_err("preparation or synthetic creation must reject this request");
            preparation.assert_hits_async(1).await;
            create.assert_hits_async(usize::from(should_create)).await;
            plan.assert_hits_async(usize::from(status != 200)).await;
        }
    }

    #[test]
    fn deploy_config_accepts_and_serializes_compute_selection() {
        let config: DeployConfigFile = toml::from_str(
            r#"
name = "production"
platform = "aws"

[compute.pools.preview]
mode = "fixed"
machines = 1
machine = "m8i.2xlarge"
"#,
        )
        .expect("compute selection should be part of the public deploy config");
        validate_compute_settings(config.compute.as_ref()).expect("selection should be valid");

        let resolved = ResolvedDeployArgs {
            name: config.name.expect("name"),
            platform: config.platform.expect("platform"),
            platform_enum: Platform::Aws,
            network_settings: None,
            compute_settings: config.compute,
            domain_settings: config.domains,
            input_values: HashMap::new(),
            public_subdomain: None,
        };
        let args =
            DeployArgs::try_parse_from(["deploy", "--name", "production", "--platform", "aws"])
                .expect("minimal deploy args should parse");
        let settings = deployment_stack_settings_json(&resolved, &args)
            .expect("compute settings should serialize");

        assert_eq!(
            settings["compute"]["pools"]["preview"],
            serde_json::json!({
                "mode": "fixed",
                "machines": 1,
                "machine": "m8i.2xlarge"
            })
        );
    }

    #[test]
    fn deploy_config_preserves_custom_domains_in_both_creation_payloads() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("deploy.toml");
        std::fs::write(
            &path,
            r#"
name = "staging"
platform = "aws"
[domains.customDomains.gateway]
domain = "api.example.com"
[domains.customDomains.gateway.certificate.aws]
certificateArn = "arn:aws:acm:us-west-2:123456789012:certificate/customer"
"#,
        )
        .expect("write config");
        let args = DeployArgs::try_parse_from(["deploy", "--config", path.to_str().expect("path")])
            .expect("parse deploy arguments");
        let resolved = resolve_deploy_args(&args).expect("resolve custom domain config");
        let settings =
            deployment_stack_settings_json(&resolved, &args).expect("group creation payload");
        let expected = serde_json::json!({ "customDomains": { "gateway": {
            "domain": "api.example.com",
            "certificate": { "aws": { "certificateArn": "arn:aws:acm:us-west-2:123456789012:certificate/customer" }}
        }}});
        assert_eq!(settings["domains"], expected);
        let sdk_settings: alien_platform_api::types::NewDeploymentRequestStackSettings =
            serde_json::from_value(settings)
                .expect("normal creation SDK accepts the same settings");
        assert_eq!(
            serde_json::to_value(sdk_settings).expect("SDK serialization")["domains"],
            expected
        );
    }

    #[test]
    fn invalid_compute_bounds_fail_before_deployment_creation() {
        let config: DeployConfigFile = toml::from_str(
            r#"
[compute.pools.workers]
mode = "autoscale"
min = 3
max = 1
"#,
        )
        .expect("compute syntax should parse");

        let error = validate_compute_settings(config.compute.as_ref())
            .expect_err("invalid bounds must fail locally");
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("minimum"));
    }

    #[test]
    fn fixed_and_autoscale_compute_convert_to_generated_sdk_contract() {
        let compute: ComputeSettings = serde_json::from_value(serde_json::json!({
            "pools": {
                "fixed": {
                    "mode": "fixed",
                    "machines": 2,
                    "machine": "m8i.2xlarge"
                },
                "elastic": {
                    "mode": "autoscale",
                    "min": 1,
                    "max": 4,
                    "machine": "n2-standard-8"
                }
            }
        }))
        .expect("core compute settings should parse");

        let sdk = to_sdk_compute_settings(Some(compute))
            .expect("core settings must match the generated SDK schema")
            .expect("compute should be present");
        let json = serde_json::to_value(sdk).expect("SDK compute should serialize");

        assert_eq!(json["pools"]["fixed"]["machines"], 2);
        assert_eq!(json["pools"]["elastic"]["min"], 1);
        assert_eq!(json["pools"]["elastic"]["max"], 4);
    }

    #[test]
    fn raw_stack_input_values_accept_json_string_lists() {
        assert_eq!(
            parse_raw_stack_input_value(r#"["one.example","two.example"]"#),
            serde_json::json!(["one.example", "two.example"])
        );
        assert_eq!(
            parse_raw_stack_input_value("one.example,two.example"),
            serde_json::json!("one.example,two.example")
        );
        assert_eq!(
            parse_raw_stack_input_value("true"),
            serde_json::json!("true")
        );
    }

    #[test]
    fn resolved_stack_input_values_reach_platform_request_types() {
        let values = HashMap::from([
            ("domain".to_string(), serde_json::json!("mail.example")),
            (
                "domains".to_string(),
                serde_json::json!(["a.example", "b.example"]),
            ),
            ("enabled".to_string(), serde_json::json!(true)),
            ("replicas".to_string(), serde_json::json!(2)),
        ]);

        let converted = to_sdk_stack_input_values(&values).expect("valid inputs should convert");
        assert!(matches!(
            converted.get("domain"),
            Some(alien_platform_api::types::StackInputValueRequest::Variant0(value))
                if value == "mail.example"
        ));
        assert!(matches!(
            converted.get("domains"),
            Some(alien_platform_api::types::StackInputValueRequest::Variant3(values))
                if values == &["a.example".to_string(), "b.example".to_string()]
        ));
        assert!(matches!(
            converted.get("enabled"),
            Some(alien_platform_api::types::StackInputValueRequest::Variant2(
                true
            ))
        ));
        assert!(matches!(
            converted.get("replicas"),
            Some(alien_platform_api::types::StackInputValueRequest::Variant1(value))
                if (*value - 2.0).abs() < f64::EPSILON
        ));

        let invalid = HashMap::from([("nested".to_string(), serde_json::json!({ "x": 1 }))]);
        let error = to_sdk_stack_input_values(&invalid)
            .expect_err("object-valued stack inputs should be rejected");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn first_party_creation_preserves_requested_release_channel() {
        let resolved_args = ResolvedDeployArgs {
            name: "preview".to_string(),
            platform: "aws".to_string(),
            platform_enum: Platform::Aws,
            network_settings: None,
            compute_settings: None,
            domain_settings: None,
            input_values: HashMap::new(),
            public_subdomain: None,
        };
        let args = DeployArgs::try_parse_from([
            "deploy",
            "--name",
            "preview",
            "--platform",
            "aws",
            "--channel",
            "staging",
        ])
        .expect("deployment arguments should parse");

        let body = deployment_create_request_body(&resolved_args, &args, "proj_test")
            .expect("deployment request should serialize");
        assert_eq!(body["releaseChannel"], "staging");

        let input_body = first_party_inputs_request_body(
            "aws",
            &HashMap::from([("endpoint".to_string(), serde_json::json!("staging.example"))]),
            "staging",
        );
        assert_eq!(input_body["releaseChannel"], "staging");
    }

    #[tokio::test]
    async fn provisioning_always_uses_deployment_bearer_with_optional_workspace_routing() {
        for workspace in [None, Some("acme")] {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind manager test server");
            let address = listener.local_addr().expect("read manager test address");
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.expect("accept manager request");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = stream
                        .read(&mut buffer)
                        .await
                        .expect("read manager request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let body = r#"{"id":"dep_test","name":"test","platform":"aws","status":"pending","deploymentGroupId":"dg_test","deploymentProtocolVersion":1,"projectId":"proj_test","workspaceId":"ws_test","retryRequested":false,"createdAt":"2026-09-17T00:00:00Z"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write manager response");
                String::from_utf8(request).expect("manager request is HTTP text")
            });

            let http_client = deployment_manager_http_client("deployment-secret", workspace)
                .expect("manager client should build");
            let client = alien_manager_api::Client::new_with_client(
                &format!("http://{address}"),
                http_client,
            );
            client
                .get_deployment()
                .id("dep_test")
                .send()
                .await
                .expect("manager request should succeed");
            let request = server.await.expect("manager test server task");
            let request_lower = request.to_ascii_lowercase();

            assert!(request_lower.contains("authorization: bearer deployment-secret\r\n"));
            match workspace {
                Some(workspace) => {
                    assert!(request_lower.contains(&format!("x-alien-workspace: {workspace}\r\n")))
                }
                None => assert!(!request_lower.contains("x-alien-workspace:")),
            }
        }
    }

    fn scripted_observer(
        script: Vec<(DeploymentStatus, Option<&'static str>)>,
    ) -> impl FnMut() -> std::future::Ready<Result<ObservedDeployment>> {
        let mut script = script.into_iter();
        let mut last = None;
        move || {
            // Once the script runs out, the deployment stays in its last status.
            let (status, error) = script.next().or(last).expect("script is not empty");
            last = Some((status, error));
            std::future::ready(Ok(ObservedDeployment {
                status,
                error_message: error.map(str::to_string),
                stack_state: None,
            }))
        }
    }

    #[tokio::test]
    async fn handoff_wait_reports_a_blocked_deployment_once_and_succeeds_when_running() {
        let mut seen = Vec::new();
        wait_for_handed_off_deployment(
            scripted_observer(vec![
                (DeploymentStatus::Provisioning, None),
                (
                    DeploymentStatus::WaitingForSecrets,
                    Some("missing: API key"),
                ),
                (
                    DeploymentStatus::WaitingForSecrets,
                    Some("missing: API key"),
                ),
                (DeploymentStatus::Provisioning, None),
                (DeploymentStatus::Running, None),
            ]),
            Duration::from_millis(1),
            Duration::from_secs(5),
            |observed| seen.push((observed.status, observed.error_message.clone())),
        )
        .await
        .expect("a deployment that reaches running succeeds");

        assert_eq!(
            seen,
            vec![
                (DeploymentStatus::Provisioning, None),
                (
                    DeploymentStatus::WaitingForSecrets,
                    Some("missing: API key".to_string())
                ),
                (DeploymentStatus::Provisioning, None),
                (DeploymentStatus::Running, None),
            ]
        );
    }

    #[tokio::test]
    async fn handoff_wait_fails_with_the_deployment_error_after_the_handoff() {
        let error = wait_for_handed_off_deployment(
            scripted_observer(vec![
                (DeploymentStatus::Provisioning, None),
                (
                    DeploymentStatus::InitialSetupFailed,
                    Some("role trust policy rejected the manager"),
                ),
            ]),
            Duration::from_millis(1),
            Duration::from_secs(5),
            |_| {},
        )
        .await
        .expect_err("a failed deployment must not exit successfully");

        assert!(
            error
                .message
                .contains("role trust policy rejected the manager"),
            "unexpected error: {}",
            error.message
        );
        assert!(
            error.message.contains("InitialSetupFailed"),
            "unexpected error: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn handoff_wait_times_out_naming_the_blocked_status() {
        let error = wait_for_handed_off_deployment(
            scripted_observer(vec![(
                DeploymentStatus::WaitingForSecrets,
                Some("missing: API key"),
            )]),
            Duration::from_millis(1),
            Duration::from_millis(20),
            |_| {},
        )
        .await
        .expect_err("a deployment still blocked at the timeout is an error");

        assert!(
            error
                .message
                .contains("WaitingForSecrets: missing: API key"),
            "unexpected error: {}",
            error.message
        );
    }
}
