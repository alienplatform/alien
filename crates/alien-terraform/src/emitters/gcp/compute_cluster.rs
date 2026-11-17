//! GCP ComputeCluster — node-side identity + network tag.
//!
//! The full GKE / Compute Engine fleet is provisioned at runtime by
//! the managed container backend; this emitter ships only the cloud-
//! side identity (a service account dedicated to cluster nodes) + the
//! network tag firewall rules will target. Everything else stays
//! controller-managed so day-2 capacity / autoscaling decisions don't
//! force a `terraform apply`.

use alien_core::{
    import::EmitContext, ComputeCluster, ErrorData, ExternalBinding, Network, NetworkSettings,
    RemoteStackManagement, Result, Storage, StorageBinding,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget,
};
use crate::block::{attr, block, nested, resource_block};
use crate::emitters::gcp::helpers::{
    downcast, emit_custom_role_and_bindings, emit_custom_role_and_bindings_for_target,
    emit_custom_roles_for_bindings, permission_context, project_iam_member, required_label,
    role_expression_for_binding, sanitize_label_value, service_account_member_for_label,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

const NODE_PROJECT_ROLES: &[&str] = &[
    "roles/logging.logWriter",
    "roles/monitoring.metricWriter",
    "roles/monitoring.viewer",
    "roles/stackdriver.resourceMetadata.writer",
];

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpComputeClusterEmitter;

impl TfEmitter for GcpComputeClusterEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let cluster = downcast::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        // A fresh stack name is not an IAM isolation boundary: older node
        // identities anywhere in this project may still mint tokens for a new
        // service account. Do not emit data grants until a supported effective
        // project boundary exists. Existing execution-only templates are kept.
        if cluster
            .node_permissions
            .as_ref()
            .is_some_and(|profile| profile.0.values().any(|sets| !sets.is_empty()))
        {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: cluster.id.clone(),
                reason: "GCP node data permissions require a supported effective project isolation boundary; legacy project-wide token minting cannot be qualified by a fresh stack or IAM update completion".into(),
            }));
        }
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        let account_id = expr::raw(format!(
            "substr(replace(\"${{local.resource_prefix}}-{label}-node\", \"_\", \"-\"), 0, 30)"
        ));
        fragment.resource_blocks.push(resource_block(
            "google_service_account",
            &format!("{label}_node"),
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("account_id", account_id),
                attr(
                    "display_name",
                    expr::template(format!("Deployment {} cluster node identity", cluster.id)),
                ),
            ],
        ));

        let node_sa_label = format!("{label}_node");
        let member = service_account_member_for_label(&node_sa_label);
        // Compute Engine checks actAs on the VM's service account when its node
        // attaches a disk. Grant it on this identity, never on every SA in the project.
        fragment.resource_blocks.push(resource_block(
            "google_service_account_iam_member",
            &format!("{label}_node_self_user"),
            [
                attr(
                    "service_account_id",
                    expr::traversal(["google_service_account", &node_sa_label, "id"]),
                ),
                attr(
                    "role",
                    Expression::String("roles/iam.serviceAccountUser".to_string()),
                ),
                attr("member", member.clone()),
            ],
        ));
        for role in NODE_PROJECT_ROLES {
            let role_suffix = sanitize_label_value(&role.replace('/', "_").replace('.', "_"));
            let binding_label = format!("{label}_node_{role_suffix}");
            fragment
                .resource_blocks
                .push(project_iam_member(&binding_label, role, member.clone()));
        }

        let execute_set = alien_permissions::get_permission_set("compute-cluster/execute")
            .ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: "missing permission set 'compute-cluster/execute' (required by GCP compute-cluster node identity)".to_string(),
                })
            })?;
        let protected = ctx.stack_settings.signing.is_some();
        let execute_set = if protected {
            without_project_token_minting(execute_set)
        } else {
            execute_set.clone()
        };
        let execute_context = permission_context(&node_sa_label, ctx.stack.id())
            .with_managing_project_id("${var.gcp_project}".to_string());
        emit_custom_role_and_bindings(
            &mut fragment,
            &node_sa_label,
            &member,
            &execute_set,
            &execute_context,
        )?;

        emit_node_storage_permissions(ctx, cluster, &node_sa_label, &member, &mut fragment)?;

        if protected {
            emit_protected_node_grants(ctx, &node_sa_label, &member, &mut fragment);
        } else if let Some(management_label) = remote_stack_management_label(ctx) {
            fragment.resource_blocks.push(resource_block(
                "google_service_account_iam_member",
                &format!("{label}_node_management_user"),
                [
                    attr(
                        "service_account_id",
                        expr::traversal(["google_service_account", &node_sa_label, "id"]),
                    ),
                    attr(
                        "role",
                        Expression::String("roles/iam.serviceAccountUser".to_string()),
                    ),
                    attr(
                        "member",
                        expr::template(format!(
                            "serviceAccount:${{google_service_account.{management_label}.email}}"
                        )),
                    ),
                ],
            ));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let cluster = downcast::<ComputeCluster>(ctx, ComputeCluster::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let network_tag = format!("workload-{}", sanitize_label_value(&cluster.id));
        Ok(expr::object([
            ("clusterId", Expression::String(cluster.id.clone())),
            (
                "nodeServiceAccountEmail",
                expr::traversal(["google_service_account", &format!("{label}_node"), "email"]),
            ),
            ("networkTag", Expression::String(network_tag)),
        ]))
    }

    /// The verifier launches a protected install's machines into the stack network's
    /// subnetwork, with the tag the cluster's firewall rules target, as the node service
    /// account this emitter creates.
    fn machine_network(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::jsonencode(expr::object([
            ("subnetIds", Expression::Array(vec![machine_subnetwork(ctx)?])),
            (
                "securityGroupIds",
                Expression::Array(vec![Expression::String(MACHINE_NETWORK_TAG.to_string())]),
            ),
            (
                "identity",
                expr::traversal(["google_service_account", &format!("{label}_node"), "email"]),
            ),
        ]))))
    }
}

