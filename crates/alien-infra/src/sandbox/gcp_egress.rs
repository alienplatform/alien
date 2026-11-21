//! The GCP gateway's signing key and shared configuration, published before any template names
//! the attachment.
//!
//! The gateway is two VMs that setup created in the deployment's VPC; their addresses and the
//! network attachment come from the data setup recorded on the Network.

use alien_core::{import::data::GcpSandboxNetwork, sandbox_egress};
use alien_error::AlienError;

use super::{egress, GcpAgentPlatformTemplateController};
use crate::{
    core::{state_utils::StackResourceStateExt, ResourceControllerContext},
    network::GcpNetworkController,
    ErrorData, Result,
};

/// The gateway's authenticated policy administration port. The sandboxes never reach it.
const GATEWAY_ADMIN_PORT: u16 = alien_core::gcp_sandbox_gateway::ADMIN_PORT;

fn invalid(
    ctx: &ResourceControllerContext<'_>,
    message: impl Into<String>,
) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: Some(ctx.desired_config.id().into()),
    })
}

/// What setup recorded on the deployment Network about the gateway wiring. An error with the
/// command to run when setup predates it, so an `internet` sandbox never starts without it.
pub(super) fn sandbox_network(ctx: &ResourceControllerContext<'_>) -> Result<GcpSandboxNetwork> {
    let network_id = ctx
        .desired_stack
        .resources()
        .find_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<alien_core::Network>()
                .map(|_| id.to_string())
        })
        .ok_or_else(|| invalid(ctx, "Internet sandboxes require the deployment Network"))?;
    let state = ctx.state.resources.get(&network_id).ok_or_else(|| {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: ctx.desired_config.id().into(),
            dependency_id: network_id.clone(),
        })
    })?;
    let network = state.get_internal_controller_typed::<GcpNetworkController>()?;
    let declared = network.sandbox_network.ok_or_else(|| {
        invalid(
            ctx,
            "the deployment has no egress gateway wiring; re-run setup to create the network \
             attachment and the gateway VMs",
        )
    })?;
    if declared.gateway_ips.len() != 2 {
        return Err(invalid(
            ctx,
            "the deployment has no egress gateway VMs; re-run setup to create them",
        ));
    }
    Ok(declared)
}

/// The VPC and workload subnet the deployment's private endpoint address comes from.
pub(super) struct WorkloadNetwork {
    pub network: String,
    pub subnetwork: String,
}

pub(super) fn workload_network(ctx: &ResourceControllerContext<'_>) -> Result<WorkloadNetwork> {
    let network_id = ctx
        .desired_stack
        .resources()
        .find_map(|(id, entry)| {
            entry
                .config
                .downcast_ref::<alien_core::Network>()
                .map(|_| id.to_string())
        })
        .ok_or_else(|| invalid(ctx, "Internet sandboxes require the deployment Network"))?;
    let state = ctx.state.resources.get(&network_id).ok_or_else(|| {
        AlienError::new(ErrorData::DependencyNotReady {
            resource_id: ctx.desired_config.id().into(),
            dependency_id: network_id.clone(),
        })
    })?;
    let network = state.get_internal_controller_typed::<GcpNetworkController>()?;
    match (network.network_self_link, network.subnetwork_self_link) {
        (Some(network), Some(subnetwork)) => Ok(WorkloadNetwork {
            network,
            subnetwork,
        }),
        _ => Err(invalid(
            ctx,
            "the deployment Network has no VPC or subnet recorded",
        )),
    }
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
            "GCP Internet sandboxes require the deployment gateway",
        ));
    }
    let network = sandbox_network(ctx)?;
    let owner = sandbox_egress::sandbox_egress_key_owner(ctx.desired_stack).unwrap_or_default();
    if owner == config.id {
        // Keep the key this deployment already uses, whichever sandbox recorded it; only a
        // deployment with none writes one.
        if public_key.is_none() {
            *public_key =
                match egress::recorded_public_key::<GcpAgentPlatformTemplateController>(ctx)? {
                    Some(key) => Some(key),
                    None => {
                        let vault = egress::deployment_vault(ctx).await?;
                        Some(egress::create_signing_key(vault.as_ref()).await?)
                    }
                };
        }
    } else {
        *public_key = Some(egress::owner_public_key::<
            GcpAgentPlatformTemplateController,
        >(ctx, owner)?);
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
            addresses: network.gateway_ips.clone(),
            port: GATEWAY_ADMIN_PORT,
            count: 2,
            min_protocol: alien_core::bindings::EGRESS_GATEWAY_ADMIN_PROTOCOL,
            proxy_port: network.proxy_port,
        }),
    }))
}

