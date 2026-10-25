//! Deferred propagation-window checks for retained storage. Call after disabling expiry.
use crate::{ErrorData, Result};
use alien_error::{AlienError, Context};
use alien_gcp_clients::gcs::{Bucket, GcsApi, Lifecycle};
use serde_json::Value;
/// A disabled Azure rule can finish an already-running lifecycle batch for up to 24 hours.
/// Refuse handoff until the setup owner disabled every applicable delete and its policy
/// change is old enough. Unknown metadata is never evidence of retained-data safety.
pub fn verify_azure_retention_policy(
    policy: &Value,
    container: &str,
    id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let rules = policy
        .pointer("/properties/policy/rules")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(id, "Azure lifecycle policy has no readable rule inventory"))?;
    let prefix = format!("{container}/");
    let mut has_delete = false;
    for rule in rules {
        let filters = rule
            .pointer("/definition/filters/prefixMatch")
            .and_then(Value::as_array);
        let applies = match filters {
            Some(filters) if !filters.is_empty() => filters.iter().any(|value| {
                value
                    .as_str()
                    .is_none_or(|filter| prefix.starts_with(filter) || filter.starts_with(&prefix))
            }),
            _ => true,
        };
        if !applies {
            continue;
        }
        let actions = rule
            .pointer("/definition/actions")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                invalid(
                    id,
                    "Azure lifecycle policy has an unreadable applicable action",
                )
            })?;
        let deletes = actions.iter().any(|(kind, action)| {
            !matches!(kind.as_str(), "baseBlob" | "snapshot" | "version")
                || action.get("delete").is_some_and(|delete| !delete.is_null())
        });
        if !deletes {
            continue;
        }
        has_delete = true;
        if rule.get("enabled").and_then(Value::as_bool) != Some(false) {
            return Err(AlienError::new(ErrorData::SetupRerunRequired {
                resource_id: id.to_string(),
                message: "Disable Azure lifecycle delete rules affecting this retained container using the original setup/account administrator, wait 24 hours for active batches to stop, then retry deletion. Shared account policies are not rewritten by runtime; retention is not complete.".into(),
            }));
        }
    }
    if has_delete {
        let changed = policy
            .pointer("/properties/lastModifiedTime")
            .and_then(Value::as_str)
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok());
        if !changed.is_some_and(|changed| {
            now.signed_duration_since(changed) >= chrono::Duration::hours(24)
        }) {
            return Err(invalid(
                id,
                "Azure lifecycle deletion may still be running. Confirm the disabled policy's lastModifiedTime is at least 24 hours old, then retry deletion; retained-data handoff is not complete",
            ));
        }
    }
    Ok(())
}

pub const GCS_EXPIRY_DISABLED_AT: &str = "alien-retention-expiry-disabled-at";

pub fn verify_gcs_expiry_window(
    bucket: &Bucket,
    id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let Some(value) = bucket
        .labels
        .as_ref()
        .and_then(|labels| labels.get(GCS_EXPIRY_DISABLED_AT))
    else {
        return Ok(());
    };
    let disabled = value.parse::<i64>().ok().and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .ok_or_else(|| invalid(id, "The retained bucket has an invalid expiry-disable timestamp; restore its protection receipt before retrying"))?;
    let ready = disabled
        .checked_add_signed(chrono::Duration::hours(24))
        .ok_or_else(|| {
            invalid(
                id,
                "The retained bucket expiry-disable time is outside the supported range",
            )
        })?;
    if now < ready {
        return Err(AlienError::new(ErrorData::SetupRerunRequired {
            resource_id: id.to_owned(),
            message: format!(
                "GCS can apply old lifecycle rules for 24 hours. Retry retention after {ready}; the expiry-disable timestamp remains on this bucket. Retry with the original setup credentials."
            ),
        }));
    }
    Ok(())
}

fn invalid(id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(id.to_string()),
        message: message.to_string(),
    })
}

fn cloud_error(id: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(id.to_string()),
        message: "Failed to protect retained storage; restore the required setup grants and retry deletion".to_string(),
    }
}

