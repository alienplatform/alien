use alien_azure_clients::authorization::Scope;

const CONTAINER_APP_NAME_MAX_LEN: usize = 32;
const CONTAINER_APP_NAME_HASH_LEN: usize = 16;
const CONTAINER_APP_IDENTITY_DOMAIN: &str = "alien.azure.container-app.v1";
const DAPR_COMPONENT_NAME_MAX_LEN: usize = 60;
const DAPR_COMPONENT_IDENTITY_DOMAIN: &str = "alien.azure.dapr.component.v1";

/// Returns a valid Azure Container App name for a worker.
///
/// Existing canonical names are preserved. Names that require normalization or
/// shortening retain a readable alphanumeric prefix and append a deterministic
/// hash of the full deployment/worker identity. Transformed names contain no
/// hyphens, while canonical names always contain the separator between the
/// resource prefix and worker ID, so the two namespaces cannot alias.
pub(super) fn get_azure_container_app_name(resource_prefix: &str, worker_id: &str) -> String {
    let raw = format!("{resource_prefix}-{worker_id}");
    let normalized = normalize_azure_container_app_name(&raw);

    if normalized == raw && normalized.len() <= CONTAINER_APP_NAME_MAX_LEN {
        return normalized;
    }

    let hash = stable_identity_hash(CONTAINER_APP_IDENTITY_DOMAIN, &[resource_prefix, worker_id]);
    let max_stem_len = CONTAINER_APP_NAME_MAX_LEN - CONTAINER_APP_NAME_HASH_LEN;
    let stem: String = normalized
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(max_stem_len)
        .collect();

    format!("{stem}{}", &hash[..CONTAINER_APP_NAME_HASH_LEN])
}

fn normalize_azure_container_app_name(raw: &str) -> String {
    let mut normalized = String::with_capacity(raw.len());
    let mut previous_was_hyphen = false;

    for character in raw.chars() {
        let character = if character.is_ascii_alphanumeric() {
            character.to_ascii_lowercase()
        } else {
            '-'
        };

        if character == '-' {
            if normalized.is_empty() || previous_was_hyphen {
                continue;
            }
        }
        normalized.push(character);
        previous_was_hyphen = character == '-';
    }

    while normalized.ends_with('-') {
        normalized.pop();
    }
    if normalized.is_empty() {
        normalized.push_str("app");
    } else if !normalized
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_lowercase())
    {
        normalized.insert_str(0, "app-");
    }

    normalized
}

/// Returns a valid Azure Container Apps Dapr component name.
///
/// Canonical names are preserved. Any normalization or shortening appends a
/// deterministic hash of the original input so distinct resource IDs cannot
/// collapse to the same component name through normalization.
pub(super) fn get_azure_dapr_component_name(raw: &str) -> String {
    let normalized = normalize_azure_dapr_component_name(raw);

    if normalized == raw && normalized.len() <= DAPR_COMPONENT_NAME_MAX_LEN {
        return normalized;
    }

    append_raw_input_hash(&normalized, raw)
}

fn normalize_azure_dapr_component_name(raw: &str) -> String {
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

    normalized
}

pub(super) fn get_azure_internal_commands_dapr_component_name(container_app_name: &str) -> String {
    structured_dapr_component_name(
        &format!("servicebus-{container_app_name}-internal-commands"),
        &["internal-commands", container_app_name],
    )
}

pub(super) fn get_legacy_azure_internal_commands_dapr_component_names(
    container_app_name: &str,
) -> Vec<String> {
    historical_dapr_component_names(&[
        format!("servicebus-{container_app_name}-commands"),
        format!("servicebus-{container_app_name}-internal-commands"),
    ])
}

pub(super) fn get_azure_queue_trigger_dapr_component_name(
    container_app_name: &str,
    queue_id: &str,
) -> String {
    structured_dapr_component_name(
        &format!("servicebus-{container_app_name}-queue-trigger-{queue_id}"),
        &["queue-trigger", container_app_name, queue_id],
    )
}

pub(super) fn get_legacy_azure_queue_trigger_dapr_component_names(
    container_app_name: &str,
    queue_id: &str,
) -> Vec<String> {
    historical_dapr_component_names(&[
        format!("servicebus-{container_app_name}-{queue_id}"),
        format!("servicebus-{container_app_name}-queue-trigger-{queue_id}"),
    ])
}

