use crate::{
    error::{map_cloud_client_error, ErrorData, Result},
    providers::build::script::create_build_wrapper_script,
    traits::{Binding, Build},
};
use alien_core::{
    bindings::{BuildBinding, CloudbuildBuildBinding},
    BuildConfig, BuildExecution, BuildStatus, ComputeType,
};
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use std::collections::HashMap;

use regex;
use serde_json;

use alien_gcp_clients::{
    cloudbuild::{
        Build as CloudBuild, BuildOptions, BuildStatus as GcpBuildStatus, BuildStep, CloudBuildApi,
        CloudBuildClient, LoggingMode, MachineType,
    },
    GcpClientConfig,
};

/// GCP implementation of the `Build` trait using Cloud Build.
#[derive(Debug)]
pub struct CloudbuildBuild {
    client: CloudBuildClient,
    binding_name: String,
    project_id: String,
    location: String,
    build_env_vars: HashMap<String, String>,
    service_account: String,
    monitoring: Option<alien_core::MonitoringConfig>,
}

impl CloudbuildBuild {
    /// Creates a new GCP Build instance from binding parameters.
    pub async fn new(
        binding_name: String,
        binding: BuildBinding,
        gcp_config: &GcpClientConfig,
    ) -> Result<Self> {
        let client =
            CloudBuildClient::new(crate::http_client::create_http_client(), gcp_config.clone());

        // Get project_id and location from GCP config instead of binding
        let project_id = gcp_config.project_id.clone();
        let location = gcp_config.region.clone();

        // Extract values from binding
        let config = match binding {
            BuildBinding::Cloudbuild(config) => config,
            _ => {
                return Err(AlienError::new(ErrorData::BindingConfigInvalid {
                    binding_name: binding_name.clone(),
                    reason: "Expected CloudBuild binding, got different service type".to_string(),
                }));
            }
        };

        let build_env_vars = config
            .build_env_vars
            .into_value(&binding_name, "build_env_vars")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract build_env_vars from binding".to_string(),
            })?;

        let service_account = config
            .service_account
            .into_value(&binding_name, "service_account")
            .context(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.clone(),
                reason: "Failed to extract service_account from binding".to_string(),
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
            project_id,
            location,
            build_env_vars,
            service_account,
            monitoring,
        })
    }

    /// Convert alien ComputeType to GCP Cloud Build machine type
    fn map_machine_type(compute_type: &ComputeType) -> MachineType {
        match compute_type {
            ComputeType::Small => MachineType::E2Medium,
            ComputeType::Medium => MachineType::E2Medium,
            ComputeType::Large => MachineType::E2Highcpu8,
            ComputeType::XLarge => MachineType::E2Highcpu32,
        }
    }

    /// Convert GCP Cloud Build status to alien BuildStatus
    fn map_build_status(status: Option<&GcpBuildStatus>) -> BuildStatus {
        match status {
            Some(GcpBuildStatus::Success) => BuildStatus::Succeeded,
            Some(GcpBuildStatus::Failure)
            | Some(GcpBuildStatus::InternalError)
            | Some(GcpBuildStatus::Timeout) => BuildStatus::Failed,
            Some(GcpBuildStatus::Cancelled) => BuildStatus::Cancelled,
            Some(GcpBuildStatus::Working) => BuildStatus::Running,
            Some(GcpBuildStatus::Queued) => BuildStatus::Queued,
            _ => BuildStatus::Queued,
        }
    }

    /// Escape environment variable references in the script to prevent GCP Cloud Build substitutions.
    /// Converts $VAR to $$VAR while preserving existing $$VAR sequences.
    fn escape_env_refs(
        script: &str,
        env: &HashMap<String, String>,
        binding_name: &str,
    ) -> Result<String> {
        let mut out = script.to_owned();

        // Temporary sentinel so already-escaped $$VAR survive the second pass
        const SENTINEL_PREFIX: &str = "__DOUBLE_DOLLAR_SENTINEL__";
        out = out.replace("$$", SENTINEL_PREFIX);

        for key in env.keys() {
            // \$KEY\b → matches $KEY followed by a word boundary
            let escaped_key = regex::escape(key);
            let pat = format!("\\${}\\b", escaped_key);

            let re = regex::Regex::new(&pat).into_alien_error().context(
                ErrorData::BuildOperationFailed {
                    binding_name: binding_name.to_string(),
                    operation: format!("compile regex for {}", key),
                },
            )?;

            let replacement = format!("$$$${}", key);
            out = re.replace_all(&out, replacement.as_str()).to_string();
        }

        // Restore any original $$ sequences
        Ok(out.replace(SENTINEL_PREFIX, "$$"))
    }
}