pub async fn stop_gcs_lifecycle(
    client: &dyn GcsApi,
    bucket: &str,
    id: &str,
    observed: &Bucket,
    wait_for_propagation: bool,
) -> Result<()> {
    let has_rules = |bucket: &Bucket| {
        bucket
            .lifecycle
            .as_ref()
            .and_then(|lifecycle| lifecycle.rule.as_ref())
            .is_some_and(|rules| !rules.is_empty())
    };
    if !has_rules(observed) {
        return if wait_for_propagation {
            verify_gcs_expiry_window(observed, id, chrono::Utc::now())
        } else {
            Ok(())
        };
    }
    // Persist the timestamp atomically with disabling expiry. Cloud metadata survives
    // a lost response or a process restart, unlike an in-memory deletion receipt.
    let disabled = chrono::Utc::now().timestamp().to_string();
    let mut labels = observed.labels.clone().unwrap_or_default();
    labels.insert(GCS_EXPIRY_DISABLED_AT.to_owned(), disabled.clone());
    // This is an owned bucket, not an account-level policy. Remove all lifecycle
    // actions without round-tripping incomplete models of foreign rule conditions.
    // Explicit [] is required: an empty lifecycle object preserves existing rules.
    client
        .update_bucket(
            bucket.to_string(),
            Bucket {
                lifecycle: Some(Lifecycle { rule: Some(vec![]) }),
                labels: Some(labels),
                ..Default::default()
            },
        )
        .await
        .context(cloud_error(id))?;
    let confirmed = client
        .get_bucket(bucket.to_string())
        .await
        .context(cloud_error(id))?;
    if has_rules(&confirmed) {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            resource_id: Some(id.to_string()),
            message: "GCS still reports lifecycle rules on the retained bucket; retry before removing access".to_string(),
        }));
    }
    if confirmed
        .labels
        .as_ref()
        .and_then(|labels| labels.get(GCS_EXPIRY_DISABLED_AT))
        != Some(&disabled)
    {
        return Err(invalid(
            id,
            "GCS has not confirmed the persisted expiry-disable timestamp; retry before removing access",
        ));
    }
    // Runtime can now remove workloads/access. The privileged setup handoff,
    // which includes Frozen scope, checks the durable 24-hour window before Retained.
    if wait_for_propagation {
        verify_gcs_expiry_window(&confirmed, id, chrono::Utc::now())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_client_core::ErrorData as ClientError;
    use alien_gcp_clients::gcs::MockGcsApi;
    #[tokio::test]
    async fn gcs_lifecycle_removal_refuses_denied_patch_and_unconfirmed_readback() {
        for denied in [true, false] {
            let mut api = MockGcsApi::new();
            let observed = Bucket {
                lifecycle: Some(Lifecycle {
                    rule: Some(vec![alien_gcp_clients::gcs::LifecycleRule::default()]),
                }),
                ..Default::default()
            };
            api.expect_update_bucket()
                .times(1)
                .returning(move |_, patch| {
                    assert_eq!(
                        serde_json::to_value(&patch.lifecycle).unwrap(),
                        json!({"rule":[]})
                    );
                    assert!(patch
                        .labels
                        .as_ref()
                        .unwrap()
                        .contains_key(GCS_EXPIRY_DISABLED_AT));
                    if denied {
                        Err(AlienError::new(ClientError::RemoteAccessDenied {
                            resource_type: "bucket".into(),
                            resource_name: "owned-data".into(),
                        }))
                    } else {
                        Ok(patch)
                    }
                });
            let readback = observed.clone();
            api.expect_get_bucket()
                .times(usize::from(!denied))
                .returning(move |_| Ok(readback.clone()));
            assert!(
                stop_gcs_lifecycle(&api, "owned-data", "data", &observed, true)
                    .await
                    .is_err()
            );
        }
    }

    use serde_json::json;
    #[test]
    fn gcs_expiry_receipt_requires_a_full_day_and_rejects_unknown_or_future_times() {
        let now = chrono::Utc::now();
        for (stamp, ready) in [
            (
                (now - chrono::Duration::hours(25)).timestamp().to_string(),
                true,
            ),
            (
                (now - chrono::Duration::hours(23)).timestamp().to_string(),
                false,
            ),
            (
                (now + chrono::Duration::hours(1)).timestamp().to_string(),
                false,
            ),
            ("invalid".to_owned(), false),
        ] {
            let bucket = Bucket {
                labels: Some(
                    [(GCS_EXPIRY_DISABLED_AT.to_owned(), stamp)]
                        .into_iter()
                        .collect(),
                ),
                ..Default::default()
            };
            assert_eq!(
                verify_gcs_expiry_window(&bucket, "data", now).is_ok(),
                ready
            );
        }
    }

    #[test]
    fn azure_retention_requires_disabled_applicable_deletes_and_completed_propagation() {
        let now = chrono::Utc::now();
        let rule = json!({"name":"expiry", "enabled":true, "type":"Lifecycle",
            "definition":{"filters":{"blobTypes":["blockBlob"],"prefixMatch":["owned-data/"]},
            "actions":{"baseBlob":{"delete":{"daysAfterModificationGreaterThan":1}}}}});
        let mut policy = json!({"properties":{"lastModifiedTime": (now - chrono::Duration::hours(25)).to_rfc3339(),
            "policy":{"rules":[rule]}}});
        assert!(verify_azure_retention_policy(&policy, "owned-data", "data", now).is_err());
        assert!(verify_azure_retention_policy(&policy, "sibling", "data", now).is_ok());
        policy["properties"]["policy"]["rules"][0]["enabled"] = json!(false);
        assert!(verify_azure_retention_policy(&policy, "owned-data", "data", now).is_ok());
        for changed in [
            Value::Null,
            json!("invalid"),
            json!((now - chrono::Duration::hours(23)).to_rfc3339()),
        ] {
            policy["properties"]["lastModifiedTime"] = changed;
            assert!(verify_azure_retention_policy(&policy, "owned-data", "data", now).is_err());
        }
        policy["properties"]["policy"]["rules"][0]["definition"]["actions"] =
            json!({"baseBlob":{"tierToCool":{"daysAfterModificationGreaterThan":1}}});
        assert!(verify_azure_retention_policy(&policy, "owned-data", "data", now).is_ok());
    }
}
