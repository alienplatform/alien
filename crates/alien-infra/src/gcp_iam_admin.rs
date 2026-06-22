use crate::error::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use google_cloud_iam_admin_v1::client::Iam;
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
