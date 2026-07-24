use super::*;

pub(super) fn get_aws_management_role_name(prefix: &str) -> String {
    format!("{}-management", prefix)
}

pub(super) fn emit_aws_remote_stack_management_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    controller: &AwsRemoteStackManagementController,
) -> Result<()> {
    let config = ctx.desired_resource_config::<RemoteStackManagement>()?;

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id: config.id.clone(),
        resource_type: RemoteStackManagement::RESOURCE_TYPE,
        controller_platform: Platform::Aws,
        backend: HeartbeatBackend::Aws,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::RemoteStackManagement(
            RemoteStackManagementHeartbeatData::AwsIamRole(AwsRemoteStackManagementHeartbeatData {
                status: RemoteStackManagementHeartbeatStatus {
                    health: ObservedHealth::Healthy,
                    lifecycle: ProviderLifecycleState::Running,
                    message: controller.role_name.as_ref().map(|role_name| {
                        format!("AWS management role '{}' is reachable", role_name)
                    }),
                    stale: false,
                    partial: false,
                    collection_issues: vec![],
                },
                role_name: controller.role_name.clone(),
                role_arn: controller.role_arn.clone(),
                management_permissions_applied: controller.management_permissions_applied,
            }),
        ),
        raw: vec![],
    });

    Ok(())
}

pub(super) fn sanitize_iam_policy_name(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub(super) fn is_remote_conflict(
    error: &alien_error::AlienError<alien_client_core::ErrorData>,
) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
    )
}

pub(super) fn is_remote_not_found(
    error: &alien_error::AlienError<alien_client_core::ErrorData>,
) -> bool {
    matches!(
        error.error,
        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
    )
}
