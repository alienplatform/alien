use super::*;

/// Resolved deployment info before manager connection.
pub(super) struct ResolvedInfo {
    pub(super) token: String,
    /// Manager URL (from override, tracker, or to be discovered via platform API).
    pub(super) manager_url: Option<String>,
    /// Platform API base URL used when manager URL must be discovered.
    pub(super) base_url: String,
    pub(super) platform: String,
    pub(super) base_platform: Option<String>,
    pub(super) name: String,
}

pub(super) fn requires_install_context(platform: Platform) -> bool {
    matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure)
}

pub(super) fn release_stack_value_for_platform(
    stack: alien_manager_api::types::StackByPlatform,
    platform: Platform,
) -> Option<serde_json::Value> {
    match platform {
        Platform::Aws => stack.aws,
        Platform::Gcp => stack.gcp,
        Platform::Azure => stack.azure,
        Platform::Kubernetes => stack.kubernetes,
        Platform::Machines => stack.machines,
        Platform::Local => stack.local,
        Platform::Test => stack.test,
    }
}

pub(super) async fn fetch_release_stack_by_id(
    client: &ServerClient,
    release_id: &str,
    platform: Platform,
) -> Result<Stack> {
    let release = client
        .get_release()
        .id(release_id)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to fetch release '{release_id}' from manager"),
        })?
        .into_inner();
    let stack_value =
        release_stack_value_for_platform(release.stack, platform).ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "Release '{}' has no stack for platform {}",
                    release_id,
                    platform.as_str()
                ),
            })
        })?;

    serde_json::from_value(stack_value)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: format!("Failed to parse release stack from release '{release_id}'"),
        })
}

pub(super) fn validate_public_endpoint_names(
    public_endpoints: &PublicEndpointUrls,
    stack: &Stack,
) -> Result<()> {
    let valid_endpoints = public_endpoint_names(stack);
    for (resource_id, endpoints) in public_endpoints {
        for endpoint_name in endpoints.keys() {
            let key = format!("{resource_id}.{endpoint_name}");
            if valid_endpoints.contains(&key) {
                continue;
            }

            let available = if valid_endpoints.is_empty() {
                "none".to_string()
            } else {
                valid_endpoints
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!(
                    "Endpoint '{key}' is not declared by the stack. Available public endpoints: {available}"
                ),
            }));
        }
    }
    Ok(())
}

fn public_endpoint_names(stack: &Stack) -> BTreeSet<String> {
    stack
        .resources()
        .flat_map(|(resource_id, entry)| {
            if let Some(daemon) = entry.config.downcast_ref::<Daemon>() {
                return daemon
                    .public_endpoints
                    .iter()
                    .map(|endpoint| format!("{resource_id}.{}", endpoint.name))
                    .collect::<Vec<_>>();
            }
            if let Some(container) = entry.config.downcast_ref::<Container>() {
                return container
                    .public_endpoints
                    .iter()
                    .map(|endpoint| format!("{resource_id}.{}", endpoint.name))
                    .collect::<Vec<_>>();
            }
            if let Some(worker) = entry.config.downcast_ref::<Worker>() {
                return worker
                    .public_endpoints
                    .iter()
                    .map(|endpoint| format!("{resource_id}.{}", endpoint.name))
                    .collect::<Vec<_>>();
            }
            Vec::new()
        })
        .collect()
}

pub(super) fn parse_base_platform(
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
        Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "base-platform".to_string(),
                message: "--base-platform must be one of: aws, gcp, azure".to_string(),
            }))
        }
    }
}

pub(super) fn resolve_deployment_info(
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
                    "--platform is required for new deployments. Choose from: aws, gcp, azure, kubernetes, machines, local."
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

pub(crate) fn resolve_optional_token(
    token: Option<String>,
    token_file: Option<&PathBuf>,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<Option<String>> {
    Ok(token
        .map(Ok)
        .or_else(|| token_file.map(|path| read_token_file(path)))
        .transpose()?
        .or_else(|| {
            embedded_config
                .and_then(|c| c.token_env_var.as_ref())
                .and_then(|env_var| std::env::var(env_var).ok())
        })
        .or_else(|| embedded_config.and_then(|c| c.token.clone())))
}

pub(super) fn resolve_token(
    args: &UpArgs,
    embedded_config: Option<&DeployCliConfig>,
) -> Result<String> {
    resolve_optional_token(args.token.clone(), args.token_file.as_ref(), embedded_config)?
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

pub(crate) fn read_token_file(path: &Path) -> Result<String> {
    let token = std::fs::read_to_string(path).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: format!("Failed to read token file {}", path.display()),
        },
    )?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "token-file".to_string(),
            message: format!("Token file {} is empty", path.display()),
        }));
    }
    Ok(token)
}

