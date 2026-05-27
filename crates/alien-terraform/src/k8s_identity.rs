//! Kubernetes identity overlay layer.
//!
//! Applied on top of cloud emitters when [`crate::TerraformTarget`] is
//! `Eks` / `Gke` / `Aks`. Wires service-account identity:
//!
//! * EKS → IRSA (IAM Role for Service Account).
//! * GKE → Workload Identity (Google service account binding via the
//!   `iam.workloadIdentityUser` role).
//! * AKS → User-Assigned Managed Identity + Federated Identity Credentials.
//!
//! The overlay reads each `aws_iam_role` / `google_service_account` /
//! `azurerm_user_assigned_identity` resource the cloud emitter produced
//! and adds cloud-side trust plus the values Helm needs:
//!
//! 1. The federated trust binding (IAM role-policy / IAM binding /
//!    federated identity credential).
//! 2. `helm_values` output data that carries service-account annotations
//!    and labels for the generated chart. Terraform intentionally does not
//!    create Kubernetes runtime objects; Helm owns those resources.

use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::TfFragment,
    expr,
    target::TerraformTarget,
};
use alien_core::{
    permission_profile_from_service_account_id, RemoteStackManagement, Result, ServiceAccount,
    Stack,
};
use hcl::expr::Expression;
use indexmap::IndexMap;

pub(crate) fn overlay_per_resource(
    stack: &Stack,
    target: TerraformTarget,
    resource_labels: &IndexMap<String, String>,
    per_resource: &mut IndexMap<String, TfFragment>,
    shared_locals: &mut IndexMap<String, Expression>,
) -> Result<()> {
    if !target.is_kubernetes() {
        return Ok(());
    }

    let mut service_accounts: Vec<(String, String, String)> = Vec::new();
    for (resource_id, entry) in stack.resources() {
        let Some(service_account) = entry.config.downcast_ref::<ServiceAccount>() else {
            continue;
        };
        let Some(label) = resource_labels.get(resource_id) else {
            continue;
        };
        service_accounts.push((
            resource_id.clone(),
            label.clone(),
            permission_profile_from_service_account_id(service_account.id()),
        ));
    }

    let mut helm_service_accounts: Vec<(String, Expression)> = Vec::new();
    let mut target_cluster_data_added = false;
    for (resource_id, label, permission_profile) in &service_accounts {
        let Some(fragment) = per_resource.get_mut(resource_id) else {
            continue;
        };
        if !target_cluster_data_added {
            add_target_cluster_data(fragment, target);
            target_cluster_data_added = true;
        }
        let service_account_name = terraform_service_account_name_expr(permission_profile);
        let helm_value = match target {
            TerraformTarget::Eks => Some(apply_eks(fragment, label, &service_account_name)),
            // GKE / AKS overlays land alongside the GCP / Azure
            // service-account emitters under T4 / T5 — wiring is below
            // but currently dormant until those cloud emitters exist.
            TerraformTarget::Gke if has_block(fragment, "google_service_account") => {
                Some(apply_gke(fragment, label, &service_account_name))
            }
            TerraformTarget::Aks if has_block(fragment, "azurerm_user_assigned_identity") => {
                Some(apply_aks(fragment, label, &service_account_name))
            }
            _ => None,
        };
        if let Some(value) = helm_value {
            helm_service_accounts.push((permission_profile.clone(), value));
        }
    }

    if !service_accounts.is_empty() {
        shared_locals.insert(
            "alien_kubernetes_namespace".to_string(),
            expr::raw("var.kubernetes_namespace"),
        );
    }
    shared_locals.insert(
        "helm_service_accounts".to_string(),
        expr::object(helm_service_accounts),
    );
    let helm_manager_service_account = overlay_manager_service_account(
        stack,
        target,
        resource_labels,
        per_resource,
        &mut target_cluster_data_added,
    );
    shared_locals.insert(
        "helm_manager_service_account".to_string(),
        helm_manager_service_account.unwrap_or_else(|| service_account_values([], [])),
    );
    if target == TerraformTarget::Eks && target_cluster_data_added {
        shared_locals.insert(
            "eks_oidc_issuer_host_path".to_string(),
            expr::raw(
                "trimprefix(data.aws_eks_cluster.target.identity[0].oidc[0].issuer, \"https://\")",
            ),
        );
    }

    Ok(())
}

