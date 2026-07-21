use std::collections::HashSet;

use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::iam::{
    CreateRoleRequest, IamApi, Role, RoleLaunchStage, RoleView, UndeleteRoleRequest,
};
use alien_permissions::{
    generators::{custom_role_prefix, GcpCustomRole},
    PermissionContext,
};
use tracing::info;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
};

pub(super) async fn ensure(
    ctx: &ResourceControllerContext<'_>,
    permission_set_id: &str,
    custom_roles: Vec<GcpCustomRole>,
) -> Result<()> {
    if custom_roles.is_empty() {
        return Ok(());
    }

    let gcp_config = ctx.get_gcp_config()?;
    let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
    let mut seen_role_names = HashSet::new();

    for custom_role in custom_roles {
        if seen_role_names.insert(custom_role.name.clone()) {
            ensure_one(iam_client.as_ref(), permission_set_id, custom_role).await?;
        }
    }

    Ok(())
}

async fn ensure_one(
    iam_client: &dyn IamApi,
    permission_set_id: &str,
    custom_role: GcpCustomRole,
) -> Result<()> {
    let desired_role = desired_role(&custom_role);

    info!(
        role_id = %custom_role.role_id,
        permission_set = %permission_set_id,
        permissions_count = custom_role.included_permissions.len(),
        "Ensuring GCP custom role exists"
    );

    match iam_client.get_role(custom_role.name.clone()).await {
        Ok(existing_role) => {
            reconcile_existing(
                iam_client,
                permission_set_id,
                &custom_role,
                existing_role,
                desired_role,
            )
            .await
        }
        Err(error) if is_not_found(&error) => {
            let request = CreateRoleRequest {
                role: desired_role.clone(),
            };
            match iam_client
                .create_role(custom_role.role_id.clone(), request)
                .await
            {
                Ok(_) => Ok(()),
                Err(create_error) if is_conflict(&create_error) => {
                    let existing_role =
                        find_role_including_deleted(iam_client, permission_set_id, &custom_role)
                            .await?;

                    match existing_role {
                        Some(existing_role) => {
                            reconcile_existing(
                                iam_client,
                                permission_set_id,
                                &custom_role,
                                existing_role,
                                desired_role,
                            )
                            .await
                        }
                        None => Err(create_error.context(cloud_error(
                            permission_set_id,
                            format!(
                                "Failed to create custom role '{}' after a conflicting create",
                                custom_role.role_id
                            ),
                        ))),
                    }
                }
                Err(create_error) => Err(create_error.context(cloud_error(
                    permission_set_id,
                    format!("Failed to create custom role '{}'", custom_role.role_id),
                ))),
            }
        }
        Err(error) => Err(error.context(cloud_error(
            permission_set_id,
            format!(
                "Failed to check existence of custom role '{}'",
                custom_role.role_id
            ),
        ))),
    }
}

async fn reconcile_existing(
    iam_client: &dyn IamApi,
    permission_set_id: &str,
    custom_role: &GcpCustomRole,
    existing_role: Role,
    desired_role: Role,
) -> Result<()> {
    if existing_role.deleted.unwrap_or(false) {
        if !role_definition_matches(&existing_role, &desired_role) {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: permission_set_id.to_string(),
                message: format!(
                    "refusing to reactivate deleted GCP custom role '{}' because its definition does not exactly match the requested permissions",
                    custom_role.role_id
                ),
            }));
        }

        let etag = existing_role.etag.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceDrift {
                resource_id: permission_set_id.to_string(),
                message: format!(
                    "refusing to reactivate deleted GCP custom role '{}' without an etag",
                    custom_role.role_id
                ),
            })
        })?;

        iam_client
            .undelete_role(
                custom_role.name.clone(),
                UndeleteRoleRequest { etag: Some(etag) },
            )
            .await
            .context(cloud_error(
                permission_set_id,
                format!(
                    "Failed to reactivate deleted custom role '{}'",
                    custom_role.role_id
                ),
            ))?;

        return Ok(());
    }

    if role_definition_matches(&existing_role, &desired_role) {
        info!(
            role_id = %custom_role.role_id,
            permission_set = %permission_set_id,
            "GCP custom role already matches desired permissions"
        );
        return Ok(());
    }

    iam_client
        .patch_role(
            custom_role.name.clone(),
            desired_role,
            Some("includedPermissions,title,description,stage".to_string()),
        )
        .await
        .context(cloud_error(
            permission_set_id,
            format!(
                "Failed to update existing custom role '{}'",
                custom_role.role_id
            ),
        ))?;

    Ok(())
}

