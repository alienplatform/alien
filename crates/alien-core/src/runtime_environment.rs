use crate::{bindings::binding_env_var_name, ErrorData, Platform, ResourceRef, Result};
use alien_error::AlienError;
use std::collections::HashMap;

pub const ENV_ALIEN_CURRENT_WORKER_BINDING_NAME: &str = "ALIEN_CURRENT_WORKER_BINDING_NAME";
pub const ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME: &str = "ALIEN_CURRENT_CONTAINER_BINDING_NAME";
pub const ENV_OPERATOR_BASE_PLATFORM: &str = "OPERATOR_BASE_PLATFORM";
pub const ENV_ALIEN_DEPLOYMENT_TYPE: &str = "ALIEN_DEPLOYMENT_TYPE";
pub const ENV_ALIEN_LAMBDA_MODE: &str = "ALIEN_LAMBDA_MODE";
pub const ENV_ALIEN_RUNTIME_SEND_OTLP: &str = "ALIEN_RUNTIME_SEND_OTLP";
pub const ENV_ALIEN_RUNTIME_SECRETS: &str = "ALIEN_RUNTIME_SECRETS";
pub const ENV_ALIEN_SECRETS: &str = "ALIEN_SECRETS";
/// Opaque deployment-managed revision that rolls workloads when referenced
/// secret values change without exposing secret-derived data in the spec.
pub const ENV_ALIEN_SECRET_ENV_REVISION: &str = "ALIEN_SECRET_ENV_REVISION";
pub const ENV_ALIEN_TRANSPORT: &str = "ALIEN_TRANSPORT";
pub const ENV_ALIEN_DEPLOYMENT_ID: &str = "ALIEN_DEPLOYMENT_ID";
pub const ENV_ALIEN_DEPLOYMENT_NAME: &str = "ALIEN_DEPLOYMENT_NAME";
/// Identifies the current app resource within its deployment stack. Unlike
/// command-specific target variables, this is the universal resource identity
/// name. External/bootstrap mint clients include it when requesting
/// resource-scoped credentials.
pub const ENV_ALIEN_RESOURCE_ID: &str = "ALIEN_RESOURCE_ID";
pub const ENV_ALIEN_PUBLIC_ENDPOINTS_JSON: &str = "ALIEN_PUBLIC_ENDPOINTS_JSON";
pub const ENV_ALIEN_COMMANDS_TOKEN: &str = "ALIEN_COMMANDS_TOKEN";
/// File containing the command receiver bearer token. Receivers re-read this
/// file after an unauthorized response so controllers can rotate credentials
/// without restarting the workload.
pub const ENV_ALIEN_COMMANDS_TOKEN_FILE: &str = "ALIEN_COMMANDS_TOKEN_FILE";
/// Lease duration requested by app-owned command receivers, in seconds.
pub const ENV_ALIEN_COMMANDS_LEASE_SECONDS: &str = "ALIEN_COMMANDS_LEASE_SECONDS";
/// Maximum command leases requested by an app-owned receiver per poll.
pub const ENV_ALIEN_COMMANDS_MAX_LEASES: &str = "ALIEN_COMMANDS_MAX_LEASES";
/// Base command receiver poll interval, in milliseconds.
pub const ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS: &str = "ALIEN_COMMANDS_POLL_INTERVAL_MS";
/// Maximum command receiver poll interval after adaptive backoff, in milliseconds.
pub const ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS: &str = "ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS";
/// Fractional jitter applied to command receiver poll intervals.
pub const ENV_ALIEN_COMMANDS_POLL_JITTER: &str = "ALIEN_COMMANDS_POLL_JITTER";
/// Graceful command receiver drain timeout, in milliseconds.
pub const ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS: &str = "ALIEN_COMMANDS_DRAIN_TIMEOUT_MS";
/// Identifies which stack resource an app-owned command receiver leases for.
pub const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID: &str = "ALIEN_COMMANDS_TARGET_RESOURCE_ID";
/// Base URL of the command server API an app-owned pull `Receiver`
/// (Container/Daemon) leases commands from. Pinned by the receiver contract;
/// the TypeScript receiver reads the same variable. Missing or
/// invalid values fail fast with `COMMAND_RECEIVER_CONFIG_INVALID`. Injected
/// by the manager and operator controllers, scoped per command-enabled
/// resource.
pub const ENV_ALIEN_COMMANDS_URL: &str = "ALIEN_COMMANDS_URL";
/// Type of the command target a pull `Receiver` leases for (`container` |
/// `daemon`). Lease requests require a typed target and a receiver must not
/// guess it (the worker runtime hardcodes `worker`; a Container/Daemon
/// receiver gets its type injected). Companion to
/// [`ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID`].
pub const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE: &str = "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE";
/// Base URL of the deployment's manager. The client-side minting-backed
/// credential resolver ([`alien-bindings`]) posts to `{ALIEN_MANAGER_URL}/v1/credentials/mint`
/// when an external/bootstrap integration explicitly configures the mint
/// environment contract. Managed cloud workloads use projected identities.
pub const ENV_ALIEN_MANAGER_URL: &str = "ALIEN_MANAGER_URL";
/// Deployment bearer token an external/bootstrap mint client presents to the
/// manager. Kept distinct from [`ENV_ALIEN_COMMANDS_TOKEN`], which authenticates
/// command delivery. Managed workload controllers do not inject this token.
pub const ENV_ALIEN_DEPLOYMENT_TOKEN: &str = "ALIEN_DEPLOYMENT_TOKEN";
/// Service-account binding name the minting resolver asks the manager to mint
/// credentials for. Required by the external/bootstrap mint request contract.
///
/// Deliberately does **not** end in `_BINDING`: names matching `ALIEN_*_BINDING`
/// are parsed as resource-binding JSON by the provider, which this is not.
pub const ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT: &str = "ALIEN_DEPLOYMENT_SERVICE_ACCOUNT";
/// Address of the worker app protocol gRPC server. The runtime binds its
/// Control + WaitUntil services here and injects the same value for the
/// application it spawns; the app connects as the gRPC client. Presence of this
/// variable is what selects the worker-protocol gRPC channel in the app SDK.
pub const ENV_ALIEN_WORKER_GRPC_ADDRESS: &str = "ALIEN_WORKER_GRPC_ADDRESS";
/// Configured maximum Worker command execution time, in seconds. Controllers
/// inject this from the trusted Worker resource rather than accepting a
/// user-provided override.
pub const ENV_ALIEN_WORKER_TIMEOUT_SECONDS: &str = "ALIEN_WORKER_TIMEOUT_SECONDS";
pub const ENV_AWS_ACCOUNT_ID: &str = "AWS_ACCOUNT_ID";
pub const ENV_AWS_REGION: &str = "AWS_REGION";
pub const ENV_AZURE_CLIENT_ID: &str = "AZURE_CLIENT_ID";
pub const ENV_AZURE_REGION: &str = "AZURE_REGION";
pub const ENV_AZURE_SUBSCRIPTION_ID: &str = "AZURE_SUBSCRIPTION_ID";
pub const ENV_AZURE_TENANT_ID: &str = "AZURE_TENANT_ID";
pub const ENV_GCP_PROJECT_ID: &str = "GCP_PROJECT_ID";
pub const ENV_GCP_REGION: &str = "GCP_REGION";
pub const ENV_GOOGLE_CLOUD_PROJECT: &str = "GOOGLE_CLOUD_PROJECT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEnvironmentValue {
    Literal(&'static str),
    AwsAccountId,
    AwsRegion,
    AzureClientId,
    AzureRegion,
    AzureSubscriptionId,
    AzureTenantId,
    BasePlatform,
    CurrentContainerBindingName,
    CurrentWorkerBindingName,
    GcpProjectId,
    GcpRegion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeEnvironmentEntry {
    pub name: &'static str,
    pub value: RuntimeEnvironmentValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEnvironmentBindingSource {
    LinkedResource(ResourceRef),
    CurrentContainer,
    CurrentWorker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEnvironmentBindingEntry {
    pub env_name: String,
    pub binding_name: String,
    pub source: RuntimeEnvironmentBindingSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEnvironmentPlanEntry {
    Scalar(RuntimeEnvironmentEntry),
    Binding(RuntimeEnvironmentBindingEntry),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeEnvironmentPlan {
    entries: Vec<RuntimeEnvironmentPlanEntry>,
}

impl RuntimeEnvironmentPlan {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_scalar_entries(
        mut self,
        entries: impl IntoIterator<Item = RuntimeEnvironmentEntry>,
    ) -> Self {
        self.entries
            .extend(entries.into_iter().map(RuntimeEnvironmentPlanEntry::Scalar));
        self
    }

    pub fn add_linked_bindings(mut self, links: &[ResourceRef]) -> Self {
        self.entries.extend(links.iter().cloned().map(|link| {
            let binding_name = link.id().to_string();
            RuntimeEnvironmentPlanEntry::Binding(RuntimeEnvironmentBindingEntry {
                env_name: binding_env_var_name(&binding_name),
                binding_name,
                source: RuntimeEnvironmentBindingSource::LinkedResource(link),
            })
        }));
        self
    }

    pub fn add_current_worker_binding(mut self, worker_id: &str) -> Self {
        self.entries.push(RuntimeEnvironmentPlanEntry::Binding(
            RuntimeEnvironmentBindingEntry {
                env_name: binding_env_var_name(worker_id),
                binding_name: worker_id.to_string(),
                source: RuntimeEnvironmentBindingSource::CurrentWorker,
            },
        ));
        self
    }

    pub fn add_current_container_binding(mut self, container_id: &str) -> Self {
        self.entries.push(RuntimeEnvironmentPlanEntry::Binding(
            RuntimeEnvironmentBindingEntry {
                env_name: binding_env_var_name(container_id),
                binding_name: container_id.to_string(),
                source: RuntimeEnvironmentBindingSource::CurrentContainer,
            },
        ));
        self
    }

    pub fn entries(&self) -> &[RuntimeEnvironmentPlanEntry] {
        &self.entries
    }
}

pub trait RuntimeEnvironmentRenderer {
    type Value;

    fn render_runtime_environment_value(
        &self,
        value: RuntimeEnvironmentValue,
    ) -> Result<Option<Self::Value>>;

    fn render_runtime_environment_binding(
        &self,
        entry: &RuntimeEnvironmentBindingEntry,
    ) -> Result<Option<Self::Value>>;
}

pub fn standard_runtime_environment_plan(platform: Platform) -> Vec<RuntimeEnvironmentEntry> {
    let mut entries = vec![RuntimeEnvironmentEntry {
        name: ENV_ALIEN_DEPLOYMENT_TYPE,
        value: RuntimeEnvironmentValue::Literal(platform.as_str()),
    }];

    match platform {
        Platform::Aws => entries.push(RuntimeEnvironmentEntry {
            name: ENV_AWS_ACCOUNT_ID,
            value: RuntimeEnvironmentValue::AwsAccountId,
        }),
        Platform::Gcp => entries.extend([
            RuntimeEnvironmentEntry {
                name: ENV_GOOGLE_CLOUD_PROJECT,
                value: RuntimeEnvironmentValue::GcpProjectId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_GCP_PROJECT_ID,
                value: RuntimeEnvironmentValue::GcpProjectId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_GCP_REGION,
                value: RuntimeEnvironmentValue::GcpRegion,
            },
        ]),
        Platform::Azure => entries.extend([
            RuntimeEnvironmentEntry {
                name: ENV_AZURE_SUBSCRIPTION_ID,
                value: RuntimeEnvironmentValue::AzureSubscriptionId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_AZURE_TENANT_ID,
                value: RuntimeEnvironmentValue::AzureTenantId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_AZURE_REGION,
                value: RuntimeEnvironmentValue::AzureRegion,
            },
        ]),
        Platform::Kubernetes => entries.push(RuntimeEnvironmentEntry {
            name: ENV_OPERATOR_BASE_PLATFORM,
            value: RuntimeEnvironmentValue::BasePlatform,
        }),
        Platform::Machines | Platform::Local | Platform::Test => {}
    }

    entries
}

pub fn kubernetes_base_platform_runtime_environment_plan(
    base_platform: Option<Platform>,
) -> Vec<RuntimeEnvironmentEntry> {
    match base_platform {
        Some(Platform::Aws) => vec![
            RuntimeEnvironmentEntry {
                name: ENV_AWS_ACCOUNT_ID,
                value: RuntimeEnvironmentValue::AwsAccountId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_AWS_REGION,
                value: RuntimeEnvironmentValue::AwsRegion,
            },
        ],
        Some(Platform::Gcp) => vec![
            RuntimeEnvironmentEntry {
                name: ENV_GOOGLE_CLOUD_PROJECT,
                value: RuntimeEnvironmentValue::GcpProjectId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_GCP_PROJECT_ID,
                value: RuntimeEnvironmentValue::GcpProjectId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_GCP_REGION,
                value: RuntimeEnvironmentValue::GcpRegion,
            },
        ],
        Some(Platform::Azure) => vec![
            RuntimeEnvironmentEntry {
                name: ENV_AZURE_SUBSCRIPTION_ID,
                value: RuntimeEnvironmentValue::AzureSubscriptionId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_AZURE_TENANT_ID,
                value: RuntimeEnvironmentValue::AzureTenantId,
            },
            RuntimeEnvironmentEntry {
                name: ENV_AZURE_REGION,
                value: RuntimeEnvironmentValue::AzureRegion,
            },
            RuntimeEnvironmentEntry {
                name: ENV_AZURE_CLIENT_ID,
                value: RuntimeEnvironmentValue::AzureClientId,
            },
        ],
        _ => Vec::new(),
    }
}

pub fn worker_transport_runtime_environment_plan(
    platform: Platform,
) -> Vec<RuntimeEnvironmentEntry> {
    match platform {
        Platform::Aws => vec![
            RuntimeEnvironmentEntry {
                name: ENV_ALIEN_TRANSPORT,
                value: RuntimeEnvironmentValue::Literal("lambda"),
            },
            RuntimeEnvironmentEntry {
                name: ENV_ALIEN_LAMBDA_MODE,
                value: RuntimeEnvironmentValue::Literal("buffered"),
            },
        ],
        Platform::Gcp => vec![RuntimeEnvironmentEntry {
            name: ENV_ALIEN_TRANSPORT,
            value: RuntimeEnvironmentValue::Literal("cloud-run"),
        }],
        Platform::Azure => vec![RuntimeEnvironmentEntry {
            name: ENV_ALIEN_TRANSPORT,
            value: RuntimeEnvironmentValue::Literal("container-app"),
        }],
        Platform::Kubernetes | Platform::Machines => vec![RuntimeEnvironmentEntry {
            name: ENV_ALIEN_TRANSPORT,
            value: RuntimeEnvironmentValue::Literal("http"),
        }],
        Platform::Local | Platform::Test => vec![RuntimeEnvironmentEntry {
            name: ENV_ALIEN_TRANSPORT,
            // Local/Test Workers run under the runtime's `local` transport (the
            // in-process HTTP invocation proxy the worker manager selects via
            // `TransportType::Local`). The env-plan value now matches that reality,
            // so `ALIEN_TRANSPORT` for Workers is exactly the transport set
            // `lambda | cloud-run | container-app | http | local`.
            value: RuntimeEnvironmentValue::Literal("local"),
        }],
    }
}

pub fn worker_runtime_environment_plan(platform: Platform) -> Vec<RuntimeEnvironmentEntry> {
    let mut entries = standard_runtime_environment_plan(platform);
    entries.extend(worker_transport_runtime_environment_plan(platform));
    entries.push(RuntimeEnvironmentEntry {
        name: ENV_ALIEN_RUNTIME_SEND_OTLP,
        value: RuntimeEnvironmentValue::Literal("true"),
    });
    entries.push(RuntimeEnvironmentEntry {
        name: ENV_ALIEN_CURRENT_WORKER_BINDING_NAME,
        value: RuntimeEnvironmentValue::CurrentWorkerBindingName,
    });
    if platform == Platform::Azure {
        entries.push(RuntimeEnvironmentEntry {
            name: ENV_AZURE_CLIENT_ID,
            value: RuntimeEnvironmentValue::AzureClientId,
        });
    }
    entries
}

pub fn worker_runtime_environment_contract(
    platform: Platform,
    worker_id: &str,
    links: &[ResourceRef],
) -> RuntimeEnvironmentPlan {
    RuntimeEnvironmentPlan::new()
        .add_scalar_entries(worker_runtime_environment_plan(platform))
        .add_linked_bindings(links)
        .add_current_worker_binding(worker_id)
}

pub fn container_runtime_environment_plan(platform: Platform) -> Vec<RuntimeEnvironmentEntry> {
    let mut entries = standard_runtime_environment_plan(platform);
    // `ALIEN_TRANSPORT` is Worker-only. Command-enabled Containers run the pull
    // receiver configured per resource through the `ALIEN_COMMANDS_*` contract.
    entries.push(RuntimeEnvironmentEntry {
        name: ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME,
        value: RuntimeEnvironmentValue::CurrentContainerBindingName,
    });
    entries
}

pub fn container_runtime_environment_contract(
    platform: Platform,
    container_id: &str,
    links: &[ResourceRef],
) -> RuntimeEnvironmentPlan {
    RuntimeEnvironmentPlan::new()
        .add_scalar_entries(container_runtime_environment_plan(platform))
        .add_linked_bindings(links)
        .add_current_container_binding(container_id)
}

pub fn daemon_runtime_environment_plan(platform: Platform) -> Vec<RuntimeEnvironmentEntry> {
    // Daemons run under direct supervision and receive only the standard
    // platform-identity vars. `ALIEN_TRANSPORT` and the container self-binding
    // var are not part of the Daemon contract. Command-enabled Daemons get their
    // `ALIEN_COMMANDS_*` receiver config from the resource controller.
    standard_runtime_environment_plan(platform)
}

pub fn daemon_runtime_environment_contract(
    platform: Platform,
    links: &[ResourceRef],
) -> RuntimeEnvironmentPlan {
    RuntimeEnvironmentPlan::new()
        .add_scalar_entries(daemon_runtime_environment_plan(platform))
        .add_linked_bindings(links)
}

pub fn render_runtime_environment_entries<R>(
    entries: impl IntoIterator<Item = RuntimeEnvironmentEntry>,
    renderer: &R,
) -> Result<Vec<(&'static str, R::Value)>>
where
    R: RuntimeEnvironmentRenderer,
{
    let mut rendered = Vec::new();
    for entry in entries {
        if let Some(value) = renderer.render_runtime_environment_value(entry.value)? {
            rendered.push((entry.name, value));
        }
    }
    Ok(rendered)
}

pub fn render_runtime_environment_plan<R>(
    plan: &RuntimeEnvironmentPlan,
    renderer: &R,
) -> Result<Vec<(String, R::Value)>>
where
    R: RuntimeEnvironmentRenderer,
{
    let mut rendered = Vec::new();
    for entry in plan.entries() {
        match entry {
            RuntimeEnvironmentPlanEntry::Scalar(entry) => {
                if let Some(value) = renderer.render_runtime_environment_value(entry.value)? {
                    rendered.push((entry.name.to_string(), value));
                }
            }
            RuntimeEnvironmentPlanEntry::Binding(entry) => {
                if let Some(value) = renderer.render_runtime_environment_binding(entry)? {
                    rendered.push((entry.env_name.clone(), value));
                }
            }
        }
    }
    Ok(rendered)
}

pub fn is_runtime_environment_contract_name(name: &str) -> bool {
    matches!(
        name,
        ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME
            | ENV_ALIEN_CURRENT_WORKER_BINDING_NAME
            | ENV_OPERATOR_BASE_PLATFORM
            | ENV_ALIEN_DEPLOYMENT_TYPE
            | ENV_ALIEN_LAMBDA_MODE
            | ENV_ALIEN_RUNTIME_SEND_OTLP
            | ENV_ALIEN_TRANSPORT
            | ENV_AWS_ACCOUNT_ID
            | ENV_AWS_REGION
            | ENV_AZURE_CLIENT_ID
            | ENV_AZURE_REGION
            | ENV_AZURE_SUBSCRIPTION_ID
            | ENV_AZURE_TENANT_ID
            | ENV_GCP_PROJECT_ID
            | ENV_GCP_REGION
            | ENV_GOOGLE_CLOUD_PROJECT
    ) || (name.starts_with("ALIEN_") && name.ends_with("_BINDING"))
}

pub fn is_reserved_runtime_environment_name(name: &str) -> bool {
    is_runtime_environment_contract_name(name)
        || matches!(
            name,
            ENV_ALIEN_WORKER_GRPC_ADDRESS
                | ENV_ALIEN_WORKER_TIMEOUT_SECONDS
                | ENV_ALIEN_COMMANDS_TOKEN
                | ENV_ALIEN_COMMANDS_TOKEN_FILE
                | ENV_ALIEN_COMMANDS_LEASE_SECONDS
                | ENV_ALIEN_COMMANDS_MAX_LEASES
                | ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS
                | ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS
                | ENV_ALIEN_COMMANDS_POLL_JITTER
                | ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS
                | ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID
                | ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE
                | ENV_ALIEN_COMMANDS_URL
                | ENV_ALIEN_DEPLOYMENT_ID
                | ENV_ALIEN_DEPLOYMENT_NAME
                | ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT
                | ENV_ALIEN_DEPLOYMENT_TOKEN
                | ENV_ALIEN_MANAGER_URL
                | ENV_ALIEN_RESOURCE_ID
                | ENV_ALIEN_PUBLIC_ENDPOINTS_JSON
                | ENV_ALIEN_RUNTIME_SECRETS
                | ENV_ALIEN_SECRET_ENV_REVISION
                | ENV_ALIEN_SECRETS
        )
        || name.starts_with("ALIEN_BINDING_")
}

pub fn validate_runtime_environment_user_vars<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let reserved: Vec<String> = names
        .into_iter()
        .filter(|name| is_reserved_runtime_environment_name(name))
        .map(ToString::to_string)
        .collect();
    if reserved.is_empty() {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::GenericError {
        message: format!(
            "Environment variables use reserved Alien runtime names: {}",
            reserved.join(", ")
        ),
    }))
}

pub fn validate_runtime_environment_user_map(env: &HashMap<String, String>) -> Result<()> {
    validate_runtime_environment_user_vars(env.keys().map(String::as_str))
}

pub fn validate_prepared_runtime_environment_vars<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let reserved: Vec<String> = names
        .into_iter()
        .filter(|name| is_runtime_environment_contract_name(name))
        .map(ToString::to_string)
        .collect();
    if reserved.is_empty() {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::GenericError {
        message: format!(
            "Environment variables collide with Alien runtime contract names: {}",
            reserved.join(", ")
        ),
    }))
}

pub fn validate_prepared_runtime_environment_map(env: &HashMap<String, String>) -> Result<()> {
    validate_prepared_runtime_environment_vars(env.keys().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_builtin_and_binding_environment_names() {
        assert!(is_reserved_runtime_environment_name(ENV_ALIEN_TRANSPORT));
        assert!(is_reserved_runtime_environment_name(
            ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME
        ));
        assert!(is_reserved_runtime_environment_name(
            ENV_OPERATOR_BASE_PLATFORM
        ));
        assert!(is_reserved_runtime_environment_name(ENV_ALIEN_SECRETS));
        assert!(is_reserved_runtime_environment_name(
            ENV_ALIEN_SECRET_ENV_REVISION
        ));
        assert!(is_reserved_runtime_environment_name(
            ENV_ALIEN_WORKER_GRPC_ADDRESS
        ));
        assert_eq!(ENV_ALIEN_WORKER_GRPC_ADDRESS, "ALIEN_WORKER_GRPC_ADDRESS");
        assert!(is_reserved_runtime_environment_name(
            ENV_ALIEN_WORKER_TIMEOUT_SECONDS
        ));
        assert_eq!(
            ENV_ALIEN_WORKER_TIMEOUT_SECONDS,
            "ALIEN_WORKER_TIMEOUT_SECONDS"
        );
        assert!(is_reserved_runtime_environment_name(
            "ALIEN_STORAGE_BINDING"
        ));
        assert!(is_reserved_runtime_environment_name(
            "ALIEN_BINDING_STORAGE_URL"
        ));
        assert!(!is_reserved_runtime_environment_name("USER_DEFINED"));
    }

    #[test]
    fn reserves_minting_credential_resolver_names() {
        assert!(is_reserved_runtime_environment_name(ENV_ALIEN_MANAGER_URL));
        assert!(is_reserved_runtime_environment_name(
            ENV_ALIEN_DEPLOYMENT_TOKEN
        ));
        assert!(is_reserved_runtime_environment_name(
            ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT
        ));
        assert!(is_reserved_runtime_environment_name(ENV_ALIEN_RESOURCE_ID));
        assert_eq!(ENV_ALIEN_MANAGER_URL, "ALIEN_MANAGER_URL");
        assert_eq!(ENV_ALIEN_DEPLOYMENT_TOKEN, "ALIEN_DEPLOYMENT_TOKEN");
        assert_eq!(ENV_ALIEN_RESOURCE_ID, "ALIEN_RESOURCE_ID");
        assert_eq!(
            ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT,
            "ALIEN_DEPLOYMENT_SERVICE_ACCOUNT"
        );
        // Must not match the `ALIEN_*_BINDING` resource-binding pattern, or the
        // provider would try to parse its value as binding JSON.
        assert!(!ENV_ALIEN_DEPLOYMENT_SERVICE_ACCOUNT.ends_with("_BINDING"));
    }

    #[test]
    fn reserves_commands_target_resource_id() {
        assert!(is_reserved_runtime_environment_name(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID
        ));
        assert_eq!(
            ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID,
            "ALIEN_COMMANDS_TARGET_RESOURCE_ID"
        );
    }

    #[test]
    fn reserves_command_receiver_names() {
        for (constant, expected) in [
            (ENV_ALIEN_COMMANDS_URL, "ALIEN_COMMANDS_URL"),
            (ENV_ALIEN_COMMANDS_TOKEN_FILE, "ALIEN_COMMANDS_TOKEN_FILE"),
            (
                ENV_ALIEN_COMMANDS_LEASE_SECONDS,
                "ALIEN_COMMANDS_LEASE_SECONDS",
            ),
            (ENV_ALIEN_COMMANDS_MAX_LEASES, "ALIEN_COMMANDS_MAX_LEASES"),
            (
                ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS,
                "ALIEN_COMMANDS_POLL_INTERVAL_MS",
            ),
            (
                ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS,
                "ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS",
            ),
            (ENV_ALIEN_COMMANDS_POLL_JITTER, "ALIEN_COMMANDS_POLL_JITTER"),
            (
                ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS,
                "ALIEN_COMMANDS_DRAIN_TIMEOUT_MS",
            ),
            (
                ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
                "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE",
            ),
        ] {
            assert_eq!(constant, expected);
            assert!(is_reserved_runtime_environment_name(constant));
        }
    }

    #[test]
    fn rejects_reserved_user_environment_names() {
        let error = validate_runtime_environment_user_vars(["USER_DEFINED", ENV_ALIEN_TRANSPORT])
            .unwrap_err();

        assert!(error.to_string().contains(ENV_ALIEN_TRANSPORT));
    }

    #[test]
    fn prepared_environment_allows_deployment_managed_names() {
        validate_prepared_runtime_environment_vars([
            ENV_ALIEN_SECRETS,
            ENV_ALIEN_DEPLOYMENT_ID,
            ENV_ALIEN_DEPLOYMENT_NAME,
            ENV_ALIEN_PUBLIC_ENDPOINTS_JSON,
        ])
        .unwrap();

        let error =
            validate_prepared_runtime_environment_vars([ENV_ALIEN_SECRETS, ENV_ALIEN_TRANSPORT])
                .unwrap_err();

        assert!(error.to_string().contains(ENV_ALIEN_TRANSPORT));
        assert!(!error.to_string().contains(ENV_ALIEN_SECRETS));
    }

    #[test]
    fn kubernetes_standard_environment_declares_base_platform() {
        let entries = standard_runtime_environment_plan(Platform::Kubernetes);

        assert!(entries.iter().any(|entry| {
            entry.name == ENV_OPERATOR_BASE_PLATFORM
                && entry.value == RuntimeEnvironmentValue::BasePlatform
        }));
    }

    #[test]
    fn kubernetes_gcp_base_environment_declares_gcp_identity() {
        let entries = kubernetes_base_platform_runtime_environment_plan(Some(Platform::Gcp));

        assert!(entries.iter().any(|entry| {
            entry.name == ENV_GOOGLE_CLOUD_PROJECT
                && entry.value == RuntimeEnvironmentValue::GcpProjectId
        }));
        assert!(entries.iter().any(|entry| {
            entry.name == ENV_GCP_PROJECT_ID && entry.value == RuntimeEnvironmentValue::GcpProjectId
        }));
        assert!(entries.iter().any(|entry| {
            entry.name == ENV_GCP_REGION && entry.value == RuntimeEnvironmentValue::GcpRegion
        }));
    }

    #[test]
    fn kubernetes_worker_environment_uses_http_proxy_transport() {
        let entries = worker_transport_runtime_environment_plan(Platform::Kubernetes);

        assert!(entries.iter().any(|entry| {
            entry.name == ENV_ALIEN_TRANSPORT
                && entry.value == RuntimeEnvironmentValue::Literal("http")
        }));
    }

    #[test]
    fn container_environment_does_not_inject_worker_transport() {
        for platform in [
            Platform::Local,
            Platform::Kubernetes,
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Test,
        ] {
            let entries = container_runtime_environment_plan(platform);
            assert!(
                !entries
                    .iter()
                    .any(|entry| entry.name == ENV_ALIEN_TRANSPORT),
                "container plan for {platform:?} must not set ALIEN_TRANSPORT"
            );
            // The container binding-name var is still present.
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.name == ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME),
                "container plan for {platform:?} must still declare the binding-name var"
            );
        }
    }

    #[test]
    fn daemon_environment_is_standard_identity_only() {
        // Daemons use the standard platform-identity set. Receiver config is
        // injected per resource, not by the static environment plan.
        for platform in [
            Platform::Local,
            Platform::Kubernetes,
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Test,
        ] {
            let entries = daemon_runtime_environment_plan(platform);
            assert!(
                !entries
                    .iter()
                    .any(|entry| entry.name == ENV_ALIEN_TRANSPORT),
                "daemon plan for {platform:?} must not set ALIEN_TRANSPORT"
            );
            assert!(
                !entries
                    .iter()
                    .any(|entry| entry.name == ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME),
                "daemon plan for {platform:?} must not set the container self-binding var"
            );
            // The standard deployment-type identity var is always present.
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.name == ENV_ALIEN_DEPLOYMENT_TYPE),
                "daemon plan for {platform:?} must declare ALIEN_DEPLOYMENT_TYPE"
            );
        }
    }

    #[test]
    fn worker_local_transport_uses_local_proxy() {
        // Local/Test Workers run under `TransportType::Local`, selected by the
        // Worker manager.
        for platform in [Platform::Local, Platform::Test] {
            let entries = worker_transport_runtime_environment_plan(platform);
            assert!(entries.iter().any(|entry| {
                entry.name == ENV_ALIEN_TRANSPORT
                    && entry.value == RuntimeEnvironmentValue::Literal("local")
            }));
        }
    }
}
