use std::sync::{Arc, Mutex};

use alien_aws_clients::iam::{
    AttachedPolicies, AttachedPolicy, CreatePolicyResponse, CreatePolicyResult,
    CreatePolicyVersionResponse, CreatePolicyVersionResult, ListAttachedRolePoliciesResponse,
    ListAttachedRolePoliciesResult, ListPolicyVersionsResponse, ListPolicyVersionsResult,
    MockIamApi, Policy, PolicyVersion, PolicyVersions,
};
use alien_client_core::ErrorData as ClientError;
use futures::executor::block_on;
use mockall::Sequence;

use super::*;
use crate::core::{
    controller_test::SingleControllerExecutor, MockPlatformServiceProvider, ResourceController,
};

const ARN: &str = "arn:aws:iam::123456789012:policy/test-prefix-deployment-management-boundary";

async fn executor(iam: MockIamApi) -> SingleControllerExecutor {
    let iam = Arc::new(iam);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam.clone()));
    SingleControllerExecutor::builder()
        .resource(RemoteStackManagement::new("management".to_string()).build())
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(AwsRemoteStackManagementController::default())
        .platform(Platform::Aws)
        .resource_prefix("test-prefix")
        .with_stack_resource(
            crate::core::controller_test::test_function_1(),
            ResourceLifecycle::Live,
        )
        .service_provider(Arc::new(provider))
        .build()
        .await
        .unwrap()
}

fn version(id: &str, default: bool) -> PolicyVersion {
    PolicyVersion {
        document: None,
        version_id: id.to_string(),
        is_default_version: default,
        create_date: Some(id.to_string()),
    }
}

fn versions() -> ListPolicyVersionsResponse {
    ListPolicyVersionsResponse {
        list_policy_versions_result: ListPolicyVersionsResult {
            versions: Some(PolicyVersions {
                member: (1..=5).map(|i| version(&format!("v{i}"), i == 1)).collect(),
            }),
            is_truncated: Some(false),
            marker: None,
        },
    }
}

#[tokio::test]
async fn installation_records_exact_document_and_retries_without_reinstalling() {
    let installed = Arc::new(Mutex::new(None));
    let captured = installed.clone();
    let mut iam = MockIamApi::new();
    let mut sequence = Sequence::new();
    iam.expect_create_policy()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _, _| {
            Err(AlienError::new(ClientError::Timeout {
                message: "write failed".to_string(),
            }))
        });
    iam.expect_create_policy()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |name, document, _| {
            assert_eq!(name, "test-prefix-deployment-management-boundary");
            *captured.lock().unwrap() =
                Some(serde_json::from_str::<serde_json::Value>(document).unwrap());
            Ok(CreatePolicyResponse {
                create_policy_result: CreatePolicyResult {
                    policy: Policy {
                        policy_name: Some(name.to_string()),
                        policy_id: None,
                        arn: ARN.to_string(),
                        path: None,
                        default_version_id: None,
                        attachment_count: None,
                        is_attachable: None,
                        create_date: None,
                        update_date: None,
                    },
                },
            })
        });
    let executor = executor(iam).await;
    let mut controller = AwsRemoteStackManagementController::default();
    executor
        .with_context(|ctx| block_on(controller.install_permissions_boundary(ctx)))
        .unwrap_err();
    assert_eq!(controller.permissions_boundary_arn.as_deref(), Some(ARN));
    assert!(controller.permissions_boundary_document.is_none());
    executor
        .with_context(|ctx| block_on(controller.install_permissions_boundary(ctx)))
        .unwrap();
    assert_eq!(controller.permissions_boundary_arn.as_deref(), Some(ARN));
    assert_eq!(
        controller.permissions_boundary_document,
        *installed.lock().unwrap()
    );
    executor
        .with_context(|ctx| block_on(controller.install_permissions_boundary(ctx)))
        .unwrap();
    let restored: AwsRemoteStackManagementController =
        serde_json::from_value(serde_json::to_value(&controller).unwrap()).unwrap();
    assert_eq!(
        restored.permissions_boundary_document,
        controller.permissions_boundary_document
    );
    let policies = executor
        .with_context(|ctx| controller.generate_management_policy_documents(ctx))
        .unwrap();
    let mut writes = 0;
    for policy in policies {
        let policy: serde_json::Value = serde_json::from_str(&policy).unwrap();
        for statement in policy["Statement"].as_array().unwrap() {
            if statement["Effect"] == "Allow"
                && statement["Action"].as_array().is_some_and(|actions| {
                    actions.iter().any(|action| {
                        matches!(
                            action.as_str(),
                            Some(
                                "iam:CreateRole"
                                    | "iam:PutRolePolicy"
                                    | "iam:AttachRolePolicy"
                                    | "iam:UpdateAssumeRolePolicy"
                                    | "iam:PutRolePermissionsBoundary"
                            )
                        )
                    })
                })
            {
                writes += 1;
                assert_eq!(
                    statement["Condition"]["StringEquals"]["iam:PermissionsBoundary"],
                    ARN
                );
            }
        }
    }
    assert!(writes > 0);
}

