//! Exact vault-entry access for the gateway and sandbox-management workloads.

use alien_core::{
    permissions::{PermissionProfile, PermissionSet, PermissionSetReference},
    sandbox_egress::{sandbox_egress_vault_id, SANDBOX_EGRESS_GATEWAY_ID},
    Build, Container, Daemon, Platform, ResourceRef, Stack, Vault, Worker, SECRETS_VAULT_ID,
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

/// AWS SSM parameter grants and Azure Key Vault secret-scope role assignments can both be
/// installed before the values exist. Explicit profile entries prevent resource-link defaults
/// from granting access to the whole vault. The gateway's profile is dedicated to that
/// container.
///
/// On AWS the entries live in the secrets vault, which an explicit deny hides from other
/// workloads. Azure has no user-assignable deny and its Workers read the whole secrets vault,
/// so the entries live in a dedicated vault that only these exact grants reach.
pub(crate) fn configure_gateway_permissions(stack: &mut Stack, platform: Platform) -> Result<()> {
    let vault_id = sandbox_egress_vault_id(platform);
    let Some(gateway) = stack
        .resources
        .get(SANDBOX_EGRESS_GATEWAY_ID)
        .and_then(|entry| entry.config.downcast_ref::<Container>())
    else {
        return Ok(());
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
    let mut gateway_profile = PermissionProfile::new();
    for link in &gateway.links {
        if *link.resource_type() != Vault::RESOURCE_TYPE || link.id() != vault_id {
            return Err(AlienError::new(invalid(
                "The sandbox gateway may link only its config vault",
            )));
        }
    }

    gateway_profile.0.insert(
        vault_id.into(),
        vec![PermissionSetReference::from_inline(exact_read(
            "sandbox-egress-gateway-config",
        )?)],
    );
    if stack
        .permissions
        .profiles
        .get(&profile_name)
        .is_some_and(|profile| !profile.0.is_empty() && profile != &gateway_profile)
    {
        return Err(AlienError::new(invalid(
            "The sandbox gateway profile must grant only its exact gateway config entry",
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
    let vault = ResourceRef::new(Vault::RESOURCE_TYPE, vault_id);
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
            let grants = profile.0.entry(vault_id.into()).or_default();
            let set = exact_read("sandbox-egress-signing-key")?;
            if !grants.iter().any(|existing| existing.id() == set.id) {
                grants.push(PermissionSetReference::from_inline(set));
            }
        }
    }
    if platform == Platform::Azure {
        // A stack-wide vault grant is a resource-group role, which would also cover the
        // dedicated vault. Azure cannot carve the vault out of it.
        for (name, profile) in &stack.permissions.profiles {
            if super::secrets_vault::has_stack_wide_vault_grant(profile)
                || profile.0.get(vault_id).is_some_and(|sets| {
                    sets.iter()
                        .any(|set| !set.id().starts_with("vault/sandbox-egress-"))
                })
            {
                return Err(AlienError::new(invalid(format!(
                    "Permission profile '{name}' grants vault access that would include the sandbox gateway vault; grant vault access per vault instead"
                ))));
            }
        }
        // Controllers write the signing key and gateway configuration.
        super::secrets_vault::add_vault_permissions_to_management(stack, vault_id, true, false)?;
    }
    Ok(())
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

/// One vault entry: its SSM parameter on AWS, and its Key Vault secret on Azure. Azure
/// accepts a role assignment scoped to a secret that does not exist yet.
fn exact_read(key: &str) -> Result<PermissionSet> {
    let parameter =
        alien_core::vault_naming::parameter_store_parameter_name("${resourceName}", key);
    let secret = alien_core::vault_naming::key_vault_secret_name(key);
    serde_json::from_value(serde_json::json!({
        "id": format!("vault/{key}-read"),
        "description": format!("Read only the {key} vault entry"),
        "platforms": {
            "aws": [{
                "grant": {"actions": ["ssm:GetParameter"]},
                "binding": {"resource": {"resources": [format!("arn:aws:ssm:${{awsRegion}}:${{awsAccountId}}:parameter/{parameter}")]}}
            }],
            "azure": [{
                "grant": {"predefinedRoles": ["Key Vault Secrets User"]},
                "binding": {"resource": {"scope": format!("/subscriptions/${{subscriptionId}}/resourceGroups/${{resourceGroup}}/providers/Microsoft.KeyVault/vaults/${{resourceName}}/secrets/{secret}")}}
            }]
        }
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
