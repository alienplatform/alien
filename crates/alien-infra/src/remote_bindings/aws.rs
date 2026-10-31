use std::time::Duration;

use alien_aws_clients::iam::{CreateRoleRequest, CreateRoleTag, IamRoleWriter};
use alien_core::{
    standard_resource_tags, RemoteBindings, RemoteBindingsOutputs, ResourceOutputs, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError};
use alien_macros::controller;

use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
};

#[controller]
pub struct AwsRemoteBindingsController {
    pub(crate) role_arn: Option<String>,
    pub(crate) role_name: Option<String>,
}

#[controller]
impl AwsRemoteBindingsController {
    #[flow_entry(Create)]
    #[handler(state = CreatingRole, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<RemoteBindings>()?;
        let management = ctx.get_aws_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "AWS management configuration is required for Remote Bindings".to_string(),
                operation: Some("create_remote_bindings_identity".to_string()),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let role_name = format!("{}-access", ctx.resource_prefix);
        let client = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        let response = IamRoleWriter::new(
            client.as_ref(),
            ctx.aws_permissions_boundary_arn()?.as_deref(),
        )
        .create(
            CreateRoleRequest::builder()
                .role_name(role_name.clone())
                .assume_role_policy_document(trust_policy(&management.managing_role_arn))
                .description(format!(
                    "Application access identity. Resource prefix: {}.",
                    ctx.resource_prefix
                ))
                .tags(
                    standard_resource_tags(ctx.resource_prefix, &config.id)
                        .into_iter()
                        .map(|(key, value)| CreateRoleTag { key, value })
                        .collect(),
                )
                .build(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create Remote Bindings IAM role '{role_name}'"),
            resource_id: Some(config.id.clone()),
        })?;
        self.role_arn = Some(response.create_role_result.role.arn);
        self.role_name = Some(role_name);
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, _ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let resource_id = ctx.desired_resource_config::<RemoteBindings>()?.id.clone();
        let role_name = self.role_name.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                resource_id: Some(resource_id.clone()),
                operation: Some("update_remote_bindings_identity".to_string()),
                message: "Remote Bindings role name is missing".to_string(),
            })
        })?;
        let management = ctx.get_aws_management_config()?.ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "AWS management configuration is required for Remote Bindings".to_string(),
                operation: Some("update_remote_bindings_identity".to_string()),
                resource_id: Some(resource_id.clone()),
            })
        })?;
        let client = ctx
            .service_provider
            .get_aws_iam_client(ctx.get_aws_config()?)
            .await?;
        IamRoleWriter::new(
            client.as_ref(),
            ctx.aws_permissions_boundary_arn()?.as_deref(),
        )
        .prepare_existing(role_name, ctx.resource_prefix, &resource_id)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to prepare Remote Bindings IAM role '{role_name}'"),
            resource_id: Some(resource_id.clone()),
        })?;
        client
            .update_assume_role_policy(role_name, &trust_policy(&management.managing_role_arn))
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to update Remote Bindings IAM role trust".to_string(),
                resource_id: Some(resource_id),
            })?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let resource_id = ctx.desired_resource_config::<RemoteBindings>()?.id.clone();
        if let Some(role_name) = self.role_name.as_deref() {
            let client = ctx
                .service_provider
                .get_aws_iam_client(ctx.get_aws_config()?)
                .await?;
            match IamRoleWriter::new(
                client.as_ref(),
                ctx.aws_permissions_boundary_arn()?.as_deref(),
            )
            .prepare_for_removal(role_name, ctx.resource_prefix, &resource_id)
            .await
            {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    self.role_name = None;
                    self.role_arn = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to prepare Remote Bindings IAM role '{role_name}' for deletion"
                        ),
                        resource_id: Some(resource_id.clone()),
                    }))
                }
            }
            match client.list_role_policies(role_name).await {
                Ok(response) => {
                    if let Some(policy_names) = response.list_role_policies_result.policy_names {
                        for policy_name in &policy_names.member {
                            match client.delete_role_policy(role_name, policy_name).await {
                                Ok(()) => {}
                                Err(error)
                                    if matches!(
                                        error.error,
                                        Some(
                                            alien_client_core::ErrorData::RemoteResourceNotFound { .. }
                                        )
                                    ) => {}
                                Err(error) => {
                                    return Err(error.context(ErrorData::CloudPlatformError {
                                        message: format!(
                                            "Failed to remove Remote Bindings policy \
                                             '{policy_name}'"
                                        ),
                                        resource_id: Some(resource_id.clone()),
                                    }));
                                }
                            }
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    self.role_name = None;
                    self.role_arn = None;
                    return Ok(HandlerAction::Continue {
                        state: Deleted,
                        suggested_delay: None,
                    });
                }
                Err(error) => {
                    return Err(error
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to list Remote Bindings policies".to_string(),
                            resource_id: Some(resource_id.clone()),
                        })
                        .into());
                }
            }
            match client.delete_role(role_name).await {
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error
                        .context(ErrorData::CloudPlatformError {
                            message: "Failed to delete Remote Bindings IAM role".to_string(),
                            resource_id: Some(resource_id),
                        })
                        .into())
                }
            }
        }
        self.role_name = None;
        self.role_arn = None;
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(RemoteBindingsOutputs {
            resource_id: self.role_arn.clone()?,
            access_configuration: self.role_arn.clone()?,
        }))
    }
}

