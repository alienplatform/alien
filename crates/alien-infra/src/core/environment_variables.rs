use crate::core::{state_utils::StackResourceStateExt, ResourceControllerContext};
use crate::error::{ErrorData, Result};
#[cfg(feature = "local")]
use alien_bindings::{BindingsProviderApi as _, Vault};
use alien_core::{
    bindings::serialize_binding_as_env_var, container_runtime_environment_contract,
    daemon_runtime_environment_contract, kubernetes_base_platform_runtime_environment_plan,
    public_url_host, render_runtime_environment_entries, render_runtime_environment_plan,
    standard_runtime_environment_plan, validate_prepared_runtime_environment_map,
    worker_runtime_environment_contract, Container, Daemon, EnvironmentVariable,
    EnvironmentVariableType, ResourceRef, ResourceStatus, RuntimeEnvironmentBindingEntry,
    RuntimeEnvironmentRenderer, RuntimeEnvironmentValue, Worker,
    ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME, ENV_ALIEN_CURRENT_WORKER_BINDING_NAME,
    ENV_ALIEN_PUBLIC_ENDPOINTS_JSON, ENV_ALIEN_WORKER_TIMEOUT_SECONDS,
};
#[cfg(feature = "local")]
use alien_error::ContextError;
use alien_error::{AlienError, Context, IntoAlienError};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

pub const OTEL_EXPORTER_OTLP_HEADERS: &str = "OTEL_EXPORTER_OTLP_HEADERS";
pub const OTEL_EXPORTER_OTLP_METRICS_HEADERS: &str = "OTEL_EXPORTER_OTLP_METRICS_HEADERS";

fn matches_environment_target(resource_id: &str, target_resources: &Option<Vec<String>>) -> bool {
    match target_resources {
        None => true,
        Some(patterns) if patterns.is_empty() => false,
        Some(patterns) => patterns.iter().any(|pattern| {
            if let Some(prefix) = pattern.strip_suffix('*') {
                resource_id.starts_with(prefix)
            } else {
                resource_id == pattern
            }
        }),
    }
}

pub(crate) fn applicable_secret_environment_variables<'a>(
    resource_id: &str,
    variables: &'a [EnvironmentVariable],
) -> Vec<&'a EnvironmentVariable> {
    variables
        .iter()
        .filter(|var| var.var_type == EnvironmentVariableType::Secret)
        .filter(|var| matches_environment_target(resource_id, &var.target_resources))
        .collect()
}

/// Replaces a local workload's `ALIEN_DEPLOYER_SECRETS` list with the values
/// the developer set in the local `secrets` vault (`alien dev vault set`), read
/// as the process starts so a changed value is picked up by the next restart.
/// Returns the names it set; a required secret that is not set fails the start
/// with "missing: <label>", an optional one stays unset.
#[cfg(feature = "local")]
pub(crate) async fn resolve_local_deployer_secrets(
    ctx: &ResourceControllerContext<'_>,
    env_vars: &mut HashMap<String, String>,
) -> Result<Vec<String>> {
    let Some(deployer_secrets) = env_vars.remove(alien_core::ENV_ALIEN_DEPLOYER_SECRETS) else {
        return Ok(Vec::new());
    };
    let deployer_secrets: Vec<alien_core::DeployerSecretEnv> =
        serde_json::from_str(&deployer_secrets)
            .into_alien_error()
            .context(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "{} is not a deployer secret list",
                    alien_core::ENV_ALIEN_DEPLOYER_SECRETS
                ),
                resource_id: None,
            })?;
    if deployer_secrets.is_empty() {
        return Ok(Vec::new());
    }

    let bindings_provider = ctx
        .service_provider
        .get_local_bindings_provider()
        .ok_or_else(|| {
            AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "bindings_provider".to_string(),
            })
        })?;
    let vault = bindings_provider
        .load_vault(alien_core::SECRETS_VAULT_ID)
        .await
        .context(ErrorData::ResourceConfigInvalid {
            message: "Failed to load the local secrets vault".to_string(),
            resource_id: None,
        })?;
    read_deployer_secrets(vault.as_ref(), deployer_secrets, env_vars).await
}

