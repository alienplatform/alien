//! Shared notification translation for setup generators and live controllers.

use crate::{error::ErrorData, Queue, ResourceRef, Result, Stack, Storage, StorageNotification};
use alien_error::{AlienError, Context, IntoAlienError};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Custom GCS notification attributes `storageEventSuffixes0`, `storageEventSuffixes1`, …
/// whose values, concatenated in order, form a JSON array of the config's literal suffixes.
pub const GCS_SUFFIXES_ATTRIBUTE: &str = "storageEventSuffixes";

/// Stable config identity for additive GCS notification reconciliation.
pub const GCS_RULE_ATTRIBUTE: &str = "storageNotificationRule";

/// GCS accepts at most 5 custom attributes per notification config; one is the rule.
const GCS_MAX_SUFFIX_ATTRIBUTES: usize = 4;

/// GCS rejects custom attribute values longer than 1024 characters, and Pub/Sub attribute
/// values are limited to 1024 bytes. Chunks are cut at character boundaries by bytes.
const GCS_ATTRIBUTE_VALUE_BYTES: usize = 1024;

fn short_hash(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())[..16].to_string()
}

/// Stable identity of what a rule delivers (queue, prefix, suffix), independent of its
/// position, so adding or removing another rule never replaces this one's subscription.
pub fn notification_rule_key(rule: &StorageNotification) -> String {
    short_hash(&[
        "rule",
        &rule.queue.id,
        rule.prefix.as_deref().unwrap_or_default(),
        rule.suffix.as_deref().unwrap_or_default(),
    ])
}

/// One GCS notification config: every rule of a Storage with the same queue and prefix.
/// GCS filters by prefix only, so one config per rule would publish each object under a
/// shared prefix once per rule. The consumer checks the suffixes instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcsNotificationConfig<'a> {
    /// Stable identity of the group (queue and prefix), independent of rule positions and
    /// suffixes: a changed suffix set replaces only this group's config.
    pub key: String,
    /// Queue the config publishes to.
    pub queue: &'a ResourceRef,
    /// `object_name_prefix`, if any.
    pub prefix: Option<&'a str>,
    /// Suffix attributes; empty when a rule of the group has no suffix and so matches
    /// every object under the prefix (rules can't overlap, so it is then the only rule).
    pub suffix_attributes: Vec<(String, String)>,
}

/// GCS notification configs for a Storage's rules, in order of first appearance.
pub fn gcs_notification_configs(storage: &Storage) -> Result<Vec<GcsNotificationConfig<'_>>> {
    let mut groups: Vec<(&ResourceRef, Option<&str>, Vec<&str>, bool)> = Vec::new();
    for rule in &storage.notifications {
        let prefix = rule.prefix.as_deref();
        let index = match groups
            .iter()
            .position(|(queue, other, _, _)| queue.id == rule.queue.id && *other == prefix)
        {
            Some(index) => index,
            None => {
                groups.push((&rule.queue, prefix, Vec::new(), false));
                groups.len() - 1
            }
        };
        match rule.suffix.as_deref() {
            Some(suffix) => groups[index].2.push(suffix),
            None => groups[index].3 = true,
        }
    }
    groups
        .into_iter()
        .map(|(queue, prefix, mut suffixes, unfiltered)| {
            // Sorted, so reordering rules leaves the group's config unchanged.
            suffixes.sort_unstable();
            let suffix_attributes = if unfiltered {
                Vec::new()
            } else {
                gcs_suffix_attributes(&suffixes).context(ErrorData::GenericError {
                    message: format!(
                        "Storage '{}' notification suffixes for queue '{}' do not fit in GCS attributes",
                        storage.id, queue.id
                    ),
                })?
            };
            Ok(GcsNotificationConfig {
                key: short_hash(&["gcs", &queue.id, prefix.unwrap_or_default()]),
                queue,
                prefix,
                suffix_attributes,
            })
        })
        .collect()
}

