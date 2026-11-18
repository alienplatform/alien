use crate::{
    error::{binding_env_var, map_cloud_client_error, Error, ErrorData},
    providers::build::script::create_build_wrapper_script,
    traits::{Binding, Build},
};
use alien_core::{bindings::BuildBinding, BuildConfig, BuildExecution, BuildStatus, ComputeType};
use alien_error::Context;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use alien_azure_clients::{
    container_apps::{AzureContainerAppsClient, ContainerAppsApi},
    long_running_operation::OperationResult,
    models::jobs::{
        Container as JobContainer, ContainerResources as JobContainerResources,
        EnvironmentVar as JobEnvironmentVar, Job, JobConfiguration,
        JobConfigurationManualTriggerConfig, JobConfigurationTriggerType,
        JobExecutionPropertiesStatus, JobProperties, JobTemplate, ManagedServiceIdentity,
        ManagedServiceIdentityType, Parallelism, ReplicaCompletionCount, UserAssignedIdentities,
        UserAssignedIdentity,
    },
    AzureClientConfig, AzureTokenCache,
};
use alien_client_core::ErrorData as CloudClientErrorData;

/// How often, and how many times, to read a job whose create is still running.
const PROVISION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
const PROVISION_POLL_LIMIT: u32 = 90;

/// Azure implementation of the `Build` trait using Container Apps Jobs.
#[derive(Debug)]
pub struct AcaBuild {
    client: Arc<dyn ContainerAppsApi>,
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
        let client: Arc<dyn ContainerAppsApi> = Arc::new(AzureContainerAppsClient::new(
            crate::http_client::create_http_client(),
            AzureTokenCache::new(azure_config.clone()),
        ));

        // Extract values from binding
        let config = match binding {
            BuildBinding::Aca(config) => config,
            _ => {
                return Err(Error::new(ErrorData::BindingConfigInvalid {
                    env_var: binding_env_var(&binding_name),
                    binding_name: binding_name.clone(),
                    reason: "Expected ACA binding, got different service type".to_string(),
                }));
            }
        };

