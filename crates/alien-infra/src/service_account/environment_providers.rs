use alien_core::{ServiceAccount, ServiceAccountOutputs};
use std::collections::HashMap;

/// Helper function to generate environment variables for ServiceAccount resources.
/// This follows the same pattern as other resources in the framework.
pub fn get_service_account_environment_variables(
    state: &alien_core::StackState,
) -> HashMap<String, String> {
    let mut env = HashMap::new();

    // Find all ServiceAccount outputs in the state
    for (resource_id, resource_state) in &state.resources {
        if resource_state.config.resource_type() == ServiceAccount::RESOURCE_TYPE {
            if let Some(outputs) = &resource_state.outputs {
                if let Some(service_account_outputs) =
                    outputs.downcast_ref::<ServiceAccountOutputs>()
                {
                    // Add environment variables for the service account identity
                    // Use uppercase and underscores for environment variable names
                    let var_prefix = format!(
                        "SERVICE_ACCOUNT_{}",
                        resource_id.to_uppercase().replace("-", "_")
                    );

                    env.insert(
                        format!("{}_IDENTITY", var_prefix),
                        service_account_outputs.identity.clone(),
                    );

                    env.insert(
                        format!("{}_RESOURCE_ID", var_prefix),
                        service_account_outputs.resource_id.clone(),
                    );
                }
            }
        }
    }

    env
}

/// Helper function to be used in EnvironmentVariableBuilder for linked ServiceAccount resources.
/// This can be integrated into the existing environment variable building pattern.
pub fn add_service_account_environment_variables(
    env_vars: &mut HashMap<String, String>,
    service_account_id: &str,
    service_account_outputs: &ServiceAccountOutputs,
) {
    let var_prefix = format!(
        "SERVICE_ACCOUNT_{}",
        service_account_id.to_uppercase().replace("-", "_")
    );

    env_vars.insert(
        format!("{}_IDENTITY", var_prefix),
        service_account_outputs.identity.clone(),
    );

    env_vars.insert(
        format!("{}_RESOURCE_ID", var_prefix),
        service_account_outputs.resource_id.clone(),
    );
}
