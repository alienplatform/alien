//! Setup drift: whether a setup run on an installed deployment would still change something that
//! only setup may write (the management identity's grants, a VPC flow log), judged from state
//! alone.
//!
//! The comparison is by content. A permission set whose body changed under an unchanged id
//! changes the management permissions revision this binary would record, which is how the
//! earlier by-name comparison missed it. Drift is **recommended**: callers show it and offer the
//! setup update; none pauses a release for it.

use std::collections::BTreeMap;

use alien_core::{
    InitialSetupAuthority, Platform, RemoteStackManagement, SetupDriftCode, SetupDriftReason,
    Stack, StackState,
};

use crate::core::state_utils::StackResourceStateExt;
use crate::error::Result;
use crate::remote_stack_management::{
    management_permission_set_digests, management_permissions_revision_of,
};

/// What a setup run with this binary would change on an installed deployment, as reasons.
///
/// `installed_stack` is the stack the deployment was last prepared with, and `state` its
/// recorded resource state. An empty list means setup is current.
///
/// - Direct setup (this CLI wrote the grants): content drift of the management permissions and a
///   missing flow log. A recorded denial is listed only next to one of those: rerunning setup
///   with the grants already current cannot lift it, so on its own it is not a reason to run
///   setup.
/// - Any other setup method (a template engine wrote the grants, so the CLI cannot read what is
///   installed): recorded denials only.
pub fn setup_drift(
    installed_stack: &Stack,
    state: &StackState,
    platform: Platform,
    authority: InitialSetupAuthority,
) -> Result<Vec<SetupDriftReason>> {
    let denials = recorded_denials(state);
    if authority != InitialSetupAuthority::DirectSetup {
        return Ok(denials);
    }
    let mut reasons = management_permissions_drift(installed_stack, state, platform)?;
    reasons.extend(missing_flow_logs(installed_stack, state, platform));
    if !reasons.is_empty() {
        reasons.extend(denials);
    }
    Ok(reasons)
}

/// The management permissions this binary would install, against what the install recorded.
fn management_permissions_drift(
    stack: &Stack,
    state: &StackState,
    platform: Platform,
) -> Result<Vec<SetupDriftReason>> {
    let Some(target) = management_permissions_revision_of(stack, platform, state)? else {
        return Ok(vec![]);
    };
    let Some((resource_id, management)) = state.resources.iter().find(|(_, resource)| {
        resource.resource_type == RemoteStackManagement::RESOURCE_TYPE.to_string()
    }) else {
        return Ok(vec![]);
    };
    let Some(recorded) = RecordedManagement::read(management, platform) else {
        return Ok(vec![]);
    };
    if recorded.revision.as_deref() == Some(target.as_str()) {
        return Ok(vec![]);
    }
    let target_digests = management_permission_set_digests(stack)?;
    let changed: Vec<&String> = target_digests
        .iter()
        .filter(|(id, digest)| recorded.set_digests.get(*id) != Some(*digest))
        .map(|(id, _)| id)
        .collect();
    let named_sets = !recorded.set_digests.is_empty() && !changed.is_empty();
    if recorded.revision.is_none() {
        return Ok(vec![SetupDriftReason {
            code: SetupDriftCode::ManagementPermissionsUnrecorded,
            resource: Some(resource_id.clone()),
            detail: "The installed management permissions were never recorded; setup records them"
                .to_string(),
        }]);
    }
    if named_sets {
        return Ok(changed
            .into_iter()
            .map(|id| SetupDriftReason {
                code: SetupDriftCode::ManagementPermissionsChanged,
                resource: Some(id.clone()),
                detail: format!("Management permission set '{id}' has changed"),
            })
            .collect());
    }
    // A recorded revision without per-set digests (an install from before they were recorded),
    // or a network change that moved the revision with no set changed.
    Ok(vec![SetupDriftReason {
        code: SetupDriftCode::ManagementPermissionsChanged,
        resource: Some(resource_id.clone()),
        detail: "The management permissions differ from what this version installs".to_string(),
    }])
}

