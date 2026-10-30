use crate::{error::Result, CheckResult, CompileTimeCheck};
use alien_core::{
    storage_notifications::notifications_overlap, Platform, Queue, Stack, Storage,
    StorageNotification, StorageNotificationEvent, Worker, WorkerTrigger,
};

/// Checks notification limits, destination ownership and platform support before deployment.
pub struct StorageNotificationsCheck;

#[async_trait::async_trait]
impl CompileTimeCheck for StorageNotificationsCheck {
    fn code(&self) -> Option<&'static str> {
        Some("STORAGE_NOTIFICATIONS_INVALID")
    }

    fn description(&self) -> &'static str {
        "Storage notifications target queues with compatible ownership and platform support"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<Storage>()
                .is_some_and(|storage| !storage.notifications.is_empty())
        })
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let mut result = CheckResult::success();
        for (id, entry) in &stack.resources {
            let Some(storage) = entry.config.downcast_ref::<Storage>() else {
                continue;
            };
            if storage.notifications.is_empty() {
                continue;
            }
            // Externally bound storage/queues on Kubernetes and machines do not
            // give controllers ownership of the provider's notification configuration.
            if !matches!(
                platform,
                Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Local
            ) {
                result.add_error(format!(
                    "Storage '{id}': notifications are not supported on {platform}; use AWS, GCP, Azure or local storage and queues"
                ));
            }
            if storage.notifications.len() > 10 {
                result.add_error(format!(
                    "Storage '{id}': notifications has {} rules; at most 10 are supported",
                    storage.notifications.len()
                ));
            }
            for (index, notification) in storage.notifications.iter().enumerate() {
                let label = format!("Storage '{id}': notifications[{index}]");
                if notification.events != [StorageNotificationEvent::Created] {
                    result.add_error(format!("{label}.events must contain exactly ['created']"));
                }
                for (field, value, max_bytes) in [
                    ("prefix", &notification.prefix, 512),
                    ("suffix", &notification.suffix, 128),
                ] {
                    if let Some(value) = value {
                        if value.is_empty() || value.len() > max_bytes {
                            result.add_error(format!(
                                "{label}.{field} must contain 1–{max_bytes} UTF-8 bytes; got {}",
                                value.len()
                            ));
                        }
                        if let Some(character) =
                            value.chars().find(|character| character.is_ascii_control())
                        {
                            result.add_error(format!(
                                "{label}.{field} contains control character U+{:04X}; control characters are not supported",
                                u32::from(character)
                            ));
                        }
                    }
                }
                if notification.queue.resource_type != Queue::RESOURCE_TYPE {
                    result.add_error(format!("{label}.queue must reference a Queue"));
                    continue;
                }
                let Some(queue) = stack.resources.get(&notification.queue.id) else {
                    result.add_error(format!(
                        "{label}.queue '{}' does not exist in the stack",
                        notification.queue.id
                    ));
                    continue;
                };
                if queue.config.downcast_ref::<Queue>().is_none() {
                    result.add_error(format!(
                        "{label}.queue '{}' is not a Queue",
                        notification.queue.id
                    ));
                } else if entry.lifecycle != queue.lifecycle {
                    result.add_error(format!("{label}.queue '{}': Storage and Queue must have the same lifecycle (both Frozen or both Live)", notification.queue.id));
                }
            }
            for (later, rule) in storage.notifications.iter().enumerate() {
                for (earlier, other) in storage.notifications[..later].iter().enumerate() {
                    if notifications_overlap(other, rule) {
                        result.add_error(format!(
                            "Storage '{id}': notifications[{earlier}] ({}) and notifications[{later}] ({}) overlap: an object key can match both. Give them prefixes where neither starts with the other, or suffixes where neither ends with the other",
                            describe(other),
                            describe(rule)
                        ));
                    }
                }
            }
            for (worker_id, worker_entry) in &stack.resources {
                let Some(worker) = worker_entry.config.downcast_ref::<Worker>() else {
                    continue;
                };
                if worker.triggers.iter().any(|trigger| {
                    matches!(trigger, WorkerTrigger::Storage { storage, .. }
                        if storage.resource_type == Storage::RESOURCE_TYPE && storage.id == *id)
                }) {
                    result.add_error(format!("Storage '{id}' has notifications and is a storage trigger source for Worker '{worker_id}'; consume its notifications with a queue trigger instead"));
                }
            }
        }
        Ok(result)
    }
}