/// Reads each deployer secret from `vault` into `env_vars` and returns the
/// names it set. A required secret that is not set fails with
/// "missing: <label>"; an optional one stays unset.
#[cfg(feature = "local")]
async fn read_deployer_secrets(
    vault: &dyn Vault,
    deployer_secrets: Vec<alien_core::DeployerSecretEnv>,
    env_vars: &mut HashMap<String, String>,
) -> Result<Vec<String>> {
    let mut names = Vec::with_capacity(deployer_secrets.len());
    for secret in deployer_secrets {
        match vault.get_secret(&secret.vault_key).await {
            Ok(value) => {
                env_vars.insert(secret.name.clone(), value);
                names.push(secret.name);
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_bindings::ErrorData::VaultSecretNotFound { .. })
                ) =>
            {
                if secret.required {
                    return Err(AlienError::new(ErrorData::DeployerSecretMissing {
                        label: secret.label,
                        secret_name: secret.secret_name,
                    }));
                }
            }
            Err(error) => {
                return Err(error.context(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to read deployer secret '{}'", secret.label),
                    resource_id: None,
                }))
            }
        }
    }
    Ok(names)
}

pub fn direct_monitoring_auth_headers(
    ctx: &ResourceControllerContext<'_>,
) -> BTreeMap<String, String> {
    let Some(monitoring) = &ctx.deployment_config.monitoring else {
        return BTreeMap::new();
    };

    let mut headers = BTreeMap::from([(
        OTEL_EXPORTER_OTLP_HEADERS.to_string(),
        monitoring.logs_auth_header.clone(),
    )]);
    if monitoring.metrics_endpoint.is_some() {
        headers.insert(
            OTEL_EXPORTER_OTLP_METRICS_HEADERS.to_string(),
            monitoring
                .metrics_auth_header
                .clone()
                .unwrap_or_else(|| monitoring.logs_auth_header.clone()),
        );
    }
    headers
}

/// Common environment variable preparation for worker controllers.
/// This handles the shared logic of processing linked resources and setting up
/// platform-agnostic environment variables.
pub struct EnvironmentVariableBuilder {
    env_vars: HashMap<String, String>,
    /// Track bindings for platform-specific processing (e.g., Kubernetes SecretRefs)
    /// Stored as (binding_name, binding_json) to avoid serialization round-trips
    bindings: Vec<(String, serde_json::Value)>,
}

struct ControllerRuntimeEnvironmentRenderer<'ctx, 'state> {
    ctx: &'ctx ResourceControllerContext<'state>,
    current_container_id: Option<&'ctx str>,
    current_worker_id: Option<&'ctx str>,
}

impl RuntimeEnvironmentRenderer for ControllerRuntimeEnvironmentRenderer<'_, '_> {
    type Value = String;

    fn render_runtime_environment_value(
        &self,
        value: RuntimeEnvironmentValue,
    ) -> alien_core::Result<Option<Self::Value>> {
        match value {
            RuntimeEnvironmentValue::Literal(value) => Ok(Some(value.to_string())),
            RuntimeEnvironmentValue::AwsAccountId => Ok(self
                .ctx
                .get_aws_config()
                .ok()
                .map(|config| config.account_id.clone())),
            RuntimeEnvironmentValue::AwsRegion => Ok(self
                .ctx
                .get_aws_config()
                .ok()
                .map(|config| config.region.clone())),
            RuntimeEnvironmentValue::AzureRegion => Ok(self
                .ctx
                .get_azure_config()
                .ok()
                .and_then(|config| config.region.clone())),
            RuntimeEnvironmentValue::AzureSubscriptionId => Ok(self
                .ctx
                .get_azure_config()
                .ok()
                .map(|config| config.subscription_id.clone())),
            RuntimeEnvironmentValue::AzureTenantId => Ok(self
                .ctx
                .get_azure_config()
                .ok()
                .map(|config| config.tenant_id.clone())),
            RuntimeEnvironmentValue::BasePlatform => Ok(self
                .ctx
                .deployment_config
                .base_platform
                .map(|platform| platform.as_str().to_string())),
            RuntimeEnvironmentValue::GcpProjectId => Ok(self
                .ctx
                .get_gcp_config()
                .ok()
                .map(|config| config.project_id.clone())),
            RuntimeEnvironmentValue::GcpRegion => Ok(self
                .ctx
                .get_gcp_config()
                .ok()
                .map(|config| config.region.clone())),
            RuntimeEnvironmentValue::CurrentContainerBindingName => {
                Ok(self.current_container_id.map(ToString::to_string))
            }
            RuntimeEnvironmentValue::CurrentWorkerBindingName => {
                Ok(self.current_worker_id.map(ToString::to_string))
            }
            RuntimeEnvironmentValue::AzureClientId => Ok(None),
        }
    }