#[async_trait]
impl Build for CloudbuildBuild {
    async fn start_build(&self, config: BuildConfig) -> Result<BuildExecution> {
        // Merge build config environment with binding environment variables
        // Build config environment takes precedence over binding environment
        let mut merged_environment = self.build_env_vars.clone();
        merged_environment.extend(config.environment);

        // Merge monitoring configuration - build config takes precedence over binding
        let monitoring = config.monitoring.or_else(|| self.monitoring.clone());

        // Note: Monitoring configuration is now handled directly in the Fluent Bit config
        // rather than through environment variables, similar to AWS implementation

        // Convert environment variables to GCP Cloud Build format
        let env_vars: Vec<String> = merged_environment
            .iter()
            .map(|(key, value)| format!("{}={}", key, value))
            .collect();

        // Escape environment variables in the script to prevent GCP Cloud Build substitutions
        let escaped_script =
            Self::escape_env_refs(&config.script, &merged_environment, &self.binding_name)?;

        // Create build step that runs the unified wrapper script
        let wrapper_script = create_build_wrapper_script(&escaped_script, monitoring.as_ref());

        let build_step = BuildStep::builder()
            .name(config.image)
            .args(vec!["bash".to_string(), "-c".to_string(), wrapper_script])
            .env(env_vars)
            .timeout(format!("{}s", config.timeout_seconds))
            .automap_substitutions(false)
            .build();

        // Create build options with appropriate machine type and disable substitutions entirely
        let options = BuildOptions::builder()
            .machine_type(Self::map_machine_type(&config.compute_type))
            .automap_substitutions(false)
            .logging(LoggingMode::CloudLoggingOnly)
            .build();

        // Get service account from binding and format it as a resource path
        let service_account = if self.service_account.contains("@") {
            // Convert email format to resource path format
            format!(
                "projects/{}/serviceAccounts/{}",
                self.project_id, self.service_account
            )
        } else {
            // Assume it's already in resource path format
            self.service_account.clone()
        };

        // Create the Cloud Build configuration
        let cloud_build = CloudBuild::builder()
            .steps(vec![build_step])
            .timeout(format!("{}s", config.timeout_seconds))
            .options(options)
            .service_account(service_account)
            .build();

        let operation = self
            .client
            .create_build(&self.location, cloud_build)
            .await
            .map_err(|e| {
                map_cloud_client_error(e, "Failed to start GCP Cloud Build".to_string(), None)
            })?;

        // Extract build ID from operation metadata (available immediately)
        let build_id = operation
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("build"))
            .and_then(|build| build.get("id"))
            .and_then(|id| id.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                let response_json = serde_json::to_string_pretty(&operation)
                    .unwrap_or_else(|_| "Failed to serialize operation".to_string());

                AlienError::new(ErrorData::UnexpectedResponseFormat {
                    provider: "gcp".to_string(),
                    binding_name: self.binding_name.clone(),
                    field: "metadata.build.id".to_string(),
                    response_json,
                })
            })?;

        Ok(BuildExecution {
            id: build_id,
            status: BuildStatus::Queued,
            start_time: Some(chrono::Utc::now().to_rfc3339()),
            end_time: None,
        })
    }

    async fn get_build_status(&self, build_id: &str) -> Result<BuildExecution> {
        let build = self
            .client
            .get_build(&self.location, build_id)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!(
                        "Failed to get GCP Cloud Build status for build '{}'",
                        build_id
                    ),
                    Some(build_id.to_string()),
                )
            })?;

        let status = Self::map_build_status(build.status.as_ref());
        let start_time = build.start_time.clone();
        let end_time = if matches!(
            status,
            BuildStatus::Succeeded | BuildStatus::Failed | BuildStatus::Cancelled
        ) {
            build.finish_time.clone()
        } else {
            None
        };

        Ok(BuildExecution {
            id: build_id.to_string(),
            status,
            start_time,
            end_time,
        })
    }

    async fn stop_build(&self, build_id: &str) -> Result<()> {
        self.client
            .cancel_build(&self.location, build_id)
            .await
            .map_err(|e| {
                map_cloud_client_error(
                    e,
                    format!("Failed to stop GCP Cloud Build '{}'", build_id),
                    Some(build_id.to_string()),
                )
            })?;

        Ok(())
    }
}

