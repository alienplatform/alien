use crate::{
    error::{Error, ErrorData},
    providers::build::script::create_build_wrapper_script,
    traits::{Binding, Build},
};
use alien_core::{bindings::BuildBinding, BuildConfig, BuildExecution, BuildStatus};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use aws_sdk_codebuild::{primitives::DateTimeFormat, types::EnvironmentVariable};
use std::{collections::HashMap, fmt::Debug, sync::Arc};

/// Minimal CodeBuild operations required by the build binding.
#[async_trait]
pub trait CodeBuildClient: Debug + Send + Sync {
    /// Start a build.
    async fn start_build(
        &self,
        project_name: &str,
        buildspec_override: String,
        environment: Vec<(String, String)>,
    ) -> Result<BuildExecution, Error>;

    /// Get a build by ID.
    async fn get_build(&self, build_id: &str) -> Result<BuildExecution, Error>;

    /// Stop a build by ID.
    async fn stop_build(&self, build_id: &str) -> Result<(), Error>;
}

#[async_trait]
impl CodeBuildClient for aws_sdk_codebuild::Client {
    async fn start_build(
        &self,
        project_name: &str,
        buildspec_override: String,
        environment: Vec<(String, String)>,
    ) -> Result<BuildExecution, Error> {
        let env_vars = environment
            .into_iter()
            .map(|(name, value)| {
                EnvironmentVariable::builder()
                    .name(name)
                    .value(value)
                    .build()
            })
            .collect::<Result<Vec<_>, _>>()
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: project_name.to_string(),
                operation: "build CodeBuild environment override".to_string(),
            })?;

        let response = self
            .start_build()
            .project_name(project_name)
            .buildspec_override(buildspec_override)
            .set_environment_variables_override(Some(env_vars))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: project_name.to_string(),
                operation: format!("start CodeBuild build '{}'", project_name),
            })?;

        let build = response.build_value().ok_or_else(|| {
            AlienError::new(ErrorData::BuildOperationFailed {
                binding_name: project_name.to_string(),
                operation: "read CodeBuild start response".to_string(),
            })
        })?;

        build_execution_from_codebuild(build, project_name)
    }

    async fn get_build(&self, build_id: &str) -> Result<BuildExecution, Error> {
        let response = self
            .batch_get_builds()
            .ids(build_id)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: build_id.to_string(),
                operation: format!("get CodeBuild status for build '{}'", build_id),
            })?;

        let build = response.builds().first().ok_or_else(|| {
            AlienError::new(ErrorData::BuildOperationFailed {
                binding_name: build_id.to_string(),
                operation: format!("find build {}", build_id),
            })
        })?;

        build_execution_from_codebuild(build, build_id)
    }

    async fn stop_build(&self, build_id: &str) -> Result<(), Error> {
        self.stop_build()
            .id(build_id)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: build_id.to_string(),
                operation: format!("stop CodeBuild build '{}'", build_id),
            })?;

        Ok(())
    }
}

fn build_execution_from_codebuild(
    build: &aws_sdk_codebuild::types::Build,
    fallback_id: &str,
) -> Result<BuildExecution, Error> {
    Ok(BuildExecution {
        id: build.id().unwrap_or(fallback_id).to_string(),
        status: map_build_status(build.build_status().map(|status| status.as_str())),
        start_time: build
            .start_time()
            .map(|time| time.fmt(DateTimeFormat::DateTime))
            .transpose()
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: fallback_id.to_string(),
                operation: "format CodeBuild start time".to_string(),
            })?,
        end_time: build
            .end_time()
            .map(|time| time.fmt(DateTimeFormat::DateTime))
            .transpose()
            .into_alien_error()
            .context(ErrorData::BuildOperationFailed {
                binding_name: fallback_id.to_string(),
                operation: "format CodeBuild end time".to_string(),
            })?,
    })
}

/// Convert AWS CodeBuild status string to alien BuildStatus.
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

/// AWS implementation of the `Build` trait using CodeBuild.
#[derive(Debug)]
pub struct CodebuildBuild {
    client: Arc<dyn CodeBuildClient>,
    binding_name: String,
    project_name: String,
    build_env_vars: HashMap<String, String>,
    monitoring: Option<alien_core::MonitoringConfig>,
}

impl CodebuildBuild {
    /// Creates a new AWS Build instance from binding parameters.
    pub fn new(
        binding_name: String,
        binding: BuildBinding,
        client: Arc<dyn CodeBuildClient>,
    ) -> Result<Self, Error> {
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
}

#[async_trait]
impl Build for CodebuildBuild {
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution, Error> {
        let mut merged_env = self.build_env_vars.clone();
        merged_env.extend(config.environment);

        let monitoring = config.monitoring.or_else(|| self.monitoring.clone());

        let environment = merged_env.into_iter().collect::<Vec<_>>();
        let wrapper_script = create_build_wrapper_script(&config.script, monitoring.as_ref());

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

        self.client
            .start_build(&self.project_name, buildspec_content, environment)
            .await
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: format!("start CodeBuild build '{}'", self.project_name),
            })
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution, Error> {
        self.client
            .get_build(build_id)
            .await
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: format!("get CodeBuild status for build '{}'", build_id),
            })
    }

    async fn stop_build(&self, build_id: &str) -> Result<(), Error> {
        self.client
            .stop_build(build_id)
            .await
            .context(ErrorData::BuildOperationFailed {
                binding_name: self.binding_name.clone(),
                operation: format!("stop CodeBuild build '{}'", build_id),
            })
    }
}

impl Binding for CodebuildBuild {}