        let managed_environment_id = config
            .managed_environment_id
            .into_value(&binding_name, "managed_environment_id")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&binding_name),
                binding_name: binding_name.clone(),
                reason: "Failed to extract managed_environment_id from binding".to_string(),
            })?;

        let resource_group_name = config
            .resource_group_name
            .into_value(&binding_name, "resource_group_name")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&binding_name),
                binding_name: binding_name.clone(),
                reason: "Failed to extract resource_group_name from binding".to_string(),
            })?;

        let build_env_vars = config
            .build_env_vars
            .into_value(&binding_name, "build_env_vars")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&binding_name),
                binding_name: binding_name.clone(),
                reason: "Failed to extract build_env_vars from binding".to_string(),
            })?;

        let managed_identity_id = config
            .managed_identity_id
            .into_value(&binding_name, "managed_identity_id")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&binding_name),
                binding_name: binding_name.clone(),
                reason: "Failed to extract managed_identity_id from binding".to_string(),
            })?;

        let resource_prefix = config
            .resource_prefix
            .into_value(&binding_name, "resource_prefix")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&binding_name),
                binding_name: binding_name.clone(),
                reason: "Failed to extract resource_prefix from binding".to_string(),
            })?;

        let monitoring = config
            .monitoring
            .into_value(&binding_name, "monitoring")
            .context(ErrorData::BindingConfigInvalid {
                env_var: binding_env_var(&binding_name),
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
                    env_var: binding_env_var(&binding_name_clone),
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

    /// Convert the status of a Container Apps job execution to alien BuildStatus.
    fn map_execution_status(status: Option<&JobExecutionPropertiesStatus>) -> BuildStatus {
        match status {
            Some(JobExecutionPropertiesStatus::Succeeded) => BuildStatus::Succeeded,
            Some(JobExecutionPropertiesStatus::Failed | JobExecutionPropertiesStatus::Degraded) => {
                BuildStatus::Failed
            }
            Some(JobExecutionPropertiesStatus::Stopped) => BuildStatus::Cancelled,
            Some(
                JobExecutionPropertiesStatus::Running | JobExecutionPropertiesStatus::Processing,
            ) => BuildStatus::Running,
            Some(JobExecutionPropertiesStatus::Unknown) | None => BuildStatus::Queued,
        }
    }

    /// A build id is the execution id `.../jobs/<job>/executions/<execution>` (or the tail
    /// `<job>/executions/<execution>`). A bare job name, from an older build, has no execution.
    fn split_build_id(build_id: &str) -> (&str, Option<&str>) {
        let parts: Vec<&str> = build_id.split('/').collect();
        match parts.iter().rposition(|p| *p == "executions") {
            Some(i) if i >= 1 && i + 1 < parts.len() => (parts[i - 1], Some(parts[i + 1])),
            _ => (parts.last().copied().unwrap_or(build_id), None),
        }
    }

    /// The execution of a job that was just started: the job is new, so it has exactly one.
    async fn find_started_execution(&self, job_name: &str) -> Option<String> {
        match self
            .client
            .list_job_executions(&self.resource_group_name, job_name)
            .await
        {
            Ok(executions) => executions.into_iter().find_map(|execution| execution.name),
            Err(error) => {
                tracing::warn!(job = %job_name, error = %error, "Could not list the executions of a job that was started");
                None
            }
        }
    }

    /// Polls the job's provisioning state until the create has finished. A create that ends in
    /// Failed or Canceled is an error; one still running after the limit is too.
    async fn wait_until_provisioned(&self, job_name: &str) -> Result<(), Error> {
        for _ in 0..PROVISION_POLL_LIMIT {
            let job = self
                .client
                .get_job(&self.resource_group_name, job_name)
                .await
                .map_err(|e| {
                    map_cloud_client_error(
                        e,
                        format!(
                            "Failed to read Azure Container Apps job '{}' while it is created",
                            job_name
                        ),
                        None,
                    )
                })?;
            let state = job
                .properties
                .as_ref()
                .and_then(|props| props.provisioning_state.as_ref())
                .map(|state| format!("{state:?}"));
            match state.as_deref() {
                Some("Succeeded") => return Ok(()),
                Some(state @ ("Failed" | "Canceled" | "Cancelled")) => {
                    return Err(Error::new(ErrorData::BuildOperationFailed {
                        binding_name: self.binding_name.clone(),
                        operation: format!(
                            "create Azure Container Apps job '{job_name}': provisioning {state}"
                        ),
                    }));
                }
                _ => tokio::time::sleep(PROVISION_POLL_INTERVAL).await,
            }
        }
        Err(Error::new(ErrorData::BuildOperationFailed {
            binding_name: self.binding_name.clone(),
            operation: format!(
                "create Azure Container Apps job '{job_name}': still provisioning after {} seconds",
                PROVISION_POLL_LIMIT as u64 * PROVISION_POLL_INTERVAL.as_secs()
            ),
        }))
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

        // From here the job exists but has not run. If it cannot be provisioned or started it
        // would stay behind, unstarted and unnamed by any build id, so delete it before failing.
        let started_execution: Result<Option<String>, Error> = async {
            // A long-running create has not finished when it returns: starting the job before it has
            // provisioned is refused. Wait for the job to be provisioned, then start it.
            if let OperationResult::LongRunning(_) = operation_result {
                self.wait_until_provisioned(&job_name).await?;
            }

            // The job has a Manual trigger: creating it runs nothing. Start one execution now. Its id
            // is the build id, and its status is what get_build_status reports.
            let started = self
                .client
                .start_job(&self.resource_group_name, &job_name)
                .await
                .map_err(|e| {
                    map_cloud_client_error(
                        e,
                        format!("Failed to start Azure Container Apps job '{}'", job_name),
                        None,
                    )
                })?;
            let execution_name = match started {
                OperationResult::Completed(execution) => execution.name,
                OperationResult::LongRunning(_) => None,
            };
            // The start was accepted: an execution may be running even when it is not listed yet,
            // so the job is never deleted from here on. A build id without an execution names the
            // job, and the status call finds the execution once it is listed.
            let execution_name = match execution_name {
                Some(name) => Some(name),
                None => self.find_started_execution(&job_name).await,
            };
            Ok(execution_name)
        }
        .await;
        let execution_name = match started_execution {
            Ok(name) => name,
            Err(error) => {
                if let Err(cleanup) = self
                    .client
                    .delete_job(&self.resource_group_name, &job_name)
                    .await
                {
                    tracing::warn!(job = %job_name, error = %cleanup, "Failed to delete the job of a build that did not start");
                }
                return Err(error);
            }
        };

        Ok(BuildExecution {
            id: match execution_name {
                Some(execution_name) => format!("{job_name}/executions/{execution_name}"),
                None => job_name,
            },
            status: BuildStatus::Queued,
            start_time: Some(chrono::Utc::now().to_rfc3339()),
            end_time: None,
        })
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution, Error> {
        let (job_name, execution_name) = Self::split_build_id(build_id);

        let result = async {
            let execution_name = match execution_name {
                Some(name) => name.to_string(),
                None => self
                    .client
                    .list_job_executions(&self.resource_group_name, job_name)
                    .await?
                    .into_iter()
                    .find_map(|execution| execution.name)
                    .unwrap_or_default(),
            };
            if execution_name.is_empty() {
                return Ok(None);
            }
            self.client
                .get_job_execution(&self.resource_group_name, job_name, &execution_name)
                .await
                .map(Some)
        }
        .await;

        match result {
            Ok(execution) => {
                let properties = execution.and_then(|e| e.properties);
                let status =
                    Self::map_execution_status(properties.as_ref().and_then(|p| p.status.as_ref()));
                let end_time = if matches!(
                    status,
                    BuildStatus::Succeeded | BuildStatus::Failed | BuildStatus::Cancelled
                ) {
                    Some(
                        properties
                            .as_ref()
                            .and_then(|p| p.end_time.clone())
                            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                    )
                } else {
                    None
                };
                if end_time.is_some() {
                    // A finished job has nothing left to run. The build grants cover every job
                    // in the resource group, so a job that stays behind is a job another holder
                    // could start or change. This call already read the terminal status; a later
                    // call finds no job and reports the build as cancelled.
                    if let Err(error) = self
                        .client
                        .delete_job(&self.resource_group_name, job_name)
                        .await
                    {
                        tracing::warn!(job = %job_name, error = %error, "Failed to delete the job of a finished build");
                    }
                }
                Ok(BuildExecution {
                    id: build_id.to_string(),
                    status,
                    start_time: properties
                        .and_then(|p| p.start_time)
                        .or_else(|| Some(chrono::Utc::now().to_rfc3339())),
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
                    Err(map_cloud_client_error(
                        err,
                        format!(
                            "Failed to get Azure Container Apps job execution status for '{}'",
                            build_id
                        ),
                        Some(build_id.to_string()),
                    ))
                }
            }
        }
    }

    async fn stop_build(&self, build_id: &str) -> Result<(), Error> {
        let (job_name, _) = Self::split_build_id(build_id);

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

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::container_apps::MockContainerAppsApi;
    use std::sync::Mutex;

    fn build(client: MockContainerAppsApi) -> AcaBuild {
        AcaBuild {
            client: Arc::new(client),
            binding_name: "b".to_string(),
            resource_prefix: "p".to_string(),
            subscription_id: "sub".to_string(),
            resource_group_name: "rg".to_string(),
            managed_environment_id: "env".to_string(),
            managed_identity_id: None,
            build_env_vars: HashMap::new(),
            region: "eastus".to_string(),
            monitoring: None,
        }
    }

    fn config() -> BuildConfig {
        BuildConfig {
            image: "ubuntu:22.04".to_string(),
            script: "true".to_string(),
            environment: HashMap::new(),
            timeout_seconds: 30,
            compute_type: ComputeType::Small,
            monitoring: None,
        }
    }

    /// A Manual-trigger job runs nothing until it is started: `start_build` must create the job
    /// and then start it, in that order, on the job it created.
    #[tokio::test]
    async fn start_build_creates_the_job_and_then_starts_it() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut client = MockContainerAppsApi::new();
        let log = calls.clone();
        client
            .expect_create_or_update_job()
            .times(1)
            .returning(move |_, name, _| {
                log.lock().unwrap().push(format!("create {name}"));
                let job = serde_json::from_str(r#"{"location":"eastus"}"#).unwrap();
                Ok(OperationResult::Completed(job))
            });
        let log = calls.clone();
        client
            .expect_start_job()
            .times(1)
            .returning(move |rg, name| {
                log.lock().unwrap().push(format!("start {rg} {name}"));
                let execution = serde_json::from_str(r#"{"name":"j-abc"}"#).unwrap();
                Ok(OperationResult::Completed(execution))
            });

        let execution = build(client).start_build(config()).await.unwrap();
        assert_eq!(execution.status, BuildStatus::Queued);

        let calls = calls.lock().unwrap();
        assert!(
            execution.id.ends_with("/executions/j-abc"),
            "the build id names the started execution: {}",
            execution.id
        );
        assert_eq!(calls.len(), 2, "{calls:?}");
        let job = calls[0].strip_prefix("create ").expect("create first");
        assert!(job.starts_with("build-"), "{job}");
        assert_eq!(calls[1], format!("start rg {job}"));
    }

    /// A long-running create must finish before the job is started: the job is read until it is
    /// provisioned, and only then started.
    #[tokio::test(start_paused = true)]
    async fn start_build_waits_for_a_long_running_create_before_starting() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut client = MockContainerAppsApi::new();
        let log = calls.clone();
        client
            .expect_create_or_update_job()
            .times(1)
            .returning(move |_, _, _| {
                log.lock().unwrap().push("create".to_string());
                Ok(OperationResult::LongRunning(
                    alien_azure_clients::long_running_operation::LongRunningOperation {
                        url: "https://management.azure.com/op".to_string(),
                        retry_after: None,
                        location_url: None,
                    },
                ))
            });
        let log = calls.clone();
        let reads = Arc::new(Mutex::new(0));
        client.expect_get_job().returning(move |_, _| {
            let mut reads = reads.lock().unwrap();
            *reads += 1;
            let state = if *reads < 3 {
                "InProgress"
            } else {
                "Succeeded"
            };
            log.lock().unwrap().push(format!("get {state}"));
            Ok(serde_json::from_str(&format!(
                r#"{{"location":"eastus","properties":{{"provisioningState":"{state}"}}}}"#
            ))
            .unwrap())
        });
        let log = calls.clone();
        client.expect_start_job().times(1).returning(move |_, _| {
            log.lock().unwrap().push("start".to_string());
            Ok(OperationResult::Completed(
                serde_json::from_str(r#"{"name":"j-abc"}"#).unwrap(),
            ))
        });

        build(client).start_build(config()).await.unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "create",
                "get InProgress",
                "get InProgress",
                "get Succeeded",
                "start"
            ]
        );
    }

    /// A create that ends Failed is an error and the job is never started.
    #[tokio::test(start_paused = true)]
    async fn start_build_does_not_start_a_job_whose_create_failed() {
        let mut client = MockContainerAppsApi::new();
        client.expect_create_or_update_job().returning(|_, _, _| {
            Ok(OperationResult::LongRunning(
                alien_azure_clients::long_running_operation::LongRunningOperation {
                    url: "https://management.azure.com/op".to_string(),
                    retry_after: None,
                    location_url: None,
                },
            ))
        });
        client.expect_get_job().returning(|_, _| {
            Ok(serde_json::from_str(
                r#"{"location":"eastus","properties":{"provisioningState":"Failed"}}"#,
            )
            .unwrap())
        });
        client.expect_start_job().times(0);
        // The unstarted job is deleted, not left behind.
        client
            .expect_delete_job()
            .times(1)
            .returning(|_, _| Ok(OperationResult::Completed(())));

        assert!(build(client).start_build(config()).await.is_err());
    }

    /// A start that is refused deletes the job it created.
    #[tokio::test]
    async fn start_build_deletes_the_job_when_the_start_fails() {
        let mut client = MockContainerAppsApi::new();
        client.expect_create_or_update_job().returning(|_, _, _| {
            Ok(OperationResult::Completed(
                serde_json::from_str(r#"{"location":"eastus"}"#).unwrap(),
            ))
        });
        client.expect_start_job().returning(|_, _| {
            Err(alien_client_core::Error::new(
                alien_client_core::ErrorData::RemoteAccessDenied {
                    resource_type: "job".to_string(),
                    resource_name: "j".to_string(),
                },
            ))
        });
        client
            .expect_delete_job()
            .times(1)
            .returning(|_, _| Ok(OperationResult::Completed(())));

        assert!(build(client).start_build(config()).await.is_err());
    }

    /// A start that returns no execution (a long-running start) is resolved by listing the new
    /// job's executions.
    #[tokio::test]
    async fn start_build_finds_the_execution_when_start_returns_none() {
        let mut client = MockContainerAppsApi::new();
        client.expect_create_or_update_job().returning(|_, _, _| {
            Ok(OperationResult::Completed(
                serde_json::from_str(r#"{"location":"eastus"}"#).unwrap(),
            ))
        });
        client.expect_start_job().returning(|_, _| {
            Ok(OperationResult::Completed(
                serde_json::from_str("{}").unwrap(),
            ))
        });
        client
            .expect_list_job_executions()
            .returning(|_, _| Ok(vec![serde_json::from_str(r#"{"name":"j-xyz"}"#).unwrap()]));
        let started = build(client).start_build(config()).await.unwrap();
        assert!(started.id.ends_with("/executions/j-xyz"), "{}", started.id);
    }

    /// A start the cloud accepted may be running even when no execution is listed yet. The job is
    /// never deleted then: the build id names the job, and the status call finds the execution.
    #[tokio::test]
    async fn an_accepted_start_with_no_listed_execution_keeps_the_job() {
        let mut client = MockContainerAppsApi::new();
        client.expect_create_or_update_job().returning(|_, _, _| {
            Ok(OperationResult::Completed(
                serde_json::from_str(r#"{"location":"eastus"}"#).unwrap(),
            ))
        });
        client.expect_start_job().returning(|_, _| {
            Ok(OperationResult::Completed(
                serde_json::from_str("{}").unwrap(),
            ))
        });
        client
            .expect_list_job_executions()
            .returning(|_, _| Ok(vec![]));
        client.expect_delete_job().times(0);
        let started = build(client).start_build(config()).await.unwrap();
        assert!(!started.id.contains("/executions/"), "{}", started.id);
    }

    fn status_of_execution(status: &'static str) -> MockContainerAppsApi {
        status_of_execution_counting(status, Arc::new(std::sync::atomic::AtomicUsize::new(0)))
    }

    fn status_of_execution_counting(
        status: &'static str,
        deletes: Arc<std::sync::atomic::AtomicUsize>,
    ) -> MockContainerAppsApi {
        let mut client = MockContainerAppsApi::new();
        client
            .expect_delete_job()
            .withf(|rg, job| rg == "rg" && job == "build-p-1")
            .returning(move |_, _| {
                deletes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(OperationResult::Completed(()))
            });
        client
            .expect_get_job_execution()
            .withf(|rg, job, exec| rg == "rg" && job == "build-p-1" && exec == "e1")
            .returning(move |_, _, _| {
                Ok(serde_json::from_str(&format!(
                    r#"{{"name":"e1","properties":{{"status":"{status}"}}}}"#
                ))
                .unwrap())
            });
        // The job's own provisioning state is Succeeded the whole time: it must not be read.
        client.expect_get_job().times(0);
        client
    }

    /// The status is the execution's, not the job's: a job that is provisioned while its
    /// execution still runs reports Running, and a Failed execution reports Failed.
    #[tokio::test]
    async fn get_build_status_reads_the_execution_status() {
        for (azure, want) in [
            ("Running", BuildStatus::Running),
            ("Processing", BuildStatus::Running),
            ("Succeeded", BuildStatus::Succeeded),
            ("Failed", BuildStatus::Failed),
            ("Stopped", BuildStatus::Cancelled),
        ] {
            let got = build(status_of_execution(azure))
                .get_build_status("build-p-1/executions/e1")
                .await
                .unwrap();
            assert_eq!(got.status, want, "{azure}");
            assert_eq!(
                got.end_time.is_some(),
                want != BuildStatus::Running,
                "{azure}"
            );
        }
    }

    /// A finished build's job is deleted (nothing else removes it, and the grants cover every
    /// job in the group); a running build's job is left alone.
    #[tokio::test]
    async fn a_finished_build_deletes_its_job_and_a_running_one_does_not() {
        for (azure, deleted) in [
            ("Running", 0),
            ("Succeeded", 1),
            ("Failed", 1),
            ("Stopped", 1),
        ] {
            let deletes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            build(status_of_execution_counting(azure, deletes.clone()))
                .get_build_status("build-p-1/executions/e1")
                .await
                .unwrap();
            assert_eq!(
                deletes.load(std::sync::atomic::Ordering::SeqCst),
                deleted,
                "{azure}"
            );
        }
    }

    /// A full execution resource id works as a build id.
    #[tokio::test]
    async fn get_build_status_accepts_a_full_execution_resource_id() {
        let id = "/subscriptions/s/resourceGroups/rg/providers/Microsoft.App/jobs/build-p-1/executions/e1";
        let got = build(status_of_execution("Running"))
            .get_build_status(id)
            .await
            .unwrap();
        assert_eq!(got.status, BuildStatus::Running);
    }
}