async fn find_role_including_deleted(
    iam_client: &dyn IamApi,
    permission_set_id: &str,
    custom_role: &GcpCustomRole,
) -> Result<Option<Role>> {
    let mut page_token = None;

    loop {
        let response = iam_client
            .list_roles(Some(1_000), page_token, Some(true), Some(RoleView::Full))
            .await
            .context(cloud_error(
                permission_set_id,
                format!(
                    "Failed to locate conflicting custom role '{}'",
                    custom_role.role_id
                ),
            ))?;

        if let Some(role) = response
            .roles
            .into_iter()
            .find(|role| role.name.as_deref() == Some(custom_role.name.as_str()))
        {
            return Ok(Some(role));
        }

        match response.next_page_token {
            Some(token) if !token.is_empty() => page_token = Some(token),
            _ => return Ok(None),
        }
    }
}

fn desired_role(custom_role: &GcpCustomRole) -> Role {
    Role::builder()
        .title(custom_role.title.clone())
        .description(custom_role.description.clone())
        .included_permissions(custom_role.included_permissions.clone())
        .stage(RoleLaunchStage::Ga)
        .build()
}

fn role_definition_matches(existing: &Role, desired: &Role) -> bool {
    let mut existing_permissions = existing.included_permissions.clone();
    let mut desired_permissions = desired.included_permissions.clone();
    existing_permissions.sort();
    desired_permissions.sort();

    existing.title == desired.title
        && existing.description == desired.description
        && existing.stage == desired.stage
        && existing_permissions == desired_permissions
}

fn is_not_found(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        error.error,
        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
    )
}

fn is_conflict(error: &AlienError<CloudClientErrorData>) -> bool {
    matches!(
        error.error,
        Some(CloudClientErrorData::RemoteResourceConflict { .. })
    )
}

fn cloud_error(permission_set_id: &str, message: String) -> ErrorData {
    ErrorData::CloudPlatformError {
        message,
        resource_id: Some(permission_set_id.to_string()),
    }
}

pub(super) async fn delete(
    ctx: &ResourceControllerContext<'_>,
    permission_context: &PermissionContext,
) -> Result<()> {
    let gcp_config = ctx.get_gcp_config()?;
    let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_config)?;
    let role_name_prefix = stack_role_name_prefix(permission_context);
    let mut role_names = Vec::new();
    let mut page_token = None;

    loop {
        let response = iam_client
            .list_roles(Some(100), page_token, Some(false), None)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to list GCP custom roles before cleanup".to_string(),
                resource_id: Some(ctx.resource_prefix.to_string()),
            })?;

        role_names.extend(response.roles.into_iter().filter_map(|role| {
            role.name
                .filter(|role_name| role_name.starts_with(&role_name_prefix))
        }));

        match response.next_page_token {
            Some(token) if !token.is_empty() => page_token = Some(token),
            _ => break,
        }
    }

    for role_name in role_names {
        let role_id = role_name
            .rsplit('/')
            .next()
            .unwrap_or(role_name.as_str())
            .to_string();
        match iam_client.delete_role(role_name).await {
            Ok(_) => info!(role_id = %role_id, "Deleted GCP custom role"),
            Err(error) if is_not_found(&error) => {
                info!(role_id = %role_id, "GCP custom role already deleted");
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete GCP custom role '{}'", role_id),
                    resource_id: Some(ctx.resource_prefix.to_string()),
                }));
            }
        }
    }

    Ok(())
}

pub(super) fn stack_role_name_prefix(permission_context: &PermissionContext) -> String {
    let project = permission_context
        .project_name
        .as_deref()
        .unwrap_or("PROJECT_NAME");
    format!(
        "projects/{project}/roles/{}",
        custom_role_prefix(permission_context)
    )
}

