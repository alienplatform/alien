use super::*;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeployConfigFile {
    /// Deployment name.
    pub(super) name: Option<String>,
    /// Target platform: aws, gcp, azure, kubernetes, machines, or local.
    pub(super) platform: Option<String>,
    /// Base cloud platform when `platform = "kubernetes"`.
    pub(super) base_platform: Option<String>,
    /// Network settings for cloud deployments.
    pub(super) network: Option<DeployConfigNetwork>,
    /// Update delivery mode.
    pub(super) updates: Option<UpdatesMode>,
    /// Telemetry delivery mode.
    pub(super) telemetry: Option<TelemetryMode>,
    /// Static compute selections for Alien-managed runtime pools.
    pub(super) compute: Option<ComputeSettings>,
    /// Generic public endpoint URLs for pull-model deployments.
    pub(super) public_endpoints: Option<PublicEndpointUrls>,
    /// Deployer-provided stack inputs.
    pub(super) inputs: Option<HashMap<String, String>>,
    /// Secret deployer-provided stack inputs.
    pub(super) secret_inputs: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum DeployConfigNetwork {
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

pub(super) fn load_deploy_config(args: &UpArgs) -> Result<Option<DeployConfigFile>> {
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

pub(super) fn load_public_endpoints(
    args: &UpArgs,
    platform: Platform,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<Option<PublicEndpointUrls>> {
    let mut public_endpoints = deploy_config
        .and_then(|config| config.public_endpoints.clone())
        .unwrap_or_default();
    if !public_endpoints.is_empty() {
        validate_public_endpoint_urls(&public_endpoints).context(ErrorData::ValidationError {
            field: "publicEndpoints".to_string(),
            message: "Invalid public endpoint URL in deployment config".to_string(),
        })?;
    }

    let mut cli_endpoints = BTreeSet::new();
    for value in &args.public_endpoints {
        let (resource_id, endpoint_name, public_url) = parse_public_endpoint_assignment(value)
            .context(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: "Expected --public-endpoint <resource-id>.<endpoint-name>=<absolute-url>"
                    .to_string(),
            })?;
        let key = format!("{resource_id}.{endpoint_name}");
        if !cli_endpoints.insert(key.clone()) {
            return Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!("Duplicate public endpoint URL for '{key}'"),
            }));
        }
        public_endpoints
            .entry(resource_id)
            .or_default()
            .insert(endpoint_name, public_url);
    }

    if public_endpoints.is_empty() {
        return Ok(None);
    }

    match platform {
        Platform::Local | Platform::Machines => Ok(Some(public_endpoints)),
        Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Kubernetes | Platform::Test => {
            Err(AlienError::new(ErrorData::ValidationError {
                field: "public-endpoint".to_string(),
                message: format!(
                    "--public-endpoint is currently supported only for local or machines deployments, got '{}'",
                    platform.as_str()
                ),
            }))
        }
    }
}

pub(super) fn load_stack_settings(
    args: &UpArgs,
    platform: Platform,
    deploy_config: Option<&DeployConfigFile>,
) -> Result<StackSettings> {
    let mut settings = StackSettings::default();

    // The manager owns the deployment model for cloud platforms (push) and
    // Kubernetes (pull), so this CLI omits it there. Local is the exception:
    // the manager defaults Local to push for its own embedded dev loop, while
    // `deploy --platform local` is the remote-operator install flow — it must
    // request pull explicitly or the manager creates a push deployment whose
    // initial setup has no local platform services.
    if platform == Platform::Local {
        settings.deployment_model = DeploymentModel::Pull;
    }

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
        if let Some(compute) = config.compute.clone() {
            settings.compute = Some(compute);
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
