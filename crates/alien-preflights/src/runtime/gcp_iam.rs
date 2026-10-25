//! IAM custom-role usage includes soft-deleted roles, not just live roles.

use crate::quotas::{
    evaluate, gcp_custom_role_quota_identity, QuotaDemand, QuotaObservation, QuotaOutcome,
    QuotaResult,
};
use crate::CheckResult;
use alien_core::{
    ComputeCluster, GcpClientConfig, Platform, RemoteStackManagement, ServiceAccount, Stack,
    StackState,
};
use alien_gcp_clients::{service_usage_quotas::QuotaMetric, IamApi, ServiceUsageQuotasApi};
use alien_permissions::{
    generators::{
        custom_role_namespace_for_prefix, legacy_custom_role_namespace_for_prefix,
        GcpBindingTargetScope, GcpRuntimePermissionsGenerator,
    },
    get_permission_set, management_identity_global_refs, BindingTarget, PermissionContext,
};
use serde::Deserialize;
use std::collections::BTreeSet;

const ROLE_METRIC: &str = "iam.googleapis.com/roles/custom_roles_count";

/// Identity metadata needed to preserve the naming rule of older deployments.
/// This projection deliberately has no dependency on controller implementations.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityNaming {
    service_account_email: Option<String>,
    custom_role_naming: Option<RoleNaming>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RoleNaming {
    HashedLongPrefix,
    TruncatedPrefix,
}

fn role_namespace(state: &StackState) -> Option<String> {
    let mut ids: Vec<_> = state.resources.keys().collect();
    ids.sort();
    let mut recorded = None;
    let mut legacy = false;
    for id in ids {
        let resource = &state.resources[id];
        if resource
            .controller_platform
            .is_some_and(|platform| platform != Platform::Gcp)
            || (resource.resource_type != ServiceAccount::RESOURCE_TYPE.as_ref()
                && resource.resource_type != RemoteStackManagement::RESOURCE_TYPE.as_ref())
        {
            continue;
        }
        let Some(internal) = &resource.internal_state else {
            continue;
        };
        let identity: IdentityNaming = serde_json::from_value(internal.clone()).ok()?;
        match (identity.service_account_email, identity.custom_role_naming) {
            (_, Some(naming)) => {
                recorded.get_or_insert(naming);
            }
            (Some(_), None) => legacy = true,
            _ => {}
        }
    }
    Some(
        match recorded.unwrap_or(if legacy {
            RoleNaming::TruncatedPrefix
        } else {
            RoleNaming::HashedLongPrefix
        }) {
            RoleNaming::HashedLongPrefix => {
                custom_role_namespace_for_prefix(&state.resource_prefix)
            }
            RoleNaming::TruncatedPrefix => {
                legacy_custom_role_namespace_for_prefix(&state.resource_prefix)
            }
        },
    )
}