/// The permission a machine uses to mint tokens for the service accounts its containers run as.
const MINT_TOKENS: &str = "iam.serviceAccounts.getAccessToken";

/// `compute-cluster/execute` without its project-wide token minting. On a protected install the
/// project holds the verifier and management accounts too, and a machine that could mint their
/// tokens could change what runs. The custom role that carried it isn't rendered.
fn without_project_token_minting(
    set: &alien_core::permissions::PermissionSet,
) -> alien_core::permissions::PermissionSet {
    let mut set = set.clone();
    if let Some(entries) = set.platforms.gcp.as_mut() {
        for entry in entries.iter_mut() {
            if let Some(permissions) = entry.grant.permissions.as_mut() {
                permissions.retain(|permission| permission != MINT_TOKENS);
            }
        }
        entries.retain(|entry| {
            entry.grant.predefined_roles.is_some()
                || entry
                    .grant
                    .permissions
                    .as_ref()
                    .is_some_and(|permissions| !permissions.is_empty())
        });
    }
    set
}

/// The setup label of a protected install's verifier service account, which
/// `alien-terraform` renders beside the signing parameters.
const VERIFIER_LABEL: &str = "signing_verifier";

/// What a protected install's machines need, and who may launch them:
/// - tokens only for the stack's workload service accounts, the ones the verifier may act as;
/// - reads of the signing settings (Parameter Manager `signed-<prefix>-fingerprint`), reads of
///   the ledger's newest version and new ledger versions (Secret Manager `signed-<prefix>-state`,
///   bound on that secret), and reads of the deployer inputs (Secret Manager
///   `signed-<prefix>-input-*`). the machine agent reads the settings by listing their versions and getting
///   the newest, which `parameterViewer` allows; `parameterAccessor` only renders a version it
///   names;
/// - `actAs` on the node account for the verifier, which alone launches machines. Management
///   gets none: an identity that can launch a machine as the node account can run anything
///   with these grants.
///
/// The conditions name parameters by their last path segment, which is the same whether the
/// resource name carries the project id or number. Checked on GCP on 2026-11-07 with these
/// conditions and roles: as the node account, listing and getting the settings versions is
/// allowed; a neighbor's `signed-…` parameters, a settings version and listing the project's
/// parameters are refused.
fn emit_protected_node_grants(
    ctx: &EmitContext<'_>,
    node_label: &str,
    member: &Expression,
    fragment: &mut TfFragment,
) {
    let node_account = || expr::traversal(["google_service_account", node_label, "id"]);
    for workload in ctx.stack.resources().filter_map(|(id, entry)| {
        (entry.config.resource_type() == alien_core::ServiceAccount::RESOURCE_TYPE)
            .then(|| ctx.name_for(id))
            .flatten()
    }) {
        fragment.resource_blocks.push(resource_block(
            "google_service_account_iam_member",
            &format!("{node_label}_token_{workload}"),
            [
                attr(
                    "service_account_id",
                    expr::traversal(["google_service_account", workload, "name"]),
                ),
                attr(
                    "role",
                    Expression::String("roles/iam.serviceAccountTokenCreator".to_string()),
                ),
                attr("member", member.clone()),
            ],
        ));
    }
    fragment.resource_blocks.push(resource_block(
        "google_service_account_iam_member",
        &format!("{node_label}_verifier_user"),
        [
            attr("service_account_id", node_account()),
            attr(
                "role",
                Expression::String("roles/iam.serviceAccountUser".to_string()),
            ),
            attr(
                "member",
                expr::template(format!(
                    "serviceAccount:${{google_service_account.{VERIFIER_LABEL}.email}}"
                )),
            ),
        ],
    ));

    use alien_signing::settings::names;
    let prefix = "${local.resource_prefix}";
    let parameter = |name: &str| {
        format!(
            r#"(resource.name.endsWith("/parameters/{name}") || resource.name.extract("/parameters/{{parameter}}/versions/") == "{name}")"#
        )
    };
    let settings = names::gcp_settings(prefix);
    let inputs = names::gcp_input(prefix, "");
    for (suffix, role, title, expression) in [
        (
            "reads_signing",
            "roles/parametermanager.parameterViewer",
            "This install's signing settings",
            parameter(&settings),
        ),
        (
            "reads_inputs",
            "roles/secretmanager.secretAccessor",
            "This install's deployer inputs",
            format!(r#"resource.name.extract("/secrets/{{name}}").startsWith("{inputs}")"#),
        ),
    ] {
        fragment.resource_blocks.push(resource_block(
            "google_project_iam_member",
            &format!("{node_label}_{suffix}"),
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("role", Expression::String(role.to_string())),
                attr("member", member.clone()),
                nested(block(
                    "condition",
                    [
                        attr("title", Expression::String(title.to_string())),
                        attr(
                            "expression",
                            expr::template(expression.replace('\\', "\\\\").replace('"', "\\\"")),
                        ),
                    ],
                )),
            ],
        ));
    }
    // The ledger is the secret setup creates beside the settings; the machines read its newest
    // version and add versions, on that secret only.
    for (suffix, role) in [
        ("reads_ledger", "roles/secretmanager.secretAccessor"),
        ("records_ledger", "roles/secretmanager.secretVersionAdder"),
    ] {
        fragment.resource_blocks.push(resource_block(
            "google_secret_manager_secret_iam_member",
            &format!("{node_label}_{suffix}"),
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "secret_id",
                    expr::traversal(["google_secret_manager_secret", "signing_state", "secret_id"]),
                ),
                attr("role", Expression::String(role.to_string())),
                attr("member", member.clone()),
            ],
        ));
    }
}

