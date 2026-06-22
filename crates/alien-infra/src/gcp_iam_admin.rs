use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use google_cloud_gax::error::rpc::Code as GaxRpcCode;
use google_cloud_iam_admin_v1::client::Iam;
use google_cloud_iam_admin_v1::model::{
    CreateRoleRequest, CreateServiceAccountRequest, ListRolesResponse, Role, ServiceAccount,
};
use google_cloud_iam_v1::model::{GetPolicyOptions, Policy};

pub(crate) fn service_account_resource_name(
    project_id: &str,
    service_account_name: &str,
) -> String {
    if service_account_name.starts_with("projects/") {
        service_account_name.to_string()
    } else {
        format!("projects/{project_id}/serviceAccounts/{service_account_name}")
    }
}

pub(crate) async fn get_service_account_iam_policy(
    client: &Iam,
    project_id: &str,
    service_account_name: &str,
    options: Option<GetPolicyOptions>,
) -> Result<Policy> {
    let mut request = client
        .get_iam_policy()
        .set_resource(service_account_resource_name(
            project_id,
            service_account_name,
        ));
    if let Some(options) = options {
        request = request.set_options(options);
    }

    request
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "IAM get_iam_policy request failed".to_string(),
            resource_id: Some(service_account_name.to_string()),
        })
}

pub(crate) async fn create_service_account(
    client: &Iam,
    project_id: &str,
    mut request: CreateServiceAccountRequest,
) -> Result<ServiceAccount> {
    if request.name.is_empty() {
        request.name = format!("projects/{project_id}");
    }
    let account_id = request.account_id.clone();

    match client
        .create_service_account()
        .with_request(request)
        .send()
        .await
    {
        Ok(service_account) => Ok(service_account),
        Err(error) if gax_error_is_conflict(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceConflict {
                resource_type: "GCP service account".to_string(),
                resource_name: account_id,
                message: "create_service_account reported the account already exists".to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "IAM create_service_account request failed".to_string(),
                resource_id: Some(account_id),
            })),
    }
}

pub(crate) async fn delete_service_account(
    client: &Iam,
    project_id: &str,
    service_account_name: &str,
) -> Result<()> {
    match client
        .delete_service_account()
        .set_name(service_account_resource_name(
            project_id,
            service_account_name,
        ))
        .send()
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCP service account".to_string(),
                resource_name: service_account_name.to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "IAM delete_service_account request failed".to_string(),
                resource_id: Some(service_account_name.to_string()),
            })),
    }
}

pub(crate) async fn get_service_account(
    client: &Iam,
    project_id: &str,
    service_account_name: &str,
) -> Result<ServiceAccount> {
    match client
        .get_service_account()
        .set_name(service_account_resource_name(
            project_id,
            service_account_name,
        ))
        .send()
        .await
    {
        Ok(service_account) => Ok(service_account),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCP service account".to_string(),
                resource_name: service_account_name.to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "IAM get_service_account request failed".to_string(),
                resource_id: Some(service_account_name.to_string()),
            })),
    }
}

pub(crate) async fn create_role(
    client: &Iam,
    project_id: &str,
    mut request: CreateRoleRequest,
) -> Result<Role> {
    if request.parent.is_empty() {
        request.parent = format!("projects/{project_id}");
    }
    let role_id = request.role_id.clone();

    match client.create_role().with_request(request).send().await {
        Ok(role) => Ok(role),
        Err(error) if gax_error_is_conflict(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceConflict {
                resource_type: "GCP custom role".to_string(),
                resource_name: role_id,
                message: "create_role reported the role already exists".to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "IAM create_role request failed".to_string(),
                resource_id: Some(role_id),
            })),
    }
}

pub(crate) async fn delete_role(client: &Iam, role_name: &str) -> Result<Role> {
    client
        .delete_role()
        .set_name(full_role_name("", role_name))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "IAM delete_role request failed".to_string(),
            resource_id: Some(role_name.to_string()),
        })
}

pub(crate) async fn undelete_role(client: &Iam, role_name: &str) -> Result<Role> {
    client
        .undelete_role()
        .set_name(full_role_name("", role_name))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "IAM undelete_role request failed".to_string(),
            resource_id: Some(role_name.to_string()),
        })
}

pub(crate) async fn get_role(client: &Iam, project_id: &str, role_name: &str) -> Result<Role> {
    match client
        .get_role()
        .set_name(full_role_name(project_id, role_name))
        .send()
        .await
    {
        Ok(role) => Ok(role),
        Err(error) if gax_error_is_not_found(&error) => {
            Err(AlienError::new(ErrorData::CloudResourceNotFound {
                resource_type: "GCP custom role".to_string(),
                resource_name: role_name.to_string(),
            }))
        }
        Err(error) => Err(error
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "IAM get_role request failed".to_string(),
                resource_id: Some(role_name.to_string()),
            })),
    }
}

