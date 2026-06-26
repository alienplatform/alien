use crate::core::{state_utils::StackResourceStateExt, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{
    bindings::serialize_binding_as_env_var, container_runtime_environment_contract,
    kubernetes_base_platform_runtime_environment_plan,
    passthrough_transport_runtime_environment_plan, public_url_host,
    render_runtime_environment_entries, render_runtime_environment_plan,
    standard_runtime_environment_plan, validate_prepared_runtime_environment_map,
    worker_runtime_environment_contract, Container, Daemon, ResourceRef, ResourceStatus,
    RuntimeEnvironmentBindingEntry, RuntimeEnvironmentRenderer, RuntimeEnvironmentValue, Worker,
    ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME, ENV_ALIEN_CURRENT_WORKER_BINDING_NAME,
    ENV_ALIEN_PUBLIC_ENDPOINTS_JSON,
};
use alien_error::{AlienError, Context, IntoAlienError};
use serde::Serialize;
use std::collections::HashMap;

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
        let Some(endpoint_urls) = ctx
            .deployment_config
            .public_endpoints
            .as_ref()
            .and_then(|resources| resources.get(resource_id))
        else {
            return Ok(self);
        };

        let wildcard_endpoints = current_resource_wildcard_endpoints(ctx);
        let mut env_endpoints = HashMap::new();
        for (endpoint_name, public_url) in endpoint_urls {
            let Some(host) = public_url_host(public_url) else {
                continue;
            };
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

            // Try to get binding params from internal controller first
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
                            let value = match external {
                                alien_core::ExternalBinding::Storage(b) => serde_json::to_value(b),
                                alien_core::ExternalBinding::Queue(b) => serde_json::to_value(b),
                                alien_core::ExternalBinding::Kv(b) => serde_json::to_value(b),
                                alien_core::ExternalBinding::ArtifactRegistry(b) => {
                                    serde_json::to_value(b)
                                }
                                alien_core::ExternalBinding::Vault(b) => serde_json::to_value(b),
                                alien_core::ExternalBinding::ContainerAppsEnvironment(b) => {
                                    serde_json::to_value(b)
                                }
                            }
                            .into_alien_error()
                            .context(
                                ErrorData::ResourceStateSerializationFailed {
                                    resource_id: binding_name.to_string(),
                                    message: "Failed to serialize external binding parameters"
                                        .to_string(),
                                },
                            )?;
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
                    .into_alien_error()
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

    /// Add passthrough transport for non-Worker runtime workloads.
    pub fn add_passthrough_transport_env_vars(mut self) -> Self {
        for entry in passthrough_transport_runtime_environment_plan() {
            if let RuntimeEnvironmentValue::Literal(value) = entry.value {
                self.env_vars
                    .insert(entry.name.to_string(), value.to_string());
            }
        }
        self
    }

    /// Add the function's own binding to its environment variables for self-introspection.
    /// This adds both:
    /// 1. ALIEN_CURRENT_WORKER_BINDING_NAME - the function's ID for identifying itself
    /// 2. ALIEN_{FUNCTION_ID}_BINDING - the function's full binding parameters (if available)
    ///
    /// This allows the function to introspect itself via alien_context.get_current_worker().
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

        let (env_vars, bindings) = builder.build_with_bindings();

        // Verify bindings are tracked
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, "cache");
        assert_eq!(bindings[0].1, binding_json);
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
}
