//! Exact vault-entry access for the gateway and sandbox-management workloads.

use alien_core::{
    permissions::{
        ManagementPermissions, PermissionProfile, PermissionSet, PermissionSetReference,
    },
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
    Build, Container, Daemon, RemoteStackManagement, ResourceRef, Stack, Vault, Worker,
    SECRETS_VAULT_ID,
};
use alien_error::{AlienError, Context, IntoAlienError};

use crate::error::{ErrorData, Result};

fn invalid(message: impl Into<String>) -> ErrorData {
    ErrorData::StackMutationFailed {
        mutation_name: "SandboxGatewayPermissions".into(),
        message: message.into(),
        resource_id: Some(SANDBOX_EGRESS_GATEWAY_ID.into()),
    }
}

/// The gateway's profile name, after refusing any other workload that shares it.
fn dedicated_gateway_profile(stack: &Stack) -> Result<Option<String>> {
    let Some(gateway) = stack
        .resources
        .get(SANDBOX_EGRESS_GATEWAY_ID)
        .and_then(|entry| entry.config.downcast_ref::<Container>())
    else {
        return Ok(None);
    };
    let profile_name = gateway.permissions.clone();
    if stack.resources().any(|(id, entry)| {
        let profile = entry
            .config
            .downcast_ref::<Container>()
            .map(|resource| &resource.permissions)
            .or_else(|| {
                entry
                    .config
                    .downcast_ref::<Worker>()
                    .map(|resource| &resource.permissions)
            })
            .or_else(|| {
                entry
                    .config
                    .downcast_ref::<Build>()
                    .map(|resource| &resource.permissions)
            })
            .or_else(|| {
                entry
                    .config
                    .downcast_ref::<Daemon>()
                    .and_then(|resource| resource.permissions.as_ref())
            });
        id != SANDBOX_EGRESS_GATEWAY_ID && profile == Some(&profile_name)
    }) {
        return Err(AlienError::new(invalid(
            "The sandbox gateway requires a dedicated permission profile",
        )));
    }
    Ok(Some(profile_name))
}

/// On Kubernetes the controller projects the gateway configuration into the workload's
/// own Secret, so the gateway pod gets a service account with no data permissions.
pub(crate) fn configure_kubernetes_gateway_permissions(stack: &mut Stack) -> Result<()> {
    let Some(profile_name) = dedicated_gateway_profile(stack)? else {
        return Ok(());
    };
    let gateway = stack.resources[SANDBOX_EGRESS_GATEWAY_ID]
        .config
        .downcast_ref::<Container>()
        .expect("checked above");
    if !gateway.links.is_empty() {
        return Err(AlienError::new(invalid(
            "The Kubernetes sandbox gateway must not link other resources",
        )));
    }
    match stack.permissions.profiles.get(&profile_name) {
        Some(profile) if !profile.0.is_empty() => Err(AlienError::new(invalid(
            "The Kubernetes sandbox gateway profile must not grant permissions",
        ))),
        Some(_) => Ok(()),
        None => {
            stack
                .permissions
                .profiles
                .insert(profile_name, PermissionProfile::new());
            Ok(())
        }
    }
}

/// AWS setup can install exact SSM parameter grants before the values exist.
/// Explicit profile entries prevent resource-link defaults from granting access
/// to the whole vault. The gateway's profile is dedicated to that container.
pub(crate) fn configure_aws_gateway_permissions(stack: &mut Stack) -> Result<()> {
    let Some(profile_name) = dedicated_gateway_profile(stack)? else {
        return Ok(());
    };
    let gateway = stack.resources[SANDBOX_EGRESS_GATEWAY_ID]
        .config
        .downcast_ref::<Container>()
        .expect("checked above");
    let mut gateway_profile = PermissionProfile::new();
    for link in &gateway.links {
        if *link.resource_type() != Vault::RESOURCE_TYPE || link.id() != SECRETS_VAULT_ID {
            return Err(AlienError::new(invalid(
                "The sandbox gateway may link only its config vault",
            )));
        }
    }

    gateway_profile.0.insert(
        SECRETS_VAULT_ID.into(),
        vec![PermissionSetReference::from_inline(exact_read(
            "sandbox-egress-gateway-config",
        )?)],
    );
    gateway_profile.0.insert(
        "*".into(),
        vec![PermissionSetReference::from_inline(replica_secret_read()?)],
    );
    if stack
        .permissions
        .profiles
        .get(&profile_name)
        .is_some_and(|profile| !profile.0.is_empty() && profile != &gateway_profile)
    {
        return Err(AlienError::new(invalid(
            "The sandbox gateway profile must grant only its exact gateway config entry and replica secret",
        )));
    }
    stack
        .permissions
        .profiles
        .insert(profile_name, gateway_profile);
    let gateway = stack
        .resources
        .get_mut(SANDBOX_EGRESS_GATEWAY_ID)
        .unwrap()
        .config
        .downcast_mut::<Container>()
        .unwrap();
    let vault = ResourceRef::new(Vault::RESOURCE_TYPE, SECRETS_VAULT_ID);
    if !gateway.links.contains(&vault) {
        gateway.links.push(vault);
    }

    let sandboxes: std::collections::HashSet<_> = stack
        .resources()
        .filter_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<alien_core::Sandbox>()
                .filter(|sandbox| matches!(sandbox.egress, alien_core::SandboxEgress::Internet))
                .map(|_| id.clone())
        })
        .collect();
    for profile in stack.permissions.profiles.values_mut() {
        if profile.0.iter().any(|(target, sets)| {
            (target == "*" || sandboxes.contains(target))
                && sets.iter().any(grants_sandbox_management)
        }) {
            let grants = profile.0.entry(SECRETS_VAULT_ID.into()).or_default();
            let set = exact_read("sandbox-egress-signing-key")?;
            if !grants.iter().any(|existing| existing.id() == set.id) {
                grants.push(PermissionSetReference::from_inline(set));
            }
        }
    }
    Ok(())
}

