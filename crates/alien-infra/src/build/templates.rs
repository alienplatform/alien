use alien_aws_clients::CodeBuildApi as _;
use alien_aws_clients::AwsCredentialProvider;
use async_trait::async_trait;
use tracing::info;

use crate::error::{ErrorData, Result};
use crate::{AwsBuildController, AwsBuildState, ResourceController};
use alien_aws_clients::codebuild::{BatchGetProjectsRequest, CodeBuildClient, Project};
use alien_core::{Build, Resource};
use alien_error::{AlienError, Context};
use std::collections::HashMap;

/// CloudFormation importer for AWS Build resources
#[derive(Debug, Clone, Default)]
pub struct AwsBuildCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsBuildCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>> {
        use crate::cloudformation::utils::sanitize_to_pascal_case;

        let build = resource.downcast_ref::<Build>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: Build::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        // Generate logical ID the same way as generator.rs
        let logical_id = sanitize_to_pascal_case(&build.id);

        let physical_id = context.cfn_resources.get(&logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: logical_id.clone(),
                stack_name: "unknown".to_string(),
                resource_id: Some(build.id.clone()),
            })
        })?;

        let project_name = physical_id.as_str();

        let credentials = AwsCredentialProvider::from_config(context.aws_config.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        let client = CodeBuildClient::new(reqwest::Client::new(), credentials);

        info!(name=%project_name, "Importing CodeBuild project state from CloudFormation");

        // Fetch the project details to get environment variables
        let request = alien_aws_clients::codebuild::BatchGetProjectsRequest::builder()
            .names(vec![project_name.to_string()])
            .build();

        let response =
            client
                .batch_get_projects(request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to fetch CodeBuild project details during import".to_string(),
                    resource_id: Some(build.id.clone()),
                })?;

        let project = response
            .projects
            .as_ref()
            .and_then(|projects| projects.first())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "CodeBuild project '{}' not found during import",
                        project_name
                    ),
                    resource_id: Some(build.id.clone()),
                })
            })?;

        let project_arn = project.arn.clone();

        // Extract and parse environment variables from the CodeBuild project
        let build_env_vars = extract_build_environment_variables(project);

        Ok(Box::new(AwsBuildController {
            state: AwsBuildState::Ready,
            project_name: Some(project_name.to_string()),
            project_arn,
            build_env_vars,
            _internal_stay_count: None,
        }))
    }
}

/// Extracts build environment variables from a CodeBuild project
///
/// This function reconstructs the environment variables from the CodeBuild project.
/// It supports both individual environment variables and the _ENVIRONMENT_JSON format.
fn extract_build_environment_variables(project: &Project) -> Option<HashMap<String, String>> {
    let environment = project.environment.as_ref()?;
    let env_vars = environment.environment_variables.as_ref()?;

    // First, look for environment variables that end with _ENVIRONMENT_JSON (legacy format)
    for env_var in env_vars {
        let name = &env_var.name;
        let value = &env_var.value;

        if name.ends_with("_ENVIRONMENT_JSON") {
            // Try to parse the JSON value back into a HashMap
            if let Ok(parsed_env_vars) = serde_json::from_str::<HashMap<String, String>>(value) {
                return Some(parsed_env_vars);
            }
        }
    }

    // If no _ENVIRONMENT_JSON found, extract individual environment variables
    let mut extracted_vars = HashMap::new();
    for env_var in env_vars {
        let name = &env_var.name;
        let value = &env_var.value;

        // Include all environment variables - this matches what the controller stores
        // The EnvironmentVariableBuilder includes linked resource variables, so we should too
        extracted_vars.insert(name.clone(), value.clone());
    }

    // Return Some even if empty, as this indicates environment was processed
    Some(extracted_vars)
}
