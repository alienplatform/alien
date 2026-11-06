//! Strict validation of durable metric-retention loss receipts.
use alien_error::AlienError;
use reqwest::{header::HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetentionLoss {
    pub loss_receipt: String,
    pub cutoff_nanos: u64,
    pub rejected_count: u64,
    pub fresh_accepted: bool,
    pub accepted_receipt: Option<String>,
}
impl RetentionLoss {
    /// A partial success is handled only when its exact rejected count matches
    /// the durable loss receipt. Every other rejection remains an export failure.
    pub fn from_response(
        status: StatusCode,
        headers: &HeaderMap,
        partial_count: Option<i64>,
    ) -> Option<Self> {
        let text = |name: &str| headers.get(name)?.to_str().ok();
        if text("x-otlp-rejection-reason")? != "retention-expired" {
            return None;
        }
        let loss_receipt = text("x-otlp-loss-receipt")?;
        if loss_receipt.len() != 64 || !loss_receipt.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let cutoff_nanos = text("x-otlp-retention-cutoff-nanos")?.parse::<u64>().ok()?;
        let rejected_count = text("x-otlp-rejected-count")?.parse::<u64>().ok()?;
        if cutoff_nanos == 0 || rejected_count == 0 || rejected_count > i64::MAX as u64 {
            return None;
        }
        let fresh_accepted = status == StatusCode::OK
            && partial_count.and_then(|n| u64::try_from(n).ok()) == Some(rejected_count);
        let accepted_receipt = if fresh_accepted {
            let receipt = text("x-otlp-accepted-receipt")?;
            if receipt.is_empty() || receipt.len() > 256 {
                return None;
            }
            Some(receipt.to_owned())
        } else {
            None
        };
        if !fresh_accepted && status != StatusCode::GONE {
            return None;
        }
        Some(Self {
            loss_receipt: loss_receipt.to_ascii_lowercase(),
            cutoff_nanos,
            rejected_count,
            fresh_accepted,
            accepted_receipt,
        })
    }
    pub fn into_error(self) -> AlienError {
        AlienError::new(crate::ErrorData::OtlpRetentionLoss {
            loss_receipt: self.loss_receipt,
            cutoff_nanos: self.cutoff_nanos,
            rejected_count: self.rejected_count,
            fresh_accepted: self.fresh_accepted,
            accepted_receipt: self.accepted_receipt,
        })
        .into_generic()
    }
    pub fn from_error(error: &AlienError) -> Option<Self> {
        if error.code != "OTLP_RETENTION_LOSS" {
            return None;
        }
        let value: Self = serde_json::from_value(error.context.clone()?).ok()?;
        if value.loss_receipt.len() != 64
            || !value.loss_receipt.bytes().all(|b| b.is_ascii_hexdigit())
            || value.cutoff_nanos == 0
            || value.rejected_count == 0
            || value.rejected_count > i64::MAX as u64
            || (value.fresh_accepted
                && value.accepted_receipt.as_ref().is_none_or(|r| {
                    r.is_empty()
                        || r.len() > 256
                        || reqwest::header::HeaderValue::from_str(r).is_err()
                }))
        {
            return None;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_durable_retention_outcomes_are_handled() {
        let mut headers = HeaderMap::new();
        for (key, value) in [
            ("x-otlp-rejection-reason", "retention-expired".to_owned()),
            ("x-otlp-loss-receipt", "a".repeat(64)),
            ("x-otlp-retention-cutoff-nanos", "123".to_owned()),
            ("x-otlp-rejected-count", "2".to_owned()),
        ] {
            headers.insert(key, value.parse().unwrap());
        }
        assert!(RetentionLoss::from_response(StatusCode::OK, &headers, Some(2)).is_none());
        headers.insert("x-otlp-accepted-receipt", "durable-fresh".parse().unwrap());
        assert!(RetentionLoss::from_response(StatusCode::OK, &headers, Some(1)).is_none());
        assert!(
            RetentionLoss::from_response(StatusCode::SERVICE_UNAVAILABLE, &headers, Some(2))
                .is_none()
        );
        let loss = RetentionLoss::from_response(StatusCode::OK, &headers, Some(2)).unwrap();
        assert!(loss.fresh_accepted);
        assert!(
            RetentionLoss::from_error(&loss.into_error())
                .unwrap()
                .fresh_accepted
        );
        assert!(
            !RetentionLoss::from_response(StatusCode::GONE, &headers, None)
                .unwrap()
                .fresh_accepted
        );
        headers.insert("x-otlp-loss-receipt", "invalid".parse().unwrap());
        assert!(RetentionLoss::from_response(StatusCode::GONE, &headers, None).is_none());
    }
}