fn gcs_suffix_attributes(suffixes: &[&str]) -> Result<Vec<(String, String)>> {
    let encoded =
        serde_json::to_string(suffixes)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: "Encode notification suffixes".to_string(),
            })?;
    let mut chunks = Vec::new();
    let mut rest = encoded.as_str();
    while !rest.is_empty() {
        let mut end = rest.len().min(GCS_ATTRIBUTE_VALUE_BYTES);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(&rest[..end]);
        rest = &rest[end..];
    }
    if chunks.len() > GCS_MAX_SUFFIX_ATTRIBUTES {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "{} encoded bytes need {} attributes; GCS allows {GCS_MAX_SUFFIX_ATTRIBUTES}",
                encoded.len(),
                chunks.len()
            ),
        }));
    }
    Ok(chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            (
                format!("{GCS_SUFFIXES_ATTRIBUTE}{index}"),
                chunk.to_string(),
            )
        })
        .collect())
}

/// The literal suffixes a GCS notification's attributes carry, or `None` when they carry
/// none (every object under the config's prefix matches).
pub fn gcs_suffixes(attributes: &HashMap<String, String>) -> Result<Option<Vec<String>>> {
    let mut encoded = String::new();
    let mut index = 0;
    while let Some(chunk) = attributes.get(&format!("{GCS_SUFFIXES_ATTRIBUTE}{index}")) {
        encoded.push_str(chunk);
        index += 1;
    }
    if index == 0 {
        return Ok(None);
    }
    serde_json::from_str(&encoded)
        .map(Some)
        .into_alien_error()
        .context(ErrorData::GenericError {
            message: "Storage notification suffix attributes are not a JSON string array"
                .to_string(),
        })
}

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
        rule.matches_key(key)
    }

    /// Rules overlap exactly when they share a key that S3 would have to route twice.
    /// Keys are long enough (6 characters vs filters of at most 3) that every
    /// overlapping pair has a witness key.
    #[test]
    fn overlap_equals_existence_of_a_key_matching_both_rules() {
        let keys = keys();
        let filters = filters();
        for (ap, asf) in filters
            .iter()
            .flat_map(|p| filters.iter().map(move |s| (p, s)))
        {
            let a = notification(ap.as_deref(), asf.as_deref());
            for (bp, bsf) in filters
                .iter()
                .flat_map(|p| filters.iter().map(move |s| (p, s)))
            {
                let b = notification(bp.as_deref(), bsf.as_deref());
                let witnessed = keys.iter().any(|key| matches(&a, key) && matches(&b, key));
                assert_eq!(notifications_overlap(&a, &b), witnessed, "a={a:?} b={b:?}");
            }
        }
    }

    fn storage(rules: Vec<StorageNotification>) -> Storage {
        Storage::new("uploads".into()).notifications(rules).build()
    }

    /// A GCS config as the consumer sees it: queue, prefix and decoded suffixes.
    type Decoded<'a> = (&'a str, Option<&'a str>, Option<Vec<String>>);

    /// Queues of the configs that deliver `key`: GCS filters by prefix, the consumer by the
    /// decoded suffixes.
    fn delivered_to<'a>(configs: &[Decoded<'a>], key: &str) -> Vec<&'a str> {
        configs
            .iter()
            .filter(|(_, prefix, suffixes)| {
                prefix.is_none_or(|prefix| key.starts_with(prefix))
                    && suffixes
                        .as_ref()
                        .is_none_or(|suffixes| suffixes.iter().any(|suffix| key.ends_with(suffix)))
            })
            .map(|(queue, _, _)| *queue)
            .collect()
    }

    fn decoded(storage: &Storage) -> Vec<Decoded<'_>> {
        gcs_notification_configs(storage)
            .unwrap()
            .into_iter()
            .map(|config| {
                let attributes: HashMap<_, _> = config.suffix_attributes.into_iter().collect();
                (
                    config.queue.id.as_str(),
                    config.prefix,
                    gcs_suffixes(&attributes).unwrap(),
                )
            })
            .collect()
    }

    /// For every pair of non-overlapping rules: rules with one queue and prefix share a
    /// config, and every key is delivered exactly once if a rule matches it, else never.
    #[test]
    fn grouped_gcs_configs_deliver_exactly_the_keys_each_rule_matches() {
        let filters: Vec<_> = filters()
            .into_iter()
            .filter(|filter| {
                filter
                    .as_ref()
                    .is_none_or(|value| value.chars().count() <= 2)
            })
            .collect();
        let keys: Vec<_> = keys()
            .into_iter()
            .filter(|key| key.chars().count() <= 4)
            .collect();
        let rules: Vec<_> = filters
            .iter()
            .flat_map(|prefix| {
                filters
                    .iter()
                    .map(move |suffix| notification(prefix.as_deref(), suffix.as_deref()))
            })
            .collect();
        for (index, a) in rules.iter().enumerate() {
            for b in &rules[index + 1..] {
                if notifications_overlap(a, b) {
                    continue;
                }
                let storage = storage(vec![a.clone(), b.clone()]);
                let configs = decoded(&storage);
                assert_eq!(configs.len(), if a.prefix == b.prefix { 1 } else { 2 });
                for key in &keys {
                    let expected = [a, b].iter().filter(|rule| matches(rule, key)).count();
                    assert_eq!(
                        delivered_to(&configs, key).len(),
                        expected,
                        "a={a:?} b={b:?} key={key:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn gcs_configs_split_by_queue_and_keep_unfiltered_rules_unfiltered() {
        let rule = |queue: &str, prefix: &str, suffix: Option<&str>| {
            StorageNotification::builder()
                .queue(ResourceRef::new(Queue::RESOURCE_TYPE, queue))
                .prefix(prefix.into())
                .maybe_suffix(suffix.map(str::to_string))
                .build()
        };
        let storage = storage(vec![
            rule("work", "logs/", Some(".json")),
            rule("audit", "logs/", Some(".csv")),
            rule("work", "logs/", Some(".gz")),
            rule("work", "raw/", None),
        ]);
        let configs = decoded(&storage);
        assert_eq!(
            configs,
            [
                (
                    "work",
                    Some("logs/"),
                    Some(vec![".gz".into(), ".json".into()])
                ),
                ("audit", Some("logs/"), Some(vec![".csv".into()])),
                ("work", Some("raw/"), None),
            ]
        );
        assert_eq!(delivered_to(&configs, "logs/a.json"), ["work"]);
        assert_eq!(delivered_to(&configs, "logs/a.csv"), ["audit"]);
        assert!(delivered_to(&configs, "logs/a.txt").is_empty());
        assert_eq!(delivered_to(&configs, "raw/anything"), ["work"]);
    }

    /// The largest suffix set the portable limits allow (10 rules, 128-byte suffixes) must
    /// fit the GCS limits measured on real GCS: at most 5 custom attributes per config
    /// (one is the rule) and values of at most 1024 characters.
    #[test]
    fn largest_allowed_suffix_set_fits_gcs_attribute_limits_and_round_trips() {
        let suffixes: Vec<String> = (0..10)
            .map(|index| {
                let mut suffix = index.to_string();
                for character in ['"', '\\', '*', 'é'].iter().cycle() {
                    if suffix.len() + character.len_utf8() > 128 {
                        break;
                    }
                    suffix.push(*character);
                }
                suffix
            })
            .collect();
        assert!(suffixes.iter().all(|suffix| suffix.len() >= 127));
        let borrowed: Vec<&str> = suffixes.iter().map(String::as_str).collect();
        let attributes = gcs_suffix_attributes(&borrowed).unwrap();
        assert!(attributes.len() <= GCS_MAX_SUFFIX_ATTRIBUTES);
        assert!(
            attributes.len() > 1,
            "the worst case must exercise chunking"
        );
        for (_, value) in &attributes {
            assert!(value.len() <= GCS_ATTRIBUTE_VALUE_BYTES);
        }
        let attributes: HashMap<_, _> = attributes.into_iter().collect();
        assert_eq!(gcs_suffixes(&attributes).unwrap(), Some(suffixes));
        assert_eq!(gcs_suffixes(&HashMap::new()).unwrap(), None);
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