/// The managing identity's only read in the AWS secrets vault: the gateway configuration. The
/// shared-key owner reads it back only when the configuration it publishes changed, to carry the
/// gateway's replica secret into the new value. Everything else in the vault, the signing key
/// included, management writes without reading.
pub(crate) fn grant_management_gateway_config_read(stack: &mut Stack) -> Result<()> {
    if !alien_core::sandbox_egress::has_sandbox_gateway(stack)
        || !stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
    {
        return Ok(());
    }
    let read = management_gateway_config_read()?;
    let profile = match &mut stack.permissions.management {
        // An override is the author's complete grant; nothing is added to it.
        ManagementPermissions::Override(_) => return Ok(()),
        ManagementPermissions::Auto => {
            stack.permissions.management = ManagementPermissions::Extend(PermissionProfile::new());
            match &mut stack.permissions.management {
                ManagementPermissions::Extend(profile) => profile,
                _ => unreachable!("just set to Extend"),
            }
        }
        ManagementPermissions::Extend(profile) => profile,
    };
    let grants = profile.0.entry(SECRETS_VAULT_ID.into()).or_default();
    // An author's grant cannot take the read's place by reusing its id.
    grants.retain(|existing| existing.id() != read.id());
    grants.push(read);
    Ok(())
}

/// The managing identity's exact read of the gateway configuration entry.
pub(crate) fn management_gateway_config_read() -> Result<PermissionSetReference> {
    Ok(PermissionSetReference::from_inline(exact_read(
        "sandbox-egress-gateway-config",
    )?))
}

/// Only the built-in management grant carries the signing key. An inline set counts only
/// when its contents equal the built-in set: an ID alone can be reused for a narrower,
/// deliberately restricted grant that must not be able to sign policy writes.
fn grants_sandbox_management(set: &PermissionSetReference) -> bool {
    match set {
        PermissionSetReference::Name(name) => name == "sandbox/management",
        PermissionSetReference::Inline(inline) => {
            alien_permissions::get_permission_set("sandbox/management") == Some(inline)
        }
    }
}

/// The gateway reads its replica secret, which setup generates in Secrets Manager.
pub(crate) fn replica_secret_read() -> Result<PermissionSet> {
    let arn = format!(
        "arn:aws:secretsmanager:${{awsRegion}}:${{awsAccountId}}:secret:${{stackPrefix}}-{}-??????",
        alien_core::sandbox_egress::SANDBOX_EGRESS_REPLICA_SECRET
    );
    serde_json::from_value(serde_json::json!({
        "id": "sandbox/egress-replica-secret-read",
        "description": "Read the sandbox gateway's replica secret",
        "platforms": {"aws": [{
            "grant": {"actions": ["secretsmanager:GetSecretValue"]},
            "binding": {"stack": {"resources": [arn]}, "resource": {"resources": [arn]}}
        }]}
    }))
    .into_alien_error()
    .context(invalid("Failed to construct the sandbox gateway replica secret grant"))
}

fn exact_read(key: &str) -> Result<PermissionSet> {
    let parameter =
        alien_core::vault_naming::parameter_store_parameter_name("${resourceName}", key);
    serde_json::from_value(serde_json::json!({
        "id": format!("vault/{key}-read"),
        "description": format!("Read only the {key} vault entry"),
        "platforms": {"aws": [{
            "grant": {"actions": ["ssm:GetParameter"]},
            "binding": {"resource": {"resources": [format!("arn:aws:ssm:${{awsRegion}}:${{awsAccountId}}:parameter/{parameter}")]}}
        }]}
    })).into_alien_error().context(invalid("Failed to construct exact sandbox vault-entry permissions"))
}

/// The replica secret and derived credential CA belong only to the gateway. Application
/// profiles may read other vault entries, including the SDK's separate signing key grant.
/// Run after link grants so a global vault grant cannot expose the gateway configuration.
pub(crate) fn protect_aws_gateway_config(stack: &mut Stack) -> Result<()> {
    if !alien_core::sandbox_egress::has_sandbox_gateway(stack) {
        return Ok(());
    }
    let parameter = alien_core::vault_naming::parameter_store_parameter_name(
        "${resourceName}",
        "sandbox-egress-gateway-config",
    );
    let guard: PermissionSet = serde_json::from_value(serde_json::json!({
        "id": "vault/protect-sandbox-gateway-config",
        "description": "Only the gateway workload may access its replica secret and configuration",
        "platforms": {"aws": [{"effect":"Deny", "grant":{"actions":["ssm:*"]},
            "binding":{"resource":{"resources":[format!("arn:aws:ssm:${{awsRegion}}:${{awsAccountId}}:parameter/{parameter}")]}}
        }]}
    })).into_alien_error().context(invalid("Failed to construct gateway configuration protection"))?;
    for (name, profile) in &mut stack.permissions.profiles {
        if name == SANDBOX_EGRESS_GATEWAY_ID {
            continue;
        }
        let grants = profile.0.entry(SECRETS_VAULT_ID.into()).or_default();
        grants.retain(|permission| permission.id() != guard.id);
        grants.push(PermissionSetReference::from_inline(guard.clone()));
    }
    Ok(())
}
