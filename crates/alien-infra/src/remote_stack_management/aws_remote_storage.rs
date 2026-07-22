use std::collections::HashSet;

#[cfg(test)]
use alien_core::Storage;
use alien_core::{KubernetesCluster, ResourceLifecycle, StorageBinding, Worker};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AwsIamStatement, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};

use super::{concrete_storage_binding_value, remote_storage_binding};
use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};

pub(super) fn append_resource_scoped_management_statements(
    ctx: &ResourceControllerContext<'_>,
    management_profile: &alien_core::permissions::PermissionProfile,
    base_permission_context: &PermissionContext,
    generator: &AwsRuntimePermissionsGenerator,
    all_statements: &mut Vec<AwsIamStatement>,
) -> Result<()> {
    let mut seen = HashSet::new();
    for (resource_id, permission_set_refs) in management_profile
        .0
        .iter()
        .filter(|(scope, _)| *scope != "*")
    {
        let Some(resource_entry) = ctx.desired_stack.resources.get(resource_id) else {
            continue;
        };
        let permission_context = if resource_entry.lifecycle == ResourceLifecycle::Live {
            live_resource_permission_context(
                ctx,
                base_permission_context,
                resource_id,
                resource_entry,
            )?
        } else if resource_entry.is_remote_frozen_storage() {
            let bucket_name = aws_remote_storage_bucket_name(ctx, resource_id)?;
            base_permission_context
                .clone()
                .with_resource_id(resource_id.to_string())
                .with_resource_name(bucket_name)
        } else {
            continue;
        };

        for permission_set_ref in permission_set_refs {
            if !seen.insert((resource_id.clone(), permission_set_ref.id().to_string())) {
                continue;
            }
            if permission_set_ref.id().ends_with("/provision") {
                continue;
            }
            let Some(permission_set) =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned())
            else {
                continue;
            };
            if permission_set.platforms.aws.is_none() {
                continue;
            }

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate resource-scoped IAM policy for management permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("generate_management_policy_document".to_string()),
                    resource_id: Some(resource_id.clone()),
                })?;
            all_statements.extend(policy.statement);
        }
    }

    Ok(())
}

fn live_resource_permission_context(
    ctx: &ResourceControllerContext<'_>,
    base_permission_context: &PermissionContext,
    resource_id: &str,
    resource_entry: &alien_core::ResourceEntry,
) -> Result<PermissionContext> {
    if let Some(cluster) = resource_entry.config.downcast_ref::<KubernetesCluster>() {
        return ResourcePermissionsHelper::aws_kubernetes_cluster_permission_context(ctx, cluster)
            .map(|context| context.with_resource_id(resource_id.to_string()));
    }

    let mut context = base_permission_context
        .clone()
        .with_resource_id(resource_id.to_string());
    context.resource_name = None;

    if resource_entry.config.downcast_ref::<Worker>().is_some() {
        return Ok(context.with_resource_name(format!("{}-{}", ctx.resource_prefix, resource_id)));
    }

    Ok(context)
}

fn aws_remote_storage_bucket_name(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<String> {
    match remote_storage_binding(ctx, resource_id)? {
        Some(StorageBinding::S3(binding)) => concrete_storage_binding_value(
            binding.bucket_name,
            resource_id,
            "bucketName",
            "AWS S3",
        ),
        Some(other) => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "Remote Storage resource '{resource_id}' must use an S3 binding on AWS, got {other:?}"
            ),
            resource_id: Some(resource_id.to_string()),
        })),
        None => Ok(format!("{}-{}", ctx.resource_prefix, resource_id)),
    }
}