#[cfg(test)]
mod tests {
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_error::AlienError;
    use alien_gcp_clients::iam::{ListRolesResponse, MockIamApi};
    use mockall::{predicate::eq, Sequence};

    use super::*;

    const PERMISSION_SET_ID: &str = "storage/read";
    const ROLE_ID: &str = "role_stack_storage_read";
    const ROLE_NAME: &str = "projects/test-project/roles/role_stack_storage_read";

    fn custom_role() -> GcpCustomRole {
        GcpCustomRole {
            role_id: ROLE_ID.to_string(),
            name: ROLE_NAME.to_string(),
            title: "Read storage".to_string(),
            description: "Read objects from one bucket".to_string(),
            included_permissions: vec!["storage.objects.get".to_string()],
            stage: "GA".to_string(),
        }
    }

    fn listed_role(deleted: bool, permissions: &[&str], etag: Option<&str>) -> Role {
        Role {
            name: Some(ROLE_NAME.to_string()),
            title: Some("Read storage".to_string()),
            description: Some("Read objects from one bucket".to_string()),
            included_permissions: permissions
                .iter()
                .map(|permission| (*permission).to_string())
                .collect(),
            stage: Some(RoleLaunchStage::Ga),
            etag: etag.map(str::to_string),
            deleted: Some(deleted),
        }
    }

