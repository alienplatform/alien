use crate::{error::Result, CheckResult, CompileTimeCheck};
use alien_core::{
    Platform, Queue, Stack, Storage, StorageNotificationEvent, Worker, WorkerTrigger,
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
                for (field, value) in [
                    ("prefix", &notification.prefix),
                    ("suffix", &notification.suffix),
                ] {
                    if let Some(value) = value {
                        if value.is_empty() || value.len() > 512 {
                            result.add_error(format!(
                                "{label}.{field} must contain 1–512 UTF-8 bytes; got {}",
                                value.len()
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
    async fn accepts_overlapping_rules_for_frozen_and_live_resources() {
        for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
            let stack = stack(vec![notification(); 10], lifecycle);
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
            "Storage 'objects': notifications[0].suffix must contain 1–512 UTF-8 bytes; got 0",
        ]
        );
        let mut rule = notification();
        rule.prefix = Some("é".repeat(256));
        rule.suffix = Some("a".repeat(512));
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
                &stack(vec![notification(); 11], ResourceLifecycle::Live),
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
        let stack = stack(vec![notification(); 11], ResourceLifecycle::Frozen);
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
}