pub(crate) async fn list_roles(
    client: &Iam,
    project_id: &str,
    page_size: Option<i32>,
    page_token: Option<String>,
    show_deleted: Option<bool>,
) -> Result<ListRolesResponse> {
    let mut request = client
        .list_roles()
        .set_parent(format!("projects/{project_id}"));
    if let Some(page_size) = page_size {
        request = request.set_page_size(page_size);
    }
    if let Some(page_token) = page_token {
        request = request.set_page_token(page_token);
    }
    if let Some(show_deleted) = show_deleted {
        request = request.set_show_deleted(show_deleted);
    }

    request
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "IAM list_roles request failed".to_string(),
            resource_id: Some(project_id.to_string()),
        })
}

pub(crate) async fn patch_role(
    client: &Iam,
    project_id: &str,
    role_name: &str,
    role: Role,
    update_mask: Option<String>,
) -> Result<Role> {
    let mut request = client
        .update_role()
        .set_name(full_role_name(project_id, role_name))
        .set_role(role);
    if let Some(update_mask) = update_mask {
        request = request.set_update_mask(field_mask_from_comma_separated(update_mask));
    }

    request
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "IAM patch_role request failed".to_string(),
            resource_id: Some(role_name.to_string()),
        })
}

fn full_role_name(project_id: &str, role_name: &str) -> String {
    if role_name.starts_with("projects/") || role_name.starts_with("organizations/") {
        role_name.to_string()
    } else {
        format!("projects/{project_id}/roles/{role_name}")
    }
}

fn field_mask_from_comma_separated(update_mask: String) -> wkt::FieldMask {
    wkt::FieldMask::default().set_paths(
        update_mask
            .split(',')
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(ToString::to_string),
    )
}

fn gax_error_is_not_found(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::NotFound)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::NOT_FOUND.as_u16())
}

fn gax_error_is_conflict(error: &google_cloud_gax::error::Error) -> bool {
    error
        .status()
        .is_some_and(|status| status.code == GaxRpcCode::AlreadyExists)
        || error
            .http_status_code()
            .is_some_and(|code| code == http::StatusCode::CONFLICT.as_u16())
}

pub(crate) async fn set_service_account_iam_policy(
    client: &Iam,
    project_id: &str,
    service_account_name: &str,
    policy: Policy,
) -> Result<Policy> {
    client
        .set_iam_policy()
        .set_resource(service_account_resource_name(
            project_id,
            service_account_name,
        ))
        .set_policy(policy)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "IAM set_iam_policy request failed".to_string(),
            resource_id: Some(service_account_name.to_string()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use google_cloud_gax::{options::RequestOptions, response::Response};
    use google_cloud_iam_admin_v1::stub::Iam as IamStub;
    use google_cloud_iam_v1::model::{GetIamPolicyRequest, SetIamPolicyRequest};

    mockall::mock! {
        #[derive(Debug)]
        Iam {}

        impl IamStub for Iam {
            async fn get_iam_policy(
                &self,
                request: GetIamPolicyRequest,
                options: RequestOptions,
            ) -> google_cloud_iam_admin_v1::Result<Response<Policy>>;

            async fn set_iam_policy(
                &self,
                request: SetIamPolicyRequest,
                options: RequestOptions,
            ) -> google_cloud_iam_admin_v1::Result<Response<Policy>>;
        }
    }

    #[tokio::test]
    async fn service_account_iam_helpers_use_sdk_native_iam_stub() {
        let mut stub = MockIam::new();
        stub.expect_get_iam_policy()
            .withf(|request, _| {
                request.resource
                    == "projects/test-project/serviceAccounts/runtime@test.iam.gserviceaccount.com"
                    && request
                        .options
                        .as_ref()
                        .is_some_and(|options| options.requested_policy_version == 3)
            })
            .once()
            .returning(|_, _| Ok(Response::from(Policy::new().set_version(3))));
        stub.expect_set_iam_policy()
            .withf(|request, _| {
                request.resource
                    == "projects/test-project/serviceAccounts/runtime@test.iam.gserviceaccount.com"
                    && request
                        .policy
                        .as_ref()
                        .is_some_and(|policy| policy.version == 3)
            })
            .once()
            .returning(|request, _| {
                Ok(Response::from(
                    request.policy.expect("set request should include policy"),
                ))
            });

        let client = Iam::from_stub(stub);
        let current = get_service_account_iam_policy(
            &client,
            "test-project",
            "runtime@test.iam.gserviceaccount.com",
            Some(GetPolicyOptions::new().set_requested_policy_version(3)),
        )
        .await
        .expect("service account IAM policy should be fetched");
        assert_eq!(current.version, 3);

        let updated = set_service_account_iam_policy(
            &client,
            "test-project",
            "runtime@test.iam.gserviceaccount.com",
            Policy::new().set_version(3),
        )
        .await
        .expect("service account IAM policy should be set");
        assert_eq!(updated.version, 3);
    }
}