pub(crate) fn resolve_base_url_option(
    base_url: Option<&String>,
    embedded_config: Option<&DeployCliConfig>,
) -> String {
    base_url
        .cloned()
        .or_else(|| embedded_config.and_then(|c| c.api_base_url.clone()))
        .unwrap_or_else(|| "https://api.alien.dev".to_string())
}

fn resolve_base_url(args: &UpArgs, embedded_config: Option<&DeployCliConfig>) -> String {
    resolve_base_url_option(args.base_url.as_ref(), embedded_config)
}

pub(crate) fn resolve_platform_option(
    platform: Option<&String>,
    embedded_config: Option<&DeployCliConfig>,
    command: &str,
) -> Result<String> {
    platform
        .cloned()
        .or_else(|| embedded_config.and_then(|c| c.default_platform.clone()))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ValidationError {
                field: "platform".to_string(),
                message: format!(
                    "--platform is required for {command} when --manager-url is not set and the binary has no embedded default platform."
                ),
            })
        })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeploymentInfoResponse {
    pub(super) setup_config: Option<DeploymentInfoSetupConfig>,
    pub(super) readiness: Option<DeploymentReadiness>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeploymentInfoSetupConfig {
    pub(super) inputs: Option<Vec<StackInputDefinition>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeploymentReadiness {
    pub(super) status: String,
    pub(super) checks: Vec<DeploymentReadinessCheck>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeploymentReadinessCheck {
    pub(super) code: String,
    pub(super) status: String,
    pub(super) message: String,
}

pub(super) async fn fetch_deployment_info(
    base_url: &str,
    token: &str,
    platform: Platform,
) -> Result<DeploymentInfoResponse> {
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

    let mut url = reqwest::Url::parse(&format!(
        "{}/v1/deployment-info",
        base_url.trim_end_matches('/')
    ))
    .into_alien_error()
    .context(ErrorData::ConfigurationError {
        message: "Invalid platform API base URL".to_string(),
    })?;
    url.query_pairs_mut()
        .append_pair("platform", platform.as_str());
    let response = http_client
        .get(url)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to fetch deployment info from platform API".to_string(),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!("Failed to fetch deployment info (HTTP {status}): {body}"),
        }));
    }

    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Failed to parse deployment info response".to_string(),
        })
}

pub(super) fn deployer_inputs_from_info(
    info: &DeploymentInfoResponse,
    platform: Platform,
) -> Vec<StackInputDefinition> {
    info.setup_config
        .as_ref()
        .and_then(|setup_config| setup_config.inputs.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|input| stack_input_matches_context(input, platform))
        .collect()
}

pub(super) fn validate_deployment_readiness(
    info: &DeploymentInfoResponse,
    platform: Platform,
) -> Result<()> {
    let Some(readiness) = &info.readiness else {
        return Ok(());
    };
    if readiness.status == "notReady" {
        let failures = readiness
            .checks
            .iter()
            .filter(|check| check.status == "failed")
            .map(|check| format!("{}: {}", check.code, check.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "{} deployments are not ready: {failures}",
                platform.as_str()
            ),
        }));
    }
    for check in readiness
        .checks
        .iter()
        .filter(|check| check.status == "unknown")
    {
        output::warn(&format!(
            "Readiness unknown ({}): {}",
            check.code, check.message
        ));
    }
    Ok(())
}

pub(crate) async fn resolve_manager_url_option(
    manager_url: Option<String>,
    base_url: &str,
    token: &str,
    platform: &str,
) -> Result<String> {
    if let Some(manager_url) = manager_url {
        return Ok(manager_url);
    }

    discover_manager_install_context(base_url, token, platform)
        .await
        .map(|context| context.manager_url)
}
