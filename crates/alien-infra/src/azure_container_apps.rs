use crate::core::{LongRunningOperation, OperationResult};
use alien_client_core::{ErrorData as CloudClientErrorData, Result as CloudClientResult};
use alien_core::AzureClientConfig;
use alien_error::{AlienError, Context, IntoAlienError};
use azure_core::credentials::{AccessToken, TokenCredential};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::{collections::HashMap, fmt::Debug, sync::Arc, time::Duration};

#[cfg(any(test, feature = "test-utils"))]
use mockall::automock;

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait ContainerAppsApi: Send + Sync + Debug {
    async fn create_or_update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>>;

    async fn update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>>;

    async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<ContainerApp>;

    async fn list_container_apps_by_resource_group(
        &self,
        resource_group_name: &str,
    ) -> CloudClientResult<ContainerAppCollection>;

    async fn delete_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<OperationResult<()>>;

    async fn create_or_update_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        managed_environment: &ManagedEnvironment,
    ) -> CloudClientResult<OperationResult<ManagedEnvironment>>;

    async fn get_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> CloudClientResult<ManagedEnvironment>;

    async fn delete_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> CloudClientResult<OperationResult<()>>;

    async fn create_or_update_managed_environment_certificate(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        certificate_name: &str,
        certificate: &ManagedEnvironmentCertificate,
    ) -> CloudClientResult<ManagedEnvironmentCertificateResponse>;

    async fn delete_managed_environment_certificate(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        certificate_name: &str,
    ) -> CloudClientResult<OperationResult<()>>;

    async fn create_or_update_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
        dapr_component: &DaprComponent,
    ) -> CloudClientResult<OperationResult<DaprComponent>>;

    async fn delete_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
    ) -> CloudClientResult<OperationResult<()>>;
}