pub(super) fn get_azure_blob_trigger_dapr_component_name(
    container_app_name: &str,
    storage_id: &str,
) -> String {
    structured_dapr_component_name(
        &format!("blobstorage-{container_app_name}-{storage_id}"),
        &["blob-trigger", container_app_name, storage_id],
    )
}

pub(super) fn get_legacy_azure_blob_trigger_dapr_component_names(
    container_app_name: &str,
    storage_id: &str,
) -> Vec<String> {
    historical_dapr_component_names(&[format!("blobstorage-{container_app_name}-{storage_id}")])
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

pub(super) fn commands_queue_name(container_app_name: &str) -> String {
    format!("{container_app_name}-rq")
}

pub(super) fn storage_trigger_queue_name(container_app_name: &str, storage_id: &str) -> String {
    format!("{container_app_name}-storage-{storage_id}")
}

pub(super) fn service_bus_queue_scope(
    resource_group_name: &str,
    namespace_name: &str,
    queue_name: &str,
) -> Scope {
    Scope::Resource {
        resource_group_name: resource_group_name.to_string(),
        resource_provider: "Microsoft.ServiceBus".to_string(),
        parent_resource_path: Some(format!("namespaces/{namespace_name}")),
        resource_type: "queues".to_string(),
        resource_name: queue_name.to_string(),
    }
}

pub(super) fn commands_sender_role_assignment_name(
    resource_prefix: &str,
    worker_id: &str,
    principal_id: &str,
    namespace_name: &str,
    queue_name: &str,
) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!(
            "deployment:azure:commands-sender:{resource_prefix}:{worker_id}:{principal_id}:{namespace_name}:{queue_name}"
        )
        .as_bytes(),
    )
    .to_string()
}

pub(super) fn storage_trigger_receiver_role_assignment_name(
    resource_prefix: &str,
    worker_id: &str,
    storage_id: &str,
    principal_id: &str,
) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!(
            "deployment:azure:storage-trigger-receiver:{resource_prefix}:{worker_id}:{storage_id}:{principal_id}"
        )
        .as_bytes(),
    )
    .to_string()
}

fn append_raw_input_hash(normalized: &str, raw: &str) -> String {
    let hash = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, raw.as_bytes())
        .simple()
        .to_string();
    append_hash(normalized, &hash)
}