#[tokio::test]
async fn existing_policy_prunes_nondefault_version_before_installing_and_records_only_success() {
    for fail in [true, false] {
        let mut sequence = Sequence::new();
        let mut iam = MockIamApi::new();
        iam.expect_create_policy()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _, _| {
                Err(AlienError::new(ClientError::RemoteResourceConflict {
                    resource_type: "policy".to_string(),
                    resource_name: ARN.to_string(),
                    message: "exists".to_string(),
                }))
            });
        iam.expect_list_policy_versions()
            .withf(|arn| arn == ARN)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(versions()));
        iam.expect_delete_policy_version()
            .withf(|arn, id| arn == ARN && id == "v2")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| Ok(()));
        iam.expect_create_policy_version()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(move |arn, document, default| {
                assert_eq!(arn, ARN);
                assert!(default);
                assert!(
                    serde_json::from_str::<serde_json::Value>(document).unwrap()["Statement"]
                        .as_array()
                        .unwrap()
                        .len()
                        > 1
                );
                if fail {
                    return Err(AlienError::new(ClientError::Timeout {
                        message: "write failed".to_string(),
                    }));
                }
                Ok(CreatePolicyVersionResponse {
                    create_policy_version_result: CreatePolicyVersionResult {
                        policy_version: version("v6", true),
                    },
                })
            });
        let executor = executor(iam).await;
        let old = serde_json::json!({"Version": "2012-10-17", "Statement": []});
        let mut controller = AwsRemoteStackManagementController::default();
        controller.permissions_boundary_arn = Some(ARN.to_string());
        controller.permissions_boundary_document = Some(old.clone());
        let result =
            executor.with_context(|ctx| block_on(controller.install_permissions_boundary(ctx)));
        assert_eq!(result.is_err(), fail);
        assert_eq!(controller.permissions_boundary_document == Some(old), fail);
        assert_eq!(controller.permissions_boundary_arn.as_deref(), Some(ARN));
    }
}

#[tokio::test]
async fn setup_never_replaces_a_same_named_policy_it_did_not_record() {
    let mut iam = MockIamApi::new();
    iam.expect_create_policy().times(1).returning(|_, _, _| {
        Err(AlienError::new(ClientError::RemoteResourceConflict {
            resource_type: "policy".to_string(),
            resource_name: ARN.to_string(),
            message: "exists".to_string(),
        }))
    });
    iam.expect_list_policy_versions().never();
    iam.expect_create_policy_version().never();
    let executor = executor(iam).await;
    let mut controller = AwsRemoteStackManagementController::default();
    let error = executor
        .with_context(|ctx| block_on(controller.install_permissions_boundary(ctx)))
        .unwrap_err();
    assert!(matches!(
        error.error,
        Some(ErrorData::ExistingCloudResourceNotOwned { .. })
    ));
    // Deletion removes only a recorded boundary, so the existing policy is not adopted.
    assert_eq!(controller.permissions_boundary_arn, None);
    assert_eq!(controller.permissions_boundary_document, None);
}

