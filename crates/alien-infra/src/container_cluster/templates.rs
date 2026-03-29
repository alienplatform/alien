//! CloudFormation state importer for ContainerCluster resources.
//!
//! ContainerCluster is a `Live` resource where CloudFormation creates only
//! the IAM Role, Instance Profile, and Security Group. The imperative controller
//! creates Launch Templates and ASGs during the Provisioning phase.
//!
//! This importer reads the CF-created resources and initializes the controller
//! at the `CreatingLaunchTemplate` state — the first state after IAM + SG are done.
//! The controller then picks up and creates the remaining resources.

use async_trait::async_trait;
use std::collections::HashMap;
use tracing::info;

use crate::error::{ErrorData, Result};
use crate::{AwsContainerClusterController, AwsContainerClusterState, ResourceController};
use alien_aws_clients::iam::IamClient;
use alien_aws_clients::IamApi;
use alien_aws_clients::AwsCredentialProvider;
use alien_core::{ContainerCluster, Resource};
use alien_error::{AlienError, Context};

/// CloudFormation importer for AWS ContainerCluster resources.
///
/// Imports IAM Role, Instance Profile, and Security Group from CloudFormation.
/// Sets the controller state to `CreatingLaunchTemplate` so the controller
/// continues by creating Launch Templates and ASGs during Provisioning.
#[derive(Debug, Clone, Default)]
pub struct AwsContainerClusterCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsContainerClusterCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>> {
        use crate::cloudformation::utils::sanitize_to_pascal_case;

        let cluster = resource.downcast_ref::<ContainerCluster>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: ContainerCluster::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        let logical_id = sanitize_to_pascal_case(cluster.id());

        // === Import IAM Role ===
        let role_logical_id = format!("{}Role", logical_id);
        let role_physical_id = context.cfn_resources.get(&role_logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: role_logical_id.clone(),
                stack_name: context.stack_name.clone(),
                resource_id: Some(cluster.id().to_string()),
            })
        })?;

        info!(role=%role_physical_id, "Importing ContainerCluster IAM role from CloudFormation");

        // Verify the role exists and get its ARN
        let credentials = AwsCredentialProvider::from_config(context.aws_config.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create AWS credential provider".to_string(),
                resource_id: None,
            })?;
        let iam_client = IamClient::new(reqwest::Client::new(), credentials);
        let role_response = iam_client.get_role(role_physical_id).await.context(
            ErrorData::InfrastructureImportFailed {
                message: format!("Failed to verify IAM role '{}' exists", role_physical_id),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(cluster.id().to_string()),
            },
        )?;

        let role_arn = role_response.get_role_result.role.arn.clone();

        // === Import Instance Profile ===
        let profile_logical_id = format!("{}InstanceProfile", logical_id);
        let profile_physical_id =
            context
                .cfn_resources
                .get(&profile_logical_id)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CloudFormationResourceMissing {
                        logical_id: profile_logical_id.clone(),
                        stack_name: context.stack_name.clone(),
                        resource_id: Some(cluster.id().to_string()),
                    })
                })?;

        info!(profile=%profile_physical_id, "Importing ContainerCluster instance profile");

        // Get instance profile ARN
        let profile_response = iam_client
            .get_instance_profile(profile_physical_id)
            .await
            .context(ErrorData::InfrastructureImportFailed {
                message: format!(
                    "Failed to verify instance profile '{}' exists",
                    profile_physical_id
                ),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(cluster.id().to_string()),
            })?;

        let instance_profile_arn = profile_response
            .get_instance_profile_result
            .instance_profile
            .arn
            .clone();

        // === Import Security Group ===
        let sg_logical_id = format!("{}Sg", logical_id);
        let sg_physical_id = context.cfn_resources.get(&sg_logical_id).ok_or_else(|| {
            AlienError::new(ErrorData::CloudFormationResourceMissing {
                logical_id: sg_logical_id.clone(),
                stack_name: context.stack_name.clone(),
                resource_id: Some(cluster.id().to_string()),
            })
        })?;

        info!(sg=%sg_physical_id, "Importing ContainerCluster security group");

        // === Create controller in CreatingLaunchTemplate state ===
        // IAM + SG are imported from CF. Launch Templates and ASGs will be
        // created by the controller when it runs during the Provisioning phase.
        // horizon_cluster_id and horizon_api_url are left as None — the controller
        // reads them from deployment_config.compute_backend when it needs them.
        info!(
            cluster_id = %cluster.id(),
            "ContainerCluster imported from CF — controller will create LT + ASG during Provisioning"
        );

        Ok(Box::new(AwsContainerClusterController {
            state: AwsContainerClusterState::CreatingLaunchTemplate,
            role_name: Some(role_physical_id.clone()),
            role_arn: Some(role_arn),
            instance_profile_name: Some(profile_physical_id.clone()),
            instance_profile_arn: Some(instance_profile_arn),
            security_group_id: Some(sg_physical_id.clone()),
            target_group_arn: None,
            launch_templates: HashMap::new(),
            asg_states: HashMap::new(),
            horizon_cluster_id: None,
            horizon_api_url: None,
            boot_check_iterations: 0,
            rolling_update_poll_iterations: 0,
            otlp_auth_secret_name: None,
            otlp_metrics_auth_secret_name: None,
            new_groups_pending_ready: vec![],
            rolling_update_triggered: false,
            instance_refresh_ids: HashMap::new(),
            _internal_stay_count: None,
        }))
    }
}