/// Desired role identities, shared across all principals and concrete resources.
/// Generate the same grant plans used for setup, rather than count permission sets:
/// predefined roles need no quota, and a set may emit several custom role IDs.
/// Incomplete state or unresolved grant context leaves demand unknown.
pub fn desired_custom_role_names(
    stack: &Stack,
    state: &StackState,
    config: &GcpClientConfig,
) -> Option<BTreeSet<String>> {
    let context = PermissionContext::new()
        .with_project_name(&config.project_id)
        .with_region(&config.region)
        .with_stack_prefix(&state.resource_prefix)
        .with_gcp_custom_role_namespace(role_namespace(state)?);
    let context = if let Some(number) = &config.project_number {
        context.with_project_number(number)
    } else {
        context
    };
    let generator = GcpRuntimePermissionsGenerator::new();
    let mut names = BTreeSet::new();
    let mut collect = |set: &alien_core::PermissionSet,
                       target,
                       context: &PermissionContext,
                       selected: Option<GcpBindingTargetScope>|
     -> Option<()> {
        if set.platforms.gcp.is_none() {
            return Some(());
        }
        let plan = generator.generate_grant_plan(set, target, context).ok()?;
        let roles = match selected {
            Some(scope) => plan.custom_roles_for_bindings(&plan.bindings_for_target(scope)),
            None => plan.custom_roles,
        };
        names.extend(roles.into_iter().map(|role| role.name));
        Some(())
    };
    for (id, entry) in &stack.resources {
        if let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(entry) {
            let resource_context = context
                .clone()
                .with_service_account_name(format!("{}-remote-bindings", state.resource_prefix))
                .with_resource_name(format!("{}-{id}", state.resource_prefix));
            collect(
                get_permission_set(definition.permission_set)?,
                BindingTarget::Resource,
                &resource_context,
                Some(GcpBindingTargetScope::CurrentResource),
            )?;
        }
        if let Some(account) = entry.config.downcast_ref::<ServiceAccount>() {
            let context = context
                .clone()
                .with_service_account_name(format!("{}-{}", state.resource_prefix, account.id));
            for set in &account.stack_permission_sets {
                collect(set, BindingTarget::Stack, &context, None)?;
            }
            let legacy_profile = account
                .id
                .strip_suffix("-sa")
                .and_then(|name| stack.permissions.profiles.get(name));
            let concrete = account
                .concrete_permission_sets(legacy_profile, |name| get_permission_set(name).cloned())
                .ok()?;
            for (id, sets) in concrete.iter() {
                let resource_context = context
                    .clone()
                    .with_resource_name(format!("{}-{id}", state.resource_prefix));
                for set in sets {
                    collect(set, BindingTarget::Resource, &resource_context, None)?;
                }
            }
        }
        if let Some(cluster) = entry.config.downcast_ref::<ComputeCluster>() {
            // Node data grants need resource-specific resolved binding context.
            if cluster
                .node_permissions
                .as_ref()
                .is_some_and(|profile| profile.0.values().any(|sets| !sets.is_empty()))
            {
                return None;
            }
            // Execute roles use the current namespace even on a legacy deployment.
            let execute_context = context
                .clone()
                .with_gcp_custom_role_namespace(custom_role_namespace_for_prefix(
                    &state.resource_prefix,
                ))
                .with_service_account_name(format!("{}-{}", state.resource_prefix, cluster.id))
                .with_managing_project_id(&config.project_id);
            collect(
                get_permission_set("compute-cluster/execute")?,
                BindingTarget::Stack,
                &execute_context,
                Some(GcpBindingTargetScope::Project),
            )?;
        }
    }
    if stack.resources.values().any(|entry| {
        entry
            .config
            .downcast_ref::<RemoteStackManagement>()
            .is_some()
    }) {
        if let Some(profile) = stack.management().profile() {
            let context =
                context.with_service_account_name(format!("{}-management", state.resource_prefix));
            let global = management_identity_global_refs(stack.resources.values(), profile);
            for reference in &global {
                let set = reference.resolve(|name| get_permission_set(name).cloned())?;
                collect(&set, BindingTarget::Stack, &context, None)?;
            }
            for (id, references) in &profile.0 {
                if id == "*" {
                    continue;
                }
                let entry = stack.resources.get(id)?;
                // Kubernetes grants require resolved cluster identity context.
                if entry
                    .config
                    .downcast_ref::<alien_core::KubernetesCluster>()
                    .is_some()
                {
                    return None;
                }
                let resource_context = context
                    .clone()
                    .with_resource_name(format!("{}-{id}", state.resource_prefix));
                for reference in references {
                    let set = reference.resolve(|name| get_permission_set(name).cloned())?;
                    collect(
                        &set,
                        BindingTarget::Resource,
                        &resource_context,
                        Some(GcpBindingTargetScope::ServiceAccount),
                    )?;
                    collect(
                        &set,
                        BindingTarget::Resource,
                        &resource_context,
                        Some(GcpBindingTargetScope::CurrentResource),
                    )?;
                }
            }
        }
    }
    Some(names)
}

pub async fn check(
    stack: &Stack,
    state: &StackState,
    config: &GcpClientConfig,
    iam: &dyn IamApi,
    quotas: &dyn ServiceUsageQuotasApi,
    result: &mut CheckResult,
) {
    let desired = desired_custom_role_names(stack, state, config);
    if desired.as_ref().is_some_and(BTreeSet::is_empty) {
        return;
    }
    let outcome = match desired {
        None => QuotaOutcome::Unknown {
            reason: "custom-role naming or grant context could not be resolved".into(),
        },
        Some(desired) => match custom_role_observation(iam, quotas, &config.project_id).await {
            Some((observation, existing)) => evaluate(
                QuotaDemand {
                    peak: desired.len() as f64,
                    held: desired.intersection(&existing).count() as f64,
                },
                Some(observation),
            ),
            None => QuotaOutcome::Unknown {
                reason: "project custom-role usage or applied quota is unavailable".into(),
            },
        },
    };
    super::cloud_quotas::record(
        result,
        QuotaResult {
            quota: gcp_custom_role_quota_identity(&config.project_id),
            outcome,
        },
    );
}

