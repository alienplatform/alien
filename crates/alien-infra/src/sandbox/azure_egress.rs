//! Verify setup-owned networking before publishing the shared gateway config.

use alien_core::{import::data::AzureSandboxNetwork, sandbox_egress};
use alien_error::{AlienError, Context};

use super::{egress, AzureSandboxController};
use crate::{
    core::{state_utils::StackResourceStateExt, ResourceControllerContext},
    network::{validate_azure_sandbox_network, AzureNetworkController},
    ErrorData, Result,
};

const GATEWAY_ADMIN_PORT: u16 = 3129;

/// The setup-created gateway VMs every policy write must reach. Empty means setup predates them.
fn gateway_addresses(ctx: &ResourceControllerContext<'_>) -> Result<Vec<String>> {
    let (_, network) = network_controller(ctx)?;
    let ips = network
        .sandbox_network
        .map(|declared| declared.gateway_ips)
        .unwrap_or_default();
    if ips.len() != 2 {
        return Err(invalid(
            ctx,
            "The deployment has no egress gateway VMs; re-run setup to create them",
        ));
    }
    Ok(ips)
}

/// Records the signing key's public half in `public_key` and returns the sandbox's binding. The
/// owner writes the key once, create-only, and never reads it; every other sandbox takes the
/// owner's recorded copy.
pub(super) async fn initialize_key(
    ctx: &ResourceControllerContext<'_>,
    public_key: &mut Option<egress::GatewayPublicKey>,
) -> Result<alien_core::bindings::SandboxEgressBinding> {
    use alien_core::bindings::{
        SandboxEgressBinding, SandboxEgressGateway, SandboxEgressReplicas, SandboxEgressSigningKey,
    };
    let config = ctx.desired_resource_config::<alien_core::Sandbox>()?;
    if !sandbox_egress::has_sandbox_gateway(ctx.desired_stack) {
        return Err(invalid(
            ctx,
            "Azure Internet sandboxes require the deployment gateway",
        ));
    }
    let owner = sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack).unwrap_or_default();
    if owner == config.id {
        // Keep the key this deployment already uses, whichever sandbox recorded it; only a
        // deployment with none writes one.
        if public_key.is_none() {
            *public_key = match egress::recorded_public_key::<AzureSandboxController>(ctx)? {
                Some(key) => Some(key),
                None => {
                    let vault = egress::deployment_vault(ctx).await?;
                    Some(egress::create_signing_key(vault.as_ref()).await?)
                }
            };
        }
    } else {
        *public_key = Some(egress::owner_public_key::<AzureSandboxController>(
            ctx, owner,
        )?);
    }
    let key = public_key.as_ref().expect("key initialized above");
    Ok(SandboxEgressBinding::Gateway(SandboxEgressGateway {
        container: sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into(),
        audience: format!("{}/{}", ctx.resource_prefix, config.id),
        signing_key: SandboxEgressSigningKey {
            vault: alien_core::SECRETS_VAULT_ID.into(),
            key: egress::SIGNING_KEY.into(),
        },
        kid: key.kid.clone(),
        admin_ca_pem: None,
        replicas: Some(SandboxEgressReplicas {
            dns: String::new(),
            addresses: gateway_addresses(ctx)?,
            port: GATEWAY_ADMIN_PORT,
            count: 2,
            min_protocol: alien_core::bindings::EGRESS_GATEWAY_ADMIN_PROTOCOL,
        }),
    }))
}

pub(super) async fn publish_config(
    ctx: &ResourceControllerContext<'_>,
    binding: Option<&alien_core::bindings::SandboxEgressBinding>,
    public_key: Option<&egress::GatewayPublicKey>,
    published: &mut Option<String>,
    replica_secret_ready: &mut bool,
) -> Result<()> {
    let facts = validate_network(ctx).await?;
    let key = match (binding, public_key) {
        (Some(alien_core::bindings::SandboxEgressBinding::Gateway(gateway)), Some(key))
            if gateway.kid == key.kid =>
        {
            key
        }
        _ => {
            return Err(invalid(
                ctx,
                "Azure sandbox gateway signing key changed during initialization",
            ))
        }
    };
    egress::publish_azure_config(
        ctx,
        key,
        &facts.vpc_cidrs,
        &gateway_addresses(ctx)?,
        published,
        replica_secret_ready,
    )
    .await
}

