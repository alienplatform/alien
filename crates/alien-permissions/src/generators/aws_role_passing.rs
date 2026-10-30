use alien_core::AwsConditionValue;
use alien_error::AlienError;
use indexmap::IndexMap;

use crate::{registry::iam_action_pattern_matches, ErrorData, Result};

/// Validate grants without rewriting user-authored actions or conditions.
pub(super) fn validate_pass_role_service(
    permission: &str,
    effect: &str,
    actions: &[String],
    conditions: Option<&IndexMap<String, IndexMap<String, AwsConditionValue>>>,
) -> Result<()> {
    if effect != "Allow"
        || !actions.iter().any(|action| {
            iam_action_pattern_matches(action.to_ascii_lowercase().as_bytes(), b"iam:passrole")
        })
    {
        return Ok(());
    }

    // IfExists and ForAllValues can accept a missing key. Require an exact,
    // positive comparison with literal services, including for custom grants.
    let restricted = conditions
        .and_then(|conditions| conditions.get("StringEquals"))
        .is_some_and(|values| {
            values.iter().any(|(key, value)| {
                key.eq_ignore_ascii_case("iam:PassedToService")
                    && match value {
                        AwsConditionValue::String(service) => literal_service(service),
                        AwsConditionValue::List(services) => {
                            !services.is_empty() && services.iter().all(|s| literal_service(s))
                        }
                    }
            })
        });
    if !restricted {
        return Err(AlienError::new(ErrorData::PassRoleServiceRequired {
            permission: permission.to_string(),
        }));
    }
    Ok(())
}

fn literal_service(service: &str) -> bool {
    service
        .strip_suffix(".amazonaws.com")
        .or_else(|| service.strip_suffix(".amazonaws.com.cn"))
        .is_some_and(|name| {
            !name.is_empty()
                && name.split('.').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                })
        })
}
