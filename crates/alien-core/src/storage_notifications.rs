//! Shared notification translation for setup generators and live controllers.

use crate::{Queue, Stack, Storage, StorageNotification};
use serde_json::{json, Value};

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

/// Maximum serialized event-pattern length accepted by EventBridge.
pub const AWS_EVENTBRIDGE_MAX_PATTERN_CHARACTERS: usize = 4096;

/// The complete rule pattern, shared by setup generators and live controllers.
pub fn aws_eventbridge_pattern(notification: &StorageNotification, bucket_name: &str) -> Value {
    json!({
        "source": ["aws.s3"],
        "detail-type": ["Object Created"],
        "detail": {
            "bucket": { "name": [bucket_name] },
            "object": { "key": aws_eventbridge_key_matchers(notification) }
        }
    })
}

/// EventBridge ORs the entries in a field's matcher list. A wildcard covers keys
/// whose prefix and suffix do not overlap; exact alternatives cover shorter keys.
/// Both filters remain literal, including `*`, backslashes and Unicode.
pub fn aws_eventbridge_key_matchers(notification: &StorageNotification) -> Vec<Value> {
    let prefix = notification.prefix.as_deref().unwrap_or("");
    let suffix = notification.suffix.as_deref().unwrap_or("");
    let mut matchers = vec![json!({
        "wildcard": format!("{}*{}", escape_wildcard(prefix), escape_wildcard(suffix))
    })];

    // Only UTF-8 boundaries can be an overlap. Include the whole suffix as well
    // as proper prefixes, but exclude the empty overlap covered by the wildcard.
    for overlap in suffix
        .char_indices()
        .map(|(offset, _)| offset)
        .skip(1)
        .chain(std::iter::once(suffix.len()))
    {
        if overlap > 0 && prefix.ends_with(&suffix[..overlap]) {
            matchers.push(Value::String(format!("{prefix}{}", &suffix[overlap..])));
        }
    }
    matchers
}

fn escape_wildcard(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '*' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Queue, ResourceRef};
    use regex::Regex;

    fn notification(prefix: &str, suffix: &str) -> StorageNotification {
        StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "work"))
            .maybe_prefix((!prefix.is_empty()).then(|| prefix.to_string()))
            .maybe_suffix((!suffix.is_empty()).then(|| suffix.to_string()))
            .build()
    }

    fn generated_keys(max_length: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut level = vec![String::new()];
        for _ in 0..max_length {
            let next: Vec<_> = level
                .iter()
                .flat_map(|key| ['a', '*', '\\', 'é'].map(|character| format!("{key}{character}")))
                .collect();
            all.extend(next.clone());
            level = next;
        }
        all
    }

    /// Interpret the emitted EventBridge wildcard independently of its construction.
    fn wildcard_regex(wildcard: &str) -> Regex {
        let mut expression = String::from(r"(?s)\A");
        let mut characters = wildcard.chars();
        while let Some(character) = characters.next() {
            match character {
                '\\' => {
                    let escaped = characters
                        .next()
                        .expect("escape must have a following character");
                    assert!(matches!(escaped, '*' | '\\'));
                    expression.push_str(&regex::escape(&escaped.to_string()));
                }
                '*' => expression.push_str(".*"),
                literal => expression.push_str(&regex::escape(&literal.to_string())),
            }
        }
        expression.push_str(r"\z");
        Regex::new(&expression).unwrap()
    }

    #[test]
    fn eventbridge_matchers_equal_literal_filters_for_exhaustive_short_keys() {
        let filters = generated_keys(2);
        let keys = generated_keys(4);
        for prefix in &filters {
            for suffix in &filters {
                let rule = notification(prefix, suffix);
                let matchers = aws_eventbridge_key_matchers(&rule);
                let wildcard = wildcard_regex(matchers[0]["wildcard"].as_str().unwrap());
                for key in &keys {
                    let actual = wildcard.is_match(key)
                        || matchers
                            .iter()
                            .skip(1)
                            .any(|exact| exact.as_str() == Some(key));
                    assert_eq!(
                        actual,
                        key.starts_with(prefix) && key.ends_with(suffix),
                        "prefix={prefix:?}, suffix={suffix:?}, key={key:?}, matchers={matchers:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn preserves_every_overlap_and_unicode_boundary() {
        assert_eq!(
            aws_eventbridge_key_matchers(&notification("aaa", "aaa")),
            vec![
                json!({"wildcard": "aaa*aaa"}),
                json!("aaaaa"),
                json!("aaaa"),
                json!("aaa"),
            ]
        );
        assert_eq!(
            aws_eventbridge_key_matchers(&notification("é*", "*雪")),
            vec![json!({"wildcard": r"é\**\*雪"}), json!("é*雪"),]
        );
        assert_eq!(
            aws_eventbridge_key_matchers(&notification("same", "same")),
            vec![json!({"wildcard": "same*same"}), json!("same"),]
        );
    }

    #[test]
    fn maximum_nonoverlapping_filters_fit_even_with_json_escape_expansion() {
        // Backslashes expand in both wildcard and JSON encoding; quotes expand in JSON.
        // Reserve all 63 characters allowed for an S3 bucket's physical name.
        let rule = notification(&"\\".repeat(512), &"\"".repeat(128));
        let pattern = aws_eventbridge_pattern(&rule, &"b".repeat(63));
        assert_eq!(
            pattern["detail"]["object"]["key"].as_array().unwrap().len(),
            1
        );
        assert!(pattern.to_string().chars().count() <= AWS_EVENTBRIDGE_MAX_PATTERN_CHARACTERS);
    }
}
