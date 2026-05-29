use alien_client_core::{ErrorData, Result};
use alien_core::HeartbeatCollectionIssueReason;
use alien_error::AlienError;
use std::future::Future;
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalKubernetesReadSource {
    MetricsApi,
    Events,
    Nodes,
}

impl OptionalKubernetesReadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MetricsApi => "metricsApi",
            Self::Events => "events",
            Self::Nodes => "nodes",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionalKubernetesReadStatus {
    pub available: bool,
    pub reason: Option<HeartbeatCollectionIssueReason>,
    pub message: Option<String>,
}

impl OptionalKubernetesReadStatus {
    pub fn available() -> Self {
        Self {
            available: true,
            reason: None,
            message: None,
        }
    }

    pub fn unavailable(reason: HeartbeatCollectionIssueReason, message: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason),
            message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OptionalKubernetesReadContext<'a> {
    pub source: OptionalKubernetesReadSource,
    pub resource_id: &'a str,
    pub namespace: Option<&'a str>,
    pub kubernetes_resource: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionalKubernetesRead<T> {
    pub value: Option<T>,
    pub status: OptionalKubernetesReadStatus,
}

impl<T> OptionalKubernetesRead<T> {
    fn available(value: T) -> Self {
        Self {
            value: Some(value),
            status: OptionalKubernetesReadStatus::available(),
        }
    }

    fn unavailable(reason: HeartbeatCollectionIssueReason, message: String) -> Self {
        Self {
            value: None,
            status: OptionalKubernetesReadStatus::unavailable(reason, message),
        }
    }
}

pub async fn optional_kubernetes_read<T, F>(
    context: OptionalKubernetesReadContext<'_>,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: Future<Output = Result<T>>,
{
    match read.await {
        Ok(value) => Ok(OptionalKubernetesRead::available(value)),
        Err(error) => {
            if let Some((reason, message, log_at_info)) =
                classify_optional_kubernetes_error(context.source, &error)
            {
                if log_at_info {
                    info!(
                        source = context.source.as_str(),
                        resource_id = context.resource_id,
                        namespace = context.namespace,
                        kubernetes_resource = context.kubernetes_resource,
                        reason = ?reason,
                        error = %error,
                        "Optional Kubernetes heartbeat collection unavailable"
                    );
                } else {
                    debug!(
                        source = context.source.as_str(),
                        resource_id = context.resource_id,
                        namespace = context.namespace,
                        kubernetes_resource = context.kubernetes_resource,
                        reason = ?reason,
                        error = %error,
                        "Optional Kubernetes heartbeat collection unavailable"
                    );
                }

                Ok(OptionalKubernetesRead::unavailable(reason, message))
            } else {
                Err(error)
            }
        }
    }
}

pub async fn optional_metrics_read<T, F>(
    resource_id: &str,
    namespace: Option<&str>,
    kubernetes_resource: Option<&str>,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: Future<Output = Result<T>>,
{
    optional_kubernetes_read(
        OptionalKubernetesReadContext {
            source: OptionalKubernetesReadSource::MetricsApi,
            resource_id,
            namespace,
            kubernetes_resource,
        },
        read,
    )
    .await
}

pub async fn optional_events_read<T, F>(
    resource_id: &str,
    namespace: &str,
    kubernetes_resource: Option<&str>,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: Future<Output = Result<T>>,
{
    optional_kubernetes_read(
        OptionalKubernetesReadContext {
            source: OptionalKubernetesReadSource::Events,
            resource_id,
            namespace: Some(namespace),
            kubernetes_resource,
        },
        read,
    )
    .await
}

pub async fn optional_nodes_read<T, F>(
    resource_id: &str,
    read: F,
) -> Result<OptionalKubernetesRead<T>>
where
    F: Future<Output = Result<T>>,
{
    optional_kubernetes_read(
        OptionalKubernetesReadContext {
            source: OptionalKubernetesReadSource::Nodes,
            resource_id,
            namespace: None,
            kubernetes_resource: None,
        },
        read,
    )
    .await
}

fn classify_optional_kubernetes_error(
    source: OptionalKubernetesReadSource,
    error: &AlienError<ErrorData>,
) -> Option<(HeartbeatCollectionIssueReason, String, bool)> {
    let error_data = error.error.as_ref()?;
    match error_data {
        ErrorData::RemoteAccessDenied { .. } => Some((
            HeartbeatCollectionIssueReason::Forbidden,
            format!(
                "Kubernetes {} collection is forbidden by RBAC",
                source.as_str()
            ),
            true,
        )),
        ErrorData::RemoteResourceNotFound { .. }
            if matches!(source, OptionalKubernetesReadSource::MetricsApi) =>
        {
            Some((
                HeartbeatCollectionIssueReason::NotInstalled,
                "Kubernetes metrics API is not installed".to_string(),
                false,
            ))
        }
        ErrorData::RemoteServiceUnavailable { .. } => Some((
            HeartbeatCollectionIssueReason::ApiUnavailable,
            format!(
                "Kubernetes {} API is temporarily unavailable",
                source.as_str()
            ),
            false,
        )),
        ErrorData::Timeout { .. } => Some((
            HeartbeatCollectionIssueReason::TimedOut,
            format!("Kubernetes {} collection timed out", source.as_str()),
            false,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fail(error_data: ErrorData) -> Result<()> {
        Err(AlienError::new(error_data))
    }

    #[tokio::test]
    async fn metrics_not_found_becomes_not_installed_source() {
        let result = optional_metrics_read(
            "api",
            Some("default"),
            Some("api"),
            fail(ErrorData::RemoteResourceNotFound {
                resource_type: "apis".to_string(),
                resource_name: "metrics.k8s.io".to_string(),
            }),
        )
        .await
        .unwrap();

        assert!(result.value.is_none());
        assert!(!result.status.available);
        assert_eq!(
            result.status.reason,
            Some(HeartbeatCollectionIssueReason::NotInstalled)
        );
    }

    #[tokio::test]
    async fn forbidden_optional_reads_become_forbidden_sources() {
        for source in [
            OptionalKubernetesReadSource::MetricsApi,
            OptionalKubernetesReadSource::Events,
            OptionalKubernetesReadSource::Nodes,
        ] {
            let result = optional_kubernetes_read(
                OptionalKubernetesReadContext {
                    source,
                    resource_id: "api",
                    namespace: Some("default"),
                    kubernetes_resource: Some("api"),
                },
                fail(ErrorData::RemoteAccessDenied {
                    resource_type: source.as_str().to_string(),
                    resource_name: "api".to_string(),
                }),
            )
            .await
            .unwrap();

            assert!(result.value.is_none());
            assert_eq!(
                result.status.reason,
                Some(HeartbeatCollectionIssueReason::Forbidden)
            );
        }
    }

    #[tokio::test]
    async fn unexpected_optional_errors_remain_failures() {
        let result = optional_events_read(
            "api",
            "default",
            Some("api"),
            fail(ErrorData::InvalidInput {
                message: "bad selector".to_string(),
                field_name: Some("fieldSelector".to_string()),
            }),
        )
        .await;

        assert!(result.is_err());
    }
}