fn describe(rule: &StorageNotification) -> String {
    let filter = |name: &str, value: &Option<String>| match value {
        Some(value) => format!("{name} {value:?}"),
        None => format!("any {name}"),
    };
    format!(
        "{}, {}",
        filter("prefix", &rule.prefix),
        filter("suffix", &rule.suffix)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compatibility::frozen_resources_unchanged::FrozenResourcesUnchangedCheck,
        compile_time::validate_stack_dependencies, error::ErrorData, runner::PreflightRunner,
        StackCompatibilityCheck,
    };
    use alien_core::{ResourceLifecycle, ResourceRef, StorageNotification, WorkerCode};

    fn notification() -> StorageNotification {
        StorageNotification::builder()
            .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "work"))
            .prefix("in/".to_string())
            .suffix(".json".to_string())
            .build()
    }

    /// `count` rules that can never match the same key.
    fn distinct(count: usize) -> Vec<StorageNotification> {
        (0..count)
            .map(|index| {
                let mut rule = notification();
                rule.prefix = Some(format!("in{index}/"));
                rule
            })
            .collect()
    }

    fn stack(rules: Vec<StorageNotification>, lifecycle: ResourceLifecycle) -> Stack {
        Stack::new("notifications".to_string())
            .add(
                Storage::new("objects".to_string())
                    .notifications(rules)
                    .build(),
                lifecycle,
            )
            .add(Queue::new("work".to_string()).build(), lifecycle)
            .build()
    }

    async fn check(stack: &Stack, platform: Platform) -> CheckResult {
        StorageNotificationsCheck
            .check(stack, platform)
            .await
            .expect("check runs")
    }

    #[tokio::test]
    async fn accepts_ten_distinct_rules_for_frozen_and_live_resources() {
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let stack = stack(distinct(10), lifecycle);
            for platform in [
                Platform::Aws,
                Platform::Gcp,
                Platform::Azure,
                Platform::Local,
            ] {
                assert!(StorageNotificationsCheck.should_run(&stack, platform));
                assert_eq!(check(&stack, platform).await.errors, Vec::<String>::new());
            }
        }
    }

    #[tokio::test]
    async fn rejects_limits_and_empty_or_duplicate_events() {
        let mut rule = notification();
        rule.prefix = Some("é".repeat(257));
        rule.suffix = Some(String::new());
        rule.events.clear();
        let invalid = stack(vec![rule], ResourceLifecycle::Frozen);
        assert_eq!(
            check(&invalid, Platform::Aws).await.errors,
            vec![
            "Storage 'objects': notifications[0].events must contain exactly ['created']",
            "Storage 'objects': notifications[0].prefix must contain 1–512 UTF-8 bytes; got 514",
            "Storage 'objects': notifications[0].suffix must contain 1–128 UTF-8 bytes; got 0",
        ]
        );
        let mut rule = notification();
        rule.prefix = Some("é".repeat(256));
        rule.suffix = Some("a".repeat(128));
        assert!(
            check(
                &stack(vec![rule.clone()], ResourceLifecycle::Live),
                Platform::Gcp
            )
            .await
            .success
        );
        rule.events.push(StorageNotificationEvent::Created);
        assert_eq!(
            check(&stack(vec![rule], ResourceLifecycle::Live), Platform::Gcp)
                .await
                .errors,
            vec!["Storage 'objects': notifications[0].events must contain exactly ['created']"]
        );
        assert_eq!(
            check(
                &stack(distinct(11), ResourceLifecycle::Live),
                Platform::Azure
            )
            .await
            .errors,
            vec!["Storage 'objects': notifications has 11 rules; at most 10 are supported"]
        );
    }

    #[tokio::test]
    async fn validates_queue_type_existence_and_lifecycle() {
        let mut rule = notification();
        rule.queue = ResourceRef::new(Storage::RESOURCE_TYPE, "objects");
        assert_eq!(
            check(
                &stack(vec![rule.clone()], ResourceLifecycle::Live),
                Platform::Aws
            )
            .await
            .errors,
            vec!["Storage 'objects': notifications[0].queue must reference a Queue"]
        );
        rule.queue = ResourceRef::new(Queue::RESOURCE_TYPE, "missing");
        assert_eq!(
            check(
                &stack(vec![rule.clone()], ResourceLifecycle::Live),
                Platform::Aws
            )
            .await
            .errors,
            vec!["Storage 'objects': notifications[0].queue 'missing' does not exist in the stack"]
        );
        rule.queue = ResourceRef::new(Queue::RESOURCE_TYPE, "objects");
        assert_eq!(
            check(&stack(vec![rule], ResourceLifecycle::Live), Platform::Aws)
                .await
                .errors,
            vec!["Storage 'objects': notifications[0].queue 'objects' is not a Queue"]
        );
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let mut stack = stack(vec![notification()], lifecycle);
            stack.resources.get_mut("work").unwrap().lifecycle = match lifecycle {
                ResourceLifecycle::Frozen => ResourceLifecycle::Live,
                ResourceLifecycle::Live => ResourceLifecycle::Frozen,
            };
            assert_eq!(check(&stack, Platform::Aws).await.errors, vec![
                "Storage 'objects': notifications[0].queue 'work': Storage and Queue must have the same lifecycle (both Frozen or both Live)"
            ]);
        }
    }

    #[tokio::test]
    async fn rejects_direct_storage_triggers_on_every_supported_platform() {
        let mut stack = stack(vec![notification()], ResourceLifecycle::Frozen);
        let worker = Worker::new("reader".to_string())
            .code(WorkerCode::Image {
                image: "example:latest".to_string(),
            })
            .permissions("default".to_string())
            .trigger(WorkerTrigger::Storage {
                storage: ResourceRef::new(Storage::RESOURCE_TYPE, "objects"),
                events: vec!["created".to_string()],
            })
            .build();
        stack.resources.insert(
            "reader".to_string(),
            alien_core::ResourceEntry::new(worker, ResourceLifecycle::Live),
        );
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Local,
        ] {
            assert_eq!(check(&stack, platform).await.errors, vec![
                "Storage 'objects' has notifications and is a storage trigger source for Worker 'reader'; consume its notifications with a queue trigger instead"
            ]);
        }
    }

    #[tokio::test]
    async fn unsupported_platforms_fail_without_affecting_existing_storage() {
        for platform in [Platform::Kubernetes, Platform::Machines, Platform::Test] {
            let stack = stack(vec![notification()], ResourceLifecycle::Frozen);
            assert_eq!(check(&stack, platform).await.errors, vec![format!(
                "Storage 'objects': notifications are not supported on {platform}; use AWS, GCP, Azure or local storage and queues"
            )]);
        }
        let existing = stack(vec![], ResourceLifecycle::Frozen);
        for platform in Platform::DEPLOYABLE {
            assert!(!StorageNotificationsCheck.should_run(&existing, *platform));
            assert!(check(&existing, *platform).await.success);
        }
    }

    #[tokio::test]
    async fn notification_dependencies_participate_in_cycle_detection() {
        let mut stack = stack(vec![notification()], ResourceLifecycle::Live);
        assert!(validate_stack_dependencies(&stack).success);
        stack
            .resources
            .get_mut("work")
            .unwrap()
            .dependencies
            .push(ResourceRef::new(Storage::RESOURCE_TYPE, "objects"));
        let result = validate_stack_dependencies(&stack);
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Circular dependency detected"));
    }

    #[tokio::test]
    async fn build_and_template_entry_points_enforce_notification_validation() {
        let stack = stack(distinct(11), ResourceLifecycle::Frozen);
        let runner = PreflightRunner::new();
        for outcome in [
            runner
                .run_build_time_preflights(&stack, Platform::Aws)
                .await
                .map(|_| ()),
            runner
                .run_template_preflights(&stack, Platform::Aws)
                .await
                .map(|_| ()),
        ] {
            let error = outcome.expect_err("invalid notifications must fail before provisioning");
            let Some(ErrorData::ValidationFailed { results, .. }) = &error.error else {
                panic!("expected structured validation results, got {error:?}");
            };
            let failures: Vec<_> = results.iter().filter(|result| !result.success).collect();
            assert_eq!(failures.len(), 1);
            assert_eq!(
                failures[0].code.as_deref(),
                Some("STORAGE_NOTIFICATIONS_INVALID")
            );
            assert_eq!(
                failures[0].errors,
                vec!["Storage 'objects': notifications has 11 rules; at most 10 are supported"]
            );
        }
    }

    #[tokio::test]
    async fn frozen_rule_add_edit_and_remove_require_setup_but_live_edits_do_not() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let check = FrozenResourcesUnchangedCheck { platform };
            let empty = stack(vec![], ResourceLifecycle::Frozen);
            let original = stack(vec![notification()], ResourceLifecycle::Frozen);
            let mut edited = notification();
            edited.suffix = Some(".txt".to_string());
            let changed = stack(vec![edited], ResourceLifecycle::Frozen);
            for (old, new) in [
                (&empty, &original),
                (&original, &changed),
                (&original, &empty),
            ] {
                let result = check.check(old, new).await.unwrap();
                assert!(!result.success, "notification changes require setup");
                assert_eq!(result.errors.len(), 1);
                assert!(result.errors[0].contains("objects"));
                assert_ne!(old.setup_owned_digest(), new.setup_owned_digest());
            }
            assert!(check.check(&original, &original).await.unwrap().success);
            let live = stack(vec![], ResourceLifecycle::Live);
            let notified = stack(vec![notification()], ResourceLifecycle::Live);
            assert!(check.check(&live, &notified).await.unwrap().success);
        }
    }

    #[tokio::test]
    async fn suffix_limit_counts_utf8_bytes_on_every_supported_platform() {
        let mut rule = notification();
        rule.suffix = Some("é".repeat(64));
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Local,
        ] {
            assert!(
                check(
                    &stack(vec![rule.clone()], ResourceLifecycle::Frozen),
                    platform
                )
                .await
                .success
            );
            let mut too_long = rule.clone();
            too_long.suffix.as_mut().unwrap().push('x');
            assert_eq!(check(&stack(vec![too_long], ResourceLifecycle::Frozen), platform).await.errors,
                vec!["Storage 'objects': notifications[0].suffix must contain 1–128 UTF-8 bytes; got 129"]);
        }
    }

    #[tokio::test]
    async fn overlapping_rules_fail_on_every_platform_and_in_build_and_setup() {
        let rule = |prefix: Option<&str>, suffix: Option<&str>| {
            let mut rule = notification();
            rule.prefix = prefix.map(str::to_string);
            rule.suffix = suffix.map(str::to_string);
            rule
        };
        // The same cases real S3 rejected and accepted.
        for (rules, expected) in [
            (vec![rule(Some("a/"), None), rule(Some("a/b/"), None)], true),
            (
                vec![
                    rule(Some("a/"), Some("json")),
                    rule(Some("a/"), Some(".json")),
                ],
                true,
            ),
            (vec![rule(Some("a"), None), rule(Some("ab"), None)], true),
            (vec![notification(), notification()], true),
            (vec![rule(None, None), rule(Some("a/"), None)], true),
            (
                vec![
                    rule(Some("a/"), Some(".json")),
                    rule(Some("a/"), Some(".csv")),
                ],
                false,
            ),
            (
                vec![
                    rule(Some("a/"), Some(".json")),
                    rule(Some("b/"), Some(".json")),
                ],
                false,
            ),
            (
                vec![rule(None, Some(".json")), rule(Some("a/"), Some(".csv"))],
                false,
            ),
            (vec![rule(Some("A/"), None), rule(Some("a/"), None)], false),
        ] {
            let stack = stack(rules.clone(), ResourceLifecycle::Frozen);
            for platform in [
                Platform::Aws,
                Platform::Gcp,
                Platform::Azure,
                Platform::Local,
            ] {
                let errors = check(&stack, platform).await.errors;
                assert_eq!(
                    errors.len(),
                    usize::from(expected),
                    "{rules:?} on {platform}: {errors:?}"
                );
            }
        }
        let overlapping = stack(
            vec![notification(), rule(Some("in/sub/"), None)],
            ResourceLifecycle::Frozen,
        );
        assert_eq!(
            check(&overlapping, Platform::Gcp).await.errors,
            vec![
                "Storage 'objects': notifications[0] (prefix \"in/\", suffix \".json\") and notifications[1] (prefix \"in/sub/\", any suffix) overlap: an object key can match both. Give them prefixes where neither starts with the other, or suffixes where neither ends with the other"
            ]
        );
        let runner = PreflightRunner::new();
        assert!(runner
            .run_build_time_preflights(&overlapping, Platform::Aws)
            .await
            .is_err());
        assert!(runner
            .run_template_preflights(&overlapping, Platform::Aws)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn control_characters_in_either_filter_are_rejected_on_every_supported_platform() {
        for character in (0u8..=31).chain(std::iter::once(127)) {
            for field in ["prefix", "suffix"] {
                let mut rule = notification();
                let value = Some(format!("before{}after", char::from(character)));
                if field == "prefix" {
                    rule.prefix = value;
                } else {
                    rule.suffix = value;
                }
                let stack = stack(vec![rule], ResourceLifecycle::Frozen);
                for platform in [
                    Platform::Aws,
                    Platform::Gcp,
                    Platform::Azure,
                    Platform::Local,
                ] {
                    assert_eq!(check(&stack, platform).await.errors, vec![format!(
                        "Storage 'objects': notifications[0].{field} contains control character U+{character:04X}; control characters are not supported"
                    )]);
                }
            }
        }
    }
}
