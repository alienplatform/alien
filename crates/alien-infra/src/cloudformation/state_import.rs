use std::collections::HashMap;
use std::sync::Arc;

use crate::cloudformation::traits::CloudFormationImportContext;
use crate::core::{state_utils::StackResourceStateExt, ResourceRegistry};
use crate::error::{ErrorData, Result};
use crate::ResourceController;
use alien_aws_clients::cloudformation::{CloudFormationApi, CloudFormationClient};
use alien_core::ClientConfig;
use alien_core::{
    alien_event, AlienEvent, AwsManagementConfig, ManagementConfig, Platform, ResourceType, Stack,
    StackResourceState, StackSettings, StackState,
};
use alien_core::{ResourceLifecycle, ResourceStatus};
use alien_error::{AlienError, Context};
use futures_util::future::try_join_all;
use tracing::warn;

/// Import the stack state from an existing CloudFormation stack.
pub async fn import_stack_state_from_cloudformation(
    stack: Stack,
    cf_stack_name: &str,
    client_config: ClientConfig,
) -> Result<StackState> {
    import_stack_state_from_cloudformation_with_registry(
        stack,
        cf_stack_name,
        client_config,
        Arc::new(ResourceRegistry::with_built_ins()),
    )
    .await
}

/// Import the stack state from an existing CloudFormation stack with custom registry.
#[alien_event(AlienEvent::ImportingStackStateFromCloudFormation {
    cfn_stack_name: cf_stack_name.to_string(),
})]
pub async fn import_stack_state_from_cloudformation_with_registry(
    stack: Stack,
    cf_stack_name: &str,
    client_config: ClientConfig,
    registry: Arc<ResourceRegistry>,
) -> Result<StackState> {
    let aws_client_config = match &client_config {
        ClientConfig::Aws(config) => config.as_ref(),
        _ => {
            return Err(AlienError::new(ErrorData::ClientConfigMismatch {
                required_platform: Platform::Aws,
                found_platform: client_config.platform(),
            }));
        }
    };

    // Create our custom CloudFormation client
    let cfn_client = CloudFormationClient::new(reqwest::Client::new(), aws_client_config.clone());

    // Describe CloudFormation stack to get parameters
    let describe_stacks_resp = cfn_client
        .describe_stacks(
            alien_aws_clients::cloudformation::DescribeStacksRequest::builder()
                .stack_name(cf_stack_name.to_string())
                .build(),
        )
        .await
        .context(ErrorData::InfrastructureImportFailed {
            message: "Failed to describe CloudFormation stack".to_string(),
            import_source: Some("CloudFormation".to_string()),
            resource_id: None,
        })?;

    // Extract parameters from CloudFormation stack
    let cfn_params = describe_stacks_resp
        .describe_stacks_result
        .stacks
        .member
        .first()
        .and_then(|stack| stack.parameters.as_ref())
        .map(|params| &params.member);

    // Helper to get parameter value
    let get_param = |key: &str| -> Option<String> {
        cfn_params
            .and_then(|params| params.iter().find(|p| p.parameter_key == key))
            .and_then(|p| p.parameter_value.clone())
            .filter(|v| !v.is_empty())
    };

    // Extract ManagingRoleArn parameter if present
    let managing_role_arn = get_param("ManagingRoleArn");

    // Validate role ARN format if present
    if let Some(ref role_arn) = managing_role_arn {
        validate_aws_role_arn(role_arn)?;
    }

    // Create stack settings with management config if ManagingRoleArn parameter exists
    let management_config = managing_role_arn.map(|arn| {
        ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: arn,
        })
    });

    // Determine network settings based on VpcId and EnableNetwork parameters
    // Three modes:
    // 1. EnableNetwork=false or not present, VpcId empty → No network
    // 2. VpcId provided → BYO-VPC mode
    // 3. EnableNetwork=true, VpcId empty → Create mode (VPC was created by CFN)
    let enable_network = get_param("EnableNetwork")
        .map(|v| v == "true")
        .unwrap_or(false);
    let vpc_id_param = get_param("VpcId");

    let network_settings = if let Some(vpc_id) = vpc_id_param {
        // BYO-VPC mode: deployer provided existing VPC
        let public_subnet_ids = get_param("PublicSubnetIds")
            .map(|s| {
                s.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let private_subnet_ids = get_param("PrivateSubnetIds")
            .map(|s| {
                s.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let security_group_ids = get_param("SecurityGroupId")
            .map(|s| vec![s])
            .unwrap_or_default();

        Some(alien_core::NetworkSettings::ByoVpcAws {
            vpc_id,
            public_subnet_ids,
            private_subnet_ids,
            security_group_ids,
        })
    } else if enable_network {
        // Create mode: VPC was created by CloudFormation
        Some(alien_core::NetworkSettings::Create {
            cidr: get_param("VpcCidr"),
            availability_zones: 2,
        })
    } else {
        // No network: deployer chose not to use VPC
        None
    };

    let stack_settings = StackSettings {
        network: network_settings,
        ..Default::default()
    };

    let deployment_config = alien_core::DeploymentConfig::builder()
        .stack_settings(stack_settings.clone())
        .maybe_management_config(management_config.clone())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();

    // Step 1: Run compile-time preflights on the stack
    let preflight_runner = alien_preflights::runner::PreflightRunner::new();
    let _compile_time_summary = preflight_runner
        .run_build_time_preflights(&stack, Platform::Aws)
        .await
        .context(ErrorData::InfrastructureError {
            message: "Failed to run compile-time preflights for CloudFormation state import"
                .to_string(),
            operation: Some("compile-time preflights".to_string()),
            resource_id: None,
        })?;

    // Step 2: Create stack state for mutations
    let stack_state = StackState::new(Platform::Aws);

    // Step 3: Apply stack mutations to get the full stack with infrastructure resources
    let mutated_stack = preflight_runner
        .apply_mutations(stack, &stack_state, &deployment_config)
        .await
        .context(ErrorData::InfrastructureError {
            message: "Failed to apply stack mutations for CloudFormation state import".to_string(),
            operation: Some("apply mutations".to_string()),
            resource_id: None,
        })?;

    // Describe CloudFormation stack resources using our custom client
    let describe_resp = cfn_client
        .describe_stack_resources(
            alien_aws_clients::cloudformation::DescribeStackResourcesRequest::builder()
                .stack_name(cf_stack_name.to_string())
                .build(),
        )
        .await
        .context(ErrorData::InfrastructureImportFailed {
            message: "Failed to describe CloudFormation stack resources".to_string(),
            import_source: Some("CloudFormation".to_string()),
            resource_id: None,
        })?;

    // Create a new StackState with AWS platform
    let mut state = StackState::new(Platform::Aws);
    // Override the generated prefix with the actual CFN stack name
    state.resource_prefix = cf_stack_name.to_string();

    // Validate resources and build physical ID map
    let physical_id_map = validate_cloudformation_resources(
        &describe_resp
            .describe_stack_resources_result
            .stack_resources
            .member,
        cf_stack_name,
    )?;

    // Create import context
    let import_context = CloudFormationImportContext {
        cfn_resources: physical_id_map,
        aws_config: aws_client_config.clone(),
        resource_prefix: cf_stack_name.to_string(),
        stack_name: cf_stack_name.to_string(),
    };

    // Build a list of futures, one per resource (using mutated_stack which has infrastructure resources)
    let import_futures = mutated_stack
        .resources()
        .filter(|(_id, res)| res.lifecycle != ResourceLifecycle::Live)
        .map(|(id, res)| {
            // Clone data we need inside the async block
            let id = id.clone();
            let resource_cfg = res.config.clone();
            let resource_lifecycle = res.lifecycle;
            let context = import_context.clone();
            let registry = registry.clone(); // Clone the Arc
            async move {
                let (resource_type, internal_state) = import_resource_state(
                    &resource_cfg,
                    &registry, // Dereference Arc to get &ResourceRegistry
                    &context,
                )
                .await?;
                Ok((
                    id,
                    resource_type,
                    internal_state,
                    resource_lifecycle,
                    resource_cfg,
                ))
            }
        })
        .collect::<Vec<_>>();

    // Run all imports concurrently
    let import_results = try_join_all(import_futures).await?;

    // Assemble the final StackState
    for (id, resource_type, internal_state, lifecycle, config) in import_results {
        let mut resource_state = StackResourceState {
            resource_type: format!("{:?}", resource_type),
            outputs: internal_state.get_outputs(),
            remote_binding_params: internal_state.get_binding_params(),
            internal_state: None, // Will be set below
            status: ResourceStatus::Running,
            config,
            previous_config: None,
            retry_attempt: 0,
            error: None,
            is_externally_provisioned: true,
            lifecycle: Some(lifecycle),
            dependencies: Vec::new(), // External resources don't have tracked dependencies
            last_failed_state: None,
        };

        // Set the internal state using the extension trait
        resource_state.set_internal_controller(Some(internal_state))?;
        state.resources.insert(id, resource_state);
    }

    Ok(state)
}

/// Validates that an AWS IAM role ARN has the correct format
fn validate_aws_role_arn(role_arn: &str) -> Result<()> {
    // AWS IAM role ARN format: arn:aws:iam::<account-id>:role/<role-name>
    // or arn:aws-us-gov:iam::<account-id>:role/<role-name> for GovCloud
    // or arn:aws-cn:iam::<account-id>:role/<role-name> for China
    let arn_parts: Vec<&str> = role_arn.split(':').collect();

    if arn_parts.len() < 6 {
        return Err(AlienError::new(ErrorData::InfrastructureError {
            message: format!("Invalid AWS role ARN format: '{}'. Expected format: arn:aws:iam::<account-id>:role/<role-name>", role_arn),
            operation: Some("validate_role_arn".to_string()),
            resource_id: None,
        }));
    }

    // Check partition (arn)
    if arn_parts[0] != "arn" {
        return Err(AlienError::new(ErrorData::InfrastructureError {
            message: format!(
                "Invalid AWS role ARN: '{}'. Must start with 'arn'",
                role_arn
            ),
            operation: Some("validate_role_arn".to_string()),
            resource_id: None,
        }));
    }

    // Check partition (aws, aws-us-gov, aws-cn)
    if !arn_parts[1].starts_with("aws") {
        return Err(AlienError::new(ErrorData::InfrastructureError {
            message: format!(
                "Invalid AWS role ARN: '{}'. Partition must be 'aws', 'aws-us-gov', or 'aws-cn'",
                role_arn
            ),
            operation: Some("validate_role_arn".to_string()),
            resource_id: None,
        }));
    }

    // Check service (iam)
    if arn_parts[2] != "iam" {
        return Err(AlienError::new(ErrorData::InfrastructureError {
            message: format!(
                "Invalid AWS role ARN: '{}'. Service must be 'iam'",
                role_arn
            ),
            operation: Some("validate_role_arn".to_string()),
            resource_id: None,
        }));
    }

    // Region should be empty for IAM (part 3)
    if !arn_parts[3].is_empty() {
        return Err(AlienError::new(ErrorData::InfrastructureError {
            message: format!(
                "Invalid AWS role ARN: '{}'. IAM ARNs should not have a region",
                role_arn
            ),
            operation: Some("validate_role_arn".to_string()),
            resource_id: None,
        }));
    }

    // Account ID should not be empty (part 4)
    if arn_parts[4].is_empty() {
        return Err(AlienError::new(ErrorData::InfrastructureError {
            message: format!(
                "Invalid AWS role ARN: '{}'. Account ID is required",
                role_arn
            ),
            operation: Some("validate_role_arn".to_string()),
            resource_id: None,
        }));
    }

    // Resource type should be "role/" (part 5)
    if !arn_parts[5].starts_with("role/") {
        return Err(AlienError::new(ErrorData::InfrastructureError {
            message: format!(
                "Invalid AWS role ARN: '{}'. Resource type must be 'role/'",
                role_arn
            ),
            operation: Some("validate_role_arn".to_string()),
            resource_id: None,
        }));
    }

    Ok(())
}

/// Validates the status of CloudFormation resources and builds a map of Logical ID -> Physical ID.
/// Returns an error if any resourcalien_cloud_clientsful status.
fn validate_cloudformation_resources(
    stack_resources: &[alien_aws_clients::cloudformation::StackResource],
    cf_stack_name: &str,
) -> Result<HashMap<String, String>> {
    let mut physical_id_map: HashMap<String, String> = HashMap::new();
    let mut failed_resources = Vec::new(); // Store details of failed resources

    for cf_resource in stack_resources {
        let logical_id = cf_resource.logical_resource_id.clone();
        let status_str = cf_resource.resource_status.clone();

        // Check resource status - successful statuses based on CloudFormation documentation
        let status_ok = matches!(
            status_str.as_str(),
            "CREATE_COMPLETE"
                | "UPDATE_COMPLETE"
                | "IMPORT_COMPLETE"
                | "UPDATE_ROLLBACK_COMPLETE"
                | "ROLLBACK_COMPLETE"
        );

        if !status_ok {
            // Collect failed resource info instead of returning immediately
            failed_resources.push((logical_id.clone(), status_str.clone()));
            // Log a warning for each failed resource
            warn!(stack=%cf_stack_name, resource_id=%logical_id, status=%status_str, "Resource found with non-successful status during stack import check.");
        } else if let Some(ref physical_id) = cf_resource.physical_resource_id {
            // Only add successful resources to the physical ID map
            physical_id_map.insert(logical_id, physical_id.clone());
        }
    }

    // After checking all resources, return an error if any failed
    if !failed_resources.is_empty() {
        let failed_details = failed_resources
            .into_iter()
            .map(|(id, status)| format!("{} ({})", id, status))
            .collect::<Vec<String>>()
            .join(", ");

        return Err(AlienError::new(ErrorData::CloudFormationStackUnhealthy {
            stack_name: cf_stack_name.to_string(),
            failed_resources: failed_details.split(", ").map(|s| s.to_string()).collect(),
        }));
    }

    Ok(physical_id_map)
}

/// Inner helper to import a single resource using registry
async fn import_resource_state(
    resource: &alien_core::Resource,
    registry: &ResourceRegistry,
    context: &CloudFormationImportContext,
) -> Result<(ResourceType, Box<dyn ResourceController>)> {
    let resource_type = resource.resource_type();

    // Always use registry - no fallback
    let importer = registry
        .get_cloudformation_importer(resource_type.clone(), Platform::Aws)
        .context(ErrorData::ControllerNotAvailable {
            resource_type: resource_type.clone(),
            platform: Platform::Aws,
        })?;

    let internal_state = importer
        .import_cloudformation_state(resource, context)
        .await
        .context(ErrorData::InfrastructureImportFailed {
            message: format!("Failed to import resource of type '{}'", resource_type),
            import_source: Some("CloudFormation".to_string()),
            resource_id: Some(resource.id().to_string()),
        })?;

    Ok((resource_type, internal_state))
}