/// What an install recorded about the management permissions it applied.
struct RecordedManagement {
    revision: Option<String>,
    set_digests: BTreeMap<String, String>,
}

impl RecordedManagement {
    fn read(state: &alien_core::StackResourceState, platform: Platform) -> Option<Self> {
        match platform {
            Platform::Aws => {
                let controller = state
                    .get_internal_controller_typed::<crate::AwsRemoteStackManagementController>()
                    .ok()?;
                Some(Self {
                    revision: controller.management_permissions_revision,
                    set_digests: controller.management_permission_set_digests,
                })
            }
            Platform::Gcp => {
                let controller = state
                    .get_internal_controller_typed::<crate::GcpRemoteStackManagementController>()
                    .ok()?;
                Some(Self {
                    revision: controller.management_permissions_revision,
                    set_digests: BTreeMap::new(),
                })
            }
            Platform::Azure => {
                let controller = state
                    .get_internal_controller_typed::<crate::AzureRemoteStackManagementController>()
                    .ok()?;
                Some(Self {
                    revision: controller.management_permissions_revision,
                    set_digests: BTreeMap::new(),
                })
            }
            _ => None,
        }
    }
}

/// An Alien-created VPC with no complete flow log. Only a setup run adds it.
#[cfg(feature = "aws")]
fn missing_flow_logs(
    stack: &Stack,
    state: &StackState,
    platform: Platform,
) -> Vec<SetupDriftReason> {
    use alien_core::{Network, NetworkSettings, ResourceStatus};

    if platform != Platform::Aws {
        return vec![];
    }
    stack
        .resources
        .iter()
        .filter_map(|(id, entry)| entry.config.downcast_ref::<Network>().map(|n| (id, n)))
        .filter(|(_, network)| matches!(network.settings, NetworkSettings::Create { .. }))
        .filter(|(id, _)| {
            state.resources.get(*id).is_some_and(|resource| {
                resource.status == ResourceStatus::Running
                    && resource
                        .get_internal_controller_typed::<crate::network::AwsNetworkController>()
                        .is_ok_and(|controller| controller.flow_log_absent())
            })
        })
        .map(|(id, _)| SetupDriftReason {
            code: SetupDriftCode::FlowLogMissing,
            resource: Some(id.clone()),
            detail: "The VPC has no flow log; setup adds one".to_string(),
        })
        .collect()
}

#[cfg(not(feature = "aws"))]
fn missing_flow_logs(_: &Stack, _: &StackState, _: Platform) -> Vec<SetupDriftReason> {
    vec![]
}

/// Denials a resource recorded while creating something setup's grant covers.
///
/// Read from the controller's recorded state without naming its type: CloudWatch alarms keep
/// `alarms.denied` (a message) and DynamoDB point-in-time recovery keeps
/// `pointInTimeRecoveryDenial`.
fn recorded_denials(state: &StackState) -> Vec<SetupDriftReason> {
    let mut reasons = Vec::new();
    for (id, resource) in &state.resources {
        let Some(internal) = resource.internal_state.as_ref() else {
            continue;
        };
        if internal
            .pointer("/alarms/denied")
            .and_then(|denied| denied.as_str())
            .is_some_and(|denied| !denied.is_empty())
        {
            reasons.push(SetupDriftReason {
                code: SetupDriftCode::GrantDenied,
                resource: Some(id.clone()),
                detail: "CloudWatch alarms were denied by the management permissions".to_string(),
            });
        }
        if internal
            .get("pointInTimeRecoveryDenial")
            .is_some_and(|denial| !denial.is_null())
        {
            reasons.push(SetupDriftReason {
                code: SetupDriftCode::GrantDenied,
                resource: Some(id.clone()),
                detail: "Point-in-time recovery was denied by the management permissions"
                    .to_string(),
            });
        }
    }
    reasons
}

#[cfg(test)]
mod tests;
