use alien_azure_clients::authorization::Scope;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, ContextError};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

pub(super) struct ProvenRoleAssignment {
    pub(super) id: String,
}

pub(super) async fn discover_proven_role_assignments(
    ctx: &ResourceControllerContext<'_>,
    scope: &Scope,
    role_definition_id: &str,
    resource_id: &str,
    assignment_kind: &str,
    expected_name_for_principal: impl Fn(&str) -> String,
) -> Result<Vec<ProvenRoleAssignment>> {
    let azure_config = ctx.get_azure_config()?;
    let authorization_client = ctx
        .service_provider
        .get_azure_authorization_client(azure_config)?;
    let assignments = match authorization_client
        .list_role_assignments(scope, Some(role_definition_id.to_string()))
        .await
    {
        Ok(assignments) => assignments,
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(Vec::new());
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to discover {assignment_kind} role assignments"),
                resource_id: Some(resource_id.to_string()),
            }));
        }
    };

    let mut proven = Vec::new();
    for assignment in assignments {
        let properties = assignment.properties.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: resource_id.to_string(),
                message: format!(
                    "{assignment_kind} role discovery returned an assignment without properties"
                ),
            })
        })?;
        if !properties
            .role_definition_id
            .eq_ignore_ascii_case(role_definition_id)
        {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: resource_id.to_string(),
                message: format!(
                    "{assignment_kind} role discovery returned unexpected role definition '{}'",
                    properties.role_definition_id
                ),
            }));
        }
        let assignment_id = assignment.id.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: resource_id.to_string(),
                message: format!(
                    "{assignment_kind} role discovery returned an assignment without an ID"
                ),
            })
        })?;
        let assignment_name = assignment.name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: resource_id.to_string(),
                message: format!(
                    "{assignment_kind} role discovery returned an assignment without a name"
                ),
            })
        })?;
        let expected_name = expected_name_for_principal(&properties.principal_id);
        let expected_id =
            authorization_client.build_role_assignment_id(scope, expected_name.clone());
        let name_matches = assignment_name.eq_ignore_ascii_case(&expected_name);
        let id_matches = assignment_id.eq_ignore_ascii_case(&expected_id);
        if name_matches != id_matches {
            return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: resource_id.to_string(),
                message: format!(
                    "Malformed deterministic {assignment_kind} role assignment '{assignment_id}'"
                ),
            }));
        }
        if name_matches {
            proven.push(ProvenRoleAssignment {
                id: assignment_id.to_string(),
            });
        }
    }

    Ok(proven)
}
