//! Kubernetes identity overlay layer.
//!
//! Applied on top of cloud emitters when [`crate::TerraformTarget`] is
//! `Eks` / `Gke` / `Aks`. Wires service-account identity:
//!
//! * EKS → IRSA (IAM Role for Service Account, OIDC provider).
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
    block::{attr, resource_block},
    emitter::TfFragment,
    expr,
    target::TerraformTarget,
};
use alien_core::{Result, ServiceAccount, Stack};
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

    let mut sa_labels: Vec<(String, String)> = Vec::new();
    for (resource_id, entry) in stack.resources() {
        if entry.config.downcast_ref::<ServiceAccount>().is_none() {
            continue;
        }
        let Some(label) = resource_labels.get(resource_id) else {
            continue;
        };
        sa_labels.push((resource_id.clone(), label.clone()));
    }

    let mut helm_service_accounts: Vec<(String, Expression)> = Vec::new();
    for (resource_id, label) in &sa_labels {
        let Some(fragment) = per_resource.get_mut(resource_id) else {
            continue;
        };
        let helm_value = match target {
            TerraformTarget::Eks => Some(apply_eks(fragment, label)),
            // GKE / AKS overlays land alongside the GCP / Azure
            // service-account emitters under T4 / T5 — wiring is below
            // but currently dormant until those cloud emitters exist.
            TerraformTarget::Gke if has_block(fragment, "google_service_account") => {
                Some(apply_gke(fragment, label))
            }
            TerraformTarget::Aks if has_block(fragment, "azurerm_user_assigned_identity") => {
                Some(apply_aks(fragment, label))
            }
            _ => None,
        };
        if let Some(value) = helm_value {
            helm_service_accounts.push((label.clone(), value));
        }
    }

    if !sa_labels.is_empty() {
        shared_locals.insert(
            "alien_kubernetes_namespace".to_string(),
            expr::raw("var.kubernetes_namespace"),
        );
    }
    shared_locals.insert(
        "helm_service_accounts".to_string(),
        expr::object(helm_service_accounts),
    );

    Ok(())
}

fn apply_eks(_fragment: &mut TfFragment, label: &str) -> Expression {
    // The cloud IAM role's trust policy is updated by the customer
    // out-of-band (one role-trust-policy per OIDC provider per cluster
    // — see the EKS module README for the kubectl/eksctl trust-policy
    // template). The chart-level Kubernetes ServiceAccount carries the
    // `eks.amazonaws.com/role-arn` annotation; pods consuming the SA
    // get IRSA credentials via the EKS pod identity webhook.
    service_account_values(
        [(
            "eks.amazonaws.com/role-arn",
            expr::traversal(["aws_iam_role", label, "arn"]),
        )],
        [],
    )
}

fn apply_gke(fragment: &mut TfFragment, label: &str) -> Expression {
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
                    "serviceAccount:${{var.gcp_project}}.svc.id.goog[${{var.kubernetes_namespace}}/{label}]"
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

fn apply_aks(fragment: &mut TfFragment, label: &str) -> Expression {
    // Federated Identity Credential: trusts the AKS cluster's OIDC
    // issuer for the Kubernetes service-account subject.
    fragment.resource_blocks.push(resource_block(
        "azurerm_federated_identity_credential",
        &format!("{label}_federated"),
        [
            attr(
                "name",
                expr::template(format!("${{var.stack_name}}-{label}")),
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
            attr("issuer", expr::raw("var.aks_oidc_issuer_url")),
            attr(
                "subject",
                expr::template(format!(
                    "system:serviceaccount:${{var.kubernetes_namespace}}:{label}"
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
