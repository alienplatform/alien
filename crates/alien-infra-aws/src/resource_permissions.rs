use alien_core::KubernetesCluster;
use alien_permissions::PermissionContext;

use crate::core::ResourceControllerContext;
use crate::Result;

#[async_trait::async_trait]
pub trait AwsPermissionsService: Send + Sync {
    async fn apply_resource_scoped_permissions(
        &self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
    ) -> Result<()>;

    fn kubernetes_cluster_permission_context(
        &self,
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
    ) -> Result<PermissionContext>;
}

/// Compatibility facade used by AWS controllers while permission orchestration
/// remains owned by the aggregate crate.
pub struct ResourcePermissionsHelper;

impl ResourcePermissionsHelper {
    pub async fn apply_aws_resource_scoped_permissions(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        resource_name: &str,
        resource_type: &str,
    ) -> Result<()> {
        ctx.services
            .require::<dyn AwsPermissionsService>()?
            .apply_resource_scoped_permissions(ctx, resource_id, resource_name, resource_type)
            .await
    }

    pub fn aws_kubernetes_cluster_permission_context(
        ctx: &ResourceControllerContext<'_>,
        cluster: &KubernetesCluster,
    ) -> Result<PermissionContext> {
        ctx.services
            .require::<dyn AwsPermissionsService>()?
            .kubernetes_cluster_permission_context(ctx, cluster)
    }
}
