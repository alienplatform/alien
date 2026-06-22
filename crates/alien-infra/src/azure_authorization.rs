use crate::core::{map_azure_core_021_sdk_error, Scope};
use crate::error::Result;
use alien_core::AzureClientConfig;
use alien_error::{Context, IntoAlienError};
use azure_mgmt_authorization::package_2022_04_01 as azure_authorization_2022_04;
use azure_mgmt_authorization::package_2022_04_01::models::{
    RoleAssignment, RoleAssignmentCreateParameters, RoleDefinition,
};

pub(crate) async fn create_or_update_role_definition(
    client: &azure_authorization_2022_04::Client,
    config: &AzureClientConfig,
    scope: &Scope,
    role_definition_id: &str,
    role_definition: &RoleDefinition,
) -> Result<RoleDefinition> {
    let result = client
        .role_definitions_client()
        .create_or_update(
            scope.to_scope_string(config),
            role_definition_id.to_string(),
            role_definition.clone(),
        )
        .await;
    map_azure_core_021_sdk_error(
        "Azure Authorization",
        result,
        "role definition create or update",
        "Azure role definition",
        role_definition_id,
    )
}

pub(crate) async fn delete_role_definition(
    client: &azure_authorization_2022_04::Client,
    config: &AzureClientConfig,
    scope: &Scope,
    role_definition_id: &str,
) -> Result<Option<RoleDefinition>> {
    let response = client
        .role_definitions_client()
        .delete(
            scope.to_scope_string(config),
            role_definition_id.to_string(),
        )
        .send()
        .await;
    let response = map_azure_core_021_sdk_error(
        "Azure Authorization",
        response,
        "role definition delete",
        "Azure role definition",
        role_definition_id,
    )?;
    if response.as_raw_response().status() == azure_core_021::StatusCode::NoContent {
        Ok(None)
    } else {
        response
            .into_body()
            .await
            .map(Some)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure role definition '{role_definition_id}' delete response"
                ),
                resource_id: None,
            })
    }
}

pub(crate) async fn create_or_update_role_assignment_by_id(
    client: &azure_authorization_2022_04::Client,
    role_assignment_id: &str,
    role_assignment: &RoleAssignmentCreateParameters,
) -> Result<RoleAssignment> {
    let result = client
        .role_assignments_client()
        .create_by_id(role_assignment_id.to_string(), role_assignment.clone())
        .await;
    map_azure_core_021_sdk_error(
        "Azure Authorization",
        result,
        "role assignment create or update",
        "Azure role assignment",
        role_assignment_id,
    )
}

pub(crate) async fn delete_role_assignment_by_id(
    client: &azure_authorization_2022_04::Client,
    role_assignment_id: &str,
) -> Result<Option<RoleAssignment>> {
    let response = client
        .role_assignments_client()
        .delete_by_id(role_assignment_id.to_string())
        .send()
        .await;
    let response = map_azure_core_021_sdk_error(
        "Azure Authorization",
        response,
        "role assignment delete",
        "Azure role assignment",
        role_assignment_id,
    )?;
    if response.as_raw_response().status() == azure_core_021::StatusCode::NoContent {
        Ok(None)
    } else {
        response
            .into_body()
            .await
            .map(Some)
            .into_alien_error()
            .context(crate::error::ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to parse Azure role assignment '{role_assignment_id}' delete response"
                ),
                resource_id: None,
            })
    }
}

pub(crate) fn role_assignment_id(
    config: &AzureClientConfig,
    scope: &Scope,
    role_assignment_name: &str,
) -> String {
    format!(
        "/{}/providers/Microsoft.Authorization/roleAssignments/{}",
        scope.to_scope_string(config),
        role_assignment_name
    )
}
