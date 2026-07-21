use std::collections::BTreeSet;

use alien_core::{BindingValue, ResourceLifecycle, Storage, StorageBinding};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::iam::Binding;
#[cfg(test)]
use alien_permissions::PermissionContext;
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    get_permission_set, BindingTarget,
};

use crate::core::{ResourceControllerContext, ResourcePermissionsHelper};
use crate::error::{ErrorData, Result};

pub(super) struct GrantPlan {
    pub(super) bucket_name: String,
    bindings: Vec<Binding>,
    owned_role_prefixes: Vec<String>,
}

pub(super) async fn build_grant_plans(
    ctx: &ResourceControllerContext<'_>,
    generator: &GcpRuntimePermissionsGenerator,
    service_account_id: &str,
) -> Result<Vec<GrantPlan>> {
    let Some(management_profile) = ctx.desired_stack.management().profile() else {
        return Ok(Vec::new());
    };
    let mut grant_plans = Vec::new();

    for (resource_id, resource_entry) in &ctx.desired_stack.resources {
        if !is_remote_frozen_storage(resource_entry) {
            continue;
        }

        let bucket_name = remote_storage_bucket_name(ctx, resource_id)?;
        let permission_context =
            ResourcePermissionsHelper::build_gcp_permission_context(ctx, &bucket_name)?
                .with_resource_id(resource_id.clone())
                .with_service_account_name(service_account_id.to_string());
        let mut bucket_bindings = Vec::new();

        if let Some(permission_set_refs) = management_profile.0.get(resource_id) {
            for permission_set_ref in permission_set_refs {
                if permission_set_ref.id().ends_with("/provision") {
                    continue;
                }
                let Some(permission_set) =
                    permission_set_ref.resolve(|name| get_permission_set(name).cloned())
                else {
                    continue;
                };
                if permission_set.platforms.gcp.is_none() {
                    continue;
                }

                let grant_plan = generator
                    .generate_grant_plan(
                        &permission_set,
                        BindingTarget::Resource,
                        &permission_context,
                    )
                    .context(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate bucket-scoped IAM grant plan for management permission set '{}'",
                            permission_set.id
                        ),
                        operation: Some("binding_role".to_string()),
                        resource_id: Some(resource_id.clone()),
                    })?;
                ResourcePermissionsHelper::ensure_all_gcp_custom_roles(
                    ctx,
                    &permission_set.id,
                    &grant_plan,
                )
                .await?;

                bucket_bindings.extend(
                    grant_plan
                        .bindings_for_target(GcpBindingTargetScope::CurrentResource)
                        .into_iter()
                        .map(|binding| Binding {
                            role: binding.role,
                            members: binding.members,
                            condition: binding.condition.map(|condition| {
                                alien_gcp_clients::iam::Expr {
                                    expression: condition.expression,
                                    title: Some(condition.title),
                                    description: Some(condition.description),
                                    location: None,
                                }
                            }),
                        }),
                );
            }
        }

        let mut owned_permission_set_ids = vec!["storage/remote-data-write"];
        if let Some(permission_set_refs) = management_profile.0.get(resource_id) {
            owned_permission_set_ids.extend(
                permission_set_refs
                    .iter()
                    .filter(|permission_set_ref| !permission_set_ref.id().ends_with("/provision"))
                    .map(|permission_set_ref| permission_set_ref.id()),
            );
        }
        owned_permission_set_ids.sort_unstable();
        owned_permission_set_ids.dedup();
        let owned_role_prefixes =
            ResourcePermissionsHelper::gcp_permission_set_custom_role_name_prefixes(
                &permission_context,
                owned_permission_set_ids,
            );
        grant_plans.push(GrantPlan {
            bucket_name,
            bindings: bucket_bindings,
            owned_role_prefixes,
        });
    }

    grant_plans.sort_by(|left, right| left.bucket_name.cmp(&right.bucket_name));
    Ok(grant_plans)
}

