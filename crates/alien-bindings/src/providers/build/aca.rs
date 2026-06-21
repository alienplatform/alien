use crate::{
    error::{Error, ErrorData},
    providers::build::script::create_build_wrapper_script,
    traits::{Binding, Build},
};
use alien_core::{
    bindings::BuildBinding, AzureClientConfig, AzureCredentials, BuildConfig, BuildExecution,
    BuildStatus, ComputeType,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use azure_core::{
    cloud::{CloudConfiguration, CustomConfiguration},
    credentials::{AccessToken, Secret, TokenCredential, TokenRequestOptions},
    http::ClientOptions,
    time::{Duration as AzureDuration, OffsetDateTime},
};
use azure_identity::{
    ClientAssertionCredentialOptions, ClientSecretCredential, ClientSecretCredentialOptions,
    ManagedIdentityCredential, ManagedIdentityCredentialOptions, UserAssignedId,
    WorkloadIdentityCredential, WorkloadIdentityCredentialOptions,
};
use reqwest::{Client, Method, Response, Url};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

const MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";
const MANAGEMENT_ENDPOINT: &str = "https://management.azure.com";
const CONTAINER_APPS_API_VERSION: &str = "2025-01-01";

/// Azure implementation of the `Build` trait using Container Apps Jobs.
pub struct AcaBuild {
    client: Client,
    credential: Arc<dyn TokenCredential>,
    management_endpoint: String,
    binding_name: String,
    resource_prefix: String,
    subscription_id: String,
    resource_group_name: String,
    managed_environment_id: String,
    managed_identity_id: Option<String>,
    build_env_vars: HashMap<String, String>,
    region: String,
    monitoring: Option<alien_core::MonitoringConfig>,
}

impl Debug for AcaBuild {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcaBuild")
            .field("binding_name", &self.binding_name)
            .field("resource_prefix", &self.resource_prefix)
            .field("subscription_id", &self.subscription_id)
            .field("resource_group_name", &self.resource_group_name)
            .field("managed_environment_id", &self.managed_environment_id)
            .field("region", &self.region)
            .finish()
    }
}

