use alien_client_core::ErrorData as ClientError;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::cloudscheduler::{CloudSchedulerApi, SchedulerJob};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ErrorData, Result};

/// Saved before creation, including when no successful create reply was received.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GcpSchedulerJobIntent {
    /// Exact requested configuration and unique ownership marker.
    job: SchedulerJob,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::cloudscheduler::{
        HttpTarget, MockCloudSchedulerApi, SchedulerOidcToken,
    };
    use std::sync::{Arc, Mutex};

    const NAME: &str = "projects/test/locations/us-central1/jobs/owned-job";

    fn intent() -> GcpSchedulerJobIntent {
        GcpSchedulerJobIntent::new(SchedulerJob {
            name: None,
            description: None,
            schedule: "* * * * *".into(),
            time_zone: Some("UTC".into()),
            http_target: Some(HttpTarget {
                uri: "https://owned.run.app".into(),
                http_method: Some("POST".into()),
                body: None,
                headers: None,
                oidc_token: Some(SchedulerOidcToken {
                    service_account_email: "worker@test.iam.gserviceaccount.com".into(),
                    audience: Some("https://owned.run.app".into()),
                }),
            }),
            state: None,
        })
    }

    fn absent(name: String) -> AlienError<ClientError> {
        AlienError::new(ClientError::RemoteResourceNotFound {
            resource_type: "scheduler job".into(),
            resource_name: name,
        })
    }

    #[tokio::test]
    async fn foreign_conflict_after_absent_read_is_not_adopted() {
        let intent = intent();
        let mut foreign = intent.job.clone();
        foreign.name = Some(NAME.into());
        foreign.description = Some("another controller".into());
        let mut calls = mockall::Sequence::new();
        let mut client = MockCloudSchedulerApi::new();
        client
            .expect_get_job()
            .times(1)
            .in_sequence(&mut calls)
            .return_once(|name| Err(absent(name)));
        client
            .expect_create_job()
            .times(1)
            .in_sequence(&mut calls)
            .return_once(|_, _, _| {
                Err(AlienError::new(ClientError::RemoteResourceConflict {
                    resource_type: "scheduler job".into(),
                    resource_name: NAME.into(),
                    message: "foreign create won race".into(),
                }))
            });
        client
            .expect_get_job()
            .times(1)
            .in_sequence(&mut calls)
            .return_once(move |_| Ok(foreign));
        client.expect_delete_job().never();
        assert_eq!(
            intent
                .create_or_recover(&client, NAME, "us-central1", "worker")
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
    }

    #[tokio::test]
    async fn lost_create_reply_is_recovered_after_reload_and_deleted() {
        let saved = serde_json::to_value(intent()).unwrap();
        let provider: Arc<Mutex<Option<SchedulerJob>>> = Arc::new(Mutex::new(None));
        let read = provider.clone();
        let create = provider.clone();
        let delete = provider.clone();
        let mut client = MockCloudSchedulerApi::new();
        client.expect_get_job().returning(move |name| {
            assert_eq!(name, NAME);
            read.lock().unwrap().clone().ok_or_else(|| absent(name))
        });
        client
            .expect_create_job()
            .times(1)
            .returning(move |region, id, mut job| {
                assert_eq!(region, "us-central1");
                assert_eq!(id, "owned-job");
                job.name = Some(NAME.into());
                *create.lock().unwrap() = Some(job);
                Err(AlienError::new(ClientError::RemoteServiceUnavailable {
                    message: "response lost after accepted create".into(),
                }))
            });
        client.expect_delete_job().times(1).returning(move |name| {
            assert_eq!(name, NAME);
            assert!(delete.lock().unwrap().take().is_some());
            Ok(())
        });
        let first: GcpSchedulerJobIntent = serde_json::from_value(saved.clone()).unwrap();
        assert!(first
            .create_or_recover(&client, NAME, "us-central1", "worker")
            .await
            .is_err());
        let restored: GcpSchedulerJobIntent = serde_json::from_value(saved).unwrap();
        restored
            .create_or_recover(&client, NAME, "us-central1", "worker")
            .await
            .unwrap();
        restored.delete(&client, NAME, "worker").await.unwrap();
        assert!(provider.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn pending_create_can_be_cleaned_without_a_successful_reply() {
        let intent = intent();
        let mut job = intent.job.clone();
        job.name = Some(NAME.into());
        let mut client = MockCloudSchedulerApi::new();
        client
            .expect_get_job()
            .times(1)
            .return_once(move |_| Ok(job));
        client.expect_delete_job().times(1).return_once(|name| {
            assert_eq!(name, NAME);
            Ok(())
        });
        let restored: GcpSchedulerJobIntent =
            serde_json::from_value(serde_json::to_value(intent).unwrap()).unwrap();
        restored.delete(&client, NAME, "worker").await.unwrap();
    }

    #[rstest::rstest]
    #[case("owner")]
    #[case("name")]
    #[case("schedule")]
    #[case("target")]
    #[case("audience")]
    #[tokio::test]
    async fn changed_or_foreign_job_is_neither_adopted_nor_deleted(#[case] change: &str) {
        let intent = intent();
        let mut job = intent.job.clone();
        job.name = Some(NAME.into());
        match change {
            "owner" => job.description = Some("foreign owner".into()),
            "name" => {
                job.name = Some("projects/foreign/locations/us-central1/jobs/owned-job".into())
            }
            "schedule" => job.schedule = "*/5 * * * *".into(),
            "target" => job.http_target.as_mut().unwrap().uri = "https://foreign.run.app".into(),
            "audience" => {
                job.http_target
                    .as_mut()
                    .unwrap()
                    .oidc_token
                    .as_mut()
                    .unwrap()
                    .audience = Some("https://foreign.run.app".into())
            }
            _ => unreachable!(),
        }
        let mut client = MockCloudSchedulerApi::new();
        client
            .expect_get_job()
            .times(2)
            .returning(move |_| Ok(job.clone()));
        client.expect_create_job().never();
        client.expect_delete_job().never();
        assert_eq!(
            intent
                .create_or_recover(&client, NAME, "us-central1", "worker")
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
        assert_eq!(
            intent
                .delete(&client, NAME, "worker")
                .await
                .unwrap_err()
                .code,
            "RESOURCE_CONFIG_INVALID"
        );
    }
}

impl GcpSchedulerJobIntent {
    pub fn new(mut job: SchedulerJob) -> Self {
        job.description = Some(format!("Alien schedule owner {}", Uuid::new_v4()));
        Self { job }
    }

    fn confirm(&self, name: &str, job: &SchedulerJob, resource_id: &str) -> Result<()> {
        let target_matches = match (&self.job.http_target, &job.http_target) {
            (Some(expected), Some(actual)) => {
                expected.uri == actual.uri
                    && expected.http_method == actual.http_method
                    && expected.body == actual.body
                    && match (&expected.oidc_token, &actual.oidc_token) {
                        (Some(expected), Some(actual)) => {
                            expected.service_account_email == actual.service_account_email
                                && expected.audience == actual.audience
                        }
                        _ => false,
                    }
            }
            _ => false,
        };
        if job.name.as_deref() != Some(name)
            || job.description != self.job.description
            || job.schedule != self.job.schedule
            || job.time_zone != self.job.time_zone
            || !target_matches
        {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(resource_id.to_string()),
                message: format!("Refusing foreign or changed scheduler job '{name}'"),
            }));
        }
        Ok(())
    }

    pub async fn create_or_recover(
        &self,
        client: &dyn CloudSchedulerApi,
        name: &str,
        region: &str,
        resource_id: &str,
    ) -> Result<()> {
        match client.get_job(name.to_string()).await {
            Ok(existing) => return self.confirm(name, &existing, resource_id),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    resource_id: Some(resource_id.to_string()),
                    message: format!("Failed to inspect pending scheduler job '{name}'"),
                }))
            }
        }
        let job_id = name.rsplit('/').next().unwrap_or(name);
        let created = match client
            .create_job(region.to_string(), job_id.to_string(), self.job.clone())
            .await
        {
            Ok(created) => created,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceConflict { .. })
                ) =>
            {
                client
                    .get_job(name.to_string())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        resource_id: Some(resource_id.to_string()),
                        message: format!("Failed to inspect conflicting scheduler job '{name}'"),
                    })?
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    resource_id: Some(resource_id.to_string()),
                    message: format!("Failed to create scheduler job '{name}'"),
                }))
            }
        };
        self.confirm(name, &created, resource_id)
    }

    /// Resolve an uncertain create before cleanup; a same-name foreign job is never deleted.
    pub async fn delete(
        &self,
        client: &dyn CloudSchedulerApi,
        name: &str,
        resource_id: &str,
    ) -> Result<()> {
        match client.get_job(name.to_string()).await {
            Ok(existing) => self.confirm(name, &existing, resource_id)?,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(())
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    resource_id: Some(resource_id.to_string()),
                    message: format!("Failed to inspect scheduler job '{name}' before deletion"),
                }))
            }
        }
        match client.delete_job(name.to_string()).await {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientError::RemoteResourceNotFound { .. })
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                resource_id: Some(resource_id.to_string()),
                message: format!("Failed to delete owned scheduler job '{name}'"),
            })),
        }
    }
}
