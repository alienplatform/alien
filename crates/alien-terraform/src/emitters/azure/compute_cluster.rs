//! Azure ComputeCluster — setup-owned node identities and execution grants.

use std::collections::HashSet;

use alien_core::{
    import::EmitContext, AzureStorageAccount, ComputeCluster, ErrorData, ExternalBinding, Network,
    NetworkSettings, Result, Storage, StorageBinding,
};
use alien_error::{AlienError, Context};
use alien_permissions::{generators::AzureRuntimePermissionsGenerator, BindingTarget};
use crate::block::{attr, resource_block};
use crate::emitters::azure::azure_machine_signing_grants;
use crate::emitters::azure::helpers::{
    downcast, emit_role_definition_and_assignments_for_target, permission_context, required_label,
    resource_prefix_template, tags,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureComputeClusterEmitter;

/// The stack's network and its label.
fn stack_network<'a>(ctx: &'a EmitContext<'_>) -> Result<(&'a str, &'a Network)> {
    let refuse = |reason: &str| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "name the network a protected install's verifier launches machines into"
                .to_string(),
            reason: reason.to_string(),
        })
    };
    let (network_id, network) = ctx
        .stack
        .resources()
        .find_map(|(id, entry)| entry.config.downcast_ref::<Network>().map(|n| (id, n)))
        .ok_or_else(|| refuse("the stack has no network"))?;
    let label = ctx
        .name_for(network_id)
        .ok_or_else(|| refuse("the stack's network has no label"))?;
    Ok((label, network))
}

impl TfEmitter for AzureComputeClusterEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let cluster = downcast::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let execute_set = alien_permissions::get_permission_set("compute-cluster/execute")
            .ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message:
                        "Missing compute-cluster/execute permission set for Azure node identity"
                            .to_string(),
                })
            })?;

        let mut fragment = TfFragment::default();
        // Keep the legacy node identity declared; the isolated identity is
        // attached only to a separate replacement VMSS cohort by the controller.
        for (identity_label, suffix) in [
            (
                format!("{label}_kubelet"),
                format!("{}-kubelet", cluster.id),
            ),
            (
                format!("{label}_isolation_v1"),
                format!("{}-isolation-v1", cluster.id),
            ),
        ] {
            fragment.resource_blocks.push(resource_block(
                "azurerm_user_assigned_identity",
                &identity_label,
                [
                    attr("name", resource_prefix_template(&suffix)),
                    attr("location", expr::raw("var.azure_location")),
                    attr(
                        "resource_group_name",
                        expr::raw("var.azure_resource_group_name"),
                    ),
                    attr("tags", tags(ctx, "compute-cluster")),
                ],
            ));
            // Imported clusters skip runtime identity/RBAC creation. Setup must
            // grant disk attachment and ingress registration to either cohort.
            // Explicit storage data grants remain isolated below.
            emit_role_definition_and_assignments_for_target(
                &mut fragment,
                &identity_label,
                &identity_label,
                BindingTarget::Stack,
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &identity_label,
                    "principal_id",
                ]),
                execute_set,
                &permission_context(&identity_label),
                &mut HashSet::new(),
            )?;
        }
        // Protected machines run as the node identity (they have no isolated cohort), so it
        // carries their storage grants and reads their signing settings. Other machines run
        // their isolated cohort as the isolated identity.
        let kubelet = format!("{label}_kubelet");
        let node_identity = if ctx.stack_settings.signing.is_some() {
            kubelet.clone()
        } else {
            format!("{label}_isolation_v1")
        };
        emit_node_storage_permissions(ctx, cluster, &node_identity, &mut fragment)?;
        fragment
            .resource_blocks
            .extend(azure_machine_signing_grants(
                ctx,
                &kubelet,
                expr::traversal(["azurerm_user_assigned_identity", &kubelet, "principal_id"]),
            )?);
        Ok(fragment)
    }

    /// The verifier launches a protected install's machines into the stack network's private
    /// subnet, behind its workload security group when setup creates the network, as the node
    /// identity this emitter creates.
    fn machine_network(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        let (network_label, network) = stack_network(ctx)?;
        let private = format!("{network_label}_private");
        let (subnet, security_groups) = match &network.settings {
            NetworkSettings::UseDefault | NetworkSettings::Create { .. } => (
                expr::traversal(["azurerm_subnet", &private, "id"]),
                vec![expr::traversal([
                    "azurerm_network_security_group",
                    &format!("{network_label}_workload"),
                    "id",
                ])],
            ),
            // The customer's subnet keeps the security group they gave it.
            NetworkSettings::ByoVnetAzure { .. } => (
                expr::traversal(["data", "azurerm_subnet", &private, "id"]),
                Vec::new(),
            ),
            NetworkSettings::ByoVpcAws { .. } | NetworkSettings::ByoVpcGcp { .. } => {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "name the network a protected Azure install's verifier launches machines into".to_string(),
                    reason: "the stack's network is not an Azure network".to_string(),
                }))
            }
        };
        Ok(Some(expr::jsonencode(expr::object([
            ("subnetIds", Expression::Array(vec![subnet])),
            ("securityGroupIds", Expression::Array(security_groups)),
            (
                "identity",
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &format!("{label}_kubelet"),
                    "id",
                ]),
            ),
        ]))))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let cluster = downcast::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let kubelet_identity = format!("{label}_kubelet");
        let isolated_identity = format!("{label}_isolation_v1");

        Ok(expr::object([
            ("clusterId", Expression::String(cluster.id.clone())),
            (
                "identityId",
                expr::traversal(["azurerm_user_assigned_identity", &kubelet_identity, "id"]),
            ),
            (
                "clusterIdentityPrincipalId",
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &kubelet_identity,
                    "principal_id",
                ]),
            ),
            (
                "isolatedIdentityId",
                expr::traversal(["azurerm_user_assigned_identity", &isolated_identity, "id"]),
            ),
            (
                "isolatedIdentityPrincipalId",
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &isolated_identity,
                    "principal_id",
                ]),
            ),
            (
                "isolatedIdentityClientId",
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &isolated_identity,
                    "client_id",
                ]),
            ),
            (
                "kubeletIdentityClientId",
                expr::traversal([
                    "azurerm_user_assigned_identity",
                    &kubelet_identity,
                    "client_id",
                ]),
            ),
        ]))
    }
}

