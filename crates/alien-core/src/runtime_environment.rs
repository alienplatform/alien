use crate::{bindings::binding_env_var_name, ErrorData, Platform, ResourceRef, Result};
use alien_error::AlienError;
use std::collections::HashMap;

pub const ENV_ALIEN_CURRENT_FUNCTION_BINDING_NAME: &str = "ALIEN_CURRENT_FUNCTION_BINDING_NAME";
pub const ENV_ALIEN_CURRENT_CONTAINER_BINDING_NAME: &str = "ALIEN_CURRENT_CONTAINER_BINDING_NAME";
pub const ENV_ALIEN_DEPLOYMENT_TYPE: &str = "ALIEN_DEPLOYMENT_TYPE";
pub const ENV_ALIEN_LAMBDA_MODE: &str = "ALIEN_LAMBDA_MODE";
pub const ENV_ALIEN_RUNTIME_SEND_OTLP: &str = "ALIEN_RUNTIME_SEND_OTLP";
pub const ENV_ALIEN_SECRETS: &str = "ALIEN_SECRETS";
pub const ENV_ALIEN_TRANSPORT: &str = "ALIEN_TRANSPORT";
pub const ENV_ALIEN_DEPLOYMENT_ID: &str = "ALIEN_DEPLOYMENT_ID";
pub const ENV_ALIEN_COMMANDS_POLLING_ENABLED: &str = "ALIEN_COMMANDS_POLLING_ENABLED";
pub const ENV_ALIEN_COMMANDS_POLLING_URL: &str = "ALIEN_COMMANDS_POLLING_URL";
pub const ENV_ALIEN_COMMANDS_POLLING_INTERVAL_SECS: &str = "ALIEN_COMMANDS_POLLING_INTERVAL_SECS";
pub const ENV_ALIEN_COMMANDS_TOKEN: &str = "ALIEN_COMMANDS_TOKEN";
pub const ENV_ALIEN_BINDINGS_ADDRESS: &str = "ALIEN_BINDINGS_ADDRESS";
pub const ENV_ALIEN_BINDINGS_GRPC_ADDRESS: &str = "ALIEN_BINDINGS_GRPC_ADDRESS";
pub const ENV_ALIEN_BINDINGS_MODE: &str = "ALIEN_BINDINGS_MODE";
pub const ENV_AWS_ACCOUNT_ID: &str = "AWS_ACCOUNT_ID";
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
    AzureClientId,
    AzureRegion,
    AzureSubscriptionId,
    AzureTenantId,
    CurrentContainerBindingName,
    CurrentFunctionBindingName,
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
    CurrentFunction,
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

    pub fn add_current_function_binding(mut self, function_id: &str) -> Self {
        self.entries.push(RuntimeEnvironmentPlanEntry::Binding(
            RuntimeEnvironmentBindingEntry {
                env_name: binding_env_var_name(function_id),
                binding_name: function_id.to_string(),
                source: RuntimeEnvironmentBindingSource::CurrentFunction,
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
        Platform::Kubernetes | Platform::Local | Platform::Test => {}
    }

    entries
}

pub fn function_transport_runtime_environment_plan(
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
        Platform::Kubernetes | Platform::Local | Platform::Test => vec![RuntimeEnvironmentEntry {
            name: ENV_ALIEN_TRANSPORT,
            value: RuntimeEnvironmentValue::Literal("passthrough"),
        }],
    }
}

pub fn function_runtime_environment_plan(platform: Platform) -> Vec<RuntimeEnvironmentEntry> {
    let mut entries = standard_runtime_environment_plan(platform);
    entries.extend(function_transport_runtime_environment_plan(platform));
    entries.push(RuntimeEnvironmentEntry {
        name: ENV_ALIEN_RUNTIME_SEND_OTLP,
        value: RuntimeEnvironmentValue::Literal("true"),
    });
    entries.push(RuntimeEnvironmentEntry {
        name: ENV_ALIEN_CURRENT_FUNCTION_BINDING_NAME,
        value: RuntimeEnvironmentValue::CurrentFunctionBindingName,
    });
    if platform == Platform::Azure {
        entries.push(RuntimeEnvironmentEntry {
            name: ENV_AZURE_CLIENT_ID,
            value: RuntimeEnvironmentValue::AzureClientId,
        });
    }
    entries
}

pub fn function_runtime_environment_contract(
    platform: Platform,
    function_id: &str,
    links: &[ResourceRef],
) -> RuntimeEnvironmentPlan {
    RuntimeEnvironmentPlan::new()
        .add_scalar_entries(function_runtime_environment_plan(platform))
        .add_linked_bindings(links)
        .add_current_function_binding(function_id)
}

pub fn passthrough_transport_runtime_environment_plan() -> [RuntimeEnvironmentEntry; 1] {
    [RuntimeEnvironmentEntry {
        name: ENV_ALIEN_TRANSPORT,
        value: RuntimeEnvironmentValue::Literal("passthrough"),
    }]
}

pub fn container_runtime_environment_plan(platform: Platform) -> Vec<RuntimeEnvironmentEntry> {
    let mut entries = standard_runtime_environment_plan(platform);
    entries.extend(passthrough_transport_runtime_environment_plan());
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
            | ENV_ALIEN_CURRENT_FUNCTION_BINDING_NAME
            | ENV_ALIEN_DEPLOYMENT_TYPE
            | ENV_ALIEN_LAMBDA_MODE
            | ENV_ALIEN_RUNTIME_SEND_OTLP
            | ENV_ALIEN_TRANSPORT
            | ENV_AWS_ACCOUNT_ID
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
            ENV_ALIEN_BINDINGS_ADDRESS
                | ENV_ALIEN_BINDINGS_GRPC_ADDRESS
                | ENV_ALIEN_BINDINGS_MODE
                | ENV_ALIEN_COMMANDS_POLLING_ENABLED
                | ENV_ALIEN_COMMANDS_POLLING_INTERVAL_SECS
                | ENV_ALIEN_COMMANDS_POLLING_URL
                | ENV_ALIEN_COMMANDS_TOKEN
                | ENV_ALIEN_DEPLOYMENT_ID
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
        assert!(is_reserved_runtime_environment_name(ENV_ALIEN_SECRETS));
        assert!(is_reserved_runtime_environment_name(
            "ALIEN_STORAGE_BINDING"
        ));
        assert!(is_reserved_runtime_environment_name(
            "ALIEN_BINDING_STORAGE_URL"
        ));
        assert!(!is_reserved_runtime_environment_name("USER_DEFINED"));
    }

    #[test]
    fn rejects_reserved_user_environment_names() {
        let error = validate_runtime_environment_user_vars(["USER_DEFINED", ENV_ALIEN_TRANSPORT])
            .unwrap_err();

        assert!(error.to_string().contains(ENV_ALIEN_TRANSPORT));
    }

    #[test]
    fn prepared_environment_allows_deployment_managed_names() {
        validate_prepared_runtime_environment_vars([ENV_ALIEN_SECRETS, ENV_ALIEN_DEPLOYMENT_ID])
            .unwrap();

        let error =
            validate_prepared_runtime_environment_vars([ENV_ALIEN_SECRETS, ENV_ALIEN_TRANSPORT])
                .unwrap_err();

        assert!(error.to_string().contains(ENV_ALIEN_TRANSPORT));
        assert!(!error.to_string().contains(ENV_ALIEN_SECRETS));
    }
}