#[tokio::test]
async fn setup_refreshes_legacy_and_stale_boundaries_but_runtime_never_does() {
    let executor = executor(MockIamApi::new()).await;
    let mut controller = AwsRemoteStackManagementController::mock_ready("test-prefix");
    controller.management_permissions_revision = executor
        .with_context(super::super::management_permissions_revision)
        .unwrap();
    assert!(executor
        .with_context(|ctx| controller.needs_update(ctx))
        .unwrap());
    controller.permissions_boundary_arn = Some(ARN.to_string());
    controller.permissions_boundary_document = Some(
        executor
            .with_context(|ctx| controller.required_permissions_boundary(ctx))
            .unwrap(),
    );
    assert!(!executor
        .with_context(|ctx| controller.needs_update(ctx))
        .unwrap());
    controller.permissions_boundary_document = None;
    assert!(executor
        .with_context(|ctx| controller.needs_update(ctx))
        .unwrap());
    let runtime = SingleControllerExecutor::builder()
        .resource(RemoteStackManagement::new("management".to_string()).build())
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(controller.clone())
        .platform(Platform::Aws)
        .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
        .setup_run(false)
        .service_provider(Arc::new(MockPlatformServiceProvider::new()))
        .build()
        .await
        .unwrap();
    assert!(!runtime.needs_update().unwrap());
    let error = runtime
        .with_context(|ctx| block_on(controller.install_permissions_boundary(ctx)))
        .unwrap_err();
    assert_eq!(error.code, "SETUP_RERUN_REQUIRED");
}

#[tokio::test]
async fn deletion_orders_role_versions_and_boundary_and_preserves_failed_cleanup() {
    for (missing, fail) in [(false, false), (false, true), (true, false)] {
        let mut sequence = Sequence::new();
        let mut iam = MockIamApi::new();
        iam.expect_delete_role()
            .withf(|role| role == "test-prefix-management")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(()));
        iam.expect_list_policy_versions()
            .withf(|arn| arn == ARN)
            .times(1)
            .in_sequence(&mut sequence)
            .returning(move |_| {
                if missing {
                    Err(AlienError::new(ClientError::RemoteResourceNotFound {
                        resource_type: "policy".to_string(),
                        resource_name: ARN.to_string(),
                    }))
                } else {
                    Ok(versions())
                }
            });
        if !missing {
            for i in 2..=5 {
                iam.expect_delete_policy_version()
                    .withf(move |arn, id| arn == ARN && id == format!("v{i}"))
                    .times(1)
                    .in_sequence(&mut sequence)
                    .returning(|_, _| Ok(()));
            }
            iam.expect_delete_policy()
                .withf(|arn| arn == ARN)
                .times(1)
                .in_sequence(&mut sequence)
                .returning(move |_| {
                    if fail {
                        Err(AlienError::new(ClientError::RemoteResourceConflict {
                            resource_type: "policy".to_string(),
                            resource_name: ARN.to_string(),
                            message: "still in use".to_string(),
                        }))
                    } else {
                        Ok(())
                    }
                });
        }
        let executor = executor(iam).await;
        let mut controller =
            AwsRemoteStackManagementController::mock_ready("test-prefix-management");
        controller.permissions_boundary_arn = Some(ARN.to_string());
        let document = serde_json::json!({"Statement": []});
        controller.permissions_boundary_document = Some(document.clone());
        let action = executor
            .with_context(|ctx| block_on(controller.deleting_role(ctx)))
            .unwrap();
        assert!(matches!(
            action,
            AwsRemoteStackManagementHandlerAction::Continue {
                state: AwsRemoteStackManagementState::DeletingPermissionsBoundary,
                ..
            }
        ));
        assert!(controller.role_name.is_none());
        assert_eq!(controller.permissions_boundary_arn.as_deref(), Some(ARN));
        let action = executor
            .with_context(|ctx| block_on(controller.delete_start(ctx)))
            .unwrap();
        assert!(matches!(
            action,
            AwsRemoteStackManagementHandlerAction::Continue {
                state: AwsRemoteStackManagementState::DeletingPermissionsBoundary,
                ..
            }
        ));
        let result =
            executor.with_context(|ctx| block_on(controller.deleting_permissions_boundary(ctx)));
        assert_eq!(result.is_err(), fail, "missing={missing}, fail={fail}");
        assert_eq!(
            controller.permissions_boundary_arn.as_deref(),
            fail.then_some(ARN)
        );
        assert_eq!(
            controller.permissions_boundary_document,
            fail.then_some(document)
        );
    }
}

