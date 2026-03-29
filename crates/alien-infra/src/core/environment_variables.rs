use crate::core::{state_utils::StackResourceStateExt, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use alien_core::{bindings::serialize_binding_as_env_var, Platform, ResourceRef, ResourceStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use std::collections::HashMap;

/// Common environment variable preparation for function controllers.
/// This handles the shared logic of processing linked resources and setting up
/// platform-agnostic environment variables.
pub struct EnvironmentVariableBuilder {
    env_vars: HashMap<String, String>,
    /// Track bindings for platform-specific processing (e.g., Kubernetes SecretRefs)
    /// Stored as (binding_name, binding_json) to avoid serialization round-trips
    bindings: Vec<(String, serde_json::Value)>,
}

impl EnvironmentVariableBuilder {
    /// Create a new builder starting with the initial environment variables.
    pub fn new(initial_env: &HashMap<String, String>) -> Self {
        Self {
            env_vars: initial_env.clone(),
            bindings: Vec::new(),
        }
    }

    /// Add standard Alien environment variables that should be available to all resources.
    /// This includes ALIEN_DEPLOYMENT_TYPE which indicates the current platform, plus platform-specific
    /// identifiers like AWS_ACCOUNT_ID, AWS_REGION, GCP_PROJECT_ID, AZURE_TENANT_ID, etc.
    pub fn add_standard_alien_env_vars(mut self, ctx: &ResourceControllerContext<'_>) -> Self {
        // Add the current platform as ALIEN_DEPLOYMENT_TYPE
        self.env_vars.insert(
            "ALIEN_DEPLOYMENT_TYPE".to_string(),
            ctx.platform.to_string(),
        );

        // Add platform-specific environment variables
        match ctx.platform {
            Platform::Aws => {
                if let Ok(aws_config) = ctx.get_aws_config() {
                    self.env_vars
                        .insert("AWS_ACCOUNT_ID".to_string(), aws_config.account_id.clone());
                }
            }
            Platform::Gcp => {
                if let Ok(gcp_config) = ctx.get_gcp_config() {
                    // Use GOOGLE_CLOUD_PROJECT as it's the standard GCP environment variable
                    self.env_vars.insert(
                        "GOOGLE_CLOUD_PROJECT".to_string(),
                        gcp_config.project_id.clone(),
                    );
                    // Also provide GCP_PROJECT_ID for convenience
                    self.env_vars
                        .insert("GCP_PROJECT_ID".to_string(), gcp_config.project_id.clone());
                    self.env_vars
                        .insert("GCP_REGION".to_string(), gcp_config.region.clone());
                }
            }
            Platform::Azure => {
                if let Ok(azure_config) = ctx.get_azure_config() {
                    self.env_vars.insert(
                        "AZURE_SUBSCRIPTION_ID".to_string(),
                        azure_config.subscription_id.clone(),
                    );
                    self.env_vars.insert(
                        "AZURE_TENANT_ID".to_string(),
                        azure_config.tenant_id.clone(),
                    );
                    // Region is optional in Azure config
                    if let Some(region) = &azure_config.region {
                        self.env_vars
                            .insert("AZURE_REGION".to_string(), region.clone());
                    }
                }
            }
            _ => {
                // Kubernetes, Local, and Test don't have platform-specific identifiers to add
            }
        }

        self
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

    /// Set `ALIEN_TRANSPORT` for a **Function** resource based on platform.
    ///
    /// Functions are hosted by the platform's serverless runtime (Lambda, Cloud Run,
    /// Container Apps). The transport tells alien-runtime which platform API to poll for
    /// invocations and how to parse incoming events.
    ///
    /// - AWS:        `lambda` + `ALIEN_LAMBDA_MODE=buffered`
    /// - GCP:        `cloud-run`
    /// - Azure:      `container-app`
    /// - Kubernetes: `passthrough`
    /// - Local/Test: `passthrough`
    ///
    /// Do NOT call this for Container resources — use `add_container_transport_env_vars`
    /// instead, which hardcodes `passthrough` regardless of platform.
    pub fn add_function_transport_env_vars(mut self, platform: Platform) -> Self {
        match platform {
            Platform::Aws => {
                self.env_vars
                    .insert("ALIEN_TRANSPORT".to_string(), "lambda".to_string());
                // Use buffered mode: API Gateway HTTP APIs don't support Lambda
                // response streaming. Streaming only works with Function URLs.
                self.env_vars
                    .insert("ALIEN_LAMBDA_MODE".to_string(), "buffered".to_string());
            }
            Platform::Gcp => {
                self.env_vars
                    .insert("ALIEN_TRANSPORT".to_string(), "cloud-run".to_string());
            }
            Platform::Azure => {
                self.env_vars
                    .insert("ALIEN_TRANSPORT".to_string(), "container-app".to_string());
            }
            Platform::Kubernetes | Platform::Local | Platform::Test => {
                self.env_vars
                    .insert("ALIEN_TRANSPORT".to_string(), "passthrough".to_string());
            }
        };
        self
    }

    /// Set `ALIEN_TRANSPORT=passthrough` for a **Container** resource.
    ///
    /// Containers on Horizon own their HTTP port directly -- Horizon manages external
    /// networking and load balancing. The alien-runtime provides bindings (gRPC) only;
    /// it does not intercept or transform HTTP traffic. Platform-specific function
    /// transports (Lambda polling, CloudRun CloudEvents, Container App Dapr) are not
    /// applicable here.
    pub fn add_container_transport_env_vars(mut self) -> Self {
        self.env_vars
            .insert("ALIEN_TRANSPORT".to_string(), "passthrough".to_string());
        self
    }

    /// Add the function's own binding to its environment variables for self-introspection.
    /// This adds both:
    /// 1. ALIEN_CURRENT_FUNCTION_BINDING_NAME - the function's ID for identifying itself
    /// 2. ALIEN_{FUNCTION_ID}_BINDING - the function's full binding parameters (if available)
    ///
    /// This allows the function to introspect itself via alien_context.get_current_function().
    ///
    /// The binding params should be provided when available. During initial creation, binding
    /// params may be incomplete (e.g., URL not yet known). During updates or after creation
    /// completes, full binding params should be available.
    pub fn add_self_function_binding(
        mut self,
        function_id: &str,
        binding_params: Option<&serde_json::Value>,
    ) -> Result<Self> {
        // Always add the current function's binding name (its ID)
        self.env_vars.insert(
            "ALIEN_CURRENT_FUNCTION_BINDING_NAME".to_string(),
            function_id.to_string(),
        );

        // Add the full binding parameters if available
        if let Some(params) = binding_params {
            // Use the centralized function to serialize binding parameters
            let binding_env_vars = serialize_binding_as_env_var(function_id, params).context(
                ErrorData::ResourceConfigInvalid {
                    message: "Failed to serialize self function binding parameters".to_string(),
                    resource_id: Some(function_id.to_string()),
                },
            )?;

            // Add all the binding environment variables
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
}
