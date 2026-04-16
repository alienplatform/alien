use crate::{
    error::{Error, ErrorData},
    traits::{Binding, Build},
};
use alien_core::{BuildConfig, BuildExecution, BuildStatus};
use alien_error::{AlienError, Context};
use alien_k8s_clients::{
    kubernetes_client::KubernetesClient, KubernetesClientConfig, KubernetesClientConfigExt as _,
};
use async_trait::async_trait;
use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{Container, EnvVar, PodSpec, PodTemplateSpec, SecurityContext};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::BTreeMap;
use tracing::info;
use uuid::Uuid;

/// Kubernetes implementation of the `Build` trait.
///
/// This implementation creates Kubernetes Jobs to execute build operations
/// with proper sandboxing and security context.
#[derive(Debug)]
pub struct KubernetesBuild {
    binding_name: String,
    namespace: String,
    service_account_name: String,
    build_env_vars: std::collections::HashMap<String, String>,
    k8s_client: KubernetesClient,
}

impl KubernetesBuild {
    /// Creates a new Kubernetes build instance from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: alien_core::bindings::BuildBinding,
    ) -> Result<Self, Error> {
        let (namespace, service_account_name, build_env_vars) =
            Self::extract_binding_fields(&binding_name, binding)?;

        // Create Kubernetes client from environment
        let k8s_config = KubernetesClientConfig::from_std_env().await.context(
            ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to create Kubernetes configuration from environment".to_string(),
            },
        )?;

        let k8s_client =
            KubernetesClient::new(k8s_config)
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Failed to create Kubernetes client".to_string(),
                })?;

        Ok(Self {
            binding_name,
            namespace,
            service_account_name,
            build_env_vars,
            k8s_client,
        })
    }

    fn extract_binding_fields(
        binding_name: &str,
        binding: alien_core::bindings::BuildBinding,
    ) -> Result<(String, String, std::collections::HashMap<String, String>), Error> {
        let config = match binding {
            alien_core::bindings::BuildBinding::Kubernetes(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.to_string(),
                    reason: "Expected Kubernetes binding, got different service type".to_string(),
                }));
            }
        };

        let namespace = config
            .namespace
            .into_value(binding_name, "namespace")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to extract namespace from binding".to_string(),
            })?;

        let service_account_name = config
            .service_account_name
            .into_value(binding_name, "service_account_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to extract service_account_name from binding".to_string(),
            })?;

        let build_env_vars = config
            .build_env_vars
            .into_value(binding_name, "build_env_vars")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: "Failed to extract build_env_vars from binding".to_string(),
            })?;

        Ok((namespace, service_account_name, build_env_vars))
    }

    #[cfg(test)]
    async fn new_for_tests(
        binding_name: String,
        binding: alien_core::bindings::BuildBinding,
    ) -> Result<Self, Error> {
        let (namespace, service_account_name, build_env_vars) =
            Self::extract_binding_fields(&binding_name, binding)?;

        let k8s_config = KubernetesClientConfig::Manual {
            server_url: "https://example.invalid".to_string(),
            certificate_authority_data: None,
            insecure_skip_tls_verify: Some(true),
            client_certificate_data: None,
            client_key_data: None,
            token: None,
            username: None,
            password: None,
            namespace: None,
            additional_headers: std::collections::HashMap::new(),
        };
        let k8s_client =
            KubernetesClient::new(k8s_config)
                .await
                .context(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Failed to create Kubernetes client".to_string(),
                })?;

        Ok(Self {
            binding_name,
            namespace,
            service_account_name,
            build_env_vars,
            k8s_client,
        })
    }

    /// Creates a Kubernetes Job for build execution
    fn create_build_job(&self, config: &BuildConfig, build_id: &str) -> Job {
        // Convert environment variables to Kubernetes format
        let env_vars: Vec<EnvVar> = self
            .build_env_vars
            .iter()
            .chain(config.environment.iter())
            .map(|(key, value)| EnvVar {
                name: key.clone(),
                value: Some(value.clone()),
                ..Default::default()
            })
            .collect();

        // Create container with security context
        let container = Container {
            name: "build".to_string(),
            image: Some(config.image.clone()),
            command: Some(vec!["/bin/bash".to_string()]),
            args: Some(vec!["-c".to_string(), config.script.clone()]),
            env: Some(env_vars),
            security_context: Some(SecurityContext {
                allow_privilege_escalation: Some(false),
                read_only_root_filesystem: Some(true),
                run_as_non_root: Some(true),
                run_as_user: Some(65532),
                seccomp_profile: Some(k8s_openapi::api::core::v1::SeccompProfile {
                    type_: "RuntimeDefault".to_string(),
                    localhost_profile: None,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        // Create pod template with sandbox labels
        let pod_template = PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: Some({
                    let mut labels = BTreeMap::new();
                    labels.insert("alien.dev/build-sandbox".to_string(), "true".to_string());
                    labels.insert("alien.dev/build-id".to_string(), build_id.to_string());
                    labels.insert(
                        "app.kubernetes.io/managed-by".to_string(),
                        "alien".to_string(),
                    );
                    labels
                }),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                service_account_name: Some(self.service_account_name.clone()),
                restart_policy: Some("Never".to_string()),
                automount_service_account_token: Some(false),
                containers: vec![container],
                ..Default::default()
            }),
        };

        // Create job spec
        let job_spec = JobSpec {
            template: pod_template,
            backoff_limit: Some(0), // Don't retry failed builds
            active_deadline_seconds: Some(config.timeout_seconds as i64),
            ..Default::default()
        };

        // Create job metadata
        let metadata = ObjectMeta {
            name: Some(format!("build-{}", build_id)),
            namespace: Some(self.namespace.clone()),
            labels: Some({
                let mut labels = BTreeMap::new();
                labels.insert("alien.dev/build-id".to_string(), build_id.to_string());
                labels.insert(
                    "app.kubernetes.io/managed-by".to_string(),
                    "alien".to_string(),
                );
                labels
            }),
            ..Default::default()
        };

        Job {
            metadata,
            spec: Some(job_spec),
            ..Default::default()
        }
    }

    /// Maps Kubernetes job status to Alien build status
    fn map_job_status_to_build_status(&self, job: &Job) -> BuildStatus {
        if let Some(status) = &job.status {
            if let Some(_completion_time) = &status.completion_time {
                // Job has completed
                if let Some(succeeded) = status.succeeded {
                    if succeeded > 0 {
                        return BuildStatus::Succeeded;
                    }
                }
                if let Some(failed) = status.failed {
                    if failed > 0 {
                        return BuildStatus::Failed;
                    }
                }
                // If we have a completion time but no success/failure, it was cancelled
                return BuildStatus::Cancelled;
            }

            if let Some(_start_time) = &status.start_time {
                // Job has started but not completed
                return BuildStatus::Running;
            }
        }

        // Default to queued if we can't determine status
        BuildStatus::Queued
    }

    /// Extracts start time from job status
    fn extract_start_time(&self, job: &Job) -> Option<String> {
        job.status
            .as_ref()
            .and_then(|status| status.start_time.as_ref())
            .map(|time| time.0.to_rfc3339())
    }

    /// Extracts end time from job status
    fn extract_end_time(&self, job: &Job) -> Option<String> {
        job.status
            .as_ref()
            .and_then(|status| status.completion_time.as_ref())
            .map(|time| time.0.to_rfc3339())
    }
}

#[async_trait]
impl Binding for KubernetesBuild {}

#[async_trait]
impl Build for KubernetesBuild {
    async fn start_build(&self, config: BuildConfig) -> crate::error::Result<BuildExecution> {
        let build_id = Uuid::new_v4().to_string();
        let start_time = chrono::Utc::now().to_rfc3339();

        info!(
            binding_name = %self.binding_name,
            build_id = %build_id,
            namespace = %self.namespace,
            "Starting Kubernetes build job"
        );

        // Create the Kubernetes job
        let job = self.create_build_job(&config, &build_id);

        // Create the job in Kubernetes
        let _created_job = self
            .k8s_client
            .create_job(&self.namespace, &job)
            .await
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "create Kubernetes job".to_string(),
            })?;

        let execution = BuildExecution {
            id: build_id,
            status: BuildStatus::Queued,
            start_time: Some(start_time),
            end_time: None,
        };

        info!(
            binding_name = %self.binding_name,
            build_id = %execution.id,
            "Kubernetes build job created successfully"
        );

        Ok(execution)
    }

    async fn get_build_status(&self, build_id: &str) -> crate::error::Result<BuildExecution> {
        info!(
            binding_name = %self.binding_name,
            build_id = %build_id,
            "Getting Kubernetes build job status"
        );

        let job_name = format!("build-{}", build_id);

        // Get the job from Kubernetes
        let job = self
            .k8s_client
            .get_job(&self.namespace, &job_name)
            .await
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "get Kubernetes job".to_string(),
            })?;

        let status = self.map_job_status_to_build_status(&job);
        let start_time = self.extract_start_time(&job);
        let end_time = self.extract_end_time(&job);

        let execution = BuildExecution {
            id: build_id.to_string(),
            status,
            start_time,
            end_time,
        };

        info!(
            binding_name = %self.binding_name,
            build_id = %build_id,
            status = ?execution.status,
            "Retrieved Kubernetes build job status"
        );

        Ok(execution)
    }

    async fn stop_build(&self, build_id: &str) -> crate::error::Result<()> {
        info!(
            binding_name = %self.binding_name,
            build_id = %build_id,
            "Stopping Kubernetes build job"
        );

        let job_name = format!("build-{}", build_id);

        // Delete the job from Kubernetes
        self.k8s_client
            .delete_job(&self.namespace, &job_name)
            .await
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: "delete Kubernetes job".to_string(),
            })?;

        info!(
            binding_name = %self.binding_name,
            build_id = %build_id,
            "Kubernetes build job stopped successfully"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::{BindingValue, BuildBinding};
    use chrono::TimeZone as _;

    #[tokio::test]
    async fn test_kubernetes_build_creation() {
        let binding = BuildBinding::kubernetes(
            "test-namespace",
            "test-sa",
            std::collections::HashMap::new(),
        );

        let kubernetes_build = KubernetesBuild::new_for_tests("test-binding".to_string(), binding)
            .await
            .unwrap();

        assert_eq!(kubernetes_build.namespace, "test-namespace");
        assert_eq!(kubernetes_build.service_account_name, "test-sa");
        assert!(kubernetes_build.build_env_vars.is_empty());
    }

    #[tokio::test]
    async fn test_create_build_job() {
        let binding = BuildBinding::kubernetes(
            "test-namespace",
            "test-sa",
            std::collections::HashMap::new(),
        );

        let kubernetes_build = KubernetesBuild::new_for_tests("test-binding".to_string(), binding)
            .await
            .unwrap();

        let config = BuildConfig {
            image: "ubuntu:20.04".to_string(),
            script: "echo 'Hello World'".to_string(),
            environment: std::collections::HashMap::new(),
            timeout_seconds: 300,
            compute_type: alien_core::ComputeType::Medium,
            monitoring: None,
        };

        let build_id = "test-build-123";
        let job = kubernetes_build.create_build_job(&config, build_id);

        assert_eq!(job.metadata.name.as_ref().unwrap(), "build-test-build-123");
        assert_eq!(job.metadata.namespace.as_ref().unwrap(), "test-namespace");

        let container = &job
            .spec
            .as_ref()
            .unwrap()
            .template
            .spec
            .as_ref()
            .unwrap()
            .containers[0];
        assert_eq!(container.name, "build");
        assert_eq!(container.image.as_ref().unwrap(), "ubuntu:20.04");
        assert_eq!(
            container.command.as_ref().unwrap(),
            &vec!["/bin/bash".to_string()]
        );
        assert_eq!(
            container.args.as_ref().unwrap(),
            &vec!["-c".to_string(), "echo 'Hello World'".to_string()]
        );

        let security_context = container.security_context.as_ref().unwrap();
        assert_eq!(security_context.allow_privilege_escalation, Some(false));
        assert_eq!(security_context.read_only_root_filesystem, Some(true));
        assert_eq!(security_context.run_as_non_root, Some(true));
        assert_eq!(security_context.run_as_user, Some(65532));
    }

    #[tokio::test]
    async fn test_map_job_status_to_build_status() {
        let binding = BuildBinding::kubernetes(
            "test-namespace",
            "test-sa",
            std::collections::HashMap::new(),
        );

        let kubernetes_build = KubernetesBuild::new_for_tests("test-binding".to_string(), binding)
            .await
            .unwrap();

        // Test queued status (no status)
        let job = Job::default();
        assert_eq!(
            kubernetes_build.map_job_status_to_build_status(&job),
            BuildStatus::Queued
        );

        // Test running status (has start time, no completion time)
        let mut job = Job::default();
        job.status = Some(k8s_openapi::api::batch::v1::JobStatus {
            start_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap(),
            )),
            ..Default::default()
        });
        assert_eq!(
            kubernetes_build.map_job_status_to_build_status(&job),
            BuildStatus::Running
        );

        // Test succeeded status
        let mut job = Job::default();
        job.status = Some(k8s_openapi::api::batch::v1::JobStatus {
            start_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap(),
            )),
            completion_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc.with_ymd_and_hms(2023, 1, 1, 1, 0, 0).unwrap(),
            )),
            succeeded: Some(1),
            ..Default::default()
        });
        assert_eq!(
            kubernetes_build.map_job_status_to_build_status(&job),
            BuildStatus::Succeeded
        );

        // Test failed status
        let mut job = Job::default();
        job.status = Some(k8s_openapi::api::batch::v1::JobStatus {
            start_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc.with_ymd_and_hms(2023, 1, 1, 0, 0, 0).unwrap(),
            )),
            completion_time: Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                chrono::Utc.with_ymd_and_hms(2023, 1, 1, 1, 0, 0).unwrap(),
            )),
            failed: Some(1),
            ..Default::default()
        });
        assert_eq!(
            kubernetes_build.map_job_status_to_build_status(&job),
            BuildStatus::Failed
        );
    }
}