impl AcaBuild {
    /// Creates a new Azure Build instance from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: BuildBinding,
        azure_config: &AzureClientConfig,
    ) -> Result<Self, Error> {
        let client = crate::http_client::create_http_client();
        let credential = azure_credential_from_config(azure_config)?;
        let management_endpoint = azure_config
            .service_overrides
            .as_ref()
            .and_then(|overrides| overrides.endpoints.get("management"))
            .cloned()
            .unwrap_or_else(|| MANAGEMENT_ENDPOINT.to_string());

        // Extract values from binding
        let config = match binding {
            BuildBinding::Aca(config) => config,
            _ => {
                return Err(Error::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected ACA binding, got different service type".to_string(),
                }));
            }
        };

        let managed_environment_id = config
            .managed_environment_id
            .into_value(&binding_name, "managed_environment_id")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract managed_environment_id from binding".to_string(),
            })?;

        let resource_group_name = config
            .resource_group_name
            .into_value(&binding_name, "resource_group_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract resource_group_name from binding".to_string(),
            })?;

        let build_env_vars = config
            .build_env_vars
            .into_value(&binding_name, "build_env_vars")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract build_env_vars from binding".to_string(),
            })?;

        let managed_identity_id = config
            .managed_identity_id
            .into_value(&binding_name, "managed_identity_id")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract managed_identity_id from binding".to_string(),
            })?;

        let resource_prefix = config
            .resource_prefix
            .into_value(&binding_name, "resource_prefix")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract resource_prefix from binding".to_string(),
            })?;

        let monitoring = config
            .monitoring
            .into_value(&binding_name, "monitoring")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract monitoring from binding".to_string(),
            })?;

        // Get subscription_id from Azure config (this is a cloud credential)
        let subscription_id = azure_config.subscription_id.clone();

        let binding_name_clone = binding_name.clone();

        Ok(Self {
            client,
            credential,
            management_endpoint,
            binding_name,
            resource_prefix,
            subscription_id,
            resource_group_name,
            managed_environment_id,
            managed_identity_id,
            build_env_vars,
            region: azure_config.region.clone().ok_or_else(|| {
                Error::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name_clone,
                    reason: "Azure region must be specified in config".to_string(),
                })
            })?,
            monitoring,
        })
    }

    /// Convert alien ComputeType to Azure Container Apps resource allocation
    fn map_compute_resources(compute_type: &ComputeType) -> ContainerResources {
        match compute_type {
            ComputeType::Small => ContainerResources {
                cpu: Some(0.25),
                memory: Some("0.5Gi".to_string()),
            },
            ComputeType::Medium => ContainerResources {
                cpu: Some(0.5),
                memory: Some("1Gi".to_string()),
            },
            ComputeType::Large => ContainerResources {
                cpu: Some(1.0),
                memory: Some("2Gi".to_string()),
            },
            ComputeType::XLarge => ContainerResources {
                cpu: Some(2.0),
                memory: Some("4Gi".to_string()),
            },
        }
    }

    /// Convert Azure Container Apps Job status to alien BuildStatus
    fn map_build_status(status: Option<&str>) -> BuildStatus {
        match status {
            Some("Succeeded") => BuildStatus::Succeeded,
            Some("Failed") => BuildStatus::Failed,
            Some("Cancelled") | Some("Canceled") => BuildStatus::Cancelled,
            Some("Running") => BuildStatus::Running,
            Some("Pending") => BuildStatus::Queued,
            _ => BuildStatus::Queued,
        }
    }

    /// Generate a unique job name for the build
    /// Azure Container Apps Jobs have strict naming requirements:
    /// - 2-32 characters inclusive
    /// - Lower case alphanumeric characters or '-'
    /// - Start with alphabetic character, end with alphanumeric
    /// - Cannot have '--'
    fn generate_job_name(&self) -> String {
        let timestamp = chrono::Utc::now().timestamp_millis();
        // Use short hash of binding name + timestamp to stay within 32 char limit
        let short_name = self
            .resource_prefix
            .chars()
            .take(8)
            .collect::<String>()
            .replace('_', "");
        let short_timestamp = (timestamp % 1000000).to_string(); // Last 6 digits
        let job_name = format!("build-{}-{}", short_name, short_timestamp);

        // Ensure it meets Azure naming requirements
        job_name
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .take(32)
            .collect()
    }

    fn build_job_url(&self, job_name: &str) -> Result<Url, Error> {
        let mut url = Url::parse(&format!(
            "{}/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/jobs/{}",
            self.management_endpoint.trim_end_matches('/'),
            self.subscription_id,
            self.resource_group_name,
            job_name
        ))
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "build.aca".to_string(),
            reason: "Invalid Azure Container Apps job URL".to_string(),
        })?;
        url.query_pairs_mut()
            .append_pair("api-version", CONTAINER_APPS_API_VERSION);
        Ok(url)
    }

    async fn bearer_token(&self) -> Result<AccessToken, Error> {
        self.credential
            .get_token(&[MANAGEMENT_SCOPE], None)
            .await
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "build.aca".to_string(),
                reason: "Failed to get Azure management bearer token".to_string(),
            })
    }

    async fn send_json<T: Serialize + ?Sized>(
        &self,
        method: Method,
        url: Url,
        body: &T,
    ) -> Result<Response, Error> {
        let token = self.bearer_token().await?;
        self.client
            .request(method.clone(), url.clone())
            .bearer_auth(token.token.secret())
            .json(body)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.to_string(),
                method: method.to_string(),
            })
    }

    async fn send_empty(&self, method: Method, url: Url) -> Result<Response, Error> {
        let token = self.bearer_token().await?;
        self.client
            .request(method.clone(), url.clone())
            .bearer_auth(token.token.secret())
            .header("Content-Length", "0")
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                url: url.to_string(),
                method: method.to_string(),
            })
    }

    async fn parse_job_response(
        &self,
        response: Response,
        operation: &str,
        job_name: &str,
    ) -> Result<Option<Job>, Error> {
        let url = response.url().to_string();
        let status = response.status();
        let body =
            response
                .text()
                .await
                .into_alien_error()
                .context(ErrorData::HttpRequestFailed {
                    url: url.clone(),
                    method: "READ_BODY".to_string(),
                })?;

        if status.as_u16() == 202 {
            return Ok(None);
        }

        if !status.is_success() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Azure Container Apps {operation} request for job '{job_name}' to {url} failed with status {status}: {body}"
                ),
                resource_id: Some(job_name.to_string()),
            }));
        }

        if body.trim().is_empty() {
            return Ok(None);
        }

        serde_json::from_str::<Job>(&body)
            .map(Some)
            .into_alien_error()
            .context(ErrorData::UnexpectedResponseFormat {
                provider: "azure".to_string(),
                binding_name: self.binding_name.clone(),
                field: operation.to_string(),
                response_json: body,
            })
    }
}