fn trust_policy(managing_role_arn: &str) -> String {
    serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Sid": "AllowOnlySpecificSourceRole",
            "Effect": "Allow",
            "Principal": { "AWS": managing_role_arn },
            "Action": "sts:AssumeRole",
            "Condition": { "StringEquals": {
                "aws:PrincipalArn": managing_role_arn,
            }},
        }],
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use alien_aws_clients::iam::{
        ListRolePoliciesResponse, ListRolePoliciesResult, MockIamApi, PolicyNames,
    };
    use alien_core::{Platform, RemoteBindings, ResourceStatus};

    use super::*;
    use crate::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;

    const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/test-deployment-management-boundary";

    fn role() -> serde_json::Value {
        serde_json::json!({
            "Path": "/", "RoleName": "test-access", "RoleId": "id",
            "Arn": "arn:aws:iam::123456789012:role/test-access", "CreateDate": "2026-01-01",
            "Tags": {"member": standard_resource_tags("test", "access").into_iter()
                .map(|(key, value)| serde_json::json!({"Key": key, "Value": value}))
                .collect::<Vec<_>>()}
        })
    }

    async fn executor(
        iam: MockIamApi,
        state: AwsRemoteBindingsState,
        bounded: bool,
    ) -> SingleControllerExecutor {
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        let mut management =
            crate::remote_stack_management::AwsRemoteStackManagementController::mock_ready(
                "test-management",
            );
        if bounded {
            management.permissions_boundary_arn = Some(BOUNDARY.to_string());
        }
        SingleControllerExecutor::builder()
            .resource(RemoteBindings::new("access".to_string()).build())
            .controller(AwsRemoteBindingsController {
                state,
                role_name: Some("test-access".to_string()),
                role_arn: Some("arn:aws:iam::123456789012:role/test-access".to_string()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .with_dependency(
                alien_core::RemoteStackManagement::new("management".to_string()).build(),
                management,
            )
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap()
    }

    /// Removal adopts the role without auditing its attached policies.
    fn expect_removal_adoption(iam: &mut MockIamApi, sequence: &mut mockall::Sequence) {
        iam.expect_get_role()
            .times(1)
            .in_sequence(sequence)
            .returning(|name| {
                assert_eq!(name, "test-access");
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role()}}))
                        .unwrap(),
                )
            });
        iam.expect_put_role_permissions_boundary()
            .times(1)
            .in_sequence(sequence)
            .returning(|name, boundary| {
                assert_eq!(name, "test-access");
                assert_eq!(boundary, BOUNDARY);
                Ok(())
            });
    }

    fn expect_adoption(iam: &mut MockIamApi, sequence: &mut mockall::Sequence) {
        iam.expect_get_role()
            .times(1)
            .in_sequence(sequence)
            .returning(|name| {
                assert_eq!(name, "test-access");
                Ok(
                    serde_json::from_value(serde_json::json!({"GetRoleResult": {"Role": role()}}))
                        .unwrap(),
                )
            });
        iam.expect_list_attached_role_policies()
            .times(1)
            .in_sequence(sequence)
            .returning(|name| {
                assert_eq!(name, "test-access");
                Ok(serde_json::from_value(
                    serde_json::json!({"ListAttachedRolePoliciesResult": {"IsTruncated": false}}),
                )
                .unwrap())
            });
        iam.expect_put_role_permissions_boundary()
            .times(1)
            .in_sequence(sequence)
            .returning(|name, boundary| {
                assert_eq!(name, "test-access");
                assert_eq!(boundary, BOUNDARY);
                Ok(())
            });
    }

    #[tokio::test]
    async fn create_passes_the_recorded_boundary_and_ownership_tags() {
        for bounded in [false, true] {
            let mut iam = MockIamApi::new();
            iam.expect_create_role().times(1).returning(move |request| {
                assert_eq!(request.role_name, "test-access");
                assert_eq!(
                    request.permissions_boundary.as_deref(),
                    bounded.then_some(BOUNDARY)
                );
                let tags: std::collections::HashMap<_, _> = request
                    .tags
                    .unwrap()
                    .into_iter()
                    .map(|tag| (tag.key, tag.value))
                    .collect();
                assert_eq!(tags, standard_resource_tags("test", "access"));
                assert_eq!(
                    request.assume_role_policy_document,
                    trust_policy("arn:aws:iam::111122223333:role/alien-test-manager")
                );
                Ok(serde_json::from_value(
                    serde_json::json!({"CreateRoleResult": {"Role": role()}}),
                )
                .unwrap())
            });
            let mut executor = executor(iam, AwsRemoteBindingsState::CreatingRole, bounded).await;
            executor.step().await.unwrap();
            let controller = executor
                .internal_state::<AwsRemoteBindingsController>()
                .unwrap();
            assert_eq!(controller.state, AwsRemoteBindingsState::Ready);
            assert_eq!(controller.role_name.as_deref(), Some("test-access"));
            assert_eq!(
                controller.role_arn.as_deref(),
                Some("arn:aws:iam::123456789012:role/test-access")
            );
        }
    }

    #[tokio::test]
    async fn update_adopts_before_writing_trust_and_preserves_legacy_behavior() {
        for bounded in [false, true] {
            let mut iam = MockIamApi::new();
            let mut sequence = mockall::Sequence::new();
            if bounded {
                expect_adoption(&mut iam, &mut sequence);
            }
            iam.expect_update_assume_role_policy()
                .times(1)
                .in_sequence(&mut sequence)
                .returning(|name, policy| {
                    assert_eq!(name, "test-access");
                    assert_eq!(
                        policy,
                        trust_policy("arn:aws:iam::111122223333:role/alien-test-manager")
                    );
                    Ok(())
                });
            let mut executor = executor(iam, AwsRemoteBindingsState::UpdateStart, bounded).await;
            executor.step().await.unwrap();
            assert_eq!(
                executor
                    .internal_state::<AwsRemoteBindingsController>()
                    .unwrap()
                    .state,
                AwsRemoteBindingsState::Ready
            );
        }
    }

    #[tokio::test]
    async fn delete_adopts_before_removing_policies_and_role() {
        let mut iam = MockIamApi::new();
        let mut sequence = mockall::Sequence::new();
        expect_removal_adoption(&mut iam, &mut sequence);
        iam.expect_list_role_policies()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name| {
                assert_eq!(name, "test-access");
                Ok(
                    serde_json::from_value(serde_json::json!({"ListRolePoliciesResult": {
                        "PolicyNames": {"member": ["grant"]}, "IsTruncated": false
                    }}))
                    .unwrap(),
                )
            });
        iam.expect_delete_role_policy()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name, policy| {
                assert_eq!((name, policy), ("test-access", "grant"));
                Ok(())
            });
        iam.expect_delete_role()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|name| {
                assert_eq!(name, "test-access");
                Ok(())
            });
        let mut executor = executor(iam, AwsRemoteBindingsState::DeleteStart, true).await;
        executor.step().await.unwrap();
        let controller = executor
            .internal_state::<AwsRemoteBindingsController>()
            .unwrap();
        assert_eq!(controller.state, AwsRemoteBindingsState::Deleted);
        assert_eq!(controller.role_name, None);
        assert_eq!(controller.role_arn, None);
    }

    #[tokio::test]
    async fn missing_role_completes_bounded_delete_without_mutations() {
        let mut iam = MockIamApi::new();
        iam.expect_get_role().times(1).returning(|name| {
            Err(AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "IAM Role".to_string(),
                    resource_name: name.to_string(),
                },
            ))
        });
        let mut executor = executor(iam, AwsRemoteBindingsState::DeleteStart, true).await;
        executor.step().await.unwrap();
        let controller = executor
            .internal_state::<AwsRemoteBindingsController>()
            .unwrap();
        assert_eq!(controller.state, AwsRemoteBindingsState::Deleted);
        assert_eq!(controller.role_name, None);
        assert_eq!(controller.role_arn, None);
    }

    #[tokio::test]
    async fn unsafe_roles_stop_update_and_delete_before_mutation() {
        for state in [
            AwsRemoteBindingsState::UpdateStart,
            AwsRemoteBindingsState::DeleteStart,
        ] {
            for unsafe_policy in [false, true] {
                // AWS managed policies keep a role from being adopted to keep running, not
                // from being deleted.
                if unsafe_policy && state == AwsRemoteBindingsState::DeleteStart {
                    continue;
                }
                let mut iam = MockIamApi::new();
                iam.expect_get_role().times(1).returning(move |_| {
                    let mut role = role();
                    if !unsafe_policy {
                        role["Tags"] = serde_json::Value::Null;
                    }
                    Ok(
                        serde_json::from_value(
                            serde_json::json!({"GetRoleResult": {"Role": role}}),
                        )
                        .unwrap(),
                    )
                });
                if unsafe_policy {
                    iam.expect_list_attached_role_policies().times(1).returning(|_| {
                        Ok(serde_json::from_value(serde_json::json!({"ListAttachedRolePoliciesResult": {
                            "AttachedPolicies": {"member": [{"PolicyName": "AdministratorAccess", "PolicyArn": "arn:aws:iam::aws:policy/AdministratorAccess"}]}, "IsTruncated": false
                        }})).unwrap())
                    });
                }
                let mut executor = executor(iam, state.clone(), true).await;
                let error = executor
                    .step()
                    .await
                    .expect_err("unsafe role must not be modified");
                let expected_code = if unsafe_policy {
                    "INVALID_INPUT"
                } else {
                    "EXISTING_CLOUD_RESOURCE_NOT_OWNED"
                };
                assert!(
                    std::iter::successors(error.source.as_deref(), |cause| cause.source.as_deref())
                        .any(|cause| cause.code == expected_code),
                    "{error:?}"
                );
                let controller = executor
                    .internal_state::<AwsRemoteBindingsController>()
                    .unwrap();
                assert_eq!(controller.state, state);
                assert_eq!(controller.role_name.as_deref(), Some("test-access"));
                assert!(controller.role_arn.is_some());
            }
        }
    }

    /// Setup writes each published resource's grant onto this role and never records it, so the
    /// role's delete is what revokes them: every inline policy goes before the role.
    #[tokio::test]
    async fn delete_removes_every_inline_policy_then_the_role() {
        assert_eq!(
            delete_with(|| Ok(())).await,
            (
                Ok(ResourceStatus::Deleted),
                vec![
                    "policy:alien-agents-remote-access".to_string(),
                    "role".to_string()
                ]
            )
        );
    }

    /// A listed policy another step already removed is gone, not a failure.
    #[tokio::test]
    async fn delete_passes_a_policy_already_removed() {
        let gone = || {
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RemoteResourceNotFound {
                    resource_type: "IAM Role Policy".to_string(),
                    resource_name: "alien-agents-remote-access".to_string(),
                },
            ))
        };
        assert_eq!(
            delete_with(gone).await,
            (
                Ok(ResourceStatus::Deleted),
                vec![
                    "policy:alien-agents-remote-access".to_string(),
                    "role".to_string()
                ]
            )
        );
    }

    /// DeleteRole refuses a role that still holds a policy, so a policy that could not be removed
    /// stops the delete before the role.
    #[tokio::test]
    async fn delete_stops_before_the_role_when_a_policy_cannot_be_removed() {
        let denied = || {
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RemoteAccessDenied {
                    resource_type: "IAM Role Policy".to_string(),
                    resource_name: "alien-agents-remote-access".to_string(),
                },
            ))
        };
        assert_eq!(
            delete_with(denied).await,
            (
                Err("CLOUD_PLATFORM_ERROR".to_string()),
                vec!["policy:alien-agents-remote-access".to_string()]
            )
        );
    }

    async fn delete_with(
        policy_answer: fn() -> alien_client_core::Result<()>,
    ) -> (std::result::Result<ResourceStatus, String>, Vec<String>) {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut iam = MockIamApi::new();
        iam.expect_list_role_policies().returning(|_| {
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: vec!["alien-agents-remote-access".to_string()],
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        let seen = calls.clone();
        iam.expect_delete_role_policy()
            .withf(|role, _| role == "test-access")
            .times(1)
            .returning(move |_, policy| {
                seen.lock().unwrap().push(format!("policy:{policy}"));
                policy_answer()
            });
        let seen = calls.clone();
        iam.expect_delete_role()
            .withf(|role| role == "test-access")
            .times(0..=1)
            .returning(move |_| {
                seen.lock().unwrap().push("role".to_string());
                Ok(())
            });
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));

        let mut executor = SingleControllerExecutor::builder()
            .resource(RemoteBindings::new("access".to_string()).build())
            .controller(AwsRemoteBindingsController {
                state: AwsRemoteBindingsState::Ready,
                role_arn: Some("arn:aws:iam::123456789012:role/test-access".to_string()),
                role_name: Some("test-access".to_string()),
                ..Default::default()
            })
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.delete().unwrap();
        let outcome = executor
            .run_until_terminal()
            .await
            .map(|()| executor.status())
            .map_err(|error| error.code);

        let calls = calls.lock().unwrap().clone();
        (outcome, calls)
    }
}
