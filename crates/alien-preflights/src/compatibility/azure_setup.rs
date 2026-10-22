use crate::error::{ErrorData, Result};
use alien_core::{RemoteBindings, ServiceAccount, Stack};
use alien_error::{AlienError, Context};
use alien_permissions::generators::azure_setup_owned::{
    azure_runtime_roles, azure_setup_update_digest,
};
use std::collections::BTreeSet;

pub(crate) fn authorization_matches(
    installed: &Stack,
    desired: &Stack,
    authorization: &alien_core::SetupUpdateAuthorization,
) -> Result<bool> {
    let digest = |stack| {
        azure_setup_update_digest(stack).context(ErrorData::StackCompatibilityCheckFailed {
            check_name: "Azure setup registration".into(),
            message: "Could not compare the registered Azure setup".into(),
            old_resource_id: None,
            new_resource_id: None,
        })
    };
    Ok(digest(installed)? == authorization.baseline_frozen_digest
        && digest(desired)? == authorization.target_frozen_digest)
}

/// Automatic runtime may change application infrastructure, but Azure setup owns
/// the identities and role definitions named in its role-assignment conditions.
pub(crate) fn require_unchanged_setup(installed: &Stack, desired: &Stack) -> Result<()> {
    let identities: BTreeSet<_> = installed
        .resources()
        .chain(desired.resources())
        .filter(|(_, entry)| {
            let kind = entry.config.resource_type();
            kind == ServiceAccount::RESOURCE_TYPE || kind == RemoteBindings::RESOURCE_TYPE
        })
        .map(|(id, _)| id)
        .collect();
    let mut changes: Vec<String> = identities
        .into_iter()
        .filter(|id| {
            installed.resources.get(*id).map(|entry| &entry.config)
                != desired.resources.get(*id).map(|entry| &entry.config)
        })
        .map(|id| format!("identity '{id}' changed"))
        .collect();

    let roles = |stack| {
        azure_runtime_roles(stack).context(ErrorData::StackCompatibilityCheckFailed {
            check_name: "Azure setup identities and roles".into(),
            message: "Could not compare the roles installed by setup".into(),
            old_resource_id: None,
            new_resource_id: None,
        })
    };
    let installed_roles = roles(installed)?;
    let desired_roles = roles(desired)?;
    if installed_roles.management_predefined_role_ids
        != desired_roles.management_predefined_role_ids
        || installed_roles.workload_predefined_role_ids
            != desired_roles.workload_predefined_role_ids
        || installed_roles.custom_roles.len() != desired_roles.custom_roles.len()
        || desired_roles
            .custom_roles
            .iter()
            .any(|role| !installed_roles.custom_roles.contains(role))
    {
        changes.push("runtime role definitions or allowed role ids changed".into());
    }
    if changes.is_empty() {
        return Ok(());
    }
    Err(AlienError::new(ErrorData::SetupRequired {
        message: format!(
            "Azure setup must apply these changes before the automatic update: {}. Rerun setup with the desired release and register its results",
            changes.join("; ")
        ),
    }))
}
