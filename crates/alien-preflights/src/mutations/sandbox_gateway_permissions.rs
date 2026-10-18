//! Exact vault-entry access for the gateway and sandbox-management workloads.

use alien_core::{
    permissions::{PermissionProfile, PermissionSet, PermissionSetReference},
    sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID,
    Build, Container, Daemon, Kv, ResourceRef, Stack, Vault, Worker, SECRETS_VAULT_ID,
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

/// AWS setup can install exact SSM parameter grants before the values exist.
/// Explicit profile entries prevent resource-link defaults from granting access
/// to the whole vault. The gateway's profile is dedicated to that container.
pub(crate) fn configure_aws_gateway_permissions(stack: &mut Stack) -> Result<()> {
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
        if *link.resource_type() == Kv::RESOURCE_TYPE
            && link.id() == super::sandbox_gateway::POLICY_STORE_ID
        {
            gateway_profile.0.insert(
                link.id().to_string(),
                vec![PermissionSetReference::from_name("kv/data-write")],
            );
        } else if *link.resource_type() != Vault::RESOURCE_TYPE || link.id() != SECRETS_VAULT_ID {
            return Err(AlienError::new(invalid(
                "The sandbox gateway may link only its policy KV and config vault",
            )));
        }
    }
    gateway_profile.0.insert(
        SECRETS_VAULT_ID.into(),
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
        return Err(AlienError::new(invalid("The sandbox gateway profile must grant only policy KV access and its exact public config entry")));
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
        if profile.0.iter().any(|(target, sets)| (target == "*" || sandboxes.contains(target)) && sets.iter().any(|set| matches!(set, PermissionSetReference::Name(name) if name == "sandbox/management"))) {
            let grants = profile.0.entry(SECRETS_VAULT_ID.into()).or_default();
            let set = exact_read("sandbox-egress-signing-key")?;
            if !grants.iter().any(|existing| existing.id() == set.id) {
                grants.push(PermissionSetReference::from_inline(set));
            }
        }
    }
    Ok(())
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

/// Even an application profile with stack-wide DynamoDB access cannot read or alter policies.
/// This runs after link grants have authored every workload profile.
pub(crate) fn protect_aws_gateway_policy_store(stack: &mut Stack) -> Result<()> {
    if !alien_core::sandbox_egress::has_sandbox_gateway(stack) {
        return Ok(());
    }
    let guard: PermissionSet = serde_json::from_value(serde_json::json!({
        "id": "kv/protect-sandbox-egress-policies",
        "description": "Only the sandbox gateway workload may access its policy store",
        "platforms": {"aws": [{"effect":"Deny", "grant":{"actions":["dynamodb:*"]},
            "binding":{"resource":{"resources":[
                "arn:aws:dynamodb:${awsRegion}:${awsAccountId}:table/${resourceName}",
                "arn:aws:dynamodb:${awsRegion}:${awsAccountId}:table/${resourceName}/*"
            ]}}}]}
    }))
    .into_alien_error()
    .context(invalid("Failed to construct policy store protection"))?;
    for (name, profile) in &mut stack.permissions.profiles {
        if name == SANDBOX_EGRESS_GATEWAY_ID {
            continue;
        }
        let grants = profile
            .0
            .entry(super::sandbox_gateway::POLICY_STORE_ID.into())
            .or_default();
        grants.retain(|permission| permission.id() != guard.id);
        grants.push(PermissionSetReference::from_inline(guard.clone()));
    }
    Ok(())
}