/// Data grants belong to the actual node identity and retain the target's gate.
/// Blob bindings carry account/container coordinates only, so external targets
/// must belong to the configured subscription and resource group. Azure checks
/// that scope when applying the assignment; no cross-scope ownership is inferred.
fn emit_node_storage_permissions(
    ctx: &EmitContext<'_>,
    cluster: &ComputeCluster,
    identity_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let Some(profile) = &cluster.node_permissions else {
        return Ok(());
    };
    let principal = expr::traversal([
        "azurerm_user_assigned_identity",
        identity_label,
        "principal_id",
    ]);
    for (target_index, (target, references)) in profile.0.iter().enumerate() {
        if references.is_empty() {
            continue;
        }
        let invalid = || {
            ErrorData::GenericError {
            message: format!(
                "Node permissions require a supported Blob Storage target '{target}' in the configured subscription and resource group"
            ),
        }
        };
        let entry = ctx
            .stack
            .resources
            .get(target)
            .filter(|entry| entry.config.downcast_ref::<Storage>().is_some())
            .ok_or_else(|| AlienError::new(invalid()))?;
        let external = ctx
            .stack_settings
            .external_bindings
            .as_ref()
            .and_then(|bindings| bindings.get(target));
        let (account, container) = match external {
            Some(ExternalBinding::Storage(StorageBinding::Blob(binding))) => (
                binding
                    .account_name
                    .clone()
                    .into_value(target, "accountName")?,
                binding
                    .container_name
                    .clone()
                    .into_value(target, "containerName")?,
            ),
            Some(_) => return Err(AlienError::new(invalid())),
            None => {
                let storage_label = ctx
                    .name_for(target)
                    .ok_or_else(|| AlienError::new(invalid()))?;
                let account_label = ctx
                    .stack
                    .resources()
                    .find_map(|(id, entry)| {
                        entry
                            .config
                            .downcast_ref::<AzureStorageAccount>()
                            .and_then(|_| ctx.name_for(id))
                    })
                    .ok_or_else(|| AlienError::new(invalid()))?;
                (
                    format!("${{azurerm_storage_account.{account_label}.name}}"),
                    format!("${{azurerm_storage_container.{storage_label}.name}}"),
                )
            }
        };
        let context = permission_context(&identity_label)
            .with_resource_id(target)
            .with_storage_account_name(account)
            .with_resource_name(container);
        // Dedup only within a target: a sibling with a different enablement
        // gate must keep its own assignment contribution.
        let mut seen = HashSet::new();
        for (set_index, reference) in references.iter().enumerate() {
            let set = reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| AlienError::new(invalid()))?;
            let plan = AzureRuntimePermissionsGenerator::new()
                .generate_grant_plan(&set, BindingTarget::Resource, &context)
                .context(invalid())?;
            if plan.bindings.is_empty() {
                return Err(AlienError::new(invalid()));
            }
            let from = fragment.resource_blocks.len();
            let grant_label = format!("{identity_label}_nodegrant_{target_index}_{set_index}");
            emit_role_definition_and_assignments_for_target(
                fragment,
                &grant_label,
                &cluster.id,
                BindingTarget::Resource,
                principal.clone(),
                &set,
                &context,
                &mut seen,
            )?;
            fragment.mark_gated_from(from, entry.enabled_when.as_deref());
        }
    }
    Ok(())
}
