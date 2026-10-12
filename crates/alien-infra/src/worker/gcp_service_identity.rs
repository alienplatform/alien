use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::cloudrun::{CloudRunApi, Service};
use alien_gcp_clients::longrunning::Operation;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ErrorData, Result};

const OWNER_LABEL: &str = "alien-create-token";

/// Persist this identity before issuing a Cloud Run create. Names alone cannot
/// distinguish a lost response from a service owned by another deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GcpCloudRunOwner {
    /// None identifies an explicitly verified legacy service, fenced by its UID.
    token: Option<Uuid>,
    #[serde(default)]
    uid: Option<String>,
}

impl Default for GcpCloudRunOwner {
    fn default() -> Self {
        Self {
            token: Some(Uuid::new_v4()),
            uid: None,
        }
    }
}

pub(super) struct CloudRunCreatePlan<'a> {
    pub region: &'a str,
    pub name: &'a str,
    pub resource_id: &'a str,
    pub service: Service,
}

#[derive(Debug)]
pub(super) enum CloudRunCreation {
    Started(Operation),
    Recovered,
}

impl GcpCloudRunOwner {
    pub fn stamp(&self, service: &mut Service) {
        if let Some(token) = self.token {
            service
                .labels
                .get_or_insert_with(Default::default)
                .insert(OWNER_LABEL.to_string(), token.to_string());
        }
    }

    pub fn confirm(&mut self, service: &Service, resource_id: &str) -> Result<()> {
        let token = self.token.map(|token| token.to_string());
        if service
            .labels
            .as_ref()
            .and_then(|labels| labels.get(OWNER_LABEL))
            != token.as_ref()
        {
            return Err(invalid(resource_id, "Refusing a foreign Cloud Run service"));
        }
        let uid = service
            .uid
            .as_ref()
            .filter(|uid| !uid.is_empty())
            .ok_or_else(|| invalid(resource_id, "Cloud Run service has no provider UID"))?;
        if self.token.is_none() && self.uid.is_none() {
            return Err(invalid(
                resource_id,
                "Legacy Cloud Run identity has no recorded UID",
            ));
        }
        if self.uid.as_ref().is_some_and(|previous| previous != uid) {
            return Err(invalid(
                resource_id,
                "Refusing a replaced Cloud Run service",
            ));
        }
        self.uid = Some(uid.clone());
        Ok(())
    }

    /// Read-only handoff of a service whose exact name was recorded by an older
    /// controller or explicit setup import. The caller reads that name in its
    /// original project/region; this must never be used to recover a new create.
    pub fn from_recorded(
        service: &Service,
        recorded_name: &str,
        resource_id: &str,
        deployment: &str,
    ) -> Result<Self> {
        let labels = service.labels.as_ref().ok_or_else(|| {
            invalid(
                resource_id,
                "Recorded legacy Cloud Run service has no ownership labels",
            )
        })?;
        // Setup emitters normalize resource labels, while older runtime creates
        // used the resource ID directly. Both still require the recorded service name.
        let resource_label: String = resource_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .take(63)
            .collect();
        if service
            .name
            .as_deref()
            .and_then(|name| name.rsplit('/').next())
            != Some(recorded_name)
            || labels.get("deployment").map(String::as_str) != Some(deployment)
            || !matches!(labels.get("resource").map(String::as_str), Some(value) if value == resource_id || value == resource_label)
            || labels.get("resource-type").map(String::as_str) != Some("worker")
            || labels.contains_key(OWNER_LABEL)
        {
            return Err(invalid(
                resource_id,
                "Refusing a foreign recorded legacy Cloud Run service",
            ));
        }
        let mut owner = Self {
            token: None,
            uid: service.uid.clone(),
        };
        owner.confirm(service, resource_id)?;
        Ok(owner)
    }

    /// One create at most; a retry only adopts the exact persisted lifetime
    /// token, and a previously confirmed service is never silently recreated.
    pub async fn create_or_recover(
        &mut self,
        client: &dyn CloudRunApi,
        mut plan: CloudRunCreatePlan<'_>,
    ) -> Result<CloudRunCreation> {
        if self.token.is_none() {
            return Err(invalid(
                plan.resource_id,
                "A handed-off legacy Cloud Run service cannot be recreated",
            ));
        }
        match client
            .get_service(plan.region.to_string(), plan.name.to_string())
            .await
        {
            Ok(service) => {
                self.confirm(&service, plan.resource_id)?;
                return Ok(CloudRunCreation::Recovered);
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                if self.uid.is_some() {
                    return Err(invalid(
                        plan.resource_id,
                        "Previously confirmed Cloud Run service is missing",
                    ));
                }
            }
            Err(error) => {
                return Err(error.context(cloud_error(plan.resource_id, "Read Cloud Run service")))
            }
        }
        self.stamp(&mut plan.service);
        let operation = client
            .create_service(
                plan.region.to_string(),
                plan.name.to_string(),
                plan.service,
                None,
            )
            .await
            .context(cloud_error(plan.resource_id, "Create Cloud Run service"))?;
        Ok(CloudRunCreation::Started(operation))
    }
}

