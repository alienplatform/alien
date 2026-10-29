//! Resolve boundary grants using the same names as AWS workload policy writers.

use alien_core::{
    ClientConfig, DeploymentConfig, KubernetesCluster, ManagementConfig, Platform,
    RemoteStackManagement, ResourceStatus, Stack, StackResourceState, StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{
    aws_resource_policies::aws_prefixed_resource_name,
    generators::{aws_permissions_boundary_covers, aws_stack_permissions_boundary},
    ErrorData as PermissionErrorData, PermissionContext,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    build::AwsBuildController,
    core::{state_utils::StackResourceStateExt, ResourcePermissionsHelper},
    error::{ErrorData, Result},
    key::AwsKeyController,
    kv::AwsKvController,
    queue::aws::AwsQueueController,
    remote_stack_management::AwsRemoteStackManagementController,
    storage::AwsStorageController,
    vault::AwsVaultController,
    worker::AwsWorkerController,
};

/// Generate the required boundary for direct setup or a deployment compatibility check.
///
/// `context` supplies the deployment account, region, prefix and management identity.
/// Existing resources use durable controller names, including imported names outside
/// the prefix. Only resources that have never started may use the writer's naming rule.
/// This does not install a policy or assert that the installed policy covers it.
pub fn aws_permissions_boundary_for_stack(
    stack: &Stack,
    state: &StackState,
    context: &PermissionContext,
) -> Result<Value> {
    let mut base = context.clone();
    base.resource_id = None;
    base.resource_name = None;
    aws_stack_permissions_boundary(stack, &base, |_, scope| {
        boundary_grant_context(stack, state, &base, scope).context(
            PermissionErrorData::GeneratorError {
                platform: "aws".to_string(),
                message: format!("Cannot resolve permissions boundary scope '{scope}'"),
            },
        )
    })
    .context(ErrorData::InfrastructureError {
        message: "Cannot generate the required AWS permissions boundary".to_string(),
        operation: Some("generate_permissions_boundary".to_string()),
        resource_id: None,
    })
}

/// Refuse runtime reconciliation when setup's installed ceiling cannot cover this stack.
pub fn validate_installed_aws_permissions_boundary(
    stack: &Stack,
    state: &StackState,
    client_config: &ClientConfig,
    deployment_config: &DeploymentConfig,
) -> Result<()> {
    let Some(aws) = client_config.aws_config() else {
        return Ok(());
    };
    for (id, saved) in &state.resources {
        if saved.resource_type != RemoteStackManagement::RESOURCE_TYPE.as_ref() {
            continue;
        }
        let setup_required = |message: String| ErrorData::SetupRerunRequired {
            resource_id: id.clone(),
            message,
        };
        let controller = saved.get_internal_controller().context(setup_required(
            "Cannot read the installed AWS permissions boundary; regenerate and rerun setup"
                .to_string(),
        ))?;
        let controller = controller
            .as_ref()
            .and_then(|controller| controller.as_any().downcast_ref::<AwsRemoteStackManagementController>())
            .ok_or_else(|| AlienError::new(setup_required(
                "The installed AWS management controller is missing or has the wrong type; regenerate and rerun setup".to_string(),
            )))?;
        let Some(arn) = &controller.permissions_boundary_arn else {
            continue;
        };
        let installed = controller.permissions_boundary_document.as_ref().ok_or_else(|| {
            AlienError::new(setup_required(format!(
                "Installed permissions boundary '{arn}' has no recorded policy document; regenerate and rerun setup"
            )))
        })?;
        let mut context = PermissionContext::new()
            .with_stack_prefix(state.resource_prefix.clone())
            .with_aws_account_id(aws.account_id.clone())
            .with_aws_region(aws.region.clone());
        context.aws_permissions_boundary_arn = Some(arn.clone());
        match &deployment_config.management_config {
            Some(ManagementConfig::Aws(management)) => {
                context = context.with_managing_role_arn(management.managing_role_arn.clone());
                context.managing_account_id = PermissionContext::extract_account_id_from_role_arn(
                    &management.managing_role_arn,
                );
            }
            None => {}
            Some(_) => {
                return Err(AlienError::new(setup_required(
                    "The AWS boundary requires an AWS management configuration".to_string(),
                )));
            }
        }
        let required = aws_permissions_boundary_for_stack(stack, state, &context)
            .context(setup_required(format!(
                "Cannot resolve the permissions required by boundary '{arn}'; regenerate and rerun setup"
            )))?;
        let covered = aws_permissions_boundary_covers(installed, &required).context(
            setup_required(format!(
                "Cannot compare installed permissions boundary '{arn}'; regenerate and rerun setup"
            )),
        )?;
        if !covered {
            return Err(AlienError::new(setup_required(format!(
                "Installed permissions boundary '{arn}' does not cover this stack's runtime permissions; regenerate and rerun setup"
            ))));
        }
    }
    Ok(())
}

fn boundary_grant_context(
    stack: &Stack,
    state: &StackState,
    base: &PermissionContext,
    scope: &str,
) -> Result<PermissionContext> {
    if scope == "*" {
        return Ok(base.clone());
    }
    let entry = stack
        .resources
        .get(scope)
        .ok_or_else(|| unresolved(scope, "resource is absent from the desired stack"))?;
    let prefix = base
        .stack_prefix
        .as_deref()
        .ok_or_else(|| unresolved(scope, "deployment prefix is missing"))?;
    let mut context = base.clone().with_resource_id(scope.to_string());
    if let Some(cluster) = entry.config.downcast_ref::<KubernetesCluster>() {
        context.resource_name = Some(
            ResourcePermissionsHelper::kubernetes_cluster_name_for_permissions(prefix, cluster),
        );
        if let Some(cloud) = &cluster.cloud {
            if let Some(account) = &cloud.account_id {
                context.aws_account_id = Some(account.clone());
            }
            if let Some(region) = &cloud.region {
                context.aws_region = Some(region.clone());
            }
        }
        return Ok(context);
    }
    let resource_type = entry.config.resource_type().to_string();
    // These writers intentionally pass the logical id, not a physical role or ARN.
    if matches!(
        resource_type.as_str(),
        "artifact-registry" | "service-account" | "ai" | "sandbox"
    ) {
        return Ok(context.with_resource_name(scope.to_string()));
    }
    let saved = state.resources.get(scope);
    if let Some(saved) = saved {
        if saved.resource_type != resource_type
            || saved
                .controller_platform
                .is_some_and(|platform| platform != Platform::Aws)
        {
            return Err(unresolved(
                scope,
                "recorded resource type or platform does not match the AWS writer",
            ));
        }
    }
    let unstarted = saved.is_none_or(|saved| {
        saved.status == ResourceStatus::Pending
            && saved.internal_state.is_none()
            && saved.last_failed_state.is_none()
            && saved.outputs.is_none()
    });
    let name = match resource_type.as_str() {
        "storage" | "queue" | "kv" | "build" | "worker" | "vault" if unstarted => {
            Some(aws_prefixed_resource_name(prefix, scope))
        }
        "storage" => read_controller::<AwsStorageController>(saved, scope)?.bucket_name,
        "queue" => read_controller::<AwsQueueController>(saved, scope)?.queue_name,
        "kv" => read_controller::<AwsKvController>(saved, scope)?.table_name,
        "build" => read_controller::<AwsBuildController>(saved, scope)?.project_name,
        "worker" => read_controller::<AwsWorkerController>(saved, scope)?.worker_name,
        "vault" => read_controller::<AwsVaultController>(saved, scope)?.vault_prefix,
        // Key IDs are provider-assigned. Setup imports their exact ARN; a prefix
        // cannot predict it, including when the key has not been imported yet.
        "key" => read_controller::<AwsKeyController>(saved, scope)?.key_arn,
        // A custom grant may use only resourceId or literal ARNs. Leave resourceName
        // unresolved for other writers so interpolation fails if it is needed.
        _ => return Ok(context),
    };
    context.resource_name =
        Some(name.filter(|name| !name.is_empty()).ok_or_else(|| {
            unresolved(scope, "the controller has not recorded its physical name")
        })?);
    Ok(context)
}

fn read_controller<T: DeserializeOwned>(
    saved: Option<&StackResourceState>,
    scope: &str,
) -> Result<T> {
    let saved = saved.ok_or_else(|| unresolved(scope, "resource state is missing"))?;
    let internal = saved
        .internal_state
        .as_ref()
        .ok_or_else(|| unresolved(scope, "controller state is missing"))?;
    serde_json::from_value(internal.clone())
        .into_alien_error()
        .context(ErrorData::ControllerStateTypeMismatch {
            expected: std::any::type_name::<T>().to_string(),
            resource_id: scope.to_string(),
        })
}

fn unresolved(scope: &str, reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::InfrastructureError {
        message: format!("Cannot resolve AWS permissions boundary resource '{scope}': {reason}"),
        operation: Some("generate_permissions_boundary".to_string()),
        resource_id: Some(scope.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Build, Key, KubernetesCloudReference, KubernetesClusterOwnership,
        KubernetesClusterProvider, KubernetesHeartbeatMode, Kv, PermissionProfile,
        PermissionSetReference, Queue, ResourceLifecycle, Storage, Vault, Worker, WorkerCode,
    };
    use alien_permissions::generators::aws_permissions_boundary_covers;
    use serde_json::json;

    fn context() -> PermissionContext {
        PermissionContext::new()
            .with_stack_prefix("boundary-test")
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_managing_account_id("210987654321")
            .with_managing_role_arn("arn:aws:iam::210987654321:role/manager")
    }

    fn stack(lifecycle: ResourceLifecycle, global: bool) -> Stack {
        let grant = PermissionSetReference::from_inline(
            serde_json::from_value(json!({
                "id": "storage/custom", "description": "Approved objects",
                "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]}, "binding": {
                    "stack": {"resources": ["arn:aws:s3:::${stackPrefix}-*/*"]},
                    "resource": {"resources": ["arn:aws:s3:::${resourceName}/approved/*"],
                        "condition": {"StringEquals": {"s3:ExistingObjectTag/approved": "yes"}}}
                }}]}
            }))
            .unwrap(),
        );
        let mut profile = PermissionProfile::new();
        profile.0.insert(
            if global { "*" } else { "objects" }.to_string(),
            vec![grant],
        );
        Stack::new("boundary".to_string())
            .permission("reader", profile)
            .add(Storage::new("objects".to_string()).build(), lifecycle)
            .build()
    }

    fn saved_resource(stack: &Stack, id: &str, controller: Value) -> StackState {
        let entry = &stack.resources[id];
        let mut saved = StackResourceState::new_pending(
            entry.config.resource_type().to_string(),
            entry.config.clone(),
            Some(entry.lifecycle),
            vec![],
        );
        saved.status = ResourceStatus::Running;
        saved.internal_state = Some(controller);
        let mut state = StackState::new(Platform::Aws);
        state.resources.insert(id.to_string(), saved);
        state
    }

    #[test]
    fn imported_names_and_conditions_match_explicit_and_implicit_runtime_grants() {
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            for global in [false, true] {
                let stack = stack(lifecycle, global);
                let controller = AwsStorageController {
                    bucket_name: Some("external-approved-bucket".to_string()),
                    ..Default::default()
                };
                let state =
                    saved_resource(&stack, "objects", serde_json::to_value(controller).unwrap());
                let boundary =
                    aws_permissions_boundary_for_stack(&stack, &state, &context()).unwrap();
                let required = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "s3:GetObject",
                    "Resource": "arn:aws:s3:::external-approved-bucket/approved/*",
                    "Condition": {"StringEquals": {"s3:ExistingObjectTag/approved": "yes"}}
                }]});
                assert!(aws_permissions_boundary_covers(&boundary, &required).unwrap());
                let mut unconditional = required;
                unconditional["Statement"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("Condition");
                assert!(!aws_permissions_boundary_covers(&boundary, &unconditional).unwrap());
            }
        }
    }

    #[test]
    fn only_unstarted_resources_use_the_deterministic_name() {
        let stack = stack(ResourceLifecycle::Live, false);
        let mut state = StackState::new(Platform::Aws);
        assert_eq!(
            boundary_grant_context(&stack, &state, &context(), "objects")
                .unwrap()
                .resource_name
                .as_deref(),
            Some("boundary-test-objects")
        );
        let entry = &stack.resources["objects"];
        state.resources.insert(
            "objects".to_string(),
            StackResourceState::new_pending(
                "storage".to_string(),
                entry.config.clone(),
                Some(entry.lifecycle),
                vec![],
            ),
        );
        assert!(aws_permissions_boundary_for_stack(&stack, &state, &context()).is_ok());
        state.resources.get_mut("objects").unwrap().status = ResourceStatus::Running;
        let error = aws_permissions_boundary_for_stack(&stack, &state, &context()).unwrap_err();
        assert!(
            error.to_string().contains("controller state is missing"),
            "{error}"
        );
    }

    #[test]
    fn missing_or_malformed_recorded_names_never_fall_back_to_the_prefix() {
        let stack = stack(ResourceLifecycle::Frozen, false);
        for controller in [
            serde_json::to_value(AwsStorageController::default()).unwrap(),
            serde_json::to_value(AwsStorageController {
                bucket_name: Some(String::new()),
                ..Default::default()
            })
            .unwrap(),
            json!({"state": "UnknownState", "bucketName": "external-bucket"}),
        ] {
            let state = saved_resource(&stack, "objects", controller);
            assert!(aws_permissions_boundary_for_stack(&stack, &state, &context()).is_err());
        }
    }

    #[test]
    fn imported_key_boundary_uses_exact_arn_ceiling() {
        let key_arn = "arn:aws:kms:us-east-1:111122223333:key/1234abcd-12ab-34cd-56ef-1234567890ab";
        for set in [
            "key/availability",
            "key/native-storage",
            "key/native-volume",
            "key/remote-cryptography",
        ] {
            let stack = Stack::new("boundary".to_string())
                .permission(
                    "application",
                    PermissionProfile::new().resource("encryption", [set]),
                )
                .add(
                    Key::new("encryption".to_string()).build(),
                    ResourceLifecycle::Frozen,
                )
                .build();
            let controller = AwsKeyController {
                key_arn: Some(key_arn.to_string()),
                ..Default::default()
            };
            let state = saved_resource(
                &stack,
                "encryption",
                serde_json::to_value(controller).unwrap(),
            );
            let boundary = aws_permissions_boundary_for_stack(&stack, &state, &context()).unwrap();
            let policy = alien_permissions::generators::AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    alien_permissions::get_permission_set(set).unwrap(),
                    alien_permissions::BindingTarget::Resource,
                    &context().with_resource_name(key_arn),
                )
                .unwrap();
            let required = serde_json::to_value(policy).unwrap();
            assert!(
                aws_permissions_boundary_covers(&boundary, &required).unwrap(),
                "{set}"
            );
            let mut sibling = required.clone();
            for statement in sibling["Statement"].as_array_mut().unwrap() {
                statement["Resource"] = json!("arn:aws:kms:us-east-1:111122223333:key/another-key");
            }
            assert!(
                !aws_permissions_boundary_covers(&boundary, &sibling).unwrap(),
                "{set}"
            );
            for statement in required["Statement"].as_array().unwrap() {
                if statement.get("Condition").is_some() {
                    let mut unconditional = statement.clone();
                    unconditional.as_object_mut().unwrap().remove("Condition");
                    assert!(
                        aws_permissions_boundary_covers(
                            &boundary,
                            &json!({"Version": "2012-10-17", "Statement": [unconditional]})
                        )
                        .unwrap(),
                        "{set}"
                    );
                }
            }
        }
    }

    #[test]
    fn keys_without_durable_arns_never_use_a_prefixed_name() {
        let stack = Stack::new("boundary".to_string())
            .permission(
                "application",
                PermissionProfile::new().resource("encryption", ["key/native-storage"]),
            )
            .add(
                Key::new("encryption".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let empty = StackState::new(Platform::Aws);
        let error = aws_permissions_boundary_for_stack(&stack, &empty, &context()).unwrap_err();
        assert!(
            error.to_string().contains("resource state is missing"),
            "{error}"
        );
        for arn in [None, Some(String::new())] {
            let controller = AwsKeyController {
                key_arn: arn,
                ..Default::default()
            };
            let state = saved_resource(
                &stack,
                "encryption",
                serde_json::to_value(controller).unwrap(),
            );
            let error = aws_permissions_boundary_for_stack(&stack, &state, &context()).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("has not recorded its physical name"),
                "{error}"
            );
        }
    }

    #[test]
    fn vault_context_uses_the_recorded_parameter_prefix() {
        let stack = Stack::new("boundary".to_string())
            .add(
                Vault::new("secrets".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let controller = AwsVaultController {
            vault_prefix: Some("external-parameters".to_string()),
            ..Default::default()
        };
        let state = saved_resource(&stack, "secrets", serde_json::to_value(controller).unwrap());
        assert_eq!(
            boundary_grant_context(&stack, &state, &context(), "secrets")
                .unwrap()
                .resource_name
                .as_deref(),
            Some("external-parameters")
        );
    }

    #[test]
    fn catalog_only_stacks_do_not_require_unrelated_controller_state() {
        let stack = Stack::new("boundary".to_string())
            .add(
                Storage::new("objects".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = saved_resource(&stack, "objects", json!({"invalid": true}));
        let actual = aws_permissions_boundary_for_stack(&stack, &state, &context()).unwrap();
        let expected =
            alien_permissions::generators::aws_permissions_boundary(&context(), &[]).unwrap();
        assert_eq!(actual, expected);
    }
    #[test]
    fn every_named_writer_uses_its_durable_name() {
        let stack = Stack::new("boundary".to_string())
            .add(
                Queue::new("queue".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Kv::new("table".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Build::new("build".to_string())
                    .permissions("reader".to_string())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Worker::new("worker".to_string())
                    .permissions("reader".to_string())
                    .code(WorkerCode::Image {
                        image: "example:latest".to_string(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            )
            .build();
        for (id, controller, name) in [
            (
                "queue",
                serde_json::to_value(AwsQueueController {
                    queue_name: Some("imported-queue".to_string()),
                    ..Default::default()
                })
                .unwrap(),
                "imported-queue",
            ),
            (
                "table",
                serde_json::to_value(AwsKvController {
                    table_name: Some("imported-table".to_string()),
                    ..Default::default()
                })
                .unwrap(),
                "imported-table",
            ),
            (
                "build",
                serde_json::to_value(AwsBuildController {
                    project_name: Some("imported-build".to_string()),
                    ..Default::default()
                })
                .unwrap(),
                "imported-build",
            ),
            (
                "worker",
                serde_json::to_value(AwsWorkerController {
                    worker_name: Some("imported-worker".to_string()),
                    ..Default::default()
                })
                .unwrap(),
                "imported-worker",
            ),
        ] {
            let state = saved_resource(&stack, id, controller);
            let resolved = boundary_grant_context(&stack, &state, &context(), id).unwrap();
            assert_eq!(resolved.resource_name.as_deref(), Some(name));
            assert_eq!(resolved.resource_id.as_deref(), Some(id));
        }
    }

    #[test]
    fn recorded_platform_and_type_must_match_before_resolving_a_name() {
        let stack = stack(ResourceLifecycle::Frozen, false);
        let controller = AwsStorageController {
            bucket_name: Some("imported-bucket".to_string()),
            ..Default::default()
        };
        let state = saved_resource(&stack, "objects", serde_json::to_value(controller).unwrap());
        let mut wrong_type = state.clone();
        wrong_type
            .resources
            .get_mut("objects")
            .unwrap()
            .resource_type = "queue".to_string();
        let mut wrong_platform = state;
        wrong_platform
            .resources
            .get_mut("objects")
            .unwrap()
            .controller_platform = Some(Platform::Gcp);
        for state in [wrong_type, wrong_platform] {
            assert!(aws_permissions_boundary_for_stack(&stack, &state, &context()).is_err());
        }
    }
    #[test]
    fn cluster_context_preserves_boundary_and_uses_the_referenced_cloud() {
        let cluster = KubernetesCluster::new("cluster".to_string())
            .provider(KubernetesClusterProvider::Eks)
            .ownership(KubernetesClusterOwnership::Existing)
            .namespace("workloads".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApi)
            .cloud(KubernetesCloudReference {
                cluster_name: Some("existing-cluster".to_string()),
                region: Some("eu-west-1".to_string()),
                account_id: Some("111122223333".to_string()),
                ..Default::default()
            })
            .build();
        let stack = Stack::new("boundary".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .build();
        let mut base = context();
        base.aws_permissions_boundary_arn =
            Some("arn:aws:iam::123456789012:policy/boundary-test-permissions-boundary".to_string());
        let resolved =
            boundary_grant_context(&stack, &StackState::new(Platform::Aws), &base, "cluster")
                .unwrap();
        assert_eq!(resolved.resource_name.as_deref(), Some("existing-cluster"));
        assert_eq!(resolved.aws_account_id.as_deref(), Some("111122223333"));
        assert_eq!(resolved.aws_region.as_deref(), Some("eu-west-1"));
        assert_eq!(
            resolved.aws_permissions_boundary_arn,
            base.aws_permissions_boundary_arn
        );
        assert_eq!(resolved.managing_account_id, base.managing_account_id);
    }
}