fn historical_dapr_component_names(raw_names: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for raw_name in raw_names {
        for name in [
            get_azure_dapr_component_name(raw_name),
            get_legacy_eight_character_hash_dapr_component_name(raw_name),
        ] {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Reproduces the first bounded-name rollout so partially upgraded stacks can
/// be migrated. That version normalized short names without a hash and used an
/// eight-character UUID suffix only when the 60-character limit was exceeded.
fn get_legacy_eight_character_hash_dapr_component_name(raw: &str) -> String {
    let normalized = normalize_azure_dapr_component_name(raw);
    if normalized.len() <= DAPR_COMPONENT_NAME_MAX_LEN {
        return normalized;
    }

    let hash = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, raw.as_bytes())
        .simple()
        .to_string();
    append_hash(&normalized, &hash[..8])
}

fn structured_dapr_component_name(readable_stem: &str, identity_parts: &[&str]) -> String {
    let normalized = normalize_azure_dapr_component_name(readable_stem);
    let hash = structured_identity_hash(identity_parts);
    append_hash(&normalized, &hash)
}

/// Hash semantic fields independently of their human-readable concatenation.
/// Fixed-width length prefixes make tuple boundaries unambiguous, and the
/// domain version prevents future identity formats from aliasing this one.
fn structured_identity_hash(identity_parts: &[&str]) -> String {
    stable_identity_hash(DAPR_COMPONENT_IDENTITY_DOMAIN, identity_parts)
}

fn stable_identity_hash(domain: &str, identity_parts: &[&str]) -> String {
    let mut identity = Vec::new();
    for part in std::iter::once(domain).chain(identity_parts.iter().copied()) {
        identity.extend_from_slice(&(part.len() as u64).to_be_bytes());
        identity.extend_from_slice(part.as_bytes());
    }
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, &identity)
        .simple()
        .to_string()
}

fn append_hash(normalized: &str, hash: &str) -> String {
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
        commands_queue_name, commands_sender_role_assignment_name,
        get_azure_blob_trigger_dapr_component_name, get_azure_container_app_name,
        get_azure_dapr_component_name, get_azure_internal_commands_dapr_component_name,
        get_azure_queue_trigger_dapr_component_name, get_azure_storage_event_subscription_name,
        get_legacy_azure_blob_trigger_dapr_component_names,
        get_legacy_azure_internal_commands_dapr_component_names,
        get_legacy_azure_queue_trigger_dapr_component_names, service_bus_queue_scope,
        storage_trigger_queue_name, storage_trigger_receiver_role_assignment_name,
        CONTAINER_APP_NAME_MAX_LEN, DAPR_COMPONENT_NAME_MAX_LEN,
    };
    use alien_azure_clients::authorization::Scope;

    #[test]
    fn container_app_name_preserves_existing_canonical_names() {
        assert_eq!(
            get_azure_container_app_name("acme-prod", "worker"),
            "acme-prod-worker"
        );
        let max_length_worker = "w".repeat(CONTAINER_APP_NAME_MAX_LEN - "acme-prod-".len());
        assert_eq!(
            get_azure_container_app_name("acme-prod", &max_length_worker),
            format!("acme-prod-{max_length_worker}")
        );
    }

    #[test]
    fn container_app_name_bounds_current_e2e_identity_stably() {
        let resource_prefix = "e2e-10-azure-terraform-pr-0123456789";
        let worker_id = "test-alien-ts-function";
        let name = get_azure_container_app_name(resource_prefix, worker_id);

        assert_valid_container_app_name(&name);
        assert_eq!(name, "e2e10azureterraf731185acf8be53ed");
    }

    #[test]
    fn container_app_name_normalizes_invalid_characters() {
        let name = get_azure_container_app_name("123_Test.Stack", "Worker_Name_");

        assert_valid_container_app_name(&name);
        assert_ne!(name, "123_Test.Stack-Worker_Name_");
    }

    #[test]
    fn container_app_name_hash_distinguishes_shared_truncated_stems() {
        let prefix = "e2e-10-azure-terraform-pr-0123456789";
        let first = get_azure_container_app_name(prefix, "worker-with-shared-prefix-first");
        let second = get_azure_container_app_name(prefix, "worker-with-shared-prefix-second");

        assert_valid_container_app_name(&first);
        assert_valid_container_app_name(&second);
        assert_ne!(first, second);
    }

    #[test]
    fn transformed_container_app_name_cannot_alias_a_canonical_name() {
        let transformed =
            get_azure_container_app_name("acme", "worker-name-that-is-long-enough-to-hash");
        let canonical = get_azure_container_app_name("acme", "worker-nam-726449ac71e95aa5");
        let old_single_separator_output = "acme-worker-nam-726449ac71e95aa5";

        assert_eq!(canonical, old_single_separator_output);
        assert_eq!(transformed, "acmeworkernameth726449ac71e95aa5");
        assert_ne!(transformed, canonical);
        assert_valid_container_app_name(&transformed);
        assert_valid_container_app_name(&canonical);
    }

    #[test]
    fn auxiliary_resource_identities_match_the_creation_contract() {
        assert_eq!(commands_queue_name("app"), "app-rq");
        assert_eq!(
            storage_trigger_queue_name("app", "assets"),
            "app-storage-assets"
        );

        let commands_name = commands_sender_role_assignment_name(
            "deployment",
            "worker",
            "principal",
            "namespace",
            "app-rq",
        );
        assert_eq!(
            commands_name,
            uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                b"deployment:azure:commands-sender:deployment:worker:principal:namespace:app-rq",
            )
            .to_string()
        );

        let storage_name = storage_trigger_receiver_role_assignment_name(
            "deployment",
            "worker",
            "assets",
            "principal",
        );
        assert_eq!(
            storage_name,
            uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                b"deployment:azure:storage-trigger-receiver:deployment:worker:assets:principal",
            )
            .to_string()
        );

        let Scope::Resource {
            resource_group_name,
            resource_provider,
            parent_resource_path,
            resource_type,
            resource_name,
        } = service_bus_queue_scope("rg", "namespace", "app-rq")
        else {
            panic!("queue scope must be a resource scope");
        };
        assert_eq!(resource_group_name, "rg");
        assert_eq!(resource_provider, "Microsoft.ServiceBus");
        assert_eq!(
            parent_resource_path.as_deref(),
            Some("namespaces/namespace")
        );
        assert_eq!(resource_type, "queues");
        assert_eq!(resource_name, "app-rq");
    }

    fn assert_valid_container_app_name(name: &str) {
        assert!((2..=CONTAINER_APP_NAME_MAX_LEN).contains(&name.len()));
        assert!(name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase()));
        assert!(name
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_alphanumeric()));
        assert!(name.chars().all(|character| character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'));
        assert!(!name.contains("--"));
    }

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
        for container_app_name in [
            "worker",
            "e2e-03-azure-terraform-provider-very-long-worker-name",
        ] {
            let internal_commands =
                get_azure_internal_commands_dapr_component_name(container_app_name);
            let commands_queue =
                get_azure_queue_trigger_dapr_component_name(container_app_name, "commands");

            assert_ne!(internal_commands, commands_queue);
            assert_structured_dapr_component_name(&internal_commands);
            assert_structured_dapr_component_name(&commands_queue);
        }
    }

    #[test]
    fn structured_names_do_not_alias_any_legacy_name() {
        let app = "worker";
        let queue = "events";
        let storage = "archive";

        let commands = get_azure_internal_commands_dapr_component_name(app);
        assert!(get_legacy_azure_internal_commands_dapr_component_names(app)
            .iter()
            .all(|legacy| legacy != &commands));

        let queue_trigger = get_azure_queue_trigger_dapr_component_name(app, queue);
        assert!(
            get_legacy_azure_queue_trigger_dapr_component_names(app, queue)
                .iter()
                .all(|legacy| legacy != &queue_trigger)
        );

        let blob_trigger = get_azure_blob_trigger_dapr_component_name(app, storage);
        assert!(
            get_legacy_azure_blob_trigger_dapr_component_names(app, storage)
                .iter()
                .all(|legacy| legacy != &blob_trigger)
        );
    }

    #[test]
    fn legacy_names_include_first_bounded_rollout_hash() {
        let names = get_legacy_azure_internal_commands_dapr_component_names(
            "e2e-03-azure-terraform-provider-very-long-worker-name",
        );

        assert!(names
            .contains(&"servicebus-e2e-03-azure-terraform-provider-very-lon-3c4abf84".to_string()));
    }

    #[test]
    fn queue_trigger_names_disambiguate_cross_worker_tuple_boundaries() {
        let first =
            get_azure_queue_trigger_dapr_component_name("prefix-worker", "queue-trigger-events");
        let second =
            get_azure_queue_trigger_dapr_component_name("prefix-worker-queue-trigger", "events");

        assert_distinct_structured_names(&first, &second);
        assert_eq!(
            first,
            get_azure_queue_trigger_dapr_component_name("prefix-worker", "queue-trigger-events")
        );
    }

    #[test]
    fn commands_names_disambiguate_cross_kind_tuple_boundaries() {
        let internal =
            get_azure_internal_commands_dapr_component_name("prefix-worker-queue-trigger-events");
        let queue = get_azure_queue_trigger_dapr_component_name(
            "prefix-worker",
            "events-internal-commands",
        );

        assert_distinct_structured_names(&internal, &queue);
    }

    #[test]
    fn blob_trigger_names_disambiguate_cross_worker_tuple_boundaries() {
        let first = get_azure_blob_trigger_dapr_component_name("prefix-worker", "archive-files");
        let second = get_azure_blob_trigger_dapr_component_name("prefix-worker-archive", "files");

        assert_distinct_structured_names(&first, &second);
        assert_eq!(
            first,
            get_azure_blob_trigger_dapr_component_name("prefix-worker", "archive-files")
        );
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

    fn assert_structured_dapr_component_name(name: &str) {
        assert_valid_dapr_component_name(name);
        let (_, hash) = name
            .rsplit_once('-')
            .expect("structured Dapr component names should have a hash suffix");
        assert_eq!(hash.len(), 32);
        assert!(hash.chars().all(|character| character.is_ascii_hexdigit()));
    }

    fn assert_distinct_structured_names(first: &str, second: &str) {
        assert_ne!(first, second);
        assert_structured_dapr_component_name(first);
        assert_structured_dapr_component_name(second);
    }
}
