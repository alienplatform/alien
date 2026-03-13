use crate::{
    error::{map_cloud_client_error, Error, ErrorData},
    providers::utils::create_build_wrapper_script,
    traits::{Binding, Build},
};
use alien_core::{
    bindings::{BuildBinding, CodebuildBuildBinding},
    BuildConfig, BuildExecution, BuildStatus,
};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use std::collections::HashMap;

use alien_aws_clients::{
    codebuild::{
        BatchGetBuildsRequest, CodeBuildApi, CodeBuildClient, EnvironmentVariable,
        StartBuildRequest, StopBuildRequest,
    },
    AwsClientConfig,
};

/// AWS implementation of the `Build` trait using CodeBuild.
#[derive(Debug)]
pub struct CodebuildBuild {
    client: CodeBuildClient,
    binding_name: String,
    project_name: String,
    build_env_vars: HashMap<String, String>,
    monitoring: Option<alien_core::MonitoringConfig>,
}

impl CodebuildBuild {
    /// Creates a new AWS Build instance from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: BuildBinding,
        aws_config: &AwsClientConfig,
    ) -> Result<Self, Error> {
        let client =
            CodeBuildClient::new(crate::http_client::create_http_client(), aws_config.clone());

        // Extract values from binding
        let config = match binding {
            BuildBinding::Codebuild(config) => config,
            _ => {
                return Err(Error::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected CodeBuild binding, got different service type".to_string(),
                }));
            }
        };

        let project_name = config
            .project_name
            .into_value(&binding_name, "project_name")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract project_name from binding".to_string(),
            })?;

        let build_env_vars = config
            .build_env_vars
            .into_value(&binding_name, "build_env_vars")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract build_env_vars from binding".to_string(),
            })?;

        let monitoring = config
            .monitoring
            .into_value(&binding_name, "monitoring")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract monitoring from binding".to_string(),
            })?;

        Ok(Self {
            client,
            binding_name,
            project_name,
            build_env_vars,
            monitoring,
        })
    }

    /// Convert AWS CodeBuild status string to alien BuildStatus
    fn map_build_status(status: Option<&str>) -> BuildStatus {
        match status {
            Some("SUCCEEDED") => BuildStatus::Succeeded,
            Some("FAILED") | Some("FAULT") => BuildStatus::Failed,
            Some("STOPPED") => BuildStatus::Cancelled,
            Some("TIMED_OUT") => BuildStatus::TimedOut,
            Some("IN_PROGRESS") => BuildStatus::Running,
            Some("NOT_STARTED") => BuildStatus::Queued,
            _ => BuildStatus::Queued,
        }
    }
}

#[async_trait]
impl Build for CodebuildBuild {
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution, Error> {
        // Merge build config environment with binding environment variables
        // Build config environment takes precedence over binding environment
        let mut merged_env = self.build_env_vars.clone();
        merged_env.extend(config.environment);

        // Merge monitoring configuration - build config takes precedence over binding
        let monitoring = config.monitoring.or_else(|| self.monitoring.clone());

        // Convert environment variables
        let env_vars: Vec<EnvironmentVariable> = merged_env
            .iter()
            .map(|(key, value)| {
                EnvironmentVariable::builder()
                    .name(key.clone())
                    .value(value.clone())
                    .build()
            })
            .collect();

        // Create buildspec content with the unified wrapper script
        let wrapper_script = create_build_wrapper_script(&config.script, monitoring.as_ref());

        // Properly indent the wrapper script for YAML literal block
        let indented_wrapper_script = wrapper_script
            .lines()
            .map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else {
                    format!("        {}", line)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let buildspec_content = format!(
            r#"version: 0.2
phases:
  build:
    commands:
      - |
{}
"#,
            indented_wrapper_script
        );

        let start_build_request = StartBuildRequest::builder()
            .project_name(self.project_name.clone())
            .buildspec_override(buildspec_content)
            .environment_variables_override(env_vars)
            .build();

        let start_build_response =
            self.client
                .start_build(start_build_request)
                .await
                .map_err(|e| {
                    map_cloud_client_error(
                        e,
                        format!("Failed to start CodeBuild build '{}'", self.project_name),
                        None,
                    )
                })?;

        let build = &start_build_response.build;
        let build_id = build.id.as_deref().unwrap_or_default();
        let status = Self::map_build_status(build.build_status.as_deref());
        let start_time = build.start_time.map(|t| {
            chrono::DateTime::from_timestamp(t as i64, 0)
                .unwrap_or_default()
                .to_rfc3339()
        });

        Ok(BuildExecution {
            id: build_id.to_string(),
            status,
            start_time,
            end_time: None,
        })
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution, Error> {
        let batch_get_builds_request = BatchGetBuildsRequest::builder()
            .ids(vec![build_id.to_string()])
            .build();

        let batch_get_builds_response = self
            .client
            .batch_get_builds(batch_get_builds_request)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to get CodeBuild status for build '{}'", build_id),
                    Some(build_id.to_string()),
                )
            })?;

        let build = batch_get_builds_response
            .builds
            .as_ref()
            .and_then(|builds| builds.first())
            .ok_or_else(|| {
                AlienError::new(ErrorData::BuildOperationFailed {
                    binding_name: self.binding_name.clone(),
                    operation: format!("find build {}", build_id),
                })
            })?;

        let status = Self::map_build_status(build.build_status.as_deref());
        let start_time = build.start_time.map(|t| {
            chrono::DateTime::from_timestamp(t as i64, 0)
                .unwrap_or_default()
                .to_rfc3339()
        });
        let end_time = build.end_time.map(|t| {
            chrono::DateTime::from_timestamp(t as i64, 0)
                .unwrap_or_default()
                .to_rfc3339()
        });

        Ok(BuildExecution {
            id: build_id.to_string(),
            status,
            start_time,
            end_time,
        })
    }

    async fn stop_build(&self, build_id: &str) -> Result<(), Error> {
        let stop_build_request = StopBuildRequest::builder().id(build_id.to_string()).build();

        self.client
            .stop_build(stop_build_request)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to stop CodeBuild build '{}'", build_id),
                    Some(build_id.to_string()),
                )
            })?;

        Ok(())
    }
}

impl Binding for CodebuildBuild {}