/// The network tag a protected install's machines carry; the cluster's firewall rules target it.
const MACHINE_NETWORK_TAG: &str = "alien-compute-cluster";

/// The stack's network and its label, where a protected install's verifier launches machines.
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

/// The subnetwork the stack's network gives machines, as the path the verifier takes
/// (`projects/<p>/regions/<r>/subnetworks/<s>`, a subnetwork's `id`).
fn machine_subnetwork(ctx: &EmitContext<'_>) -> Result<Expression> {
    let (label, network) = stack_network(ctx)?;
    match &network.settings {
        NetworkSettings::UseDefault | NetworkSettings::ByoVpcGcp { .. } => Ok(expr::traversal([
            "data",
            "google_compute_subnetwork",
            &format!("{label}_subnet"),
            "id",
        ])),
        NetworkSettings::Create { .. } => Ok(expr::raw(format!(
            "var.network_mode == \"create-new\" ? google_compute_subnetwork.{label}_workload[0].id : data.google_compute_subnetwork.{label}_existing_subnet[0].id"
        ))),
        NetworkSettings::ByoVpcAws { .. } | NetworkSettings::ByoVnetAzure { .. } => {
            Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "name the network a protected GCP install's verifier launches machines into".to_string(),
                reason: "the stack's network is not a GCP network".to_string(),
            }))
        }
    }
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}

