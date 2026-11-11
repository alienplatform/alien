//! Bedrock inference quotas that the account has set far below the cloud's default.
//!
//! A new AWS account can have an applied quota of 0 or a few requests a minute for a model it is
//! allowed to call, so "the model answers" does not mean "the app can use it". This reads the
//! applied and default quotas (read-only Service Quotas calls with the management identity) and
//! reports the ones under 10% of the default. It never blocks anything.

use std::collections::HashMap;

use alien_aws_clients::service_quotas::{ServiceQuota, ServiceQuotasApi};
use alien_core::AiQuotaWarning;
use alien_error::Context;

use crate::error::{ErrorData, Result};

const SERVICE_CODE: &str = "bedrock";
/// A quota under this share of the default is reported.
const LOW_SHARE: f64 = 0.10;

/// All pages of one quota list for Bedrock.
async fn all_quotas(client: &dyn ServiceQuotasApi, defaults: bool) -> Result<Vec<ServiceQuota>> {
    let mut quotas = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let page = if defaults {
            client
                .list_aws_default_service_quotas(SERVICE_CODE, token.clone())
                .await
        } else {
            client
                .list_service_quotas(SERVICE_CODE, token.clone())
                .await
        }
        .context(ErrorData::CloudPlatformError {
            message: "Could not read Bedrock service quotas".to_string(),
            resource_id: None,
        })?;
        quotas.extend(page.quotas);
        token = page.next_token;
        if token.is_none() {
            return Ok(quotas);
        }
    }
}

/// Low inference quotas for each public model id in `models`, keyed by public id.
pub(crate) async fn low_quotas_for(
    client: &dyn ServiceQuotasApi,
    models: &[&str],
) -> Result<HashMap<String, Vec<AiQuotaWarning>>> {
    if models.is_empty() {
        return Ok(HashMap::new());
    }
    let applied = all_quotas(client, false).await?;
    let defaults = all_quotas(client, true).await?;
    Ok(models
        .iter()
        .map(|model| ((*model).to_string(), low_quotas(&applied, &defaults, model)))
        .filter(|(_, warnings)| !warnings.is_empty())
        .collect())
}

/// `claude-sonnet-4-6` -> `Sonnet 4.6`, the name AWS uses in quota names. `None` for ids that
/// do not look like `claude-<family>-<version…>`: those are never matched, never guessed.
fn aws_quota_label(public_id: &str) -> Option<String> {
    let rest = public_id.strip_prefix("claude-")?;
    let mut parts = rest.split('-');
    let family = parts.next()?;
    let version: Vec<&str> = parts.collect();
    if version.is_empty()
        || !version
            .iter()
            .all(|part| part.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let mut family = family.to_string();
    family[..1].make_ascii_uppercase();
    Some(format!("{family} {}", version.join(".")))
}

fn is_inference_rate_quota(name: &str, label: &str) -> bool {
    name.contains("model inference ")
        && (name.contains(" requests per minute for Anthropic Claude ")
            || name.contains(" tokens per minute for Anthropic Claude "))
        && name.ends_with(&format!("Anthropic Claude {label}"))
}

pub(crate) fn low_quotas(
    applied: &[ServiceQuota],
    defaults: &[ServiceQuota],
    public_id: &str,
) -> Vec<AiQuotaWarning> {
    let Some(label) = aws_quota_label(public_id) else {
        return Vec::new();
    };
    let default_by_code: HashMap<&str, f64> = defaults
        .iter()
        .filter_map(|quota| Some((quota.quota_code.as_deref()?, quota.value?)))
        .collect();
    let mut warnings: Vec<AiQuotaWarning> = applied
        .iter()
        .filter_map(|quota| {
            let name = quota.quota_name.as_deref()?;
            if !is_inference_rate_quota(name, &label) {
                return None;
            }
            let applied = quota.value?;
            let default = *default_by_code.get(quota.quota_code.as_deref()?)?;
            (default > 0.0 && applied < default * LOW_SHARE).then(|| AiQuotaWarning {
                quota_name: name.to_string(),
                applied,
                default,
            })
        })
        .collect();
    warnings.sort_by(|a, b| a.quota_name.cmp(&b.quota_name));
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quota(code: &str, name: &str, value: f64) -> ServiceQuota {
        ServiceQuota {
            quota_code: Some(code.into()),
            service_code: Some("bedrock".into()),
            quota_name: Some(name.into()),
            value: Some(value),
        }
    }

    // Names and values are the ones the real account returned for us-west-2.
    fn account() -> (Vec<ServiceQuota>, Vec<ServiceQuota>) {
        let rpm =
            "Cross-region model inference requests per minute for Anthropic Claude Sonnet 4.6";
        let tpm = "Cross-region model inference tokens per minute for Anthropic Claude Sonnet 4.6";
        let five = "Cross-region model inference tokens per minute for Anthropic Claude Sonnet 5";
        let batch = "Records per batch inference job for Claude Sonnet 4.6";
        (
            vec![
                quota("L-00FF3314", rpm, 10.0),
                quota("L-15B8E632", tpm, 6_000_000.0),
                quota("L-D4FBCF4E", five, 0.0),
                quota("L-01A2DC4B", batch, 1.0),
            ],
            vec![
                quota("L-00FF3314", rpm, 10_000.0),
                quota("L-15B8E632", tpm, 6_000_000.0),
                quota("L-D4FBCF4E", five, 6_000_000.0),
                quota("L-01A2DC4B", batch, 100_000.0),
            ],
        )
    }

    #[test]
    fn requests_per_minute_at_a_thousandth_of_default_is_reported() {
        let (applied, defaults) = account();
        let warnings = low_quotas(&applied, &defaults, "claude-sonnet-4-6");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(warnings[0].applied, 10.0);
        assert_eq!(warnings[0].default, 10_000.0);
    }

    #[test]
    fn a_zero_quota_is_reported_and_other_models_are_untouched() {
        let (applied, defaults) = account();
        let warnings = low_quotas(&applied, &defaults, "claude-sonnet-5");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].applied, 0.0);
        assert!(low_quotas(&applied, &defaults, "claude-haiku-4-5").is_empty());
    }

    #[test]
    fn ids_that_are_not_claude_family_versions_are_not_matched() {
        let (applied, defaults) = account();
        assert!(low_quotas(&applied, &defaults, "gpt-5").is_empty());
        assert!(low_quotas(&applied, &defaults, "claude-sonnet-4-6-extra").is_empty());
    }

    #[test]
    fn a_quota_at_the_default_is_not_reported() {
        let (_, defaults) = account();
        assert!(low_quotas(&defaults, &defaults, "claude-sonnet-4-6").is_empty());
    }
}