fn overlay_manager_service_account(
    stack: &Stack,
    target: TerraformTarget,
    resource_labels: &IndexMap<String, String>,
    per_resource: &mut IndexMap<String, TfFragment>,
    target_cluster_data_added: &mut bool,
) -> Option<Expression> {
    let manager_service_account_name = "${local.resource_prefix}-manager-sa".to_string();
    for (resource_id, entry) in stack.resources() {
        if entry
            .config
            .downcast_ref::<RemoteStackManagement>()
            .is_none()
        {
            continue;
        }
        let label = resource_labels.get(resource_id)?;
        let fragment = per_resource.get_mut(resource_id)?;
        if !*target_cluster_data_added {
            add_target_cluster_data(fragment, target);
            *target_cluster_data_added = true;
        }
        return match target {
            TerraformTarget::Eks if has_block(fragment, "aws_iam_role") => {
                Some(apply_eks(fragment, label, &manager_service_account_name))
            }
            TerraformTarget::Gke if has_block(fragment, "google_service_account") => {
                Some(apply_gke(fragment, label, &manager_service_account_name))
            }
            TerraformTarget::Aks if has_block(fragment, "azurerm_user_assigned_identity") => {
                Some(apply_aks(fragment, label, &manager_service_account_name))
            }
            _ => Some(service_account_values([], [])),
        };
    }
    None
}

fn terraform_service_account_name_expr(permission_profile: &str) -> String {
    format!("${{local.resource_prefix}}-{permission_profile}-sa")
}

fn add_target_cluster_data(fragment: &mut TfFragment, target: TerraformTarget) {
    match target {
        TerraformTarget::Eks => add_eks_cluster_data(fragment),
        TerraformTarget::Gke => add_gke_cluster_data(fragment),
        TerraformTarget::Aks => add_aks_cluster_data(fragment),
        _ => {}
    }
}

fn add_eks_cluster_data(fragment: &mut TfFragment) {
    fragment.data_blocks.push(data_block(
        "aws_eks_cluster",
        "target",
        [attr("name", expr::raw("var.eks_cluster_name"))],
    ));
}

fn add_gke_cluster_data(fragment: &mut TfFragment) {
    fragment.data_blocks.push(data_block(
        "google_container_cluster",
        "target",
        [
            attr("name", expr::raw("var.gke_cluster_name")),
            attr("location", expr::raw("var.gke_cluster_location")),
        ],
    ));
}

fn add_aks_cluster_data(fragment: &mut TfFragment) {
    fragment.data_blocks.push(data_block(
        "azurerm_kubernetes_cluster",
        "target",
        [
            attr("name", expr::raw("var.aks_cluster_name")),
            attr(
                "resource_group_name",
                expr::raw("var.aks_cluster_resource_group_name"),
            ),
        ],
    ));
}

fn apply_eks(fragment: &mut TfFragment, label: &str, service_account_name: &str) -> Expression {
    fragment.data_blocks.push(data_block(
        "aws_iam_policy_document",
        &format!("{label}_assume_role"),
        [nested(block(
            "statement",
            [
                attr("effect", Expression::String("Allow".to_string())),
                attr(
                    "actions",
                    Expression::Array(vec![Expression::String(
                        "sts:AssumeRoleWithWebIdentity".to_string(),
                    )]),
                ),
                nested(block(
                    "principals",
                    [
                        attr("type", Expression::String("Federated".to_string())),
                        attr(
                            "identifiers",
                            Expression::Array(vec![expr::raw(
                                "format(\"arn:aws:iam::%s:oidc-provider/%s\", data.aws_caller_identity.current.account_id, local.eks_oidc_issuer_host_path)",
                            )]),
                        ),
                    ],
                )),
                nested(block(
                    "condition",
                    [
                        attr("test", Expression::String("StringEquals".to_string())),
                        attr(
                            "variable",
                            expr::template("${local.eks_oidc_issuer_host_path}:sub"),
                        ),
                        attr(
                            "values",
                            Expression::Array(vec![expr::template(format!(
                                "system:serviceaccount:${{var.kubernetes_namespace}}:{service_account_name}"
                            ))]),
                        ),
                    ],
                )),
                nested(block(
                    "condition",
                    [
                        attr("test", Expression::String("StringEquals".to_string())),
                        attr(
                            "variable",
                            expr::template("${local.eks_oidc_issuer_host_path}:aud"),
                        ),
                        attr(
                            "values",
                            Expression::Array(vec![Expression::String(
                                "sts.amazonaws.com".to_string(),
                            )]),
                        ),
                    ],
                )),
            ],
        ))],
    ));
    replace_assume_role_policy(
        fragment,
        label,
        expr::raw(format!(
            "data.aws_iam_policy_document.{label}_assume_role.json"
        )),
    );

    service_account_values(
        [(
            "eks.amazonaws.com/role-arn",
            expr::traversal(["aws_iam_role", label, "arn"]),
        )],
        [],
    )
}