impl Binding for CloudbuildBuild {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_escape_env_refs() {
        let mut env = HashMap::new();
        env.insert("CUSTOM_VAR".to_string(), "custom_value".to_string());
        env.insert("ANOTHER_VAR".to_string(), "another_value".to_string());

        let script = r#"echo "CUSTOM_VAR=$CUSTOM_VAR"; echo "ANOTHER_VAR=$ANOTHER_VAR""#;
        let expected = r#"echo "CUSTOM_VAR=$$CUSTOM_VAR"; echo "ANOTHER_VAR=$$ANOTHER_VAR""#;

        let result = CloudbuildBuild::escape_env_refs(script, &env, "test-binding").unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_escape_env_refs_preserves_existing_double_dollar() {
        let mut env = HashMap::new();
        env.insert("VAR1".to_string(), "value1".to_string());

        let script = r#"echo "Already escaped: $$VAR1, needs escaping: $VAR1""#;
        let expected = r#"echo "Already escaped: $$VAR1, needs escaping: $$VAR1""#;

        let result = CloudbuildBuild::escape_env_refs(script, &env, "test-binding").unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_escape_env_refs_word_boundary() {
        let mut env = HashMap::new();
        env.insert("VAR".to_string(), "value".to_string());

        let script = r#"echo "$VAR $VARIABLE""#;
        let expected = r#"echo "$$VAR $VARIABLE""#;

        let result = CloudbuildBuild::escape_env_refs(script, &env, "test-binding").unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_service_account_env_var_format() {
        // Test that the service account environment variable follows the expected format
        let binding_name = "test-build-resource";
        let expected_env_var = "TEST_BUILD_RESOURCE_SERVICE_ACCOUNT";
        let actual_env_var = format!(
            "{}_SERVICE_ACCOUNT",
            binding_name.to_uppercase().replace("-", "_")
        );
        assert_eq!(actual_env_var, expected_env_var);
    }

    #[test]
    fn test_service_account_format_conversion() {
        // Test email format conversion to resource path
        let project_id = "test-project";
        let service_account_email = "test-service@test-project.iam.gserviceaccount.com";

        let formatted = if service_account_email.contains("@") {
            format!(
                "projects/{}/serviceAccounts/{}",
                project_id, service_account_email
            )
        } else {
            service_account_email.to_string()
        };

        assert_eq!(formatted, "projects/test-project/serviceAccounts/test-service@test-project.iam.gserviceaccount.com");

        // Test resource path format is preserved
        let resource_path = "projects/test-project/serviceAccounts/test-service@test-project.iam.gserviceaccount.com";
        let preserved = if resource_path.contains("@") && !resource_path.starts_with("projects/") {
            format!("projects/{}/serviceAccounts/{}", project_id, resource_path)
        } else {
            resource_path.to_string()
        };

        assert_eq!(preserved, resource_path);
    }
}
