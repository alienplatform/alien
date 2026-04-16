//! Azure Container Apps Environment binding definition for pre-existing environments.

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Binding configuration for a pre-existing Azure Container Apps Environment.
///
/// Used when deploying to an existing environment instead of having Alien provision one.
/// This is useful for shared environments (e.g., test infrastructure) or enterprise
/// setups where environments are managed by a separate team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ContainerAppsEnvironmentBinding {
    /// The name of the Container Apps Environment in Azure.
    pub environment_name: BindingValue<String>,
    /// The full Azure resource ID of the environment.
    pub resource_id: BindingValue<String>,
    /// The Azure resource group that contains the environment.
    /// Stored explicitly so consumers don't need to parse the ARM resource ID path.
    pub resource_group_name: BindingValue<String>,
    /// The default domain for applications in this environment.
    pub default_domain: BindingValue<String>,
    /// The static IP address of the environment (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_ip: Option<BindingValue<String>>,
}

impl ContainerAppsEnvironmentBinding {
    /// Creates a new Container Apps Environment binding with required fields.
    pub fn new(
        environment_name: impl Into<BindingValue<String>>,
        resource_id: impl Into<BindingValue<String>>,
        resource_group_name: impl Into<BindingValue<String>>,
        default_domain: impl Into<BindingValue<String>>,
    ) -> Self {
        Self {
            environment_name: environment_name.into(),
            resource_id: resource_id.into(),
            resource_group_name: resource_group_name.into(),
            default_domain: default_domain.into(),
            static_ip: None,
        }
    }

    /// Sets the static IP address.
    pub fn with_static_ip(mut self, static_ip: impl Into<BindingValue<String>>) -> Self {
        self.static_ip = Some(static_ip.into());
        self
    }
}
