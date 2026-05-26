use serde::{Deserialize, Serialize};

/// Azure ContainerAppsEnvironment ImportData — parent environment for
/// Container Apps in this stack. Realized once per stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AzureContainerAppsEnvironmentImportData {
    /// Subscription ID hosting the environment.
    pub subscription_id: String,
    /// Resource group hosting the environment.
    pub resource_group: String,
    /// Container Apps Environment name.
    pub environment_name: String,
    /// ARM resource ID of the Container Apps Environment.
    pub resource_id: String,
    /// Default domain (`<env>.<region>.azurecontainerapps.io`).
    pub default_domain: String,
    /// Domain verification ID required for Azure Container Apps custom domain
    /// DNS ownership records.
    pub custom_domain_verification_id: Option<String>,
}
