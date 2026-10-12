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
    token: Uuid,
    #[serde(default)]
    uid: Option<String>,
}

impl Default for GcpCloudRunOwner {
    fn default() -> Self {
        Self {
            token: Uuid::new_v4(),
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
        service
            .labels
            .get_or_insert_with(Default::default)
            .insert(OWNER_LABEL.to_string(), self.token.to_string());
    }

    pub fn confirm(&mut self, service: &Service, resource_id: &str) -> Result<()> {
        let token = self.token.to_string();
        if service
            .labels
            .as_ref()
            .and_then(|labels| labels.get(OWNER_LABEL))
            != Some(&token)
        {
            return Err(invalid(resource_id, "Refusing a foreign Cloud Run service"));
        }
        let uid = service
            .uid
            .as_ref()
            .filter(|uid| !uid.is_empty())
            .ok_or_else(|| invalid(resource_id, "Cloud Run service has no provider UID"))?;
        if self.uid.as_ref().is_some_and(|previous| previous != uid) {
            return Err(invalid(
                resource_id,
                "Refusing a replaced Cloud Run service",
            ));
        }
        self.uid = Some(uid.clone());
        Ok(())
    }

    /// One create at most; a retry only adopts the exact persisted lifetime
    /// token, and a previously confirmed service is never silently recreated.
    pub async fn create_or_recover(
        &mut self,
        client: &dyn CloudRunApi,
        mut plan: CloudRunCreatePlan<'_>,
    ) -> Result<CloudRunCreation> {
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
}