#[async_trait]
impl Build for AcaBuild {
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution, Error> {
        let job_name = self.generate_job_name();

        // Merge build config environment with binding environment variables
        // Build config environment takes precedence over binding environment
        let mut merged_environment = self.build_env_vars.clone();
        merged_environment.extend(config.environment);

        // Merge monitoring configuration - build config takes precedence over binding
        let monitoring = config.monitoring.or_else(|| self.monitoring.clone());

        // Convert environment variables to Azure format
        let azure_env_vars: Vec<EnvironmentVar> = merged_environment
            .iter()
            .map(|(key, value)| EnvironmentVar {
                name: Some(key.clone()),
                value: Some(value.clone()),
                secret_ref: None,
            })
            .collect();

        // Create the job container with the unified wrapper script
        let container_script = create_build_wrapper_script(&config.script, monitoring.as_ref());

        let job_container = Container {
            name: Some("build-container".to_string()),
            image: Some(config.image),
            command: vec!["bash".to_string()],
            args: vec!["-c".to_string(), container_script],
            env: azure_env_vars,
            resources: Some(Self::map_compute_resources(&config.compute_type)),
            probes: vec![],
        };

        // Create job template
        let job_template = JobTemplate {
            containers: vec![job_container],
            init_containers: vec![],
            volumes: vec![],
        };

        // Create job configuration with manual trigger
        let job_configuration = JobConfiguration {
            trigger_type: "Manual".to_string(),
            replica_timeout: config.timeout_seconds as i32,
            replica_retry_limit: Some(1),
            manual_trigger_config: Some(JobConfigurationManualTriggerConfig {
                parallelism: Some(1),
                replica_completion_count: Some(1),
            }),
            registries: vec![],
            secrets: vec![],
        };

        // Create job properties
        let job_properties = JobProperties {
            environment_id: Some(self.managed_environment_id.clone()),
            configuration: Some(job_configuration),
            template: Some(job_template),
            provisioning_state: None,
        };

        // Create managed service identity if we have a managed identity ID
        let identity =
            self.managed_identity_id
                .as_ref()
                .map(|identity_id| ManagedServiceIdentity {
                    type_: "UserAssigned".to_string(),
                    user_assigned_identities: Some(HashMap::from([(
                        identity_id.clone(),
                        UserAssignedIdentity::default(),
                    )])),
                });

        // Create the job
        let job = Job {
            location: self.region.clone(),
            properties: Some(job_properties),
            identity,
            tags: [
                ("alien-resource-type".to_string(), "build".to_string()),
                ("alien-binding-name".to_string(), self.binding_name.clone()),
            ]
            .iter()
            .cloned()
            .collect(),
            id: None,
            name: None,
            type_: None,
        };

        let url = self.build_job_url(&job_name)?;
        let response = self.send_json(Method::PUT, url, &job).await?;
        let build_id = self
            .parse_job_response(response, "create job", &job_name)
            .await?
            .and_then(|created_job| created_job.id)
            .unwrap_or_else(|| job_name.clone());

        Ok(BuildExecution {
            id: build_id,
            status: BuildStatus::Queued,
            start_time: Some(chrono::Utc::now().to_rfc3339()),
            end_time: None,
        })
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution, Error> {
        // Extract job name from build ID (could be a full resource ID or just the name)
        let job_name = if build_id.contains("/") {
            build_id.split('/').last().unwrap_or(build_id)
        } else {
            build_id
        };

        let url = self.build_job_url(job_name)?;
        let response = self.send_empty(Method::GET, url.clone()).await?;

        if response.status().as_u16() == 404 {
            return Ok(BuildExecution {
                id: build_id.to_string(),
                status: BuildStatus::Cancelled,
                start_time: Some(chrono::Utc::now().to_rfc3339()),
                end_time: Some(chrono::Utc::now().to_rfc3339()),
            });
        }

        let job = self
            .parse_job_response(response, "get job", job_name)
            .await?
            .ok_or_else(|| {
                AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "azure".to_string(),
                    binding_name: self.binding_name.clone(),
                    field: "job".to_string(),
                    response_json: "Azure GetJob returned no job body".to_string(),
                })
            })?;

        let status = job
            .properties
            .as_ref()
            .and_then(|props| props.provisioning_state.as_deref())
            .map(|ps| Self::map_build_status(Some(ps)))
            .unwrap_or(BuildStatus::Queued);

        let end_time = if matches!(
            status,
            BuildStatus::Succeeded | BuildStatus::Failed | BuildStatus::Cancelled
        ) {
            Some(chrono::Utc::now().to_rfc3339())
        } else {
            None
        };

        Ok(BuildExecution {
            id: build_id.to_string(),
            status,
            start_time: Some(chrono::Utc::now().to_rfc3339()),
            end_time,
        })
    }

    async fn stop_build(&self, build_id: &str) -> Result<(), Error> {
        // Extract job name from build ID
        let job_name = if build_id.contains("/") {
            build_id.split('/').last().unwrap_or(build_id)
        } else {
            build_id
        };

        // For Azure Container Apps Jobs, stopping means deleting the job
        let url = self.build_job_url(job_name)?;
        let response = self.send_empty(Method::DELETE, url).await?;
        let _ = self
            .parse_job_response(response, "delete job", job_name)
            .await?;

        Ok(())
    }
}

