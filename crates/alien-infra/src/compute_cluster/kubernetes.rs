//! Verify logical compute pools within an existing Kubernetes namespace.
//! Node capacity belongs to the cluster administrator; this controller never
//! provisions nodes and deliberately emits no cloud fleet outputs.

use std::time::Duration;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::{
    validate_kubernetes_compute, ComputeCluster, KubernetesClientConfig, ResourceStatus,
};
use alien_error::{AlienError, Context};
use alien_macros::controller;

#[controller]
pub struct KubernetesComputeClusterController {}

#[controller]
impl KubernetesComputeClusterController {
    #[flow_entry(Create)]
    #[handler(state = VerifyNamespace, on_failure = ProvisionFailed, status = ResourceStatus::Provisioning)]
    async fn verify_namespace(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        verify(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        verify(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = VerifyUpdate, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn verify_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        verify(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = PreserveNamespace, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn preserve_namespace(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        // Namespace and nodes are not owned by this declaration.
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(
        state = ProvisionFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    #[handler(state = RefreshFailed, on_failure = RefreshFailed, status = ResourceStatus::RefreshFailed)]
    async fn retry_namespace_verification(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        verify(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }
}

async fn verify(ctx: &ResourceControllerContext<'_>) -> Result<()> {
    let cluster = ctx.desired_resource_config::<ComputeCluster>()?;
    let errors = validate_kubernetes_compute(ctx.desired_stack);
    if !errors.is_empty() {
        return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: cluster.id.clone(),
            message: errors.join("; "),
        }));
    }
    let config = ctx.get_kubernetes_config()?;
    let namespace = match config {
        KubernetesClientConfig::InCluster { namespace, .. }
        | KubernetesClientConfig::Kubeconfig { namespace, .. }
        | KubernetesClientConfig::Manual { namespace, .. } => namespace,
    }
    .as_deref()
    .filter(|namespace| !namespace.trim().is_empty())
    .ok_or_else(|| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: cluster.id.clone(),
            message: "Compute pools require a Kubernetes namespace".to_string(),
        })
    })?;
    // Existing workload RBAC already allows this namespaced read. Reading Node
    // or Namespace objects would require additional cluster-wide permissions.
    ctx.service_provider
        .get_kubernetes_deployment_client(config)
        .await?
        .list_deployments(
            namespace,
            None,
            Some("metadata.name=alien-compute-access-check".to_string()),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            resource_id: Some(cluster.id.clone()),
            message: format!("Failed to verify compute pool access in namespace '{namespace}'"),
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use alien_core::{CapacityGroup, ClientConfig, Platform, ResourceLifecycle};
    use alien_k8s_clients::kubernetes::deployments::MockDeploymentApi;
    use std::sync::Arc;

    fn cluster() -> ComputeCluster {
        ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: None,
                profile: None,
                min_size: 1,
                max_size: 3,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build()
    }

    fn client_config(namespace: Option<&str>) -> ClientConfig {
        ClientConfig::Kubernetes(Box::new(KubernetesClientConfig::InCluster {
            namespace: namespace.map(str::to_string),
            additional_headers: None,
        }))
    }

    #[tokio::test]
    async fn verifies_capacity_boundary_without_owning_or_deleting_workloads() {
        let mut deployments = MockDeploymentApi::new();
        deployments
            .expect_list_deployments()
            .withf(|namespace, label, field| {
                namespace == "application"
                    && label.is_none()
                    && field.as_deref() == Some("metadata.name=alien-compute-access-check")
            })
            .times(3)
            .returning(|_, _, _| Ok(Default::default()));
        // No mutating API is expected. Creation, refresh, and updates observe
        // existing capacity; removing the declaration preserves that capacity.
        let deployments = Arc::new(deployments);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_kubernetes_deployment_client()
            .times(3)
            .returning(move |_| Ok(deployments.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(cluster())
            .resource_lifecycle(ResourceLifecycle::Frozen)
            .controller(KubernetesComputeClusterController::default())
            .platform(Platform::Kubernetes)
            .client_config(client_config(Some("application")))
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(executor.outputs().is_none());
        executor.step().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        let mut updated = cluster();
        updated.capacity_groups[0].max_size = 5;
        executor.update(updated).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
        assert!(executor.outputs().is_none());
        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn namespace_access_denial_cannot_report_capacity_ready() {
        let mut deployments = MockDeploymentApi::new();
        deployments
            .expect_list_deployments()
            .times(1)
            .returning(|_, _, _| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteAccessDenied {
                        resource_type: "Deployment".to_string(),
                        resource_name: "application".to_string(),
                    },
                ))
            });
        let deployments = Arc::new(deployments);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_kubernetes_deployment_client()
            .times(1)
            .returning(move |_| Ok(deployments.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(cluster())
            .controller(KubernetesComputeClusterController::default())
            .platform(Platform::Kubernetes)
            .client_config(client_config(Some("application")))
            .service_provider(Arc::new(provider))
            .build()
            .await
            .unwrap();
        let error = executor.run_until_terminal().await.unwrap_err();
        assert!(error.to_string().contains("application"));
        assert_ne!(executor.status(), ResourceStatus::Running);
        assert!(executor.outputs().is_none());
    }

    #[tokio::test]
    async fn invalid_requirements_and_missing_namespace_fail_before_remote_access() {
        for (invalid_requirements, namespace) in [
            (true, Some("application")),
            (false, None),
            (false, Some("")),
            (false, Some(" ")),
        ] {
            let mut resource = cluster();
            resource.capacity_groups[0].nested_virtualization = Some(invalid_requirements);
            // An unexpected client request fails the test. Validation must
            // reject these declarations before contacting Kubernetes.
            let mut executor = SingleControllerExecutor::builder()
                .resource(resource)
                .controller(KubernetesComputeClusterController::default())
                .platform(Platform::Kubernetes)
                .client_config(client_config(namespace))
                .service_provider(Arc::new(MockPlatformServiceProvider::new()))
                .build()
                .await
                .unwrap();
            let error = executor.run_until_terminal().await.unwrap_err();
            assert!(error.to_string().contains(if invalid_requirements {
                "nested virtualization"
            } else {
                "namespace"
            }));
            assert_ne!(executor.status(), ResourceStatus::Running);
            assert!(executor.outputs().is_none());
        }
    }
}
