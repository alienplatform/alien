//! IAM custom-role usage includes soft-deleted roles, not just live roles.

use crate::quotas::QuotaObservation;
use alien_gcp_clients::{service_usage_quotas::QuotaMetric, IamApi, ServiceUsageQuotasApi};
use std::collections::BTreeSet;

const ROLE_METRIC: &str = "iam.googleapis.com/roles/custom_roles_count";

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
    use alien_gcp_clients::iam::{ListRolesResponse, MockIamApi};
    use alien_gcp_clients::service_usage_quotas::MockServiceUsageQuotasApi;

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