impl Binding for AcaBuild {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<JobProperties>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<ManagedServiceIdentity>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    tags: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobProperties {
    #[serde(skip_serializing_if = "Option::is_none")]
    environment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    configuration: Option<JobConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    template: Option<JobTemplate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provisioning_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobConfiguration {
    trigger_type: String,
    replica_timeout: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    replica_retry_limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manual_trigger_config: Option<JobConfigurationManualTriggerConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    registries: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    secrets: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobConfigurationManualTriggerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    parallelism: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replica_completion_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobTemplate {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    containers: Vec<Container>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    init_containers: Vec<Container>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    volumes: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Container {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    command: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    env: Vec<EnvironmentVar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resources: Option<ContainerResources>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    probes: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContainerResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentVar {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedServiceIdentity {
    #[serde(rename = "type")]
    type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_assigned_identities: Option<HashMap<String, UserAssignedIdentity>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserAssignedIdentity {}

#[derive(Debug)]
struct StaticAzureAccessTokenCredential {
    token: String,
}

#[async_trait]
impl TokenCredential for StaticAzureAccessTokenCredential {
    async fn get_token(
        &self,
        scopes: &[&str],
        _options: Option<TokenRequestOptions<'_>>,
    ) -> azure_core::Result<AccessToken> {
        if scopes.is_empty() {
            return Err(azure_core::Error::with_message(
                azure_core::error::ErrorKind::Credential,
                "no scopes specified",
            ));
        }

        Ok(AccessToken::new(
            self.token.clone(),
            OffsetDateTime::now_utc() + AzureDuration::days(365),
        ))
    }
}

fn azure_credential_from_config(
    config: &AzureClientConfig,
) -> Result<Arc<dyn TokenCredential>, Error> {
    match &config.credentials {
        AzureCredentials::AccessToken { token } => Ok(Arc::new(StaticAzureAccessTokenCredential {
            token: token.clone(),
        })),
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => ClientSecretCredential::new(
            &config.tenant_id,
            client_id.clone(),
            Secret::new(client_secret.clone()),
            Some(ClientSecretCredentialOptions {
                client_options: azure_client_options(None),
            }),
        )
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "build.aca".to_string(),
            reason: "Failed to build official Azure service principal credentials".to_string(),
        }),
        AzureCredentials::WorkloadIdentity {
            client_id,
            tenant_id,
            federated_token_file,
            authority_host,
        } => WorkloadIdentityCredential::new(Some(WorkloadIdentityCredentialOptions {
            credential_options: ClientAssertionCredentialOptions {
                client_options: azure_client_options(Some(authority_host)),
            },
            client_id: Some(client_id.clone()),
            tenant_id: Some(tenant_id.clone()),
            token_file_path: Some(PathBuf::from(federated_token_file)),
        }))
        .map(|credential| credential as Arc<dyn TokenCredential>)
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "build.aca".to_string(),
            reason: "Failed to build official Azure workload identity credentials".to_string(),
        }),
        AzureCredentials::VmManagedIdentity {
            client_id,
            identity_endpoint,
        } => {
            if let Some(identity_endpoint) = identity_endpoint {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "build.aca".to_string(),
                    reason: format!(
                        "Official Azure ManagedIdentityCredential does not support per-config IMDS endpoint override '{}'; use the standard IMDS endpoint or provide an access token",
                        identity_endpoint
                    ),
                }));
            }

            ManagedIdentityCredential::new(Some(ManagedIdentityCredentialOptions {
                user_assigned_id: Some(UserAssignedId::ClientId(client_id.clone())),
                client_options: azure_client_options(None),
            }))
            .map(|credential| credential as Arc<dyn TokenCredential>)
            .into_alien_error()
            .context(ErrorData::BindingSetupFailed {
                binding_type: "build.aca".to_string(),
                reason: "Failed to build official Azure VM managed identity credentials"
                    .to_string(),
            })
        }
        AzureCredentials::ManagedIdentity {
            client_id,
            identity_endpoint,
            ..
        } => Err(AlienError::new(ErrorData::BindingSetupFailed {
            binding_type: "build.aca".to_string(),
            reason: format!(
                "Official Azure ManagedIdentityCredential cannot be constructed from explicit App Service identity endpoint '{}' for client '{}'; use workload identity, VM managed identity, or provide an access token",
                identity_endpoint, client_id
            ),
        })),
    }
}

fn azure_client_options(authority_host: Option<&str>) -> ClientOptions {
    let cloud = authority_host.map(|authority_host| {
        let mut custom = CustomConfiguration::default();
        custom.authority_host = authority_host.to_string();
        Arc::new(CloudConfiguration::Custom(custom))
    });

    ClientOptions {
        cloud,
        ..Default::default()
    }
}