#[tokio::test]
async fn kubernetes_management_scope_keeps_boundary_installed_in_current_step() {
    let executor = SingleControllerExecutor::builder()
        .resource(RemoteStackManagement::new("management".to_string()).build())
        .resource_lifecycle(ResourceLifecycle::Frozen)
        .controller(AwsRemoteStackManagementController::mock_ready(
            "test-prefix-management",
        ))
        .platform(Platform::Aws)
        .build()
        .await
        .unwrap();
    let cluster = KubernetesCluster::new("cluster".to_string())
        .provider(alien_core::KubernetesClusterProvider::Eks)
        .ownership(alien_core::KubernetesClusterOwnership::Managed)
        .namespace("default".to_string())
        .heartbeat_mode(alien_core::KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
        .build();
    let stack = alien_core::Stack::new("test".to_string())
        .add(cluster, ResourceLifecycle::Live)
        .build();
    let mut base = PermissionContext::new();
    base.aws_permissions_boundary_arn = Some(ARN.to_string());
    let resolved = executor
        .with_context(|ctx| {
            AwsRemoteStackManagementController::resource_scoped_management_permission_context(
                ctx,
                &base,
                "cluster",
                &stack.resources["cluster"],
            )
        })
        .unwrap();
    assert_eq!(resolved.aws_permissions_boundary_arn.as_deref(), Some(ARN));
    assert_eq!(resolved.resource_id.as_deref(), Some("cluster"));
}

#[tokio::test]
async fn stale_chunk_cleanup_preserves_boundary_and_unrelated_policies() {
    let controller = AwsRemoteStackManagementController::default();
    let mut iam = MockIamApi::new();
    iam.expect_list_attached_role_policies()
        .times(1)
        .returning(|_| {
            Ok(ListAttachedRolePoliciesResponse {
                list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                    attached_policies: Some(AttachedPolicies {
                        member: [
                            "test-prefix-deployment-management-0",
                            "test-prefix-deployment-management-1",
                            "test-prefix-deployment-management-boundary",
                            "other-deployment-management-1",
                            "test-prefix-deployment-management-01",
                            "test-prefix-deployment-management-custom",
                        ]
                        .into_iter()
                        .map(|name| AttachedPolicy {
                            policy_name: name.to_string(),
                            policy_arn: format!("arn:aws:iam::123456789012:policy/{name}"),
                        })
                        .collect(),
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
    let stale = "arn:aws:iam::123456789012:policy/test-prefix-deployment-management-1";
    iam.expect_detach_role_policy()
        .times(1)
        .returning(move |role, arn| {
            assert_eq!(role, "test-prefix-management");
            assert_eq!(arn, stale);
            Ok(())
        });
    iam.expect_list_policy_versions()
        .times(1)
        .returning(move |arn| {
            assert_eq!(arn, stale);
            Ok(ListPolicyVersionsResponse {
                list_policy_versions_result: ListPolicyVersionsResult {
                    versions: None,
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
    iam.expect_delete_policy().times(1).returning(move |arn| {
        assert_eq!(arn, stale);
        Ok(())
    });
    let executor = executor(MockIamApi::new()).await;
    executor
        .with_context(|ctx| {
            block_on(
                controller.reconcile_owned_management_policies(
                    ctx,
                    &iam,
                    "test-prefix-management",
                    &[
                        "arn:aws:iam::123456789012:policy/test-prefix-deployment-management-0"
                            .to_string(),
                    ],
                ),
            )
        })
        .unwrap();
}

#[test]
fn chunk_recognition_uses_the_exact_generated_name_even_after_truncation() {
    let controller = AwsRemoteStackManagementController::default();
    for prefix in ["test-prefix".to_string(), "s".repeat(200)] {
        for index in [0, 9, 10, 999] {
            let name = controller.management_policy_name(&prefix, index);
            assert!(controller.is_owned_management_policy_name(&prefix, &name));
            assert!(!controller.is_owned_management_policy_name(&prefix, &format!("{name}-copy")));
        }
        assert!(!controller.is_owned_management_policy_name(
            &prefix,
            &format!("{prefix}-deployment-management-boundary"),
        ));
    }
}