/// Read only the unambiguous project-wide applied value; never use defaults.
pub fn custom_role_limit(info: &QuotaMetric) -> Option<f64> {
    if info.metric != ROLE_METRIC || !info.name.ends_with("/services/iam.googleapis.com/consumerQuotaMetrics/iam.googleapis.com%2Froles%2Fcustom_roles_count") {
        return None;
    }
    let [limit] = info.consumer_quota_limits.as_ref()?.as_slice() else {
        return None;
    };
    if limit.metric != ROLE_METRIC || limit.unit != "1/{project}" {
        return None;
    }
    let [bucket] = limit.quota_buckets.as_ref()?.as_slice() else {
        return None;
    };
    if !bucket.dimensions.is_empty() {
        return None;
    }
    let value = bucket.effective_limit.as_ref()?.parse::<u64>().ok()?;
    // Preserve exact integer arithmetic through the f64 evaluator.
    (value <= (1_u64 << 53)).then_some(value as f64)
}

/// Deleted roles count until scheduled permanent deletion (7–14 days); later
/// ID reservation is separate from quota usage. See
/// https://cloud.google.com/iam/docs/creating-custom-roles#deleting_a_custom_role.
/// All returned identities count, including deleted roles. BASIC roles.list
/// exposes metadata only; no role bindings or customer telemetry are read.
/// Existing deleted identities also receive demand credit: controllers undelete
/// the same ID rather than allocate another quota slot.
pub async fn custom_role_names(iam: &dyn IamApi, project: &str) -> Option<BTreeSet<String>> {
    let prefix = format!("projects/{project}/roles/");
    let mut names = BTreeSet::new();
    let mut tokens = BTreeSet::new();
    let mut next = None;
    loop {
        let response = iam.list_roles(Some(100), next, Some(true)).await.ok()?;
        for role in response.roles {
            let name = role.name?;
            if !name.starts_with(&prefix) || name.len() == prefix.len() || !names.insert(name) {
                return None;
            }
        }
        next = response.next_page_token.filter(|token| !token.is_empty());
        match &next {
            None => return Some(names),
            Some(token) if !tokens.insert(token.clone()) => return None,
            Some(_) => {}
        }
    }
}

