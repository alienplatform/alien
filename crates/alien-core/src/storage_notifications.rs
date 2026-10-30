//! Shared notification translation for setup generators and live controllers.

use crate::{Queue, Stack, Storage, StorageNotification};
use sha2::{Digest, Sha256};

/// Custom GCS notification attribute carrying the literal consumer-side suffix.
pub const GCS_SUFFIX_ATTRIBUTE: &str = "storageEventSuffix";

/// Stable rule identity for additive GCS notification reconciliation.
pub const GCS_RULE_ATTRIBUTE: &str = "storageNotificationRule";

/// Whether this queue owns an ingress grant for storage notifications in its stack.
pub fn queue_receives_storage_notifications(stack: &Stack, queue_id: &str) -> bool {
    stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Storage>()
            .is_some_and(|storage| {
                storage.notifications.iter().any(|notification| {
                    notification.queue.resource_type == Queue::RESOURCE_TYPE
                        && notification.queue.id == queue_id
                })
            })
    })
}

/// Ids of the Storages in the stack that notify this queue, sorted for deterministic output.
pub fn storages_notifying_queue(stack: &Stack, queue_id: &str) -> Vec<String> {
    let mut storages: Vec<String> = stack
        .resources()
        .filter(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Storage>()
                .is_some_and(|storage| {
                    storage.notifications.iter().any(|notification| {
                        notification.queue.resource_type == Queue::RESOURCE_TYPE
                            && notification.queue.id == queue_id
                    })
                })
        })
        .map(|(id, _)| id.clone())
        .collect();
    storages.sort();
    storages
}

/// Whether one object key can match both rules. Rules on one Storage must not overlap:
/// S3 rejects such a configuration ("Configuration is ambiguously defined"), so the
/// portable declaration rejects it everywhere. Prefixes overlap when either is absent or
/// one starts with the other; suffixes when either is absent or one ends with the other.
pub fn notifications_overlap(a: &StorageNotification, b: &StorageNotification) -> bool {
    let prefixes = match (a.prefix.as_deref(), b.prefix.as_deref()) {
        (Some(a), Some(b)) => a.starts_with(b) || b.starts_with(a),
        _ => true,
    };
    let suffixes = match (a.suffix.as_deref(), b.suffix.as_deref()) {
        (Some(a), Some(b)) => a.ends_with(b) || b.ends_with(a),
        _ => true,
    };
    prefixes && suffixes
}

/// Id of a rule's S3 queue configuration. The stack prefix and a hash of the Storage's
/// identity let the owning controller recognise its own entries among everyone else's.
pub fn aws_notification_id(prefix: &str, storage_id: &str, index: usize) -> String {
    format!("{}{index}", aws_notification_id_prefix(prefix, storage_id))
}

/// Every [`aws_notification_id`] of one Storage starts with this.
pub fn aws_notification_id_prefix(prefix: &str, storage_id: &str) -> String {
    let hash = format!("{:x}", Sha256::digest(storage_id.as_bytes()));
    format!("{prefix}-sn-{}-", &hash[..16])
}

/// `Sid` of the queue policy statement that lets S3 deliver a stack's notifications.
pub const AWS_QUEUE_INGRESS_SID: &str = "StorageNotificationIngress";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Queue, ResourceRef};
    use std::collections::HashSet;

    fn notification(prefix: Option<&str>, suffix: Option<&str>) -> StorageNotification {
        StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "work"))
            .maybe_prefix(prefix.map(str::to_string))
            .maybe_suffix(suffix.map(str::to_string))
            .build()
    }

    fn filters() -> Vec<Option<String>> {
        let mut all = vec![None];
        let mut level = vec![String::new()];
        for _ in 0..3 {
            let next: Vec<_> = level
                .iter()
                .flat_map(|value| ['a', 'b', 'é'].map(|character| format!("{value}{character}")))
                .collect();
            all.extend(next.iter().cloned().map(Some));
            level = next;
        }
        all
    }

    fn keys() -> Vec<String> {
        let mut all = vec![String::new()];
        let mut level = vec![String::new()];
        for _ in 0..6 {
            let next: Vec<_> = level
                .iter()
                .flat_map(|key| ['a', 'b', 'é'].map(|character| format!("{key}{character}")))
                .collect();
            all.extend(next.clone());
            level = next;
        }
        all
    }

    fn matches(rule: &StorageNotification, key: &str) -> bool {
        rule.prefix.as_deref().map_or(true, |prefix| key.starts_with(prefix))
            && rule.suffix.as_deref().map_or(true, |suffix| key.ends_with(suffix))
    }

    /// Rules overlap exactly when they share a key that S3 would have to route twice.
    /// Keys are long enough (6 characters vs filters of at most 3) that every
    /// overlapping pair has a witness key.
    #[test]
    fn overlap_equals_existence_of_a_key_matching_both_rules() {
        let keys = keys();
        let filters = filters();
        for (ap, asf) in filters.iter().flat_map(|p| filters.iter().map(move |s| (p, s))) {
            let a = notification(ap.as_deref(), asf.as_deref());
            for (bp, bsf) in filters.iter().flat_map(|p| filters.iter().map(move |s| (p, s))) {
                let b = notification(bp.as_deref(), bsf.as_deref());
                let witnessed = keys.iter().any(|key| matches(&a, key) && matches(&b, key));
                assert_eq!(
                    notifications_overlap(&a, &b),
                    witnessed,
                    "a={a:?} b={b:?}"
                );
            }
        }
    }

    #[test]
    fn ids_are_distinct_per_storage_and_rule_and_carry_the_storage_prefix() {
        let mut ids = HashSet::new();
        for storage in ["source", "second", "long-resource-identity"] {
            let prefix = aws_notification_id_prefix("stack", storage);
            for index in 0..10 {
                let id = aws_notification_id("stack", storage, index);
                assert!(id.starts_with(&prefix));
                assert!(ids.insert(id));
            }
        }
        assert!(!aws_notification_id("stack", "source", 1)
            .starts_with(&aws_notification_id_prefix("stack", "second")));
    }
}
