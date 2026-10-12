//! Durable operation polling for regional private ingress and its global firewall.

use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, ContextError};
use alien_gcp_clients::gcp::compute::{ComputeApi, Operation};
use serde::{Deserialize, Serialize};

use crate::{ErrorData, Result};

/// A command's operation is saved before the next controller step polls it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpIngressOperation {
    pub name: String,
    /// None denotes the global operation collection, used by firewall rules.
    pub region: Option<String>,
    pub resource_id: String,
    #[serde(default)]
    pub polls: u32,
}

/// A missing operation requires rediscovery of the recorded child, never blind
/// assumption that the command completed. Operation records expire in GCP.
#[derive(Debug, PartialEq, Eq)]
pub enum GcpIngressOperationProgress {
    Pending,
    Complete,
    Rediscover,
    /// Clear the terminal operation before reporting this failure. A retry
    /// reads the recorded child before deciding whether to reissue the command.
    Failed(String),
}

impl GcpIngressOperation {
    pub fn from_command(operation: Operation, resource_id: &str) -> Result<Self> {
        let name = operation.name.ok_or_else(|| {
            AlienError::new(operation_error(
                resource_id,
                "command returned no operation name",
            ))
        })?;
        let region = operation
            .region
            .map(|url| url.rsplit('/').next().unwrap_or(&url).to_string());
        Ok(Self {
            name,
            region,
            resource_id: resource_id.to_string(),
            polls: 0,
        })
    }

    /// Performs one read and never waits inside the controller handler. The
    /// caller retains this value on errors and returns a delay while Pending.
    pub async fn poll(&mut self, client: &dyn ComputeApi) -> Result<GcpIngressOperationProgress> {
        if self.polls >= 180 {
            return Err(AlienError::new(operation_error(
                &self.resource_id,
                &format!("operation '{}' exceeded 180 polls", self.name),
            )));
        }
        let result = match &self.region {
            Some(region) => {
                client
                    .get_region_operation(region.clone(), self.name.clone())
                    .await
            }
            None => client.get_global_operation(self.name.clone()).await,
        };
        let operation = match result {
            Ok(operation) => operation,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(GcpIngressOperationProgress::Rediscover);
            }
            Err(error) => {
                return Err(error.context(operation_error(
                    &self.resource_id,
                    &format!("read operation '{}'", self.name),
                )));
            }
        };
        self.polls += 1;
        if !operation.is_done() {
            return Ok(GcpIngressOperationProgress::Pending);
        }
        if operation.has_error()
            || operation
                .http_error_status_code
                .is_some_and(|code| code >= 400)
        {
            return Ok(GcpIngressOperationProgress::Failed(format!(
                "operation '{}' failed: {:?}",
                self.name, operation.error
            )));
        }
        Ok(GcpIngressOperationProgress::Complete)
    }

    /// Manual retry grants a fresh polling window without forgetting the remote
    /// command. Reissuing a still-running create is unsafe.
    pub fn reset_poll_count(&mut self) {
        self.polls = 0;
    }
}

fn operation_error(resource_id: &str, message: &str) -> ErrorData {
    ErrorData::InfrastructureError {
        message: format!("GCP private ingress: {message}"),
        operation: Some("private-ingress-operation".to_string()),
        resource_id: Some(resource_id.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_client_core::ErrorData as ClientErrorData;
    use alien_gcp_clients::gcp::compute::{
        MockComputeApi, OperationError, OperationErrorItem, OperationStatus,
    };

    fn saved_operation() -> GcpIngressOperation {
        GcpIngressOperation::from_command(Operation {
            name: Some("create-private-frontend".to_string()),
            region: Some("https://compute.googleapis.com/compute/v1/projects/example/regions/us-central1".to_string()),
            ..Default::default()
        }, "api").unwrap()
    }

    #[tokio::test]
    async fn checkpoint_poll_preserves_command_until_completion() {
        let operation = saved_operation();
        let mut restored: GcpIngressOperation =
            serde_json::from_value(serde_json::to_value(&operation).unwrap()).unwrap();
        let mut client = MockComputeApi::new();
        let mut sequence = mockall::Sequence::new();
        client
            .expect_get_region_operation()
            .withf(|region, name| region == "us-central1" && name == "create-private-frontend")
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(Operation {
                    status: Some(OperationStatus::Running),
                    ..Default::default()
                })
            });
        client
            .expect_get_region_operation()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| {
                Ok(Operation {
                    status: Some(OperationStatus::Done),
                    ..Default::default()
                })
            });
        assert_eq!(
            restored.poll(&client).await.unwrap(),
            GcpIngressOperationProgress::Pending
        );
        assert_eq!(restored.polls, 1);
        assert_eq!(
            restored.poll(&client).await.unwrap(),
            GcpIngressOperationProgress::Complete
        );
        assert_eq!(restored.name, operation.name);
    }

    #[tokio::test]
    async fn completed_provider_failure_never_becomes_success() {
        let mut operation = saved_operation();
        let mut client = MockComputeApi::new();
        client
            .expect_get_region_operation()
            .times(1)
            .returning(|_, _| {
                Ok(Operation {
                    status: Some(OperationStatus::Done),
                    error: Some(OperationError {
                        errors: vec![OperationErrorItem {
                            code: Some("RESOURCE_IN_USE".to_string()),
                            ..Default::default()
                        }],
                    }),
                    ..Default::default()
                })
            });
        assert!(matches!(
            operation.poll(&client).await.unwrap(),
            GcpIngressOperationProgress::Failed(_)
        ));
        assert_eq!(operation.name, "create-private-frontend");
    }

    #[tokio::test]
    async fn expired_operation_requires_child_rediscovery() {
        let mut operation = saved_operation();
        let mut client = MockComputeApi::new();
        client
            .expect_get_region_operation()
            .times(1)
            .returning(|_, _| {
                Err(AlienError::new(ClientErrorData::RemoteResourceNotFound {
                    resource_type: "operation".to_string(),
                    resource_name: "expired operation".to_string(),
                }))
            });
        assert_eq!(
            operation.poll(&client).await.unwrap(),
            GcpIngressOperationProgress::Rediscover
        );
        assert_eq!(operation.polls, 0);
    }

    #[tokio::test]
    async fn global_firewall_operation_uses_global_collection() {
        let mut operation = GcpIngressOperation::from_command(
            Operation {
                name: Some("firewall-op".to_string()),
                ..Default::default()
            },
            "api",
        )
        .unwrap();
        let mut client = MockComputeApi::new();
        client
            .expect_get_global_operation()
            .times(1)
            .returning(|_| {
                Ok(Operation {
                    status: Some(OperationStatus::Done),
                    ..Default::default()
                })
            });
        assert_eq!(
            operation.poll(&client).await.unwrap(),
            GcpIngressOperationProgress::Complete
        );
        operation.polls = 180;
        assert!(operation.poll(&client).await.is_err());
        operation.reset_poll_count();
        assert_eq!(operation.polls, 0);
        assert_eq!(operation.name, "firewall-op");
    }
}