pub(super) fn desired_remote_storage_bucket_names(
    ctx: &ResourceControllerContext<'_>,
) -> Result<Vec<String>> {
    let mut bucket_names = ctx
        .desired_stack
        .resources
        .iter()
        .filter(|(_, entry)| entry.is_remote_frozen_storage())
        .map(|(resource_id, _)| aws_remote_storage_bucket_name(ctx, resource_id))
        .collect::<Result<Vec<_>>>()?;
    bucket_names.sort_unstable();
    bucket_names.dedup();
    Ok(bucket_names)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alien_core::{
        ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBinding,
        ManagementPermissions, PermissionProfile, PermissionsConfig, Platform,
        RemoteStackManagement, Resource, ResourceEntry, ResourceStatus, Stack, StackSettings,
        StackState,
    };
    use indexmap::IndexMap;

    use super::super::aws::{AwsRemoteStackManagementController, AwsRemoteStackManagementState};
    use super::*;
    use crate::core::{
        DefaultPlatformServiceProvider, HeartbeatCollector, PlatformServiceProvider,
        ResourceController, ResourceRegistry,
    };

    struct GrantPlanHarness {
        desired_config: Resource,
        stack: Stack,
        state: StackState,
        registry: Arc<ResourceRegistry>,
        service_provider: Arc<dyn PlatformServiceProvider>,
        deployment_config: DeploymentConfig,
    }

    impl GrantPlanHarness {
        fn new(remote_access: bool, bucket_name: &str) -> Self {
            let storage = Storage::new("archive".to_string()).build();
            let management = RemoteStackManagement::new("management".to_string()).build();
            let desired_config = Resource::new(management.clone());
            let mut resources = IndexMap::new();
            resources.insert(
                "archive".to_string(),
                ResourceEntry {
                    config: Resource::new(storage),
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies: Vec::new(),
                    remote_access,
                    enabled_when: None,
                },
            );
            resources.insert(
                "management".to_string(),
                ResourceEntry {
                    config: Resource::new(management),
                    lifecycle: ResourceLifecycle::Frozen,
                    dependencies: Vec::new(),
                    remote_access: false,
                    enabled_when: None,
                },
            );
            let profile = PermissionProfile::new().resource(
                "archive",
                [alien_core::PermissionSetReference::from_name(
                    "storage/remote-data-write",
                )],
            );
            let stack = Stack {
                id: "grant-plan-test".to_string(),
                resources,
                permissions: PermissionsConfig {
                    profiles: IndexMap::new(),
                    management: ManagementPermissions::Override(profile),
                },
                supported_platforms: None,
                inputs: Vec::new(),
            };
            let mut state = StackState::new(Platform::Aws);
            state.resources.insert(
                "archive".to_string(),
                alien_core::StackResourceState::builder()
                    .resource_type(Storage::RESOURCE_TYPE.as_ref().to_string())
                    .status(ResourceStatus::Running)
                    .config(Resource::new(Storage::new("archive".to_string()).build()))
                    .lifecycle(ResourceLifecycle::Frozen)
                    .remote_binding_params(
                        serde_json::to_value(StorageBinding::s3(bucket_name)).unwrap(),
                    )
                    .dependencies(Vec::new())
                    .build(),
            );
            Self {
                desired_config,
                stack,
                state,
                registry: Arc::new(ResourceRegistry::new()),
                service_provider: Arc::new(DefaultPlatformServiceProvider::default()),
                deployment_config: DeploymentConfig::builder()
                    .stack_settings(StackSettings::default())
                    .environment_variables(EnvironmentVariablesSnapshot {
                        variables: Vec::new(),
                        hash: String::new(),
                        created_at: String::new(),
                    })
                    .external_bindings(Default::default())
                    .allow_frozen_changes(false)
                    .build(),
            }
        }

        fn ctx(&self) -> ResourceControllerContext<'_> {
            ResourceControllerContext {
                desired_config: &self.desired_config,
                platform: Platform::Aws,
                client_config: ClientConfig::Test,
                state: &self.state,
                resource_prefix: "test-stack",
                registry: &self.registry,
                desired_stack: &self.stack,
                service_provider: &self.service_provider,
                deployment_config: &self.deployment_config,
                heartbeat_collector: HeartbeatCollector::default(),
            }
        }
    }

    #[test]
    fn remote_storage_management_policy_uses_the_exact_bucket() {
        let context = PermissionContext::new()
            .with_aws_account_id("123456789012".to_string())
            .with_aws_region("us-east-1".to_string())
            .with_stack_prefix("deployment-prefix".to_string())
            .with_resource_id("archive".to_string())
            .with_resource_name("setup-owned-archive-bucket".to_string());
        let permission_set = get_permission_set("storage/remote-data-write").unwrap();

        let policy = AwsRuntimePermissionsGenerator::new()
            .generate_policy(permission_set, BindingTarget::Resource, &context)
            .unwrap();

        assert_eq!(
            policy.statement[0].resource,
            [
                "arn:aws:s3:::setup-owned-archive-bucket",
                "arn:aws:s3:::setup-owned-archive-bucket/*",
            ]
        );
    }

    #[test]
    fn external_remote_storage_is_rejected_before_grant_derivation() {
        let mut harness = GrantPlanHarness::new(true, "setup-owned-bucket");
        harness.deployment_config.external_bindings.insert(
            "archive",
            ExternalBinding::Storage(StorageBinding::s3("existing-bucket")),
        );

        let error = desired_remote_storage_bucket_names(&harness.ctx())
            .expect_err("external buckets must never receive management grants");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert!(error.message.contains("cannot use an external binding"));
    }

    #[test]
    fn running_controller_schedules_enable_disable_and_rebind_grant_changes() {
        let enabled = GrantPlanHarness::new(true, "bucket-a");
        let enabled_fingerprint = super::super::desired_management_grant_fingerprint(
            &enabled.ctx(),
            &desired_remote_storage_bucket_names(&enabled.ctx()).unwrap(),
        )
        .unwrap();
        let mut controller = AwsRemoteStackManagementController {
            state: AwsRemoteStackManagementState::Ready,
            role_arn: Some("arn:aws:iam::123456789012:role/test-management".to_string()),
            role_name: Some("test-management".to_string()),
            management_permissions_applied: true,
            applied_management_grant_fingerprint: None,
            _internal_stay_count: None,
        };

        assert!(controller.needs_update(&enabled.ctx()).unwrap());
        controller.applied_management_grant_fingerprint = Some(enabled_fingerprint);
        assert!(!controller.needs_update(&enabled.ctx()).unwrap());

        let disabled = GrantPlanHarness::new(false, "bucket-a");
        assert!(controller.needs_update(&disabled.ctx()).unwrap());

        let rebound = GrantPlanHarness::new(true, "bucket-b");
        assert!(controller.needs_update(&rebound.ctx()).unwrap());
    }
}