    fn not_found() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
            resource_type: "IAM role".to_string(),
            resource_name: ROLE_NAME.to_string(),
        })
    }

    fn conflict() -> AlienError<CloudClientErrorData> {
        AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: "IAM role".to_string(),
            resource_name: ROLE_NAME.to_string(),
            message: "role ID is marked for deletion".to_string(),
        })
    }

    fn expect_missing_then_conflicting_create(mock: &mut MockIamApi) {
        mock.expect_get_role()
            .with(eq(ROLE_NAME.to_string()))
            .times(1)
            .return_once(|_| Err(not_found()));
        mock.expect_create_role()
            .withf(|role_id, request| {
                role_id == ROLE_ID && request.role.included_permissions == ["storage.objects.get"]
            })
            .times(1)
            .return_once(|_, _| Err(conflict()));
    }

    #[tokio::test]
    async fn create_conflict_recovers_exact_soft_deleted_custom_role() {
        let mut mock = MockIamApi::new();
        expect_missing_then_conflicting_create(&mut mock);
        mock.expect_list_roles()
            .with(
                eq(Some(1_000)),
                eq(None),
                eq(Some(true)),
                eq(Some(RoleView::Full)),
            )
            .times(1)
            .return_once(|_, _, _, _| {
                Ok(ListRolesResponse {
                    roles: vec![listed_role(true, &["storage.objects.get"], Some("etag-1"))],
                    next_page_token: None,
                })
            });
        mock.expect_undelete_role()
            .withf(|role_name, request| {
                role_name == ROLE_NAME && request.etag.as_deref() == Some("etag-1")
            })
            .times(1)
            .return_once(|_, _| Ok(listed_role(false, &["storage.objects.get"], None)));

        ensure_one(&mock, PERMISSION_SET_ID, custom_role())
            .await
            .expect("an exact deleted role should be safely reactivated");
    }

    #[tokio::test]
    async fn mismatched_soft_deleted_role_fails_closed_without_undelete() {
        let mut mock = MockIamApi::new();
        expect_missing_then_conflicting_create(&mut mock);
        mock.expect_list_roles().times(1).return_once(|_, _, _, _| {
            Ok(ListRolesResponse {
                roles: vec![listed_role(
                    true,
                    &["storage.objects.get", "storage.objects.delete"],
                    Some("etag-1"),
                )],
                next_page_token: None,
            })
        });

        let error = ensure_one(&mock, PERMISSION_SET_ID, custom_role())
            .await
            .expect_err("a broader deleted role must not be reactivated");

        assert!(matches!(error.error, Some(ErrorData::ResourceDrift { .. })));
    }

    #[tokio::test]
    async fn soft_deleted_role_without_etag_fails_closed() {
        let mut mock = MockIamApi::new();
        expect_missing_then_conflicting_create(&mut mock);
        mock.expect_list_roles().times(1).return_once(|_, _, _, _| {
            Ok(ListRolesResponse {
                roles: vec![listed_role(true, &["storage.objects.get"], None)],
                next_page_token: None,
            })
        });

        let error = ensure_one(&mock, PERMISSION_SET_ID, custom_role())
            .await
            .expect_err("a deleted role without an etag must not be reactivated");

        assert!(matches!(error.error, Some(ErrorData::ResourceDrift { .. })));
    }

    #[tokio::test]
    async fn missing_role_is_created_with_the_exact_definition() {
        let mut mock = MockIamApi::new();
        mock.expect_get_role()
            .with(eq(ROLE_NAME.to_string()))
            .times(1)
            .return_once(|_| Err(not_found()));
        mock.expect_create_role()
            .withf(|role_id, request| {
                role_id == ROLE_ID
                    && request.role.title.as_deref() == Some("Read storage")
                    && request.role.description.as_deref() == Some("Read objects from one bucket")
                    && request.role.included_permissions == ["storage.objects.get"]
                    && request.role.stage == Some(RoleLaunchStage::Ga)
            })
            .times(1)
            .return_once(|_, request| Ok(request.role));

        ensure_one(&mock, PERMISSION_SET_ID, custom_role())
            .await
            .expect("a missing role should be created");
    }

    #[tokio::test]
    async fn active_mismatched_role_is_patched_to_the_exact_definition() {
        let mut mock = MockIamApi::new();
        mock.expect_get_role()
            .with(eq(ROLE_NAME.to_string()))
            .times(1)
            .return_once(|_| {
                Ok(listed_role(
                    false,
                    &["storage.objects.get", "storage.objects.delete"],
                    Some("etag-1"),
                ))
            });
        mock.expect_patch_role()
            .withf(|role_name, role, update_mask| {
                role_name == ROLE_NAME
                    && role.included_permissions == ["storage.objects.get"]
                    && update_mask.as_deref() == Some("includedPermissions,title,description,stage")
            })
            .times(1)
            .return_once(|_, role, _| Ok(role));

        ensure_one(&mock, PERMISSION_SET_ID, custom_role())
            .await
            .expect("an active mismatched role should be reconciled");
    }

    #[tokio::test]
    async fn active_role_found_after_create_race_needs_no_mutation() {
        let mut mock = MockIamApi::new();
        expect_missing_then_conflicting_create(&mut mock);
        mock.expect_list_roles().times(1).return_once(|_, _, _, _| {
            Ok(ListRolesResponse {
                roles: vec![listed_role(false, &["storage.objects.get"], None)],
                next_page_token: None,
            })
        });

        ensure_one(&mock, PERMISSION_SET_ID, custom_role())
            .await
            .expect("an exact role created concurrently should be accepted");
    }

    #[tokio::test]
    async fn paginated_no_match_preserves_create_conflict() {
        let mut mock = MockIamApi::new();
        expect_missing_then_conflicting_create(&mut mock);
        let mut sequence = Sequence::new();
        mock.expect_list_roles()
            .with(
                eq(Some(1_000)),
                eq(None),
                eq(Some(true)),
                eq(Some(RoleView::Full)),
            )
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _, _, _| {
                Ok(ListRolesResponse {
                    roles: vec![],
                    next_page_token: Some("next".to_string()),
                })
            });
        mock.expect_list_roles()
            .with(
                eq(Some(1_000)),
                eq(Some("next".to_string())),
                eq(Some(true)),
                eq(Some(RoleView::Full)),
            )
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _, _, _| Ok(ListRolesResponse::default()));

        let error = ensure_one(&mock, PERMISSION_SET_ID, custom_role())
            .await
            .expect_err("the original create conflict should be returned when no role is found");

        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR");
        assert_eq!(
            error.source.as_ref().map(|source| source.code.as_str()),
            Some("REMOTE_RESOURCE_CONFLICT")
        );
    }
}
