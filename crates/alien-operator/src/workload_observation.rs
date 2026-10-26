use alien_core::{
    Container, DeploymentState, DeploymentStatus, Platform, ResourceStatus, StackResourceState,
};

pub(crate) fn is_settled(state: &DeploymentState) -> bool {
    state.platform == Platform::Kubernetes
        && matches!(
            state.status,
            DeploymentStatus::Running | DeploymentStatus::RefreshFailed
        )
        && state.target_release.as_ref().is_none_or(|target| {
            target.release_id.is_some() && Some(target) == state.current_release.as_ref()
        })
        && state.current_release.is_some()
}

/// Identity used by the passive reader, excluding health-check bookkeeping.
pub(crate) fn workload_identity(resource: &StackResourceState) -> Option<(&str, &str, bool)> {
    if resource.resource_type != Container::RESOURCE_TYPE.as_ref()
        || !matches!(
            resource.status,
            ResourceStatus::Running | ResourceStatus::RefreshFailed
        )
        || resource
            .controller_platform
            .is_some_and(|platform| platform != Platform::Kubernetes)
    {
        return None;
    }
    let internal = resource.internal_state.as_ref()?;
    if internal.get("type")?.as_str()? != "KubernetesContainerController" {
        return None;
    }
    Some((
        internal.get("namespace")?.as_str()?,
        internal.get("workloadName")?.as_str()?,
        internal.get("isStateful")?.as_bool()?,
    ))
}
