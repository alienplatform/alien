use crate::{CheckResult, CompileTimeCheck};
use alien_core::{Container, Platform, Stack};
use async_trait::async_trait;

/// Validates the snapshot schedule of each persistent volume.
///
/// The schedule has to fit every cloud's native snapshot scheduler, so a stack
/// that deploys to one cloud also deploys to the others.
pub struct VolumeBackupsCheck;

#[async_trait]
impl CompileTimeCheck for VolumeBackupsCheck {
    fn description(&self) -> &'static str {
        "Validate persistent volume backup schedules"
    }

    fn should_run(&self, stack: &Stack, _platform: Platform) -> bool {
        stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Container>()
                .is_some_and(|container| container.persistent_storage.is_some())
        })
    }

    async fn check(&self, stack: &Stack, _platform: Platform) -> crate::error::Result<CheckResult> {
        let failures: Vec<String> = stack
            .resources()
            .filter_map(|(_, entry)| entry.config.downcast_ref::<Container>())
            .filter_map(|container| {
                let storage = container.persistent_storage.as_ref()?;
                let reason = storage.backups.validation_error()?;
                Some(format!("Container '{}': {reason}", container.id))
            })
            .collect();

        if failures.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(failures))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        ContainerCode, PersistentStorage, ResourceLifecycle, ResourceSpec, VolumeBackups,
    };

    fn stack_with_backups(backups: VolumeBackups) -> Stack {
        let container = Container::new("db".to_string())
            .code(ContainerCode::Image {
                image: "postgres:16".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(5432)
            .replicas(1)
            .persistent_storage(PersistentStorage {
                size: "20Gi".to_string(),
                mount_path: "/data".to_string(),
                backups,
            })
            .stateful(true)
            .permissions("execution".to_string())
            .build();
        Stack::new("test".to_string())
            .add(container, ResourceLifecycle::Live)
            .build()
    }

    async fn errors_for(backups: VolumeBackups) -> Vec<String> {
        VolumeBackupsCheck
            .check(&stack_with_backups(backups), Platform::Aws)
            .await
            .expect("check should run")
            .errors
    }

    #[tokio::test]
    async fn accepts_defaults_and_disabled_backups() {
        assert!(errors_for(VolumeBackups::default()).await.is_empty());
        // A disabled schedule is never sent to a cloud, so its numbers don't matter.
        assert!(errors_for(VolumeBackups {
            enabled: false,
            interval_hours: 5,
            retention_days: 0,
        })
        .await
        .is_empty());
    }

    #[tokio::test]
    async fn rejects_intervals_some_cloud_cannot_schedule() {
        // AWS accepts 3 hours but Azure Disk Backup does not.
        let errors = errors_for(VolumeBackups {
            enabled: true,
            interval_hours: 3,
            retention_days: 7,
        })
        .await;
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("Container 'db': backup intervalHours must be one of"));
    }

    #[tokio::test]
    async fn rejects_more_snapshots_than_a_disk_can_hold() {
        // Hourly for 18 days is 432 snapshots; 19 days is 456, over the 450 limit.
        assert!(errors_for(VolumeBackups {
            enabled: true,
            interval_hours: 1,
            retention_days: 18,
        })
        .await
        .is_empty());
        let errors = errors_for(VolumeBackups {
            enabled: true,
            interval_hours: 1,
            retention_days: 19,
        })
        .await;
        assert_eq!(
            errors,
            vec![
                "Container 'db': backups every 1 hours for 19 days keep 456 snapshots per volume; \
                 the most is 450"
                    .to_string()
            ]
        );
    }

    #[tokio::test]
    async fn rejects_zero_retention() {
        let errors = errors_for(VolumeBackups {
            enabled: true,
            interval_hours: 24,
            retention_days: 0,
        })
        .await;
        assert_eq!(
            errors,
            vec!["Container 'db': backup retentionDays must be at least 1".to_string()]
        );
    }
}
