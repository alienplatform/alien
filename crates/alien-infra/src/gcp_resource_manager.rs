use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use google_cloud_iam_v1::model::{GetPolicyOptions, Policy};
use google_cloud_resourcemanager_v3::client::Projects;

pub(crate) async fn get_project_iam_policy(
    client: &Projects,
    project_id: &str,
    options: Option<GetPolicyOptions>,
) -> Result<Policy> {
    let mut request = client
        .get_iam_policy()
        .set_resource(format!("projects/{project_id}"));
    if let Some(options) = options {
        request = request.set_options(options);
    }

    request
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Resource Manager get_iam_policy request failed".to_string(),
            resource_id: Some(project_id.to_string()),
        })
}

pub(crate) async fn set_project_iam_policy(
    client: &Projects,
    project_id: &str,
    policy: Policy,
    update_mask: Option<String>,
) -> Result<Policy> {
    if let Some(update_mask) = update_mask {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Resource Manager set_project_iam_policy update_mask '{update_mask}' is not supported by the official client path yet"
            ),
            resource_id: Some(project_id.to_string()),
        }));
    }

    client
        .set_iam_policy()
        .set_resource(format!("projects/{project_id}"))
        .set_policy(policy)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Resource Manager set_iam_policy request failed".to_string(),
            resource_id: Some(project_id.to_string()),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use google_cloud_gax::{options::RequestOptions, response::Response};
    use google_cloud_iam_v1::model::{GetIamPolicyRequest, SetIamPolicyRequest};
    use google_cloud_resourcemanager_v3::stub::Projects as ProjectsStub;

    mockall::mock! {
        #[derive(Debug)]
        Projects {}

        impl ProjectsStub for Projects {
            async fn get_iam_policy(
                &self,
                request: GetIamPolicyRequest,
                options: RequestOptions,
            ) -> google_cloud_resourcemanager_v3::Result<Response<Policy>>;

            async fn set_iam_policy(
                &self,
                request: SetIamPolicyRequest,
                options: RequestOptions,
            ) -> google_cloud_resourcemanager_v3::Result<Response<Policy>>;
        }
    }

    #[tokio::test]
    async fn project_iam_helpers_use_sdk_native_projects_stub() {
        let mut stub = MockProjects::new();
        stub.expect_get_iam_policy()
            .withf(|request, _| {
                request.resource == "projects/test-project"
                    && request
                        .options
                        .as_ref()
                        .is_some_and(|options| options.requested_policy_version == 3)
            })
            .once()
            .returning(|_, _| Ok(Response::from(Policy::new().set_version(3))));
        stub.expect_set_iam_policy()
            .withf(|request, _| {
                request.resource == "projects/test-project"
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

        let client = Projects::from_stub(stub);
        let current = get_project_iam_policy(
            &client,
            "test-project",
            Some(GetPolicyOptions::new().set_requested_policy_version(3)),
        )
        .await
        .expect("project IAM policy should be fetched");
        assert_eq!(current.version, 3);

        let updated =
            set_project_iam_policy(&client, "test-project", Policy::new().set_version(3), None)
                .await
                .expect("project IAM policy should be set");
        assert_eq!(updated.version, 3);
    }
}