/// Publishes the shared gateway configuration (the owner only; an unchanged one is not rewritten).
pub(super) async fn publish_config(
    ctx: &ResourceControllerContext<'_>,
    binding: Option<&alien_core::bindings::SandboxEgressBinding>,
    public_key: Option<&egress::GatewayPublicKey>,
    published: &mut Option<String>,
    replica_secret_ready: &mut bool,
) -> Result<()> {
    let network = sandbox_network(ctx)?;
    let key = match (binding, public_key) {
        (Some(alien_core::bindings::SandboxEgressBinding::Gateway(gateway)), Some(key))
            if gateway.kid == key.kid =>
        {
            key
        }
        _ => {
            return Err(invalid(
                ctx,
                "GCP sandbox gateway signing key changed during initialization",
            ))
        }
    };
    egress::publish_azure_config(
        ctx,
        key,
        &network.vpc_cidrs,
        &network.gateway_ips,
        published,
        replica_secret_ready,
    )
    .await
}

/// The addresses the agent's proxy dials: each gateway VM on the proxy port setup recorded.
#[cfg(test)]
pub(crate) fn proxy_replicas(network: &GcpSandboxNetwork) -> Vec<String> {
    network
        .gateway_ips
        .iter()
        .map(|ip| format!("{ip}:{}", network.proxy_port))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_bindings::traits::BindingsProviderApi;

    #[test]
    fn the_agent_dials_each_gateway_on_the_recorded_proxy_port() {
        let network = GcpSandboxNetwork {
            network_attachment: "a".into(),
            attachment_cidr: "192.168.250.0/28".into(),
            gateway_cidr: "192.168.251.0/27".into(),
            gateway_ips: vec!["192.168.251.10".into(), "192.168.251.11".into()],
            proxy_port: 2222,
            vpc_cidrs: vec![],
        };
        assert_eq!(
            proxy_replicas(&network),
            vec!["192.168.251.10:2222", "192.168.251.11:2222"]
        );
    }
}

/// Removes the deployment's shared signing key and gateway configuration from its own vault
/// when the last Internet sandbox goes. They are written by the first Internet sandbox and used
/// by every other one and by the gateway VMs, so a sandbox removed while others remain leaves
/// them. A vault the deployment only binds to is not Alien's to empty.
pub(super) async fn delete_shared_secrets(
    ctx: &ResourceControllerContext<'_>,
    sandbox_id: &str,
    wrote_key: bool,
) -> Result<()> {
    let vault_is_owned = !ctx
        .deployment_config
        .external_bindings
        .has(alien_core::SECRETS_VAULT_ID);
    if !wrote_key || !vault_is_owned || egress::other_internet_sandbox_remains(ctx, sandbox_id) {
        return Ok(());
    }
    use alien_error::Context;
    let vault = egress::deployment_vault(ctx).await?;
    for name in [egress::GATEWAY_CONFIG, egress::SIGNING_KEY] {
        vault
            .delete_secret(name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete the sandbox egress vault entry '{name}'"),
                resource_id: Some(alien_core::SECRETS_VAULT_ID.into()),
            })?;
    }
    Ok(())
}