fn apply_gke(fragment: &mut TfFragment, label: &str, service_account_name: &str) -> Expression {
    // Workload Identity binding: the IAM service account allows the
    // Kubernetes service account to impersonate it.
    fragment.resource_blocks.push(resource_block(
        "google_service_account_iam_binding",
        &format!("{label}_workload_identity"),
        [
            attr(
                "service_account_id",
                expr::traversal(["google_service_account", label, "name"]),
            ),
            attr(
                "role",
                Expression::String("roles/iam.workloadIdentityUser".to_string()),
            ),
            attr(
                "members",
                Expression::Array(vec![expr::template(format!(
                    "serviceAccount:${{data.google_container_cluster.target.workload_identity_config[0].workload_pool}}[${{var.kubernetes_namespace}}/{service_account_name}]"
                ))]),
            ),
        ],
    ));

    service_account_values(
        [(
            "iam.gke.io/gcp-service-account",
            expr::traversal(["google_service_account", label, "email"]),
        )],
        [],
    )
}

fn apply_aks(fragment: &mut TfFragment, label: &str, service_account_name: &str) -> Expression {
    // Federated Identity Credential: trusts the AKS cluster's OIDC
    // issuer for the Kubernetes service-account subject.
    fragment.resource_blocks.push(resource_block(
        "azurerm_federated_identity_credential",
        &format!("{label}_federated"),
        [
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{label}")),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr(
                "parent_id",
                expr::traversal(["azurerm_user_assigned_identity", label, "id"]),
            ),
            attr(
                "audience",
                Expression::Array(vec![Expression::String(
                    "api://AzureADTokenExchange".to_string(),
                )]),
            ),
            attr(
                "issuer",
                expr::raw("data.azurerm_kubernetes_cluster.target.oidc_issuer_url"),
            ),
            attr(
                "subject",
                expr::template(format!(
                    "system:serviceaccount:${{var.kubernetes_namespace}}:{service_account_name}"
                )),
            ),
        ],
    ));

    service_account_values(
        [(
            "azure.workload.identity/client-id",
            expr::traversal(["azurerm_user_assigned_identity", label, "client_id"]),
        )],
        [(
            "azure.workload.identity/use",
            Expression::String("true".to_string()),
        )],
    )
}

fn replace_assume_role_policy(fragment: &mut TfFragment, label: &str, value: Expression) {
    for resource in &mut fragment.resource_blocks {
        if resource.identifier.as_str() != "resource" {
            continue;
        }
        if resource.labels.first().map(|label| label.as_str()) != Some("aws_iam_role") {
            continue;
        }
        if resource
            .labels
            .get(1)
            .map(|resource_label| resource_label.as_str())
            != Some(label)
        {
            continue;
        }
        for structure in &mut resource.body.0 {
            let hcl::structure::Structure::Attribute(attribute) = structure else {
                continue;
            };
            if attribute.key.as_str() == "assume_role_policy" {
                attribute.expr = value;
                return;
            }
        }
        resource.body.0.push(attr("assume_role_policy", value));
        return;
    }
}

fn service_account_values<const A: usize, const L: usize>(
    annotations: [(&str, Expression); A],
    labels: [(&str, Expression); L],
) -> Expression {
    expr::object([
        ("annotations", expr::object(annotations)),
        ("labels", expr::object(labels)),
    ])
}

fn has_block(fragment: &TfFragment, terraform_type: &str) -> bool {
    fragment.resource_blocks.iter().any(|block| {
        block
            .labels
            .first()
            .is_some_and(|label| label.as_str() == terraform_type)
    })
}