pub(super) fn desired_bucket_names(ctx: &ResourceControllerContext<'_>) -> Result<Vec<String>> {
    let mut bucket_names = ctx
        .desired_stack
        .resources
        .iter()
        .filter(|(_, entry)| is_remote_frozen_storage(entry))
        .map(|(resource_id, _)| remote_storage_bucket_name(ctx, resource_id))
        .collect::<Result<Vec<_>>>()?;
    bucket_names.sort_unstable();
    bucket_names.dedup();
    Ok(bucket_names)
}

/// Recover remote bucket ownership from synchronized resource state. This is
/// the migration path for controllers serialized before bucket ownership was
/// persisted, and also covers a disable/rebind planned in the same release.
pub(super) fn observed_bucket_names(ctx: &ResourceControllerContext<'_>) -> Result<Vec<String>> {
    observed_bucket_names_from_state(ctx.state)
}

fn observed_bucket_names_from_state(state: &alien_core::StackState) -> Result<Vec<String>> {
    let mut bucket_names = Vec::new();
    for (resource_id, state) in &state.resources {
        if state.resource_type != Storage::RESOURCE_TYPE.as_ref()
            || state.lifecycle != Some(ResourceLifecycle::Frozen)
        {
            continue;
        }
        let Some(value) = state.remote_binding_params.as_ref() else {
            continue;
        };
        let binding: StorageBinding = serde_json::from_value(value.clone())
            .into_alien_error()
            .context(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Remote Storage resource '{resource_id}' has invalid synchronized binding parameters"
                ),
                resource_id: Some(resource_id.clone()),
            })?;
        match binding {
            StorageBinding::Gcs(binding) => bucket_names.push(concrete_storage_binding_value(
                binding.bucket_name,
                resource_id,
                "bucketName",
                "GCP GCS",
            )?),
            other => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Remote Storage resource '{resource_id}' must use a GCS binding on GCP, got {other:?}"
                    ),
                    resource_id: Some(resource_id.clone()),
                }));
            }
        }
    }
    bucket_names.sort_unstable();
    bucket_names.dedup();
    Ok(bucket_names)
}

pub(super) async fn reconcile_grants(
    ctx: &ResourceControllerContext<'_>,
    service_account_email: &str,
    grant_plans: Vec<GrantPlan>,
    previously_owned_buckets: &[String],
) -> Result<Vec<String>> {
    let desired_buckets = grant_plans
        .iter()
        .map(|plan| plan.bucket_name.clone())
        .collect::<Vec<_>>();
    let retired_buckets = retired_bucket_names(previously_owned_buckets, &desired_buckets);
    let gcp_config = ctx.get_gcp_config()?;
    let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;
    let member = format!("serviceAccount:{service_account_email}");

    for grant_plan in grant_plans {
        let mut current_policy = client
            .get_bucket_iam_policy(grant_plan.bucket_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get IAM policy for remote Storage bucket '{}' before binding management permissions",
                    grant_plan.bucket_name
                ),
                resource_id: Some(grant_plan.bucket_name.clone()),
            })?;
        let owned_exact_roles =
            ResourcePermissionsHelper::gcp_predefined_role_names(&grant_plan.bindings);
        let changed = ResourcePermissionsHelper::reconcile_gcp_project_member_bindings(
            &mut current_policy.bindings,
            grant_plan.bindings,
            &member,
            &grant_plan.owned_role_prefixes,
            &owned_exact_roles,
        );
        if changed {
            current_policy.version = Some(3);
            client
                .set_bucket_iam_policy(grant_plan.bucket_name.clone(), current_policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to apply management permissions to remote Storage bucket '{}'",
                        grant_plan.bucket_name
                    ),
                    resource_id: Some(grant_plan.bucket_name),
                })?;
        }
    }

    revoke_grants_with_client(&*client, &member, &retired_buckets).await?;
    Ok(desired_buckets)
}

pub(super) async fn revoke_all_owned_grants(
    ctx: &ResourceControllerContext<'_>,
    service_account_email: &str,
    bucket_names: &[String],
) -> Result<()> {
    if bucket_names.is_empty() {
        return Ok(());
    }
    let gcp_config = ctx.get_gcp_config()?;
    let client = ctx.service_provider.get_gcp_gcs_client(gcp_config)?;
    let member = format!("serviceAccount:{service_account_email}");
    revoke_grants_with_client(&*client, &member, bucket_names).await
}

