//! The AWS setup permissions boundary of a new release fits IAM's managed-policy limit.
//!
//! Setup packages render the boundary from the stack after template mutations, which add
//! grants such as secret input reads. Without this check an oversized stack is released and
//! only fails later, when its setup package is built. The check prepares the stack as a setup
//! package does, with default stack settings and a placeholder management identity, and sizes
//! the boundary with the longest deployment names setup allows. Packages still check the
//! document they render, for the settings each deployment chooses.

use crate::{
    error::{ErrorData, Result},
    mutations::ComputeClusterMutation,
    runner::PreflightRunner,
    CheckResult, PreflightRegistry, StackMutation,
};
use alien_core::{
    AwsManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
    ManagementConfig, Platform, RemoteStackManagement, Stack, StackSettings, StackState,
};
use alien_error::{Context, ContextError};
use alien_permissions::generators::validate_aws_boundary_size;

/// Stable code of the check result reported when the boundary does not fit.
pub const CHECK_CODE: &str = "AWS_PERMISSIONS_BOUNDARY_SIZE";

const DESCRIPTION: &str =
    "AWS setup permissions boundary fits the 6144 character managed-policy limit";

/// Fail with `PERMISSIONS_BOUNDARY_TOO_LARGE`, naming the largest grant sources, when the
/// setup boundary of an AWS stack prepared for its setup package does not fit.
pub async fn check_aws_permissions_boundary_size(stack: &Stack) -> Result<()> {
    let prepared = prepare_as_setup_package(stack).await?;
    // Only stacks with a setup-owned management identity render a boundary.
    if !prepared
        .resources()
        .any(|(_, entry)| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
    {
        return Ok(());
    }
    let Err(error) = validate_aws_boundary_size(&prepared) else {
        return Ok(());
    };
    if error.code != "PERMISSIONS_BOUNDARY_TOO_LARGE" {
        return Err(error.context(ErrorData::CompileTimeCheckFailed {
            check_name: DESCRIPTION.to_string(),
            message: "Could not render the AWS permissions boundary".to_string(),
            resource_id: None,
        }));
    }
    let mut result = CheckResult::success();
    result.add_error(error.to_string());
    let result = result.with_check_metadata(Some(CHECK_CODE), DESCRIPTION);
    Err(error.context(ErrorData::ValidationFailed {
        error_count: 1,
        warning_count: 0,
        results: vec![result],
    }))
}

/// The AWS stack as a setup package prepares it before rendering: after the template
/// mutations, with default stack settings and a placeholder management identity.
///
/// Compute planning is left out. Machines are chosen per deployment in its stack settings,
/// which a release does not have, so planning with defaults could reject workloads that only
/// fit an explicitly chosen machine. Its choices add no grants to the boundary: node
/// permissions belong to the cluster, not to the permission profiles the boundary covers.
pub async fn prepare_as_setup_package(stack: &Stack) -> Result<Stack> {
    let stack_state = StackState::new(Platform::Aws);
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let mut registry = PreflightRegistry::with_built_ins();
    let compute_planning = ComputeClusterMutation.description();
    registry.retain_mutations(|mutation| mutation.description() != compute_planning);
    PreflightRunner::with_registry(registry)
        .apply_mutations(stack.clone(), &stack_state, &config)
        .await
        .context(ErrorData::CompileTimeCheckFailed {
            check_name: DESCRIPTION.to_string(),
            message: "Could not prepare the stack as its setup package does".to_string(),
            resource_id: None,
        })
}