fn emit_node_storage_permissions(
    ctx: &EmitContext<'_>,
    cluster: &ComputeCluster,
    node_label: &str,
    member: &Expression,
    fragment: &mut TfFragment,
) -> Result<()> {
    let Some(profile) = &cluster.node_permissions else {
        return Ok(());
    };
    for (target_index, (target, references)) in profile.0.iter().enumerate() {
        if references.is_empty() {
            continue;
        }
        let invalid = || ErrorData::GenericError {
            message: format!("Node permissions require a supported GCS Storage target '{target}'"),
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
        let bucket = match external {
            Some(ExternalBinding::Storage(StorageBinding::Gcs(binding))) => binding
                .bucket_name
                .clone()
                .into_value(target, "bucketName")?,
            Some(_) => return Err(AlienError::new(invalid())),
            None => {
                let label = ctx
                    .name_for(target)
                    .ok_or_else(|| AlienError::new(invalid()))?;
                format!("${{google_storage_bucket.{label}.name}}")
            }
        };
        let context = permission_context(node_label, ctx.stack.id())
            .with_resource_id(target)
            .with_resource_name(bucket.clone());
        for (set_index, reference) in references.iter().enumerate() {
            let set = reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
                .ok_or_else(|| AlienError::new(invalid()))?;
            let plan = GcpRuntimePermissionsGenerator::new()
                .generate_grant_plan(&set, BindingTarget::Resource, &context)
                .context(invalid())?;
            if plan.bindings.is_empty() {
                return Err(AlienError::new(invalid()));
            }
            let from = fragment.resource_blocks.len();
            let grant_label = format!("{node_label}_nodegrant_{target_index}_{set_index}");
            // The normal emitter handles Project and explicit own-ServiceAccount
            // targets, including targetResourceName and shared custom role definitions.
            emit_custom_role_and_bindings_for_target(
                fragment,
                &grant_label,
                member,
                &set,
                &context,
                BindingTarget::Resource,
            )?;
            let bindings = plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
            let roles = emit_custom_roles_for_bindings(fragment, &plan, &bindings)?;
            for (index, binding) in bindings.iter().enumerate() {
                if binding.resource_kind.is_some() {
                    return Err(AlienError::new(invalid()));
                }
                let mut body = vec![
                    attr("bucket", expr::template(bucket.clone())),
                    attr("role", role_expression_for_binding(&binding.role, &roles)?),
                    attr("member", member.clone()),
                ];
                if let Some(condition) = &binding.condition {
                    body.push(nested(block(
                        "condition",
                        [
                            attr("title", Expression::String(condition.title.clone())),
                            attr(
                                "description",
                                Expression::String(condition.description.clone()),
                            ),
                            attr("expression", expr::template(condition.expression.clone())),
                        ],
                    )));
                }
                fragment.resource_blocks.push(resource_block(
                    "google_storage_bucket_iam_member",
                    &format!("{grant_label}_bucket_{index}"),
                    body,
                ));
            }
            fragment.mark_gated_from(from, entry.enabled_when.as_deref());
        }
    }
    Ok(())
}
