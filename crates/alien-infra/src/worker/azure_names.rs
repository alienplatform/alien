const DAPR_COMPONENT_NAME_MAX_LEN: usize = 60;

/// Returns a valid Azure Container Apps Dapr component name.
///
/// Canonical names are preserved. Any normalization or shortening appends a
/// deterministic hash of the original input so distinct resource IDs cannot
/// collapse to the same component name through normalization.
pub(super) fn get_azure_dapr_component_name(raw: &str) -> String {
    let mut normalized = String::with_capacity(raw.len());
    let mut previous_was_hyphen = false;

    for character in raw.chars() {
        let character = if character.is_ascii_alphanumeric() {
            character.to_ascii_lowercase()
        } else if character == '.' {
            character
        } else {
            '-'
        };

        if character == '-' && previous_was_hyphen {
            continue;
        }
        normalized.push(character);
        previous_was_hyphen = character == '-';
    }

    while normalized
        .chars()
        .last()
        .is_some_and(|character| !character.is_ascii_alphanumeric())
    {
        normalized.pop();
    }
    if normalized.is_empty() {
        normalized.push_str("dapr");
    } else if !normalized
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
    {
        normalized.insert_str(0, "dapr-");
    }

    if normalized == raw && normalized.len() <= DAPR_COMPONENT_NAME_MAX_LEN {
        return normalized;
    }

    append_raw_input_hash(&normalized, raw)
}

pub(super) fn get_azure_internal_commands_dapr_component_name(container_app_name: &str) -> String {
    get_azure_dapr_component_name(&format!(
        "servicebus-{container_app_name}-internal-commands"
    ))
}

pub(super) fn get_azure_queue_trigger_dapr_component_name(
    container_app_name: &str,
    queue_id: &str,
) -> String {
    get_azure_dapr_component_name(&format!(
        "servicebus-{container_app_name}-queue-trigger-{queue_id}"
    ))
}

pub(super) fn get_azure_storage_event_subscription_name(
    worker_id: &str,
    storage_id: &str,
) -> String {
    let mut stem: String = format!("alien{worker_id}{storage_id}")
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    stem.truncate(31);
    let suffix = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("azure-storage-trigger:{worker_id}:{storage_id}").as_bytes(),
    )
    .simple()
    .to_string();
    format!("{stem}{suffix}")
}

fn append_raw_input_hash(normalized: &str, raw: &str) -> String {
    let hash = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, raw.as_bytes())
        .simple()
        .to_string();
    let max_stem_len = DAPR_COMPONENT_NAME_MAX_LEN - 1 - hash.len();
    let mut stem: String = normalized.chars().take(max_stem_len).collect();
    while stem
        .chars()
        .last()
        .is_some_and(|character| !character.is_ascii_alphanumeric())
    {
        stem.pop();
    }
    format!("{stem}-{hash}")
}

#[cfg(test)]
mod tests {
    use super::{
        get_azure_dapr_component_name, get_azure_internal_commands_dapr_component_name,
        get_azure_queue_trigger_dapr_component_name, get_azure_storage_event_subscription_name,
        DAPR_COMPONENT_NAME_MAX_LEN,
    };

    #[test]
    fn dapr_component_name_preserves_canonical_names_within_limit() {
        let name = "servicebus-worker-commands";
        let max_length_name = format!("a{}", "1".repeat(DAPR_COMPONENT_NAME_MAX_LEN - 1));

        assert_eq!(get_azure_dapr_component_name(name), name);
        assert_eq!(
            get_azure_dapr_component_name(&max_length_name),
            max_length_name
        );
    }

    #[test]
    fn dapr_component_name_is_valid_stable_and_distinct_after_shortening() {
        let first_raw = "servicebus-e2e-03-azure-terraform-provider-very-long-worker-name-commands";
        let second_raw = "servicebus-e2e-03-azure-terraform-provider-very-long-worker-name-events";
        let first = get_azure_dapr_component_name(first_raw);

        assert_eq!(first, get_azure_dapr_component_name(first_raw));
        assert_ne!(first, get_azure_dapr_component_name(second_raw));
        assert_valid_dapr_component_name(&first);
    }

    #[test]
    fn dapr_component_name_does_not_collapse_distinct_normalized_inputs() {
        for (first, second) in [
            ("servicebus-worker-foo_bar", "servicebus-worker-foo-bar"),
            ("servicebus-worker-Foo", "servicebus-worker-foo"),
            ("servicebus-worker-foo--bar", "servicebus-worker-foo-bar"),
        ] {
            let first_name = get_azure_dapr_component_name(first);
            let second_name = get_azure_dapr_component_name(second);

            assert_ne!(first_name, second_name, "{first:?} and {second:?} collided");
            assert_valid_dapr_component_name(&first_name);
            assert_valid_dapr_component_name(&second_name);
        }
    }

    #[test]
    fn dapr_component_name_hashes_other_normalization_changes() {
        for raw in ["___", "1-worker", "servicebus-worker-queue_"] {
            let name = get_azure_dapr_component_name(raw);

            assert_ne!(name, raw);
            assert_valid_dapr_component_name(&name);
        }
    }

    #[test]
    fn commands_and_commands_queue_trigger_have_distinct_component_names() {
        let internal_commands = get_azure_internal_commands_dapr_component_name("worker");
        let commands_queue = get_azure_queue_trigger_dapr_component_name("worker", "commands");
        assert_eq!(internal_commands, "servicebus-worker-internal-commands");
        assert_eq!(commands_queue, "servicebus-worker-queue-trigger-commands");

        for container_app_name in [
            "worker",
            "e2e-03-azure-terraform-provider-very-long-worker-name",
        ] {
            let internal_commands =
                get_azure_internal_commands_dapr_component_name(container_app_name);
            let commands_queue =
                get_azure_queue_trigger_dapr_component_name(container_app_name, "commands");

            assert_ne!(internal_commands, commands_queue);
            assert_valid_dapr_component_name(&internal_commands);
            assert_valid_dapr_component_name(&commands_queue);
        }
    }

    #[test]
    fn storage_event_subscription_name_is_stable_and_within_limit() {
        let first = get_azure_storage_event_subscription_name(
            "worker-with-a-very-long-name-that-needs-truncating",
            "storage-with-a-very-long-name-that-needs-truncating",
        );
        let second = get_azure_storage_event_subscription_name(
            "worker-with-a-very-long-name-that-needs-truncating",
            "storage-with-a-very-long-name-that-needs-truncating",
        );

        assert_eq!(first, second);
        assert!(first.len() <= 64);
        assert!(first
            .chars()
            .all(|character| character.is_ascii_alphanumeric()));
    }

    fn assert_valid_dapr_component_name(name: &str) {
        assert!(name.len() <= DAPR_COMPONENT_NAME_MAX_LEN);
        assert!(name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic()));
        assert!(name
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_alphanumeric()));
        assert!(name.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '.')
        }));
        assert!(!name.contains("--"));
    }
}
