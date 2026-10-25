//! Deferred propagation-window checks for retained storage. Call after disabling expiry.
use crate::{ErrorData, Result};
use alien_error::AlienError;
use alien_gcp_clients::gcs::Bucket;
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

#[cfg(test)]
mod tests {
    use super::*;
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