async fn revoke_grants_with_client(
    client: &dyn alien_gcp_clients::GcsApi,
    member: &str,
    bucket_names: &[String],
) -> Result<()> {
    for bucket_name in bucket_names {
        let mut current_policy = client
            .get_bucket_iam_policy(bucket_name.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get IAM policy for retired remote Storage bucket '{bucket_name}' before revoking management access"
                ),
                resource_id: Some(bucket_name.clone()),
            })?;
        // The management service account is generated and owned by Alien. Once
        // a bucket leaves its owned scope, no binding for that principal should
        // survive, including old custom-role hashes and predefined roles.
        let changed = ResourcePermissionsHelper::remove_gcp_project_member_bindings(
            &mut current_policy.bindings,
            member,
            None,
            None,
        );
        if changed {
            current_policy.version = Some(3);
            client
                .set_bucket_iam_policy(bucket_name.clone(), current_policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to revoke management access from retired remote Storage bucket '{bucket_name}'"
                    ),
                    resource_id: Some(bucket_name.clone()),
                })?;
        }
    }
    Ok(())
}

fn retired_bucket_names(previous: &[String], desired: &[String]) -> Vec<String> {
    let desired = desired.iter().collect::<BTreeSet<_>>();
    previous
        .iter()
        .filter(|bucket| !desired.contains(bucket))
        .cloned()
        .collect()
}

fn is_remote_frozen_storage(resource_entry: &alien_core::ResourceEntry) -> bool {
    resource_entry.lifecycle == ResourceLifecycle::Frozen
        && resource_entry.remote_access
        && resource_entry.config.downcast_ref::<Storage>().is_some()
}

fn remote_storage_bucket_name(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<String> {
    match remote_storage_binding(ctx, resource_id)? {
        Some(StorageBinding::Gcs(binding)) => concrete_storage_binding_value(
            binding.bucket_name,
            resource_id,
            "bucketName",
            "GCP GCS",
        ),
        Some(other) => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "Remote Storage resource '{resource_id}' must use a GCS binding on GCP, got {other:?}"
            ),
            resource_id: Some(resource_id.to_string()),
        })),
        None => Ok(format!("{}-{}", ctx.resource_prefix, resource_id)),
    }
}