/// Setup's Network resource id and its controller state.
fn network_controller(
    ctx: &ResourceControllerContext<'_>,
) -> Result<(String, AzureNetworkController)> {
    // Azure has no default VNet: UseDefault and Create both build the managed VNet, subnets
    // and NAT this verifies. Setup refuses BYO VNets for Internet sandboxes.
    let network_id = ctx
        .desired_stack
        .resources()
        .find_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<alien_core::Network>()
                .map(|network| (id, network))
        })
        .ok_or_else(|| invalid(ctx, "Internet sandboxes require the deployment Network"))
        .and_then(|(id, network)| match network.settings {
            alien_core::NetworkSettings::UseDefault
            | alien_core::NetworkSettings::Create { .. } => Ok(id.to_string()),
            _ => Err(invalid(
                ctx,
                "Internet sandboxes require a network created by setup",
            )),
        })?;
    let state = ctx.state.resources.get(&network_id).ok_or_else(|| {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: ctx.desired_config.id().into(),
            dependency_id: network_id.clone(),
        })
    })?;
    let network = state.get_internal_controller_typed::<AzureNetworkController>()?;
    Ok((network_id, network))
}

pub(super) async fn validate_network(
    ctx: &ResourceControllerContext<'_>,
) -> Result<AzureSandboxNetwork> {
    let (network_id, network) = network_controller(ctx)?;
    let network_id = network_id.as_str();
    let azure = ctx.get_azure_config()?;
    let client = ctx.service_provider.get_azure_network_client(azure)?;
    let facts = validate_azure_sandbox_network(client.as_ref(), network_id, &network).await?;
    let arm = ctx
        .service_provider
        .get_azure_sandbox_groups_client(azure)?;
    let ids = egress::internet_sandbox_ids(ctx.desired_stack);
    if facts.sandbox_cidrs_by_resource.keys().collect::<Vec<_>>() != ids.iter().collect::<Vec<_>>()
    {
        return Err(invalid(
            ctx,
            "Azure sandbox source registration differs from the Internet Sandbox resources",
        ));
    }
    // All groups are imported before runtime starts. Verify each actual connection before
    // the owner can publish any audience; registration alone cannot assign source identity.
    for id in ids {
        let state = ctx.state.resources.get(&id).ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: ctx.desired_config.id().into(),
                dependency_id: id.clone(),
            })
        })?;
        let controller = state.get_internal_controller_typed::<AzureSandboxController>()?;
        let group = controller
            .sandbox_group
            .as_deref()
            .ok_or_else(|| invalid(ctx, "Azure sandbox group is not imported"))?;
        let resource_group = controller
            .resource_group
            .as_deref()
            .ok_or_else(|| invalid(ctx, "Azure sandbox resource group is not imported"))?;
        let connection = arm
            .get_default_vnet_connection(resource_group, group)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to verify the sandbox's setup-owned VNet connection".into(),
                resource_id: Some(id.clone()),
            })?;
        let expected_id = format!(
            "/subscriptions/{}/resourceGroups/{resource_group}/providers/Microsoft.App/sandboxGroups/{group}/vnetConnections/default",
            azure.subscription_id
        );
        if !connection.id.eq_ignore_ascii_case(&expected_id)
            || connection.properties.provisioning_state.as_deref() != Some("Succeeded")
            || !facts
                .sandbox_subnet_ids
                .iter()
                .any(|subnet| subnet.eq_ignore_ascii_case(&connection.properties.subnet_id))
        {
            return Err(invalid(
                ctx,
                "Azure sandbox VNet connection is not ready in the dedicated sandbox subnets",
            ));
        }
        let subnet_name = connection
            .properties
            .subnet_id
            .rsplit('/')
            .next()
            .ok_or_else(|| invalid(ctx, "Azure subnet ID is invalid"))?;
        let subnet = client
            .get_subnet(
                network
                    .resource_group
                    .as_deref()
                    .ok_or_else(|| invalid(ctx, "Azure network resource group is missing"))?,
                network
                    .vnet_name
                    .as_deref()
                    .ok_or_else(|| invalid(ctx, "Azure VNet name is missing"))?,
                subnet_name,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to verify a sandbox connection's source ranges".into(),
                resource_id: Some(id.clone()),
            })?;
        if !subnet
            .id
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case(&connection.properties.subnet_id))
        {
            return Err(invalid(
                ctx,
                "Azure returned a different sandbox connection subnet",
            ));
        }
        let properties = subnet
            .properties
            .ok_or_else(|| invalid(ctx, "Azure subnet properties are missing"))?;
        let actual: std::collections::BTreeSet<_> = properties
            .address_prefix
            .iter()
            .chain(&properties.address_prefixes)
            .cloned()
            .collect();
        let expected: std::collections::BTreeSet<_> = facts.sandbox_cidrs_by_resource[&id]
            .iter()
            .cloned()
            .collect();
        if actual != expected {
            return Err(invalid(
                ctx,
                "Azure sandbox source ranges belong to a different resource",
            ));
        }
    }
    Ok(facts)
}

fn invalid(
    ctx: &ResourceControllerContext<'_>,
    message: impl Into<String>,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: Some(ctx.desired_config.id().into()),
    })
}