    fn render_runtime_environment_binding(
        &self,
        _entry: &RuntimeEnvironmentBindingEntry,
    ) -> alien_core::Result<Option<Self::Value>> {
        Ok(None)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicEndpointEnv {
    url: String,
    host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    wildcard_host: Option<String>,
}

fn current_resource_wildcard_endpoints(
    ctx: &ResourceControllerContext<'_>,
) -> HashMap<String, bool> {
    if let Some(container) = ctx.desired_config.downcast_ref::<Container>() {
        return container
            .public_endpoints
            .iter()
            .map(|endpoint| (endpoint.name.clone(), endpoint.wildcard_subdomains))
            .collect();
    }
    if let Some(daemon) = ctx.desired_config.downcast_ref::<Daemon>() {
        return daemon
            .public_endpoints
            .iter()
            .map(|endpoint| (endpoint.name.clone(), endpoint.wildcard_subdomains))
            .collect();
    }
    if let Some(worker) = ctx.desired_config.downcast_ref::<Worker>() {
        return worker
            .public_endpoints
            .iter()
            .map(|endpoint| (endpoint.name.clone(), endpoint.wildcard_subdomains))
            .collect();
    }

    HashMap::new()
}

fn current_resource_public_endpoint_urls(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> HashMap<String, String> {
    if let Some(endpoint_urls) = ctx
        .deployment_config
        .public_endpoints
        .as_ref()
        .and_then(|resources| resources.get(resource_id))
    {
        return endpoint_urls.clone();
    }

    let Some(resource) = ctx
        .deployment_config
        .domain_metadata
        .as_ref()
        .and_then(|metadata| metadata.resources.get(resource_id))
    else {
        return HashMap::new();
    };

    if !resource.endpoints.is_empty() {
        return resource
            .endpoints
            .iter()
            .map(|(endpoint_name, endpoint)| {
                (endpoint_name.clone(), format!("https://{}", endpoint.fqdn))
            })
            .collect();
    }

    HashMap::from([("default".to_string(), format!("https://{}", resource.fqdn))])
}

impl EnvironmentVariableBuilder {
    /// Create a new builder starting with the initial environment variables.
    pub fn new(initial_env: &HashMap<String, String>) -> Self {
        Self {
            env_vars: initial_env.clone(),
            bindings: Vec::new(),
        }
    }

    /// Create a new builder and reject user-provided Alien runtime names.
    pub fn try_new(initial_env: &HashMap<String, String>) -> Result<Self> {
        validate_prepared_runtime_environment_map(initial_env).map_err(|error| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: error.to_string(),
                resource_id: None,
            })
        })?;
        Ok(Self::new(initial_env))
    }

