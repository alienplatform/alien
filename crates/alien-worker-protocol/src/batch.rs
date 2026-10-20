//! Validation shared by batch transports and application hosts.

use std::collections::{HashMap, HashSet};

use crate::control::{queue_delivery_outcome::Disposition, QueueBatch, QueueBatchResult};

/// Validate every outcome before making any acknowledgment decision.
/// Returns dispositions in delivery order, regardless of result order.
/// Invalid input or results cause the entire batch to remain unacknowledged.
pub fn validate_outcomes(
    batch: &QueueBatch,
    result: &QueueBatchResult,
) -> Option<Vec<Disposition>> {
    if batch.deliveries.is_empty() || batch.deliveries.len() != result.outcomes.len() {
        return None;
    }
    let mut identities = HashSet::with_capacity(batch.deliveries.len());
    for delivery in &batch.deliveries {
        if delivery.delivery_id.is_empty()
            || delivery.message.is_none()
            || !identities.insert(delivery.delivery_id.as_str())
        {
            return None;
        }
    }
    let mut outcomes = HashMap::with_capacity(result.outcomes.len());
    for outcome in &result.outcomes {
        let disposition = Disposition::try_from(outcome.disposition).ok()?;
        if disposition == Disposition::Unspecified
            || !identities.contains(outcome.delivery_id.as_str())
            || outcomes
                .insert(outcome.delivery_id.as_str(), disposition)
                .is_some()
        {
            return None;
        }
    }
    batch
        .deliveries
        .iter()
        .map(|delivery| outcomes.get(delivery.delivery_id.as_str()).copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{QueueDelivery, QueueDeliveryOutcome, QueueMessage};

    fn batch() -> QueueBatch {
        QueueBatch {
            source: "jobs".into(),
            deliveries: ["attempt-a", "attempt-b"]
                .into_iter()
                .map(|id| QueueDelivery {
                    delivery_id: id.into(),
                    message: Some(QueueMessage {
                        id: "same-provider-id".into(),
                        ..Default::default()
                    }),
                })
                .collect(),
        }
    }

    fn result() -> QueueBatchResult {
        QueueBatchResult {
            outcomes: vec![
                QueueDeliveryOutcome {
                    delivery_id: "attempt-b".into(),
                    disposition: Disposition::Retry as i32,
                },
                QueueDeliveryOutcome {
                    delivery_id: "attempt-a".into(),
                    disposition: Disposition::Success as i32,
                },
            ],
        }
    }

    #[test]
    fn correlates_attempts_independently_of_provider_ids_and_result_order() {
        assert_eq!(
            validate_outcomes(&batch(), &result()),
            Some(vec![Disposition::Success, Disposition::Retry])
        );
    }

    #[test]
    fn rejects_missing_duplicate_unknown_and_unspecified_outcomes() {
        let mut missing = result();
        missing.outcomes.pop();
        assert_eq!(validate_outcomes(&batch(), &missing), None);
        let mut duplicate = result();
        duplicate.outcomes[0].delivery_id = "attempt-a".into();
        assert_eq!(validate_outcomes(&batch(), &duplicate), None);
        let mut unknown = result();
        unknown.outcomes[0].delivery_id = "attempt-c".into();
        assert_eq!(validate_outcomes(&batch(), &unknown), None);
        for invalid in [0, 99] {
            let mut unspecified = result();
            unspecified.outcomes[0].disposition = invalid;
            assert_eq!(validate_outcomes(&batch(), &unspecified), None);
        }
        let mut ambiguous = batch();
        ambiguous.deliveries[0].delivery_id = "attempt-b".into();
        assert_eq!(validate_outcomes(&ambiguous, &result()), None);
    }
}