fn remote_storage_binding(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<Option<StorageBinding>> {
    super::ensure_setup_owned_remote_storage(ctx, resource_id)?;

    let Some(binding) = ctx
        .state
        .resource(resource_id)
        .and_then(|state| state.remote_binding_params.as_ref())
    else {
        return Ok(None);
    };

    serde_json::from_value(binding.clone())
        .into_alien_error()
        .context(ErrorData::ResourceConfigInvalid {
            message: format!(
                "Remote Storage resource '{resource_id}' has invalid binding parameters"
            ),
            resource_id: Some(resource_id.to_string()),
        })
        .map(Some)
}

fn concrete_storage_binding_value(
    value: BindingValue<String>,
    resource_id: &str,
    field_name: &str,
    provider: &str,
) -> Result<String> {
    match value {
        BindingValue::Value(value) => Ok(value),
        BindingValue::Expression(_) | BindingValue::SecretRef { .. } => {
            Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Remote Storage resource '{resource_id}' requires a concrete {provider} {field_name}"
                ),
                resource_id: Some(resource_id.to_string()),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Resource, ResourceEntry};
    use alien_gcp_clients::gcs::MockGcsApi;
    use alien_gcp_clients::iam::IamPolicy;

    fn storage_entry(lifecycle: ResourceLifecycle, remote_access: bool) -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(Storage::new("archive".to_string()).build()),
            lifecycle,
            dependencies: Vec::new(),
            remote_access,
        }
    }

    #[test]
    fn only_opted_in_frozen_storage_is_managed_remotely() {
        assert!(is_remote_frozen_storage(&storage_entry(
            ResourceLifecycle::Frozen,
            true,
        )));
        assert!(!is_remote_frozen_storage(&storage_entry(
            ResourceLifecycle::Frozen,
            false,
        )));
        assert!(!is_remote_frozen_storage(&storage_entry(
            ResourceLifecycle::Live,
            true,
        )));
    }

    #[test]
    fn disable_and_rebind_retire_previous_bucket_scopes() {
        assert_eq!(
            retired_bucket_names(&["bucket-a".to_string()], &[]),
            vec!["bucket-a".to_string()],
        );
        assert_eq!(
            retired_bucket_names(&["bucket-a".to_string()], &["bucket-b".to_string()],),
            vec!["bucket-a".to_string()],
        );
        assert!(
            retired_bucket_names(&["bucket-a".to_string()], &["bucket-a".to_string()],).is_empty()
        );
    }

    #[test]
    fn legacy_synchronized_binding_recovers_previous_bucket_ownership() {
        let mut state = alien_core::StackState::new(alien_core::Platform::Gcp);
        state.resources.insert(
            "archive".to_string(),
            alien_core::StackResourceState::builder()
                .resource_type(Storage::RESOURCE_TYPE.as_ref().to_string())
                .status(alien_core::ResourceStatus::Running)
                .config(Resource::new(Storage::new("archive".to_string()).build()))
                .lifecycle(ResourceLifecycle::Frozen)
                .remote_binding_params(serde_json::json!({
                    "service": "gcs",
                    "bucketName": "legacy-bucket-a",
                }))
                .dependencies(Vec::new())
                .build(),
        );

        assert_eq!(
            observed_bucket_names_from_state(&state).unwrap(),
            ["legacy-bucket-a".to_string()],
        );
    }

    #[test]
    fn remote_storage_grant_targets_the_bucket_policy_not_project_iam() {
        let context = PermissionContext::new()
            .with_stack_prefix("test-stack".to_string())
            .with_project_name("test-project".to_string())
            .with_region("us-central1".to_string())
            .with_resource_id("archive".to_string())
            .with_resource_name("setup-owned-archive-bucket".to_string())
            .with_service_account_name("deployment-management".to_string());
        let permission_set = get_permission_set("storage/remote-data-write").unwrap();

        let grant_plan = GcpRuntimePermissionsGenerator::new()
            .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
            .unwrap();
        let bucket_bindings =
            grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);

        assert!(grant_plan
            .bindings_for_target(GcpBindingTargetScope::Project)
            .is_empty());
        assert_eq!(bucket_bindings.len(), 1);
        assert_eq!(
            bucket_bindings[0].members,
            ["serviceAccount:deployment-management@test-project.iam.gserviceaccount.com"]
        );
    }

    #[tokio::test]
    async fn retired_bucket_revocation_removes_only_the_management_principal() {
        let management_member =
            "serviceAccount:deployment-management@test-project.iam.gserviceaccount.com";
        let mut client = MockGcsApi::new();
        client
            .expect_get_bucket_iam_policy()
            .with(mockall::predicate::eq("bucket-a".to_string()))
            .times(1)
            .returning(move |_| {
                Ok(IamPolicy {
                    version: Some(3),
                    bindings: vec![
                        Binding {
                            role: "projects/test-project/roles/role_test_remote".to_string(),
                            members: vec![
                                management_member.to_string(),
                                "user:owner@example.com".to_string(),
                            ],
                            condition: None,
                        },
                        Binding {
                            role: "roles/storage.objectViewer".to_string(),
                            members: vec!["user:reader@example.com".to_string()],
                            condition: None,
                        },
                    ],
                    etag: Some("etag".to_string()),
                    kind: Some("storage#policy".to_string()),
                    resource_id: None,
                })
            });
        client
            .expect_set_bucket_iam_policy()
            .withf(move |bucket, policy| {
                bucket == "bucket-a"
                    && policy.bindings.iter().all(|binding| {
                        !binding
                            .members
                            .iter()
                            .any(|member| member == management_member)
                    })
                    && policy
                        .bindings
                        .iter()
                        .any(|binding| binding.members == ["user:owner@example.com".to_string()])
                    && policy
                        .bindings
                        .iter()
                        .any(|binding| binding.members == ["user:reader@example.com".to_string()])
            })
            .times(1)
            .returning(|_, policy| Ok(policy));

        revoke_grants_with_client(&client, management_member, &["bucket-a".to_string()])
            .await
            .expect("retired bucket grant must be revoked");
    }
}
