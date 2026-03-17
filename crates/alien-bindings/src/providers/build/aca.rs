use crate::{
    error::{map_cloud_client_error, Error, ErrorData},
    providers::build::script::create_build_wrapper_script,
    traits::{Binding, Build},
};
use alien_core::{
    bindings::{AcaBuildBinding, BuildBinding},
    BuildConfig, BuildExecution, BuildStatus, ComputeType,
};
use alien_error::Context;
use async_trait::async_trait;
use std::collections::HashMap;

use alien_azure_clients::{
    container_apps::{AzureContainerAppsClient, ContainerAppsApi},
    long_running_operation::OperationResult,
    models::jobs::{
        Container as JobContainer, ContainerResources as JobContainerResources,
        EnvironmentVar as JobEnvironmentVar, Job, JobConfiguration,
        JobConfigurationManualTriggerConfig, JobConfigurationTriggerType, JobProperties,
        JobTemplate, ManagedServiceIdentity, ManagedServiceIdentityType, Parallelism,
        ReplicaCompletionCount, UserAssignedIdentities, UserAssignedIdentity,
    },
    AzureClientConfig,
};
use alien_client_core::ErrorData as CloudClientErrorData;

/// Azure implementation of the `Build` trait using Container Apps Jobs.
#[derive(Debug)]
pub struct AcaBuild {
    client: AzureContainerAppsClient,
    binding_name: String,
    resource_prefix: String,
    #[allow(dead_code)]
    subscription_id: String,
    resource_group_name: String,
    managed_environment_id: String,
    managed_identity_id: Option<String>,
    build_env_vars: HashMap<String, String>,
    region: String,
    monitoring: Option<alien_core::MonitoringConfig>,
}

impl AcaBuild {
    /// Creates a new Azure Build instance from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: BuildBinding,
        azure_config: &AzureClientConfig,
    ) -> Result<Self, Error> {
        let client = AzureContainerAppsClient::new(
            crate::http_client::create_http_client(),
            azure_config.clone(),
        );

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
    fn map_compute_resources(compute_type: &ComputeType) -> JobContainerResources {
        match compute_type {
            ComputeType::Small => JobContainerResources {
                cpu: Some(0.25),
                memory: Some("0.5Gi".to_string()),
                ephemeral_storage: None,
            },
            ComputeType::Medium => JobContainerResources {
                cpu: Some(0.5),
                memory: Some("1Gi".to_string()),
                ephemeral_storage: None,
            },
            ComputeType::Large => JobContainerResources {
                cpu: Some(1.0),
                memory: Some("2Gi".to_string()),
                ephemeral_storage: None,
            },
            ComputeType::XLarge => JobContainerResources {
                cpu: Some(2.0),
                memory: Some("4Gi".to_string()),
                ephemeral_storage: None,
            },
        }
    }

    /// Convert Azure Container Apps Job status to alien BuildStatus
    fn map_build_status(status: Option<&str>) -> BuildStatus {
        match status {
            Some("Succeeded") => BuildStatus::Succeeded,
            Some("Failed") => BuildStatus::Failed,
            Some("Cancelled") => BuildStatus::Cancelled,
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
        let azure_env_vars: Vec<JobEnvironmentVar> = merged_environment
            .iter()
            .map(|(key, value)| JobEnvironmentVar {
                name: Some(key.clone()),
                value: Some(value.clone()),
                secret_ref: None,
            })
            .collect();

        // Create the job container with the unified wrapper script
        let container_script = create_build_wrapper_script(&config.script, monitoring.as_ref());

        let job_container = JobContainer {
            name: Some("build-container".to_string()),
            image: Some(config.image),
            command: vec!["bash".to_string()],
            args: vec!["-c".to_string(), container_script],
            env: azure_env_vars,
            resources: Some(Self::map_compute_resources(&config.compute_type)),
            probes: vec![],
            volume_mounts: vec![],
        };

        // Create job template
        let job_template = JobTemplate {
            containers: vec![job_container],
            init_containers: vec![],
            volumes: vec![],
        };

        // Create job configuration with manual trigger
        let job_configuration = JobConfiguration {
            trigger_type: JobConfigurationTriggerType::Manual,
            replica_timeout: config.timeout_seconds as i32,
            replica_retry_limit: Some(1),
            manual_trigger_config: Some(JobConfigurationManualTriggerConfig {
                parallelism: Some(Parallelism(1)),
                replica_completion_count: Some(ReplicaCompletionCount(1)),
            }),
            registries: vec![],
            secrets: vec![],
            event_trigger_config: None,
            schedule_trigger_config: None,
            identity_settings: vec![],
        };

        // Create job properties
        let job_properties = JobProperties {
            environment_id: Some(self.managed_environment_id.clone()),
            configuration: Some(job_configuration),
            template: Some(job_template),
            workload_profile_name: None,
            provisioning_state: None,
            event_stream_endpoint: None,
            outbound_ip_addresses: vec![],
        };

        // Create managed service identity if we have a managed identity ID
        let identity =
            self.managed_identity_id
                .as_ref()
                .map(|identity_id| ManagedServiceIdentity {
                    type_: ManagedServiceIdentityType::UserAssigned,
                    user_assigned_identities: Some(UserAssignedIdentities(
                        std::collections::HashMap::from([(
                            identity_id.clone(),
                            UserAssignedIdentity::default(),
                        )]),
                    )),
                    principal_id: None,
                    tenant_id: None,
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
            system_data: None,
        };

        let operation_result = self
            .client
            .create_or_update_job(&self.resource_group_name, &job_name, &job)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to create Azure Container Apps job '{}'", job_name),
                    None,
                )
            })?;

        let build_id = match operation_result {
            OperationResult::Completed(created_job) => {
                created_job.id.unwrap_or_else(|| job_name.clone())
            }
            OperationResult::LongRunning(_) => {
                // For long-running operations, we'll use the job name as ID
                job_name.clone()
            }
        };

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

        let job_result = self
            .client
            .get_job(&self.resource_group_name, job_name)
            .await;

        match job_result {
            Ok(job) => {
                let status = job
                    .properties
                    .as_ref()
                    .and_then(|props| props.provisioning_state.as_ref())
                    .map(|ps| Self::map_build_status(Some(&format!("{:?}", ps))))
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
            Err(err) => {
                // Check if this is a "resource not found" error (job was deleted/stopped)
                if let Some(CloudClientErrorData::RemoteResourceNotFound { .. }) = &err.error {
                    // Job was deleted (stopped), return cancelled status
                    Ok(BuildExecution {
                        id: build_id.to_string(),
                        status: BuildStatus::Cancelled,
                        start_time: Some(chrono::Utc::now().to_rfc3339()),
                        end_time: Some(chrono::Utc::now().to_rfc3339()),
                    })
                } else {
                    // For other errors, propagate them
                    Err(map_cloud_client_error(
                        err,
                        format!(
                            "Failed to get Azure Container Apps job status for '{}'",
                            job_name
                        ),
                        Some(build_id.to_string()),
                    ))
                }
            }
        }
    }

    async fn stop_build(&self, build_id: &str) -> Result<(), Error> {
        // Extract job name from build ID
        let job_name = if build_id.contains("/") {
            build_id.split('/').last().unwrap_or(build_id)
        } else {
            build_id
        };

        // For Azure Container Apps Jobs, stopping means deleting the job
        self.client
            .delete_job(&self.resource_group_name, job_name)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to stop Azure Container Apps job '{}'", job_name),
                    Some(build_id.to_string()),
                )
            })?;

        Ok(())
    }
}

impl Binding for AcaBuild {}