#[cfg_attr(any(test, feature = "test-utils"), automock)]
#[async_trait::async_trait]
pub trait LongRunningOperationApi: Send + Sync + Debug {
    async fn check_status(
        &self,
        operation: &LongRunningOperation,
        operation_name: &str,
        resource_name: &str,
    ) -> CloudClientResult<Option<String>>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContainerApp {
    /// Fully qualified Azure resource ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Managed identity assigned to the app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<ManagedServiceIdentity>,
    /// Azure region.
    pub location: String,
    /// Optional resource name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Container App properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<ContainerAppProperties>,
    /// Resource tags.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    /// Resource type.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(
        rename = "extendedLocation",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub extended_location: Option<serde_json::Value>,
    #[serde(rename = "managedBy", default, skip_serializing_if = "Option::is_none")]
    pub managed_by: Option<String>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub system_data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContainerAppCollection {
    /// Link to the next result page.
    #[serde(rename = "nextLink", default, skip_serializing_if = "Option::is_none")]
    pub next_link: Option<String>,
    /// Container App resources.
    #[serde(default, deserialize_with = "null_to_default")]
    pub value: Vec<ContainerApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContainerAppProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Configuration>,
    #[serde(
        rename = "customDomainVerificationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_domain_verification_id: Option<String>,
    #[serde(
        rename = "environmentId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub environment_id: Option<String>,
    #[serde(
        rename = "eventStreamEndpoint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub event_stream_endpoint: Option<String>,
    #[serde(
        rename = "latestReadyRevisionName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_ready_revision_name: Option<String>,
    #[serde(
        rename = "latestRevisionFqdn",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_revision_fqdn: Option<String>,
    #[serde(
        rename = "latestRevisionName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub latest_revision_name: Option<String>,
    #[serde(
        rename = "managedEnvironmentId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub managed_environment_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub outbound_ip_addresses: Vec<String>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub provisioning_state: Option<ContainerAppPropertiesProvisioningState>,
    #[serde(
        rename = "runningStatus",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub running_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,
    #[serde(
        rename = "workloadProfileName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub workload_profile_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContainerAppPropertiesProvisioningState {
    InProgress,
    Succeeded,
    Failed,
    Canceled,
    Deleting,
}

impl std::fmt::Display for ContainerAppPropertiesProvisioningState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InProgress => "InProgress",
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Canceled => "Canceled",
            Self::Deleting => "Deleting",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    /// Active revision mode.
    #[serde(rename = "activeRevisionsMode", default)]
    pub active_revisions_mode: ConfigurationActiveRevisionsMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dapr: Option<Dapr>,
    #[serde(
        rename = "identitySettings",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub identity_settings: Vec<IdentitySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress: Option<Ingress>,
    #[serde(
        rename = "maxInactiveRevisions",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_inactive_revisions: Option<i32>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub registries: Vec<RegistryCredentials>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub secrets: Vec<Secret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<serde_json::Value>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
            dapr: None,
            identity_settings: vec![],
            ingress: None,
            max_inactive_revisions: None,
            registries: vec![],
            runtime: None,
            secrets: vec![],
            service: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConfigurationActiveRevisionsMode {
    Multiple,
    Single,
}

impl Default for ConfigurationActiveRevisionsMode {
    fn default() -> Self {
        Self::Single
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dapr {
    #[serde(rename = "appId", default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(rename = "appPort", default, skip_serializing_if = "Option::is_none")]
    pub app_port: Option<i32>,
    #[serde(rename = "appProtocol", default)]
    pub app_protocol: DaprAppProtocol,
    #[serde(
        rename = "enableApiLogging",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enable_api_logging: Option<bool>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(
        rename = "httpMaxRequestSize",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub http_max_request_size: Option<i32>,
    #[serde(
        rename = "httpReadBufferSize",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub http_read_buffer_size: Option<i32>,
    #[serde(rename = "logLevel", default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
}

impl Default for Dapr {
    fn default() -> Self {
        Self {
            app_id: None,
            app_port: None,
            app_protocol: DaprAppProtocol::Http,
            enable_api_logging: None,
            enabled: false,
            http_max_request_size: None,
            http_read_buffer_size: None,
            log_level: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DaprAppProtocol {
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "grpc")]
    Grpc,
}

impl Default for DaprAppProtocol {
    fn default() -> Self {
        Self::Http
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySettings {
    /// Identity resource ID or `system`.
    pub identity: String,
    /// Lifecycle where the identity is available.
    #[serde(default)]
    pub lifecycle: IdentitySettingsLifecycle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IdentitySettingsLifecycle {
    Init,
    Main,
    None,
    All,
}

impl Default for IdentitySettingsLifecycle {
    fn default() -> Self {
        Self::All
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Ingress {
    #[serde(
        rename = "additionalPortMappings",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub additional_port_mappings: Vec<serde_json::Value>,
    #[serde(rename = "allowInsecure", default)]
    pub allow_insecure: bool,
    #[serde(
        rename = "clientCertificateMode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub client_certificate_mode: Option<serde_json::Value>,
    #[serde(
        rename = "corsPolicy",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cors_policy: Option<serde_json::Value>,
    #[serde(
        rename = "customDomains",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub custom_domains: Vec<CustomDomain>,
    #[serde(
        rename = "exposedPort",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub exposed_port: Option<i32>,
    #[serde(default)]
    pub external: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fqdn: Option<String>,
    #[serde(
        rename = "ipSecurityRestrictions",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub ip_security_restrictions: Vec<serde_json::Value>,
    #[serde(
        rename = "stickySessions",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sticky_sessions: Option<serde_json::Value>,
    #[serde(
        rename = "targetPort",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub target_port: Option<i32>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub traffic: Vec<TrafficWeight>,
    #[serde(default)]
    pub transport: IngressTransport,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IngressTransport {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "http2")]
    Http2,
    #[serde(rename = "tcp")]
    Tcp,
}

impl Default for IngressTransport {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomDomain {
    #[serde(
        rename = "bindingType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub binding_type: Option<CustomDomainBindingType>,
    #[serde(
        rename = "certificateId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub certificate_id: Option<String>,
    /// Hostname.
    pub name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CustomDomainBindingType {
    Disabled,
    SniEnabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrafficWeight {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "latestRevision", default)]
    pub latest_revision: bool,
    #[serde(
        rename = "revisionName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub revision_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub containers: Vec<Container>,
    #[serde(
        rename = "initContainers",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub init_containers: Vec<serde_json::Value>,
    #[serde(
        rename = "revisionSuffix",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub revision_suffix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Scale>,
    #[serde(
        rename = "serviceBinds",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub service_binds: Vec<serde_json::Value>,
    #[serde(
        rename = "terminationGracePeriodSeconds",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub termination_grace_period_seconds: Option<i64>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub volumes: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub args: Vec<String>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub command: Vec<String>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub env: Vec<EnvironmentVar>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub probes: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ContainerResources>,
    #[serde(
        rename = "volumeMounts",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub volume_mounts: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "secretRef", default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContainerResources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(
        rename = "ephemeralStorage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ephemeral_storage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Scale {
    #[serde(
        rename = "cooldownPeriod",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cooldown_period: Option<i32>,
    #[serde(rename = "maxReplicas", default = "default_max_replicas")]
    pub max_replicas: i32,
    #[serde(
        rename = "minReplicas",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub min_replicas: Option<i32>,
    #[serde(
        rename = "pollingInterval",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub polling_interval: Option<i32>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub rules: Vec<serde_json::Value>,
}

fn default_max_replicas() -> i32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(
        rename = "keyVaultUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub key_vault_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RegistryCredentials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(
        rename = "passwordSecretRef",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub password_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedServiceIdentity {
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub principal_id: Option<uuid::Uuid>,
    #[serde(rename = "tenantId", default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<uuid::Uuid>,
    #[serde(rename = "type")]
    pub type_: ManagedServiceIdentityType,
    #[serde(
        rename = "userAssignedIdentities",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub user_assigned_identities: Option<UserAssignedIdentities>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManagedServiceIdentityType {
    None,
    SystemAssigned,
    UserAssigned,
    #[serde(rename = "SystemAssigned,UserAssigned")]
    SystemAssignedUserAssigned,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserAssignedIdentities(pub HashMap<String, UserAssignedIdentity>);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserAssignedIdentity {
    #[serde(rename = "clientId", default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<uuid::Uuid>,
    #[serde(
        rename = "principalId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub principal_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEnvironment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<ManagedEnvironmentProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub system_data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEnvironmentProperties {
    #[serde(
        rename = "appLogsConfiguration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub app_logs_configuration: Option<serde_json::Value>,
    #[serde(
        rename = "customDomainConfiguration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_domain_configuration: Option<CustomDomainConfiguration>,
    #[serde(
        rename = "daprAIConnectionString",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dapr_ai_connection_string: Option<String>,
    #[serde(
        rename = "daprAIInstrumentationKey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dapr_ai_instrumentation_key: Option<String>,
    #[serde(
        rename = "daprConfiguration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub dapr_configuration: Option<serde_json::Value>,
    #[serde(
        rename = "defaultDomain",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_domain: Option<String>,
    #[serde(
        rename = "deploymentErrors",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub deployment_errors: Option<String>,
    #[serde(
        rename = "eventStreamEndpoint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub event_stream_endpoint: Option<String>,
    #[serde(
        rename = "infrastructureResourceGroup",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub infrastructure_resource_group: Option<String>,
    #[serde(
        rename = "kedaConfiguration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub keda_configuration: Option<serde_json::Value>,
    #[serde(
        rename = "peerAuthentication",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub peer_authentication: Option<serde_json::Value>,
    #[serde(
        rename = "peerTrafficConfiguration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub peer_traffic_configuration: Option<serde_json::Value>,
    #[serde(
        rename = "provisioningState",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub provisioning_state: Option<ManagedEnvironmentPropertiesProvisioningState>,
    #[serde(rename = "staticIp", default, skip_serializing_if = "Option::is_none")]
    pub static_ip: Option<String>,
    #[serde(
        rename = "vnetConfiguration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub vnet_configuration: Option<VnetConfiguration>,
    #[serde(
        rename = "workloadProfiles",
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub workload_profiles: Vec<WorkloadProfile>,
    #[serde(
        rename = "zoneRedundant",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub zone_redundant: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManagedEnvironmentPropertiesProvisioningState {
    Succeeded,
    Failed,
    Canceled,
    Waiting,
    InitializationInProgress,
    InfrastructureSetupInProgress,
    InfrastructureSetupComplete,
    ScheduledForDelete,
    UpgradeRequested,
    UpgradeFailed,
}

impl std::fmt::Display for ManagedEnvironmentPropertiesProvisioningState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Succeeded => "Succeeded",
            Self::Failed => "Failed",
            Self::Canceled => "Canceled",
            Self::Waiting => "Waiting",
            Self::InitializationInProgress => "InitializationInProgress",
            Self::InfrastructureSetupInProgress => "InfrastructureSetupInProgress",
            Self::InfrastructureSetupComplete => "InfrastructureSetupComplete",
            Self::ScheduledForDelete => "ScheduledForDelete",
            Self::UpgradeRequested => "UpgradeRequested",
            Self::UpgradeFailed => "UpgradeFailed",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VnetConfiguration {
    #[serde(
        rename = "dockerBridgeCidr",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub docker_bridge_cidr: Option<String>,
    #[serde(
        rename = "infrastructureSubnetId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub infrastructure_subnet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal: Option<bool>,
    #[serde(
        rename = "platformReservedCidr",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub platform_reserved_cidr: Option<String>,
    #[serde(
        rename = "platformReservedDnsIP",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub platform_reserved_dns_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CustomDomainConfiguration {
    #[serde(
        rename = "customDomainVerificationId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_domain_verification_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadProfile {
    /// Workload profile name.
    pub name: String,
    #[serde(rename = "workloadProfileType")]
    pub workload_profile_type: String,
    #[serde(
        rename = "minimumCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub minimum_count: Option<i32>,
    #[serde(
        rename = "maximumCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub maximum_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEnvironmentCertificate {
    /// Azure region.
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<ManagedEnvironmentCertificateProperties>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEnvironmentCertificateProperties {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_key_vault_properties: Option<ManagedEnvironmentCertificateKeyVaultProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEnvironmentCertificateKeyVaultProperties {
    /// Identity used to access Key Vault.
    pub identity: String,
    /// Key Vault certificate secret URL.
    pub key_vault_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedEnvironmentCertificateResponse {
    /// Certificate resource ID.
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DaprComponent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<DaprComponentProperties>,
    #[serde(
        rename = "systemData",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub system_data: Option<serde_json::Value>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DaprComponentProperties {
    #[serde(
        rename = "componentType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub component_type: Option<String>,
    #[serde(rename = "ignoreErrors", default)]
    pub ignore_errors: bool,
    #[serde(
        rename = "initTimeout",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub init_timeout: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub metadata: Vec<DaprMetadata>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub scopes: Vec<String>,
    #[serde(
        rename = "secretStoreComponent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub secret_store_component: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_to_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub secrets: Vec<DaprSecretDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DaprMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "secretRef", default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DaprSecretDefinition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(
        rename = "keyVaultUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub key_vault_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

pub struct OfficialAzureContainerAppsClient {
    config: AzureClientConfig,
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl Debug for OfficialAzureContainerAppsClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureContainerAppsClient")
            .field("subscription_id", &self.config.subscription_id)
            .finish_non_exhaustive()
    }
}

impl OfficialAzureContainerAppsClient {
    pub fn new(config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            config,
            credential,
            http_client: reqwest::Client::new(),
        }
    }

    fn base_url(&self) -> String {
        crate::core::azure_management_endpoint(&self.config)
            .trim_end_matches('/')
            .to_string()
    }

    fn container_app_url(&self, resource_group_name: &str, container_app_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps/{}?api-version=2025-01-01",
            self.base_url(), self.config.subscription_id, resource_group_name, container_app_name
        )
    }

    fn container_apps_by_resource_group_url(&self, resource_group_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/containerApps?api-version=2025-01-01",
            self.base_url(), self.config.subscription_id, resource_group_name
        )
    }

    fn managed_environment_url(&self, resource_group_name: &str, environment_name: &str) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}?api-version=2025-01-01",
            self.base_url(), self.config.subscription_id, resource_group_name, environment_name
        )
    }

    fn certificate_url(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        certificate_name: &str,
    ) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}/certificates/{}?api-version=2025-01-01",
            self.base_url(),
            self.config.subscription_id,
            resource_group_name,
            environment_name,
            certificate_name
        )
    }

    fn dapr_component_url(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
    ) -> String {
        format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}/daprComponents/{}?api-version=2025-01-01",
            self.base_url(),
            self.config.subscription_id,
            resource_group_name,
            environment_name,
            component_name
        )
    }

    async fn request(
        &self,
        method: reqwest::Method,
        url: String,
        body: Option<String>,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<(reqwest::StatusCode, reqwest::header::HeaderMap, String)> {
        azure_arm_request(
            &self.http_client,
            self.credential.as_ref(),
            method,
            url,
            body,
            resource_type,
            resource_name,
        )
        .await
    }

    fn parse_response<T: DeserializeOwned>(
        &self,
        resource_type: &str,
        resource_name: &str,
        body: &str,
    ) -> CloudClientResult<T> {
        parse_response(resource_type, resource_name, body)
    }
}

#[async_trait::async_trait]
impl ContainerAppsApi for OfficialAzureContainerAppsClient {
    async fn create_or_update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>> {
        self.put_lro(
            self.container_app_url(resource_group_name, container_app_name),
            container_app,
            "Azure Container App",
            container_app_name,
        )
        .await
    }

    async fn get_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<ContainerApp> {
        let (_, _, body) = self
            .request(
                reqwest::Method::GET,
                self.container_app_url(resource_group_name, container_app_name),
                None,
                "Azure Container App",
                container_app_name,
            )
            .await?;
        self.parse_response("Azure Container App", container_app_name, &body)
    }

    async fn update_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
        container_app: &ContainerApp,
    ) -> CloudClientResult<OperationResult<ContainerApp>> {
        self.patch_lro(
            self.container_app_url(resource_group_name, container_app_name),
            container_app,
            "Azure Container App",
            container_app_name,
        )
        .await
    }

    async fn list_container_apps_by_resource_group(
        &self,
        resource_group_name: &str,
    ) -> CloudClientResult<ContainerAppCollection> {
        let (_, _, body) = self
            .request(
                reqwest::Method::GET,
                self.container_apps_by_resource_group_url(resource_group_name),
                None,
                "Azure Container Apps",
                resource_group_name,
            )
            .await?;
        self.parse_response("Azure Container Apps", resource_group_name, &body)
    }

    async fn delete_container_app(
        &self,
        resource_group_name: &str,
        container_app_name: &str,
    ) -> CloudClientResult<OperationResult<()>> {
        self.delete_lro(
            self.container_app_url(resource_group_name, container_app_name),
            "Azure Container App",
            container_app_name,
        )
        .await
    }

    async fn create_or_update_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        managed_environment: &ManagedEnvironment,
    ) -> CloudClientResult<OperationResult<ManagedEnvironment>> {
        self.put_lro(
            self.managed_environment_url(resource_group_name, environment_name),
            managed_environment,
            "Azure Container Apps Managed Environment",
            environment_name,
        )
        .await
    }

    async fn get_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> CloudClientResult<ManagedEnvironment> {
        let (_, _, body) = self
            .request(
                reqwest::Method::GET,
                self.managed_environment_url(resource_group_name, environment_name),
                None,
                "Azure Container Apps Managed Environment",
                environment_name,
            )
            .await?;
        self.parse_response(
            "Azure Container Apps Managed Environment",
            environment_name,
            &body,
        )
    }

    async fn delete_managed_environment(
        &self,
        resource_group_name: &str,
        environment_name: &str,
    ) -> CloudClientResult<OperationResult<()>> {
        self.delete_lro(
            self.managed_environment_url(resource_group_name, environment_name),
            "Azure Container Apps Managed Environment",
            environment_name,
        )
        .await
    }

    async fn create_or_update_managed_environment_certificate(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        certificate_name: &str,
        certificate: &ManagedEnvironmentCertificate,
    ) -> CloudClientResult<ManagedEnvironmentCertificateResponse> {
        let body = serialize_request(
            "Azure Container Apps Managed Environment Certificate",
            certificate_name,
            certificate,
        )?;
        let (_, _, body) = self
            .request(
                reqwest::Method::PUT,
                self.certificate_url(resource_group_name, environment_name, certificate_name),
                Some(body),
                "Azure Container Apps Managed Environment Certificate",
                certificate_name,
            )
            .await?;
        if body.trim().is_empty() {
            return Ok(ManagedEnvironmentCertificateResponse::default());
        }
        self.parse_response(
            "Azure Container Apps Managed Environment Certificate",
            certificate_name,
            &body,
        )
    }

    async fn delete_managed_environment_certificate(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        certificate_name: &str,
    ) -> CloudClientResult<OperationResult<()>> {
        self.delete_lro(
            self.certificate_url(resource_group_name, environment_name, certificate_name),
            "Azure Container Apps Managed Environment Certificate",
            certificate_name,
        )
        .await
    }

    async fn create_or_update_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
        dapr_component: &DaprComponent,
    ) -> CloudClientResult<OperationResult<DaprComponent>> {
        self.put_lro(
            self.dapr_component_url(resource_group_name, environment_name, component_name),
            dapr_component,
            "Azure Container Apps Dapr Component",
            component_name,
        )
        .await
    }

    async fn delete_dapr_component(
        &self,
        resource_group_name: &str,
        environment_name: &str,
        component_name: &str,
    ) -> CloudClientResult<OperationResult<()>> {
        self.delete_lro(
            self.dapr_component_url(resource_group_name, environment_name, component_name),
            "Azure Container Apps Dapr Component",
            component_name,
        )
        .await
    }
}

impl OfficialAzureContainerAppsClient {
    async fn put_lro<T>(
        &self,
        url: String,
        resource: &T,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<OperationResult<T>>
    where
        T: Serialize + DeserializeOwned,
    {
        let body = serialize_request(resource_type, resource_name, resource)?;
        let (status, headers, body) = self
            .request(
                reqwest::Method::PUT,
                url,
                Some(body),
                resource_type,
                resource_name,
            )
            .await?;
        operation_result_from_response(status, &headers, &body, resource_type, resource_name)
    }

    async fn patch_lro<T>(
        &self,
        url: String,
        resource: &T,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<OperationResult<T>>
    where
        T: Serialize + DeserializeOwned,
    {
        let body = serialize_request(resource_type, resource_name, resource)?;
        let (status, headers, body) = self
            .request(
                reqwest::Method::PATCH,
                url,
                Some(body),
                resource_type,
                resource_name,
            )
            .await?;
        operation_result_from_response(status, &headers, &body, resource_type, resource_name)
    }

    async fn delete_lro(
        &self,
        url: String,
        resource_type: &str,
        resource_name: &str,
    ) -> CloudClientResult<OperationResult<()>> {
        let (status, headers, _) = self
            .request(
                reqwest::Method::DELETE,
                url,
                None,
                resource_type,
                resource_name,
            )
            .await?;
        if status == reqwest::StatusCode::ACCEPTED {
            return Ok(OperationResult::LongRunning(
                long_running_operation_from_headers(&headers, resource_type, resource_name)?,
            ));
        }
        Ok(OperationResult::Completed(()))
    }
}

pub struct OfficialAzureLongRunningOperationClient {
    credential: Arc<dyn TokenCredential>,
    http_client: reqwest::Client,
}

impl Debug for OfficialAzureLongRunningOperationClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OfficialAzureLongRunningOperationClient")
            .finish_non_exhaustive()
    }
}

impl OfficialAzureLongRunningOperationClient {
    pub fn new(_config: AzureClientConfig, credential: Arc<dyn TokenCredential>) -> Self {
        Self {
            credential,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl LongRunningOperationApi for OfficialAzureLongRunningOperationClient {
    async fn check_status(
        &self,
        operation: &LongRunningOperation,
        operation_name: &str,
        resource_name: &str,
    ) -> CloudClientResult<Option<String>> {
        let (status, _, body) = azure_arm_request(
            &self.http_client,
            self.credential.as_ref(),
            reqwest::Method::GET,
            operation.url.clone(),
            None,
            operation_name,
            resource_name,
        )
        .await?;

        match status {
            reqwest::StatusCode::OK => {
                azure_operation_body_status(operation, operation_name, resource_name, body)
            }
            reqwest::StatusCode::NO_CONTENT => Ok(Some(String::new())),
            reqwest::StatusCode::ACCEPTED => Ok(None),
            _ => Err(AlienError::new(CloudClientErrorData::HttpResponseError {
                message: format!(
                    "Azure {operation_name} for '{resource_name}' returned HTTP {}",
                    status.as_u16()
                ),
                url: operation.url.clone(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(body),
            })),
        }
    }
}

async fn azure_arm_request(
    http_client: &reqwest::Client,
    credential: &dyn TokenCredential,
    method: reqwest::Method,
    url: String,
    body: Option<String>,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<(reqwest::StatusCode, reqwest::header::HeaderMap, String)> {
    let token = azure_bearer_token(credential).await?;
    let mut request = http_client
        .request(method, &url)
        .bearer_auth(token.token.secret());

    if let Some(body) = body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
    }

    let response = request.send().await.into_alien_error().context(
        CloudClientErrorData::HttpRequestFailed {
            message: format!("Azure ARM request failed for {resource_type} '{resource_name}'"),
        },
    )?;
    let status = response.status();
    let headers = response.headers().clone();
    let text = response.text().await.into_alien_error().context(
        CloudClientErrorData::HttpRequestFailed {
            message: format!(
                "Failed to read Azure ARM response for {resource_type} '{resource_name}'"
            ),
        },
    )?;

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(AlienError::new(
            CloudClientErrorData::RemoteResourceNotFound {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
            },
        ));
    }

    if status == reqwest::StatusCode::CONFLICT {
        return Err(AlienError::new(
            CloudClientErrorData::RemoteResourceConflict {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
                message: text,
            },
        ));
    }

    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        }));
    }

    if !status.is_success() {
        return Err(AlienError::new(CloudClientErrorData::HttpResponseError {
            message: format!(
                "Azure ARM request for {resource_type} '{resource_name}' returned HTTP {}",
                status.as_u16()
            ),
            url,
            http_status: status.as_u16(),
            http_request_text: None,
            http_response_text: Some(text),
        }));
    }

    Ok((status, headers, text))
}

async fn azure_bearer_token(credential: &dyn TokenCredential) -> CloudClientResult<AccessToken> {
    credential
        .get_token(&["https://management.azure.com/.default"], None)
        .await
        .into_alien_error()
        .context(CloudClientErrorData::AuthenticationError {
            message: "Failed to get Azure management access token".to_string(),
        })
}

fn operation_result_from_response<T>(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<OperationResult<T>>
where
    T: DeserializeOwned,
{
    if status == reqwest::StatusCode::ACCEPTED {
        return Ok(OperationResult::LongRunning(
            long_running_operation_from_headers(headers, resource_type, resource_name)?,
        ));
    }

    parse_response(resource_type, resource_name, body).map(OperationResult::Completed)
}

fn long_running_operation_from_headers(
    headers: &reqwest::header::HeaderMap,
    resource_type: &str,
    resource_name: &str,
) -> CloudClientResult<LongRunningOperation> {
    let async_operation_url = header_to_string(headers, "azure-asyncoperation")?;
    let location_url = header_to_string(headers, "location")?;
    let url = async_operation_url
        .clone()
        .or_else(|| location_url.clone())
        .ok_or_else(|| {
            AlienError::new(CloudClientErrorData::GenericError {
                message: format!(
                    "{resource_type} '{resource_name}' returned 202 without Azure-AsyncOperation or Location header"
                ),
            })
        })?;
    let retry_after = header_to_string(headers, "retry-after")?
        .map(|value| {
            value
                .parse::<u64>()
                .map(Duration::from_secs)
                .map_err(|error| {
                    AlienError::new(CloudClientErrorData::SerializationError {
                        message: format!(
                            "Failed to parse Azure Retry-After header '{value}': {error}"
                        ),
                    })
                })
        })
        .transpose()?;

    Ok(LongRunningOperation {
        url,
        retry_after,
        location_url: async_operation_url.and(location_url),
    })
}

fn header_to_string(
    headers: &reqwest::header::HeaderMap,
    name: &'static str,
) -> CloudClientResult<Option<String>> {
    headers
        .get(name)
        .map(|value| {
            value.to_str().map(ToString::to_string).map_err(|error| {
                AlienError::new(CloudClientErrorData::SerializationError {
                    message: format!("Failed to parse Azure {name} header: {error}"),
                })
            })
        })
        .transpose()
}

fn serialize_request<T: Serialize>(
    resource_type: &str,
    resource_name: &str,
    request: &T,
) -> CloudClientResult<String> {
    serde_json::to_string(request).into_alien_error().context(
        CloudClientErrorData::SerializationError {
            message: format!("Failed to serialize {resource_type} '{resource_name}' request"),
        },
    )
}

fn parse_response<T: DeserializeOwned>(
    resource_type: &str,
    resource_name: &str,
    body: &str,
) -> CloudClientResult<T> {
    serde_json::from_str(body).into_alien_error().context(
        CloudClientErrorData::SerializationError {
            message: format!("Failed to parse {resource_type} '{resource_name}' response"),
        },
    )
}

fn azure_operation_body_status(
    operation: &LongRunningOperation,
    operation_name: &str,
    resource_name: &str,
    body: String,
) -> CloudClientResult<Option<String>> {
    if body.trim().is_empty() {
        return Ok(Some(body));
    }

    let value: serde_json::Value = serde_json::from_str(&body).into_alien_error().context(
        CloudClientErrorData::HttpResponseError {
            message: format!("Azure {operation_name}: failed to parse operation JSON"),
            url: operation.url.clone(),
            http_status: 200,
            http_request_text: None,
            http_response_text: Some(body.clone()),
        },
    )?;

    if let Some(status) = value.get("status").and_then(serde_json::Value::as_str) {
        match status.to_ascii_lowercase().as_str() {
            "succeeded" => return Ok(Some(body)),
            "failed" | "canceled" => {
                return Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: format!(
                        "Azure {operation_name} for '{resource_name}' {}: {}",
                        status.to_ascii_lowercase(),
                        value
                            .get("error")
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "no error details".to_string())
                    ),
                }));
            }
            _ => return Ok(None),
        }
    }

    if let Some(state) = value
        .get("properties")
        .and_then(|properties| properties.get("provisioningState"))
        .and_then(serde_json::Value::as_str)
    {
        match state.to_ascii_lowercase().as_str() {
            "succeeded" => return Ok(Some(body)),
            "failed" | "canceled" => {
                return Err(AlienError::new(CloudClientErrorData::GenericError {
                    message: format!(
                        "Azure {operation_name} for '{resource_name}' failed with provisioningState: {state}"
                    ),
                }));
            }
            _ => return Ok(None),
        }
    }

    Ok(Some(body))
}

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