fn invalid(resource_id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(resource_id.to_string()),
        message: message.to_string(),
    })
}

fn cloud_error(resource_id: &str, message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(resource_id.to_string()),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::cloudrun::MockCloudRunApi;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Cloud {
        service: Option<Service>,
        creates: usize,
    }

    fn plan() -> CloudRunCreatePlan<'static> {
        CloudRunCreatePlan {
            region: "us-central1",
            name: "test-api",
            resource_id: "api",
            service: Service::default(),
        }
    }

    #[tokio::test]
    async fn lost_service_create_response_recovers_exact_identity_after_restore() {
        let cloud = Arc::new(Mutex::new(Cloud::default()));
        let mut client = MockCloudRunApi::new();
        let store = cloud.clone();
        client.expect_get_service().returning(move |_, _| {
            store.lock().unwrap().service.clone().ok_or_else(|| {
                AlienError::new(ClientErrorData::RemoteResourceNotFound {
                    resource_type: "Cloud Run service".to_string(),
                    resource_name: "test-api".to_string(),
                })
            })
        });
        let store = cloud.clone();
        client
            .expect_create_service()
            .times(1)
            .returning(move |_, _, mut service, _| {
                let mut store = store.lock().unwrap();
                assert!(store.service.is_none());
                service.uid = Some("original-uid".to_string());
                store.service = Some(service);
                store.creates += 1;
                Err(AlienError::new(ClientErrorData::AuthenticationError {
                    message: "create committed, response lost".to_string(),
                }))
            });
        let mut owner = GcpCloudRunOwner::default();
        assert!(owner.create_or_recover(&client, plan()).await.is_err());
        owner = serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap();
        assert!(matches!(
            owner.create_or_recover(&client, plan()).await.unwrap(),
            CloudRunCreation::Recovered
        ));
        assert_eq!(owner.uid.as_deref(), Some("original-uid"));
        assert_eq!(cloud.lock().unwrap().creates, 1);

        let original = cloud.lock().unwrap().service.clone().unwrap();
        let mut replacement = original.clone();
        replacement.uid = Some("replacement".to_string());
        cloud.lock().unwrap().service = Some(replacement);
        assert!(owner
            .create_or_recover(&client, plan())
            .await
            .unwrap_err()
            .message
            .contains("replaced"));
        let mut foreign = original;
        foreign.labels.as_mut().unwrap().clear();
        cloud.lock().unwrap().service = Some(foreign);
        assert!(owner
            .create_or_recover(&client, plan())
            .await
            .unwrap_err()
            .message
            .contains("foreign"));
        cloud.lock().unwrap().service = None;
        assert!(owner
            .create_or_recover(&client, plan())
            .await
            .unwrap_err()
            .message
            .contains("missing"));
        assert_eq!(cloud.lock().unwrap().creates, 1);
    }

    #[tokio::test]
    async fn recorded_legacy_handoff_preserves_uid_without_permission_writes() {
        let service = Service {
            name: Some("projects/test-project/locations/us-central1/services/test-api".to_string()),
            uid: Some("original-uid".to_string()),
            labels: Some(std::collections::HashMap::from([
                ("deployment".to_string(), "test".to_string()),
                ("resource".to_string(), "api".to_string()),
                ("resource-type".to_string(), "worker".to_string()),
            ])),
            ..Default::default()
        };
        let mut owner =
            GcpCloudRunOwner::from_recorded(&service, "test-api", "api", "test").unwrap();
        owner = serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap();
        owner.confirm(&service, "api").unwrap();
        let mut stamped = service.clone();
        owner.stamp(&mut stamped);
        assert_eq!(
            stamped.labels, service.labels,
            "handoff must not alter provider labels"
        );
        let mut replaced = service.clone();
        replaced.uid = Some("replacement".to_string());
        assert!(owner
            .confirm(&replaced, "api")
            .unwrap_err()
            .message
            .contains("replaced"));
        assert!(owner
            .create_or_recover(&MockCloudRunApi::new(), plan())
            .await
            .is_err());
        for label in ["deployment", "resource", "resource-type"] {
            let mut foreign = service.clone();
            foreign
                .labels
                .as_mut()
                .unwrap()
                .insert(label.to_string(), "foreign".to_string());
            assert!(GcpCloudRunOwner::from_recorded(&foreign, "test-api", "api", "test").is_err());
        }
        let mut foreign = service.clone();
        foreign
            .labels
            .as_mut()
            .unwrap()
            .insert(OWNER_LABEL.to_string(), Uuid::new_v4().to_string());
        assert!(GcpCloudRunOwner::from_recorded(&foreign, "test-api", "api", "test").is_err());
        assert!(GcpCloudRunOwner::from_recorded(&service, "other-api", "api", "test").is_err());
        let mut missing_uid = service;
        missing_uid.uid = None;
        assert!(GcpCloudRunOwner::from_recorded(&missing_uid, "test-api", "api", "test").is_err());
    }
}
