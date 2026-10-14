//! Project service-account inventory and applied quota reads.

use crate::quotas::{QuotaDemand, QuotaObservation};
use alien_core::{
    ArtifactRegistry, ComputeCluster, Platform, RemoteBindings, RemoteStackManagement,
    ResourceStatus, ServiceAccount, Stack, StackState,
};
use alien_gcp_clients::{service_usage_quotas::QuotaMetric, IamApi, ServiceUsageQuotasApi};
use std::collections::BTreeSet;

const ACCOUNT_METRIC: &str = "iam.googleapis.com/quota/service-account-count";

/// Only an unambiguous project-wide effective limit is usable; defaults are not.
pub fn service_account_limit(info: &QuotaMetric) -> Option<f64> {
    if info.metric != ACCOUNT_METRIC
        || !info.name.ends_with("/services/iam.googleapis.com/consumerQuotaMetrics/iam.googleapis.com%2Fquota%2Fservice-account-count")
    {
        return None;
    }
    let [limit] = info.consumer_quota_limits.as_deref()? else {
        return None;
    };
    if limit.metric != ACCOUNT_METRIC || limit.unit != "1/{project}" {
        return None;
    }
    let [bucket] = limit.quota_buckets.as_deref()? else {
        return None;
    };
    if !bucket.dimensions.is_empty() {
        return None;
    }
    let value = bucket.effective_limit.as_deref()?.parse::<u64>().ok()?;
    (value <= (1_u64 << 53)).then_some(value as f64)
}

/// Count accounts created by desired resources. Credit only identities present in
/// both stored state and the complete project inventory. Resource removal cannot
/// fund a create: deletion may happen later. Partial or incompatible state is
/// unknown rather than a reason to block an update with inflated demand.
pub fn service_account_demand(
    stack: &Stack,
    state: &StackState,
    emails: &BTreeSet<String>,
) -> Option<QuotaDemand> {
    let mut demand = QuotaDemand {
        peak: 0.0,
        held: 0.0,
    };
    let mut claimed = BTreeSet::new();
    for (id, entry) in &stack.resources {
        let fields: &[&str] = if entry.config.downcast_ref::<ArtifactRegistry>().is_some() {
            &["pullServiceAccountEmail", "pushServiceAccountEmail"]
        } else if entry.config.downcast_ref::<ServiceAccount>().is_some()
            || entry
                .config
                .downcast_ref::<RemoteStackManagement>()
                .is_some()
            || entry.config.downcast_ref::<RemoteBindings>().is_some()
            || entry.config.downcast_ref::<ComputeCluster>().is_some()
        {
            &["serviceAccountEmail"]
        } else {
            continue;
        };
        demand.peak += fields.len() as f64;
        let Some(current) = state.resources.get(id) else {
            continue;
        };
        if current.status == ResourceStatus::Deleted {
            continue;
        }
        if current.resource_type != entry.config.resource_type().as_ref()
            || current
                .controller_platform
                .is_some_and(|p| p != Platform::Gcp)
        {
            return None;
        }
        let Some(internal) = &current.internal_state else {
            if current.status == ResourceStatus::Pending && current.outputs.is_none() {
                continue;
            }
            return None;
        };
        let internal = internal.as_object()?;
        for field in fields {
            // An absent identity in a controller that has started may mean a
            // successful create whose response was not persisted yet.
            let email = internal.get(*field)?.as_str()?;
            if !emails.contains(email) || !claimed.insert(email) {
                return None;
            }
            demand.held += 1.0;
        }
    }
    Some(demand)
}

/// Read every page, including continuations after empty pages. Count all listed
/// accounts, including default and disabled accounts; do not infer exclusions
/// from their email suffix. Identity metadata is also needed for held credit.
/// Missing fields, duplicates, foreign projects and cursor cycles are unknown,
/// rather than a partial inventory that could underestimate usage.
pub async fn service_account_emails(iam: &dyn IamApi, project: &str) -> Option<BTreeSet<String>> {
    let prefix = format!("projects/{project}/serviceAccounts/");
    let mut emails = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut tokens = BTreeSet::new();
    let mut next = None;
    loop {
        let response = iam.list_service_accounts(Some(100), next).await.ok()?;
        for account in response.accounts {
            let email = account.email?;
            let id = account.unique_id?;
            let name = account.name?;
            if account.project_id.as_deref() != Some(project)
                || id.is_empty()
                || !id.bytes().all(|byte| byte.is_ascii_digit())
                || !name
                    .strip_prefix(&prefix)
                    .is_some_and(|suffix| suffix == email || suffix == id)
                || email.split_once('@').is_none_or(|(local, domain)| {
                    local.is_empty() || domain.is_empty() || domain.contains('@')
                })
                || !ids.insert(id)
                || !emails.insert(email)
            {
                return None;
            }
        }
        next = response.next_page_token.filter(|token| !token.is_empty());
        match &next {
            None => return Some(emails),
            Some(token) if !tokens.insert(token.clone()) => return None,
            Some(_) => {}
        }
    }
}