    /// Add standard Alien environment variables that should be available to all resources.
    /// This includes ALIEN_DEPLOYMENT_TYPE which indicates the current platform, plus platform-specific
    /// identifiers like AWS_ACCOUNT_ID, AWS_REGION, GCP_PROJECT_ID, AZURE_TENANT_ID, etc.
    pub fn add_standard_alien_env_vars(
        mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Self> {
        let renderer = ControllerRuntimeEnvironmentRenderer {
            ctx,
            current_container_id: None,
            current_worker_id: None,
        };
        for (name, value) in render_runtime_environment_entries(
            standard_runtime_environment_plan(ctx.platform),
            &renderer,
        )
        .map_err(|error| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: error.to_string(),
                resource_id: None,
            })
        })? {
            self.env_vars.insert(name.to_string(), value);
        }
        self.add_kubernetes_base_platform_env_vars(ctx, &renderer)?;

        Ok(self)
    }

    pub fn add_current_resource_public_endpoint(
        mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<Self> {
        let endpoint_urls = current_resource_public_endpoint_urls(ctx, resource_id);
        if endpoint_urls.is_empty() {
            return Ok(self);
        }

        let wildcard_endpoints = current_resource_wildcard_endpoints(ctx);
        let mut env_endpoints = HashMap::new();
        for (endpoint_name, public_url) in &endpoint_urls {
            let host = public_url_host(public_url).ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "public endpoint '{endpoint_name}' URL does not contain a valid host"
                    ),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;
            let wildcard_host = wildcard_endpoints
                .get(endpoint_name)
                .copied()
                .unwrap_or(false)
                .then(|| format!("*.{host}"));
            env_endpoints.insert(
                endpoint_name.clone(),
                PublicEndpointEnv {
                    url: public_url.clone(),
                    host,
                    wildcard_host,
                },
            );
        }

        if !env_endpoints.is_empty() {
            let json = serde_json::to_string(&env_endpoints)
                .into_alien_error()
                .context(ErrorData::ResourceConfigInvalid {
                    message: "failed to serialize public endpoint environment metadata".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })?;
            self.env_vars
                .insert(ENV_ALIEN_PUBLIC_ENDPOINTS_JSON.to_string(), json);
        }

        Ok(self)
    }

    /// Add the complete scalar runtime environment for a Worker.
    pub fn add_worker_runtime_env_vars(
        mut self,
        ctx: &ResourceControllerContext<'_>,
        worker_id: &str,
        timeout_seconds: u32,
    ) -> Result<Self> {
        let renderer = ControllerRuntimeEnvironmentRenderer {
            ctx,
            current_container_id: None,
            current_worker_id: Some(worker_id),
        };
        let plan = worker_runtime_environment_contract(ctx.platform, worker_id, &[]);
        for (name, value) in render_runtime_environment_plan(&plan, &renderer).map_err(|error| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: error.to_string(),
                resource_id: Some(worker_id.to_string()),
            })
        })? {
            self.env_vars.insert(name, value);
        }
        self.env_vars.insert(
            ENV_ALIEN_WORKER_TIMEOUT_SECONDS.to_string(),
            timeout_seconds.to_string(),
        );
        self.add_kubernetes_base_platform_env_vars(ctx, &renderer)?;

        Ok(self)
    }

    /// Add the complete scalar runtime environment for a Container.
    pub fn add_container_runtime_env_vars(
        mut self,
        ctx: &ResourceControllerContext<'_>,
        container_id: &str,
    ) -> Result<Self> {
        let renderer = ControllerRuntimeEnvironmentRenderer {
            ctx,
            current_container_id: Some(container_id),
            current_worker_id: None,
        };
        let plan = container_runtime_environment_contract(ctx.platform, container_id, &[]);
        for (name, value) in render_runtime_environment_plan(&plan, &renderer).map_err(|error| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: error.to_string(),
                resource_id: Some(container_id.to_string()),
            })
        })? {
            self.env_vars.insert(name, value);
        }
        self.add_kubernetes_base_platform_env_vars(ctx, &renderer)?;

        Ok(self)
    }

    /// Add the complete scalar runtime environment for a Daemon.
    ///
    /// Daemons run runtime-less under direct supervision: the
    /// contract is the standard platform-identity set only — no transport var,
    /// no self-binding var. Command-enabled Daemons receive their pull-receiver
    /// config (`ALIEN_COMMANDS_*`) per-resource through `config.environment`,
    /// not from this plan.
    pub fn add_daemon_runtime_env_vars(
        mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Self> {
        let renderer = ControllerRuntimeEnvironmentRenderer {
            ctx,
            current_container_id: None,
            current_worker_id: None,
        };
        let plan = daemon_runtime_environment_contract(ctx.platform, &[]);
        for (name, value) in render_runtime_environment_plan(&plan, &renderer).map_err(|error| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: error.to_string(),
                resource_id: None,
            })
        })? {
            self.env_vars.insert(name, value);
        }
        self.add_kubernetes_base_platform_env_vars(ctx, &renderer)?;

        Ok(self)
    }

    /// Adds monitoring credentials for a runtime-less workload at the final
    /// provisioning boundary. The values come from `DeploymentConfig`, not the
    /// resource config: Local passes them directly to the process, and
    /// Kubernetes replaces them with Secret refs before serializing a Pod.
    pub fn add_direct_monitoring_auth_headers(
        mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Self {
        self.env_vars.extend(direct_monitoring_auth_headers(ctx));

        self
    }

    fn add_kubernetes_base_platform_env_vars(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        renderer: &ControllerRuntimeEnvironmentRenderer<'_, '_>,
    ) -> Result<()> {
        if ctx.platform != alien_core::Platform::Kubernetes {
            return Ok(());
        }

        for (name, value) in render_runtime_environment_entries(
            kubernetes_base_platform_runtime_environment_plan(ctx.deployment_config.base_platform),
            renderer,
        )
        .map_err(|error| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: error.to_string(),
                resource_id: None,
            })
        })? {
            self.env_vars.insert(name.to_string(), value);
        }

        Ok(())
    }

    /// Add environment variables for linked resources.
    /// This handles the common pattern of ALIEN_{BINDING_NAME}_* variables.
    ///
    /// Checks for binding params in this order:
    /// 1. Internal controller's `get_binding_params()` (for Alien-provisioned resources)
    /// 2. External bindings (for pre-existing infrastructure)
    pub async fn add_linked_resources(
        mut self,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
        resource_id_for_errors: &str,
    ) -> Result<Self> {
        for link in links {
            let binding_name = link.id();

            // Get the dependency's state
            let resource_state = ctx.state.resources.get(binding_name).ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: resource_id_for_errors.to_string(),
                    dependency_id: binding_name.to_string(),
                })
            })?;

            // Ensure the dependency is actually in a stable state that can provide environment variables
            let dependency_status = resource_state.status;
            if !matches!(dependency_status, ResourceStatus::Running) {
                return Err(AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: resource_id_for_errors.to_string(),
                    dependency_id: binding_name.to_string(),
                }));
            }

            // Synced binding coordinates only (Local Postgres strips its password here); the linked
            // workload's compute-target manager delivers any runtime-only secret at process start.
            let binding_params =
                if let Some(dependency_controller) = resource_state.get_internal_controller()? {
                    dependency_controller.get_binding_params()?
                } else {
                    None
                };

            // If no internal controller or no binding params, check external bindings
            let binding_params = match binding_params {
                Some(params) => Some(params),
                None => {
                    // Check if there's an external binding for this resource
                    match ctx.deployment_config.external_bindings.get(binding_name) {
                        Some(external) => {
                            // External Postgres and AI on the local platform: the binding
                            // inlines a raw secret, and the local runtime-only channel would
                            // either persist it into worker metadata (Postgres) or silently
                            // replace it with the dev key from the process environment (AI).
                            if ctx.platform == alien_core::Platform::Local
                                && matches!(
                                    external,
                                    alien_core::ExternalBinding::Postgres(_)
                                        | alien_core::ExternalBinding::Ai(_)
                                )
                            {
                                let kind = match external {
                                    alien_core::ExternalBinding::Postgres(_) => {
                                        "external (Remote Access) Postgres"
                                    }
                                    _ => "an external (BYO-key) AI binding",
                                };
                                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                                    message: format!(
                                        "{kind} is not supported on the local platform"
                                    ),
                                    resource_id: Some(binding_name.to_string()),
                                }));
                            }
                            let value = external
                                .to_env_binding_value()
                                .into_alien_error()
                                .context(ErrorData::ResourceStateSerializationFailed {
                                    resource_id: binding_name.to_string(),
                                    message: "Failed to serialize external binding parameters"
                                        .to_string(),
                                })?;
                            Some(value)
                        }
                        None => None,
                    }
                }
            };

            // Add binding environment variables if we have params
            if let Some(params) = binding_params {
                // Store binding metadata for platform-specific processing (e.g., Kubernetes SecretRefs)
                self.bindings
                    .push((binding_name.to_string(), params.clone()));

                let binding_env_vars = serialize_binding_as_env_var(binding_name, &params)
                    .context(ErrorData::ResourceConfigInvalid {
                        message: "Failed to serialize binding parameters".to_string(),
                        resource_id: Some(binding_name.to_string()),
                    })?;

                self.env_vars.extend(binding_env_vars);
            }
        }

        Ok(self)
    }

    /// Add a single environment variable.
    pub fn add_env_var(mut self, key: String, value: String) -> Self {
        self.env_vars.insert(key, value);
        self
    }

    /// Add the function's own binding to its environment variables for self-introspection.
    /// This adds both:
    /// 1. ALIEN_CURRENT_WORKER_BINDING_NAME - the function's ID for identifying itself
    /// 2. ALIEN_{FUNCTION_ID}_BINDING - the function's full binding parameters (if available)
    ///
    /// The binding params should be provided when available. During initial creation, binding
    /// params may be incomplete (e.g., URL not yet known). During updates or after creation
    /// completes, full binding params should be available.
    pub fn add_self_worker_binding(
        mut self,
        worker_id: &str,
        binding_params: Option<&serde_json::Value>,
    ) -> Result<Self> {
        // Always add the current function's binding name (its ID)
        self.env_vars.insert(
            ENV_ALIEN_CURRENT_WORKER_BINDING_NAME.to_string(),
            worker_id.to_string(),
        );

        // Add the full binding parameters if available
        if let Some(params) = binding_params {
            // Use the centralized function to serialize binding parameters
            let binding_env_vars = serialize_binding_as_env_var(worker_id, params).context(
                ErrorData::ResourceConfigInvalid {
                    message: "Failed to serialize self worker binding parameters".to_string(),
                    resource_id: Some(worker_id.to_string()),
                },
            )?;

            // Add all the binding environment variables
            self.env_vars.extend(binding_env_vars);
        }

        Ok(self)
    }

    /// Add the container's own binding to its environment variables for self-introspection.
    pub fn add_self_container_binding(
        mut self,
        container_id: &str,
        binding_params: Option<&serde_json::Value>,
    ) -> Result<Self> {
        self.env_vars.insert(
            ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME.to_string(),
            container_id.to_string(),
        );

        if let Some(params) = binding_params {
            let binding_env_vars = serialize_binding_as_env_var(container_id, params).context(
                ErrorData::ResourceConfigInvalid {
                    message: "Failed to serialize self container binding parameters".to_string(),
                    resource_id: Some(container_id.to_string()),
                },
            )?;

            self.env_vars.extend(binding_env_vars);
        }

        Ok(self)
    }

    /// Build the final environment variables map.
    pub fn build(self) -> HashMap<String, String> {
        self.env_vars
    }

    /// Build with bindings for platform-specific processing (e.g., Kubernetes SecretRefs).
    /// Returns (env_vars, bindings) where bindings is a list of (binding_name, binding_json).
    pub fn build_with_bindings(
        self,
    ) -> (HashMap<String, String>, Vec<(String, serde_json::Value)>) {
        (self.env_vars, self.bindings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(feature = "local")]
    mod local_deployer_secrets {
        use super::*;
        use alien_bindings::providers::vault::LocalVault;
        use alien_core::DeployerSecretEnv;

        fn secret(name: &str, vault_key: &str, label: &str, required: bool) -> DeployerSecretEnv {
            DeployerSecretEnv {
                name: name.to_string(),
                vault_key: vault_key.to_string(),
                secret_name: format!("secrets/{vault_key}"),
                label: label.to_string(),
                required,
            }
        }

        fn vault(dir: &tempfile::TempDir) -> LocalVault {
            LocalVault::new("secrets".to_string(), dir.path().to_path_buf())
        }

        #[tokio::test]
        async fn a_missing_required_secret_blocks_the_start() {
            let dir = tempfile::tempdir().unwrap();
            let mut env = HashMap::new();

            let error = read_deployer_secrets(
                &vault(&dir),
                vec![secret(
                    "DATABASE_PASSWORD",
                    "input-database-password",
                    "Database password",
                    true,
                )],
                &mut env,
            )
            .await
            .unwrap_err();

            assert_eq!(error.code, "DEPLOYER_SECRET_MISSING");
            assert!(
                error.message.starts_with("missing: Database password"),
                "{}",
                error.message
            );
            assert!(!env.contains_key("DATABASE_PASSWORD"));
        }

        #[tokio::test]
        async fn a_missing_optional_secret_stays_unset() {
            let dir = tempfile::tempdir().unwrap();
            let mut env = HashMap::new();

            let names = read_deployer_secrets(
                &vault(&dir),
                vec![secret(
                    "LICENSE_KEY",
                    "input-license-key",
                    "License key",
                    false,
                )],
                &mut env,
            )
            .await
            .unwrap();

            assert!(names.is_empty());
            assert!(!env.contains_key("LICENSE_KEY"));
        }

        #[tokio::test]
        async fn every_start_reads_the_current_value() {
            let dir = tempfile::tempdir().unwrap();
            let secrets = vec![secret(
                "DATABASE_PASSWORD",
                "input-database-password",
                "Database password",
                true,
            )];
            vault(&dir)
                .set_secret("input-database-password", "first")
                .await
                .unwrap();

            let mut env = HashMap::new();
            let names = read_deployer_secrets(&vault(&dir), secrets.clone(), &mut env)
                .await
                .unwrap();
            assert_eq!(names, vec!["DATABASE_PASSWORD".to_string()]);
            assert_eq!(env["DATABASE_PASSWORD"], "first");

            // Rotation: the next start sees the new value.
            vault(&dir)
                .set_secret("input-database-password", "rotated")
                .await
                .unwrap();
            let mut env = HashMap::new();
            read_deployer_secrets(&vault(&dir), secrets, &mut env)
                .await
                .unwrap();
            assert_eq!(env["DATABASE_PASSWORD"], "rotated");
        }
    }

    #[test]
    fn test_bindings_tracked_for_k8s_processing() {
        // Verify that when we add linked resources, the binding JSON is tracked
        let env = HashMap::new();
        let mut builder = EnvironmentVariableBuilder::new(&env);

        // Manually track a binding (simulating what add_linked_resources does)
        let binding_json = json!({
            "service": "redis",
            "host": "redis.internal",
            "port": 6379
        });
        builder
            .bindings
            .push(("cache".to_string(), binding_json.clone()));

        let (_env_vars, bindings) = builder.build_with_bindings();

        // Verify bindings are tracked
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, "cache");
        assert_eq!(bindings[0].1, binding_json);
    }

    /// Real-path assertion for the AI external-binding env-var injection: drives the
    /// PRODUCTION serialization (`ExternalBinding::to_env_binding_value`, the same
    /// call `add_linked_resources` makes) through `serialize_binding_as_env_var` and
    /// asserts the `ALIEN_<NAME>_BINDING` value is the exact service-tagged JSON the
    /// SDK's `ai(name)` parser consumes. A drift here (e.g. dropping the
    /// `AiBinding::External` wrap) silently routes external OpenAI/Anthropic to the
    /// wrong upstream.
    #[test]
    fn test_ai_external_binding_injects_service_tagged_env_var() {
        use alien_core::bindings::{
            binding_env_var_name, serialize_binding_as_env_var, ExternalAiBinding,
        };
        use alien_core::ExternalBinding;

        let external = ExternalBinding::Ai(ExternalAiBinding {
            provider: "openai".to_string(),
            api_key: "sk-test".into(),
        });

        let value = external
            .to_env_binding_value()
            .expect("external AI binding should serialize");

        let env = serialize_binding_as_env_var("llm", &value).unwrap();
        let key = binding_env_var_name("llm");
        assert_eq!(key, "ALIEN_LLM_BINDING");

        let injected: serde_json::Value =
            serde_json::from_str(env.get(&key).expect("binding env var present")).unwrap();
        assert_eq!(
            injected,
            json!({
                "service": "external-ai",
                "provider": "openai",
                "apiKey": "sk-test",
            }),
            "injected AI binding env var must match the SDK's expected service-tagged shape"
        );
    }

    #[test]
    fn test_build_without_bindings_returns_empty_list() {
        let env = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        let builder = EnvironmentVariableBuilder::new(&env);

        let (env_vars, bindings) = builder.build_with_bindings();

        assert_eq!(env_vars.len(), 1);
        assert_eq!(env_vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(bindings.len(), 0);
    }

    /// Ensures neither the Container
    /// nor the Daemon compute env plan may inject any retired worker/binding env
    /// var on any platform. Fails loudly if a forbidden name reappears in either
    /// static plan (the controller manifest tests guard the rendered manifests).
    #[test]
    fn forbidden_env_absent_from_container_and_daemon_plans() {
        use alien_core::{
            container_runtime_environment_plan, daemon_runtime_environment_plan, Platform,
        };

        // Retired worker-runtime / lazy-binding signals. Command-capable
        // Container/Daemon receivers use the `ALIEN_COMMANDS_*` contract instead;
        // none of these may leak into a compute env plan.
        const FORBIDDEN: &[&str] = &[
            "ALIEN_TRANSPORT",
            "ALIEN_WORKER_GRPC_ADDRESS",
            "ALIEN_BINDINGS_MODE",
            "ALIEN_BINDINGS_GRPC_ADDRESS",
            "ALIEN_BINDINGS_ADDRESS",
            "ALIEN_SECRETS",
            "ALIEN_RUNTIME_SECRETS",
        ];

        for platform in [
            Platform::Local,
            Platform::Kubernetes,
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Test,
        ] {
            for (label, entries) in [
                ("container", container_runtime_environment_plan(platform)),
                ("daemon", daemon_runtime_environment_plan(platform)),
            ] {
                for forbidden in FORBIDDEN {
                    assert!(
                        !entries.iter().any(|entry| entry.name == *forbidden),
                        "{label} plan for {platform:?} must not inject forbidden env var {forbidden}"
                    );
                }
            }
        }
    }

    #[test]
    fn public_url_host_extracts_host_from_common_public_urls() {
        assert_eq!(
            public_url_host("https://gateway.dep123.byoc.example.test"),
            Some("gateway.dep123.byoc.example.test".to_string())
        );
        assert_eq!(
            public_url_host("https://gateway.dep123.byoc.example.test:8443"),
            Some("gateway.dep123.byoc.example.test".to_string())
        );
        assert_eq!(
            public_url_host("http://[::1]:8080"),
            Some("[::1]".to_string())
        );
        assert_eq!(public_url_host(""), None);
    }

    #[test]
    fn public_endpoint_environment_projection_remains_url_host_and_wildcard_host() {
        let json = serde_json::to_string(&PublicEndpointEnv {
            url: "https://gateway.example.test".to_string(),
            host: "gateway.example.test".to_string(),
            wildcard_host: Some("*.gateway.example.test".to_string()),
        })
        .expect("environment endpoint should serialize");

        assert_eq!(
            json,
            r#"{"url":"https://gateway.example.test","host":"gateway.example.test","wildcardHost":"*.gateway.example.test"}"#
        );
    }
}