/// Read the applied quota and usage. A disabled API, denied permission, malformed
/// response or pagination inconsistency must become a non-blocking Unknown.
pub async fn custom_role_observation(
    iam: &dyn IamApi,
    quotas: &dyn ServiceUsageQuotasApi,
    project: &str,
) -> Option<(QuotaObservation, BTreeSet<String>)> {
    let info = quotas
        .get_quota_metric("iam.googleapis.com", ROLE_METRIC)
        .await
        .ok()?;
    let limit = custom_role_limit(&info)?;
    let names = custom_role_names(iam, project).await?;
    Some((
        QuotaObservation {
            used: names.len() as f64,
            limit,
        },
        names,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quotas::{evaluate, QuotaDemand, QuotaOutcome};
    use crate::RuntimeCheck;
    use alien_gcp_clients::iam::{ListRolesResponse, MockIamApi};
    use alien_gcp_clients::service_usage_quotas::MockServiceUsageQuotasApi;

    fn config() -> GcpClientConfig {
        GcpClientConfig {
            project_id: "test".into(),
            region: "us-east4".into(),
            credentials: alien_core::GcpCredentials::AccessToken {
                token: "unused".into(),
            },
            project_number: Some("123456".into()),
            service_overrides: None,
        }
    }

    fn role_stack() -> Stack {
        let set = get_permission_set("storage/data-write").unwrap().clone();
        Stack::new("quota-test".into())
            .add(
                ServiceAccount::new("one-sa".into())
                    .stack_permission_set(set.clone())
                    .build(),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                ServiceAccount::new("two-sa".into())
                    .stack_permission_set(set)
                    .build(),
                alien_core::ResourceLifecycle::Live,
            )
            .build()
    }

    #[test]
    fn role_identity_deduplication_ignores_predefined_roles() {
        let stack = role_stack();
        let state = StackState::with_resource_prefix(Platform::Gcp, "q5-roles".into());
        let names = desired_custom_role_names(&stack, &state, &config()).unwrap();
        // Bucket access is predefined; only service-account signing creates a role.
        assert_eq!(names.len(), 1);
        assert!(names
            .iter()
            .all(|name| name.starts_with("projects/test/roles/role_q5_roles_storage_data_write")));
        let predefined: alien_core::PermissionSet = serde_json::from_value(serde_json::json!({
            "id":"only-predefined", "description":"No custom roles",
            "platforms":{"gcp":[{"grant":{"predefinedRoles":["roles/iam.serviceAccountTokenCreator"]}, "binding":{"stack":{"scope":"projects/${projectName}"}}}]}
        })).unwrap();
        let stack = Stack::new("predefined".into())
            .add(
                ServiceAccount::new("sa".into())
                    .stack_permission_set(predefined)
                    .build(),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        assert!(desired_custom_role_names(&stack, &state, &config())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn legacy_namespace_recorded_precedence_and_malformed_state() {
        let stack = role_stack();
        let prefix = "q5-long-deployment-prefix";
        let mut state = StackState::with_resource_prefix(Platform::Gcp, prefix.into());
        assert_eq!(
            role_namespace(&state),
            Some(custom_role_namespace_for_prefix(prefix))
        );
        let mut resource = alien_core::StackResourceState::new_pending(
            "service-account".into(),
            stack.resources["one-sa"].config.clone(),
            None,
            vec![],
        );
        resource.internal_state =
            Some(serde_json::json!({"serviceAccountEmail":"sa@test.iam.gserviceaccount.com"}));
        state.resources.insert("a".into(), resource.clone());
        assert_eq!(
            role_namespace(&state),
            Some(legacy_custom_role_namespace_for_prefix(prefix))
        );
        assert!(desired_custom_role_names(&stack, &state, &config())
            .unwrap()
            .iter()
            .all(|name| name.contains(&format!(
                "role_{}_",
                legacy_custom_role_namespace_for_prefix(prefix)
            ))));
        resource.internal_state.as_mut().unwrap()["customRoleNaming"] =
            serde_json::json!("hashedLongPrefix");
        state.resources.insert("b".into(), resource);
        assert_eq!(
            role_namespace(&state),
            Some(custom_role_namespace_for_prefix(prefix))
        );
        state
            .resources
            .get_mut("a")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()["serviceAccountEmail"] = serde_json::json!(123);
        assert!(desired_custom_role_names(&stack, &state, &config()).is_none());
    }

    #[tokio::test]
    async fn full_project_blocks_only_new_role_ids_and_credits_deleted_identities() {
        let stack = role_stack();
        let state = StackState::with_resource_prefix(Platform::Gcp, "q5-roles".into());
        let desired = desired_custom_role_names(&stack, &state, &config()).unwrap();
        for reuse in [false, true] {
            let mut quotas = MockServiceUsageQuotasApi::new();
            quotas.expect_get_quota_metric().times(1).returning(|_, _| {
                Ok(
                    serde_json::from_str(include_str!("fixtures/gcp-custom-role-quota.json"))
                        .unwrap(),
                )
            });
            let mut roles = (0..300).map(|i| serde_json::json!({"name":format!("projects/test/roles/existing_{i}"), "deleted":true})).collect::<Vec<_>>();
            if reuse {
                for (i, name) in desired.iter().enumerate() {
                    roles[i]["name"] = serde_json::json!(name);
                }
            }
            let mut iam = MockIamApi::new();
            iam.expect_list_roles()
                .times(1)
                .withf(|_, _, deleted| *deleted == Some(true))
                .returning(move |_, _, _| {
                    Ok(serde_json::from_value(serde_json::json!({"roles":roles})).unwrap())
                });
            let mut result = CheckResult::success();
            check(&stack, &state, &config(), &iam, &quotas, &mut result).await;
            assert_eq!(result.success, reuse);
            assert_eq!(result.quota_results.len(), 1);
            assert_eq!(
                result.quota_results[0].outcome,
                if reuse {
                    QuotaOutcome::Pass {
                        used: 300.0,
                        limit: 300.0,
                        needed: 0.0,
                    }
                } else {
                    QuotaOutcome::Fail {
                        used: 300.0,
                        limit: 300.0,
                        needed: 1.0,
                    }
                }
            );
            if !reuse {
                assert!(result.errors[0].contains(ROLE_METRIC));
            }
        }
    }

    #[test]
    fn management_global_and_concrete_self_roles_are_combined() {
        let profile = alien_core::PermissionProfile::new()
            .resource(
                "*",
                [
                    "service-account/provision",
                    "service-account/management",
                    "service-account/heartbeat",
                ],
            )
            .resource("one-sa", ["service-account/impersonate"]);
        let mut stack = role_stack();
        stack.permissions.management = alien_core::ManagementPermissions::Override(profile);
        stack.resources.insert(
            "management".into(),
            alien_core::ResourceEntry {
                config: alien_core::Resource::new(
                    RemoteStackManagement::new("management".into()).build(),
                ),
                lifecycle: alien_core::ResourceLifecycle::Live,
                dependencies: vec![],
                added_by_alien: false,
                display_name: None,
                description: None,
                remote_access: false,
                enabled_when: None,
            },
        );
        let state = StackState::with_resource_prefix(Platform::Gcp, "q5-roles".into());
        let names = desired_custom_role_names(&stack, &state, &config()).unwrap();
        // Storage signing, provisioning, project inventory, and one quota role
        // shared by heartbeat/management.
        assert_eq!(names.len(), 4);
        assert_eq!(
            names
                .iter()
                .filter(|name| name.ends_with("_service_account_quota_inventory"))
                .count(),
            1,
            "project-inventory role missing from {names:?}"
        );
        assert_eq!(
            names
                .iter()
                .filter(|name| name.ends_with("_service_account_quota_metadata"))
                .count(),
            1,
            "shared quota-metadata role missing from {names:?}"
        );
        assert!(names
            .iter()
            .any(|name| name.contains("_service_account_provision_")));
        assert!(super::super::cloud_quotas::CloudQuotasCheck.should_run(&stack, Platform::Gcp));
        assert!(!super::super::cloud_quotas::CloudQuotasCheck.should_run(&stack, Platform::Local));
    }

    #[test]
    fn compute_execute_roles_use_current_namespace_and_deduplicate_across_clusters() {
        let stack = Stack::new("compute".into())
            .add(
                ComputeCluster::new("one".into()).build(),
                alien_core::ResourceLifecycle::Live,
            )
            .add(
                ComputeCluster::new("two".into()).build(),
                alien_core::ResourceLifecycle::Live,
            )
            .build();
        let prefix = "q5-legacy-compute-deployment";
        let mut state = StackState::with_resource_prefix(Platform::Gcp, prefix.into());
        let mut legacy = alien_core::StackResourceState::new_pending(
            "service-account".into(),
            alien_core::Resource::new(ServiceAccount::new("legacy-sa".into()).build()),
            None,
            vec![],
        );
        legacy.internal_state =
            Some(serde_json::json!({"serviceAccountEmail":"sa@test.iam.gserviceaccount.com"}));
        state.resources.insert("legacy-sa".into(), legacy);
        assert_eq!(
            role_namespace(&state),
            Some(legacy_custom_role_namespace_for_prefix(prefix))
        );
        let names = desired_custom_role_names(&stack, &state, &config()).unwrap();
        // Three runtime grants (impersonation, disks, NEG) shared by both clusters.
        assert_eq!(names.len(), 3);
        let current_prefix = format!(
            "projects/test/roles/role_{}_",
            custom_role_namespace_for_prefix(prefix)
        );
        assert!(names.iter().all(|name| name.starts_with(&current_prefix)));
    }

    #[tokio::test]
    async fn unresolved_naming_is_visible_unknown_without_cloud_calls() {
        let stack = role_stack();
        let mut state = StackState::with_resource_prefix(Platform::Gcp, "q5-roles".into());
        let mut resource = alien_core::StackResourceState::new_pending(
            "service-account".into(),
            stack.resources["one-sa"].config.clone(),
            None,
            vec![],
        );
        resource.internal_state = Some(serde_json::json!({"customRoleNaming":"unexpected"}));
        state.resources.insert("one-sa".into(), resource);
        let mut result = CheckResult::success();
        check(
            &stack,
            &state,
            &config(),
            &MockIamApi::new(),
            &MockServiceUsageQuotasApi::new(),
            &mut result,
        )
        .await;
        assert!(result.success);
        assert_eq!(result.status, Some(crate::CheckStatus::Unknown));
        assert_eq!(result.quota_results.len(), 1);
        assert!(matches!(
            result.quota_results[0].outcome,
            QuotaOutcome::Unknown { .. }
        ));
        assert_eq!(result.warnings.len(), 1);
    }

    #[tokio::test]
    async fn denied_applied_limit_never_becomes_a_default_or_zero_usage() {
        let mut quotas = MockServiceUsageQuotasApi::new();
        quotas
            .expect_get_quota_metric()
            .times(1)
            .withf(|service, id| service == "iam.googleapis.com" && id == ROLE_METRIC)
            .returning(|_, _| {
                Err(alien_error::AlienError::new(
                    alien_gcp_clients::ErrorData::RemoteAccessDenied {
                        resource_type: "QuotaInfo".into(),
                        resource_name: ROLE_METRIC.into(),
                    },
                ))
            });
        let iam = MockIamApi::new();
        let observation = custom_role_observation(&iam, &quotas, "test").await;
        assert!(observation.is_none());
        let outcome = evaluate(
            QuotaDemand {
                peak: 1.0,
                held: 0.0,
            },
            observation.map(|(value, _)| value),
        );
        assert!(matches!(outcome, QuotaOutcome::Unknown { .. }));
        assert!(!outcome.blocks());
    }

    #[test]
    fn applied_role_limit_rejects_ambiguous_or_invalid_values() {
        let valid: QuotaMetric =
            serde_json::from_str(include_str!("fixtures/gcp-custom-role-quota.json")).unwrap();
        assert_eq!(custom_role_limit(&valid), Some(300.0));
        for value in ["-1", "NaN", "1.5", "9007199254740993"] {
            let mut info = valid.clone();
            info.consumer_quota_limits.as_mut().unwrap()[0]
                .quota_buckets
                .as_mut()
                .unwrap()[0]
                .effective_limit = Some(value.into());
            assert_eq!(custom_role_limit(&info), None);
        }
        let mut info = valid.clone();
        info.consumer_quota_limits = None;
        assert_eq!(custom_role_limit(&info), None);
        let mut info = valid.clone();
        let limit = info.consumer_quota_limits.as_ref().unwrap()[0].clone();
        info.consumer_quota_limits.as_mut().unwrap().push(limit);
        assert_eq!(custom_role_limit(&info), None);
        let mut info = valid.clone();
        info.consumer_quota_limits.as_mut().unwrap()[0].unit = "1/min/{project}".into();
        assert_eq!(custom_role_limit(&info), None);
        let mut info = valid;
        info.metric = "another_metric".into();
        assert_eq!(custom_role_limit(&info), None);
    }

    #[tokio::test]
    async fn includes_deleted_roles_and_every_page() {
        let mut iam = MockIamApi::new();
        iam.expect_list_roles()
            .times(1)
            .withf(|size, token, deleted| {
                *size == Some(100) && token.is_none() && *deleted == Some(true)
            })
            .returning(|_, _, _| {
                Ok(serde_json::from_str::<ListRolesResponse>(
                    r#"{"roles":[{"name":"projects/test/roles/live"}],"nextPageToken":"two"}"#,
                )
                .unwrap())
            });
        iam.expect_list_roles()
            .times(1)
            .withf(|_, token, deleted| token.as_deref() == Some("two") && *deleted == Some(true))
            .returning(|_, _, _| {
                Ok(serde_json::from_str::<ListRolesResponse>(
                    r#"{"roles":[{"name":"projects/test/roles/deleted","deleted":true}]}"#,
                )
                .unwrap())
            });
        assert_eq!(
            custom_role_names(&iam, "test").await.unwrap(),
            BTreeSet::from([
                "projects/test/roles/live".into(),
                "projects/test/roles/deleted".into(),
            ])
        );
    }

    #[tokio::test]
    async fn incomplete_duplicates_foreign_scope_and_repeated_tokens_are_unknown() {
        for json in [
            r#"{"roles":[{}]}"#,
            r#"{"roles":[{"name":"projects/other/roles/role"}]}"#,
            r#"{"roles":[{"name":"projects/test/roles/"}]}"#,
            r#"{"roles":[{"name":"projects/test/roles/r"},{"name":"projects/test/roles/r"}]}"#,
            r#"{"nextPageToken":"repeat"}"#,
        ] {
            let mut iam = MockIamApi::new();
            iam.expect_list_roles()
                .returning(move |_, _, _| Ok(serde_json::from_str(json).unwrap()));
            assert_eq!(custom_role_names(&iam, "test").await, None);
        }
    }
}