/// A failed read or incomplete inventory cannot become zero usage. No telemetry
/// read is needed: only quota values and service-account identity metadata.
pub async fn service_account_observation(
    iam: &dyn IamApi,
    quotas: &dyn ServiceUsageQuotasApi,
    project: &str,
) -> Option<(QuotaObservation, BTreeSet<String>)> {
    let info = quotas
        .get_quota_metric("iam.googleapis.com", ACCOUNT_METRIC)
        .await
        .ok()?;
    let limit = service_account_limit(&info)?;
    let emails = service_account_emails(iam, project).await?;
    Some((
        QuotaObservation {
            used: emails.len() as f64,
            limit,
        },
        emails,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{ResourceLifecycle, StackResourceState};
    use alien_gcp_clients::{
        iam::{ListServiceAccountsResponse, MockIamApi},
        service_usage_quotas::MockServiceUsageQuotasApi,
    };
    use serde_json::{json, Value};

    fn identity_stack() -> Stack {
        Stack::new("quota-test".into())
            .add(
                ServiceAccount::new("sa".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteStackManagement::new("management".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                RemoteBindings::new("bindings".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ComputeCluster::new("cluster".into()).build(),
                ResourceLifecycle::Live,
            )
            .add(
                ArtifactRegistry::new("registry".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build()
    }

    fn identity_state(stack: &Stack) -> (StackState, BTreeSet<String>) {
        let mut state = StackState::with_resource_prefix(Platform::Gcp, "q5-demand".into());
        let mut emails = BTreeSet::new();
        for (id, entry) in &stack.resources {
            let mut resource = StackResourceState::new_pending(
                entry.config.resource_type().as_ref().into(),
                entry.config.clone(),
                Some(entry.lifecycle),
                vec![],
            );
            resource.status = ResourceStatus::Running;
            resource.controller_platform = Some(Platform::Gcp);
            let fields: &[&str] = if id == "registry" {
                &["pullServiceAccountEmail", "pushServiceAccountEmail"]
            } else {
                &["serviceAccountEmail"]
            };
            let mut internal = serde_json::Map::new();
            for field in fields {
                let email = format!("{id}-{field}@test.iam.gserviceaccount.com");
                emails.insert(email.clone());
                internal.insert((*field).into(), json!(email));
            }
            resource.internal_state = Some(Value::Object(internal));
            state.resources.insert(id.clone(), resource);
        }
        (state, emails)
    }

    #[test]
    fn all_five_creators_count_six_accounts_and_updates_credit_observed_identities() {
        let stack = identity_stack();
        let empty = StackState::with_resource_prefix(Platform::Gcp, "q5-demand".into());
        let demand = service_account_demand(&stack, &empty, &BTreeSet::new()).unwrap();
        assert_eq!(
            (demand.peak, demand.held, demand.additional()),
            (6.0, 0.0, Some(6.0))
        );
        let (state, emails) = identity_state(&stack);
        let demand = service_account_demand(&stack, &state, &emails).unwrap();
        assert_eq!(
            (demand.peak, demand.held, demand.additional()),
            (6.0, 6.0, Some(0.0))
        );
        let mut updated = stack;
        updated.resources.shift_remove("registry");
        let demand = service_account_demand(&updated, &state, &emails).unwrap();
        assert_eq!(
            (demand.peak, demand.held, demand.additional()),
            (4.0, 4.0, Some(0.0))
        );
    }

    #[test]
    fn deleted_and_unstarted_resources_need_new_accounts_without_removed_resource_credit() {
        let stack = identity_stack();
        let (mut state, emails) = identity_state(&stack);
        state.resources.get_mut("sa").unwrap().status = ResourceStatus::Deleted;
        let registry = state.resources.get_mut("registry").unwrap();
        registry.status = ResourceStatus::Pending;
        registry.internal_state = None;
        let demand = service_account_demand(&stack, &state, &emails).unwrap();
        assert_eq!(
            (demand.peak, demand.held, demand.additional()),
            (6.0, 3.0, Some(3.0))
        );
    }

    #[test]
    fn partial_foreign_missing_and_duplicate_held_identities_are_unknown() {
        let stack = identity_stack();
        let (original, emails) = identity_state(&stack);
        for change in [
            "missing",
            "null",
            "malformed",
            "foreign",
            "type",
            "partial",
            "duplicate",
        ] {
            let mut state = original.clone();
            let duplicate = state.resources["management"]
                .internal_state
                .as_ref()
                .unwrap()["serviceAccountEmail"]
                .clone();
            let resource = state.resources.get_mut("sa").unwrap();
            match change {
                "missing" => resource.internal_state = None,
                "null" => resource.internal_state = Some(json!({"serviceAccountEmail":null})),
                "malformed" => resource.internal_state = Some(json!([])),
                "foreign" => resource.controller_platform = Some(Platform::Aws),
                "type" => resource.resource_type = "storage".into(),
                "partial" => {
                    resource.internal_state =
                        Some(json!({"serviceAccountEmail":"absent@test.iam.gserviceaccount.com"}))
                }
                "duplicate" => {
                    resource.internal_state = Some(json!({"serviceAccountEmail":duplicate}))
                }
                _ => unreachable!(),
            }
            assert!(
                service_account_demand(&stack, &state, &emails).is_none(),
                "{change}"
            );
        }
        let mut state = original;
        state
            .resources
            .get_mut("registry")
            .unwrap()
            .internal_state
            .as_mut()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("pushServiceAccountEmail");
        assert!(service_account_demand(&stack, &state, &emails).is_none());
    }

    fn account(id: &str, email: &str) -> Value {
        json!({"projectId":"test", "name":format!("projects/test/serviceAccounts/{email}"), "uniqueId":id, "email":email})
    }

    #[tokio::test]
    async fn includes_default_and_disabled_accounts_across_empty_pages() {
        let default = "123-compute@developer.gserviceaccount.com";
        let disabled = "disabled@test.iam.gserviceaccount.com";
        let mut disabled_account = account("2", disabled);
        disabled_account["disabled"] = json!(true);
        let mut iam = MockIamApi::new();
        // Expectations validate each requested cursor, not just the number of calls.
        for (cursor, response) in [
            (
                None,
                json!({"accounts":[account("1", default)],"nextPageToken":"empty"}),
            ),
            (Some("empty"), json!({"nextPageToken":"last"})),
            (Some("last"), json!({"accounts":[disabled_account]})),
        ] {
            let expected = cursor.map(str::to_owned);
            iam.expect_list_service_accounts()
                .times(1)
                .withf(move |size, token| *size == Some(100) && token == &expected)
                .return_once(move |_, _| {
                    Ok(serde_json::from_value::<ListServiceAccountsResponse>(response).unwrap())
                });
        }
        assert_eq!(
            service_account_emails(&iam, "test").await,
            Some(BTreeSet::from([default.into(), disabled.into()]))
        );
    }

    #[tokio::test]
    async fn incomplete_duplicate_foreign_and_repeated_cursor_inventories_are_unknown() {
        let valid = account("1", "one@test.iam.gserviceaccount.com");
        let mut invalid_accounts = vec![json!({}), valid.clone()];
        invalid_accounts[1]["projectId"] = json!("other");
        for field in ["name", "email", "uniqueId", "projectId"] {
            let mut invalid = valid.clone();
            invalid.as_object_mut().unwrap().remove(field);
            invalid_accounts.push(invalid);
        }
        for (field, value) in [
            (
                "name",
                "projects/other/serviceAccounts/one@test.iam.gserviceaccount.com",
            ),
            (
                "name",
                "projects/test/serviceAccounts/different@test.iam.gserviceaccount.com",
            ),
            ("uniqueId", ""),
            ("uniqueId", "not-numeric"),
            ("email", "invalid"),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = json!(value);
            invalid_accounts.push(invalid);
        }
        let mut responses: Vec<_> = invalid_accounts
            .into_iter()
            .map(|value| json!({"accounts":[value]}))
            .collect();
        responses.push(json!({"accounts":[valid.clone(),valid]}));
        responses.push(json!({"accounts":[account("1", "one@test.iam.gserviceaccount.com"),account("1", "two@test.iam.gserviceaccount.com")]}));
        responses.push(json!({"accounts":[account("1", "one@test.iam.gserviceaccount.com"),account("2", "one@test.iam.gserviceaccount.com")]}));
        responses.push(json!({"nextPageToken":"cycle"}));
        for response in responses {
            let mut iam = MockIamApi::new();
            iam.expect_list_service_accounts()
                .returning(move |_, _| Ok(serde_json::from_value(response.clone()).unwrap()));
            assert!(service_account_emails(&iam, "test").await.is_none());
        }
    }

    #[tokio::test]
    async fn effective_limit_and_complete_inventory_produce_observation() {
        let mut quotas = MockServiceUsageQuotasApi::new();
        quotas
            .expect_get_quota_metric()
            .times(1)
            .withf(|service, metric| service == "iam.googleapis.com" && metric == ACCOUNT_METRIC)
            .returning(|_, _| {
                Ok(
                    serde_json::from_str(include_str!("fixtures/gcp-service-account-quota.json"))
                        .unwrap(),
                )
            });
        let mut iam = MockIamApi::new();
        iam.expect_list_service_accounts()
            .times(1)
            .returning(|_, _| {
                Ok(serde_json::from_value(
                    json!({"accounts":[account("1", "one@test.iam.gserviceaccount.com")]}),
                )
                .unwrap())
            });
        let (observation, emails) = service_account_observation(&iam, &quotas, "test")
            .await
            .unwrap();
        assert_eq!(observation.used, 1.0);
        assert_eq!(observation.limit, 100.0);
        assert_eq!(
            emails,
            BTreeSet::from(["one@test.iam.gserviceaccount.com".into()])
        );
    }

    #[tokio::test]
    async fn denied_later_page_discards_partial_usage_and_denied_limit_skips_inventory() {
        let mut iam = MockIamApi::new();
        iam.expect_list_service_accounts().times(1)
            .withf(|_, token| token.is_none())
            .returning(|_, _| Ok(serde_json::from_value(json!({"accounts":[account("1", "one@test.iam.gserviceaccount.com")],"nextPageToken":"denied"})).unwrap()));
        iam.expect_list_service_accounts()
            .times(1)
            .withf(|_, token| token.as_deref() == Some("denied"))
            .returning(|_, _| {
                Err(alien_error::AlienError::new(
                    alien_gcp_clients::ErrorData::RemoteAccessDenied {
                        resource_type: "ServiceAccount".into(),
                        resource_name: "test".into(),
                    },
                ))
            });
        assert!(service_account_emails(&iam, "test").await.is_none());
        let mut quotas = MockServiceUsageQuotasApi::new();
        quotas.expect_get_quota_metric().times(1).returning(|_, _| {
            Err(alien_error::AlienError::new(
                alien_gcp_clients::ErrorData::RemoteAccessDenied {
                    resource_type: "QuotaMetric".into(),
                    resource_name: ACCOUNT_METRIC.into(),
                },
            ))
        });
        assert!(
            service_account_observation(&MockIamApi::new(), &quotas, "test")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn denied_listing_never_becomes_zero_usage() {
        let mut iam = MockIamApi::new();
        iam.expect_list_service_accounts()
            .times(1)
            .returning(|_, _| {
                Err(alien_error::AlienError::new(
                    alien_gcp_clients::ErrorData::RemoteAccessDenied {
                        resource_type: "ServiceAccount".into(),
                        resource_name: "test".into(),
                    },
                ))
            });
        assert!(service_account_emails(&iam, "test").await.is_none());
    }

    #[test]
    fn applied_limit_rejects_defaults_invalid_values_and_ambiguous_dimensions() {
        let valid: QuotaMetric =
            serde_json::from_str(include_str!("fixtures/gcp-service-account-quota.json")).unwrap();
        assert_eq!(service_account_limit(&valid), Some(100.0));
        for value in [
            None,
            Some("-1"),
            Some("NaN"),
            Some("1.5"),
            Some("9007199254740993"),
        ] {
            let mut info = valid.clone();
            info.consumer_quota_limits.as_mut().unwrap()[0]
                .quota_buckets
                .as_mut()
                .unwrap()[0]
                .effective_limit = value.map(str::to_owned);
            assert_eq!(service_account_limit(&info), None);
        }
        let mut info = valid.clone();
        info.consumer_quota_limits.as_mut().unwrap()[0]
            .quota_buckets
            .as_mut()
            .unwrap()[0]
            .effective_limit = Some("7".into());
        assert_eq!(service_account_limit(&info), Some(7.0));
        for change in ["metric", "unit", "buckets", "limits", "dimensions"] {
            let mut info = valid.clone();
            let limits = info.consumer_quota_limits.as_mut().unwrap();
            match change {
                "metric" => info.metric = "other".into(),
                "unit" => limits[0].unit = "1/min/{project}".into(),
                "buckets" => {
                    let bucket = limits[0].quota_buckets.as_ref().unwrap()[0].clone();
                    limits[0].quota_buckets.as_mut().unwrap().push(bucket);
                }
                "limits" => limits.push(limits[0].clone()),
                "dimensions" => {
                    limits[0].quota_buckets.as_mut().unwrap()[0]
                        .dimensions
                        .insert("region".into(), "any".into());
                }
                _ => unreachable!(),
            }
            assert_eq!(service_account_limit(&info), None);
        }
    }
}
