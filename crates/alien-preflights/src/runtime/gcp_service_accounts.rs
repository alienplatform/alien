//! Project service-account inventory and applied quota reads.

use crate::quotas::QuotaObservation;
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
    use alien_gcp_clients::{
        iam::{ListServiceAccountsResponse, MockIamApi},
        service_usage_quotas::MockServiceUsageQuotasApi,
    };
    use serde_json::{json, Value};

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
