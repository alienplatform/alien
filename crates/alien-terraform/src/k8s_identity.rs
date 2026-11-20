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
//! and adds the cloud-side trust needed by imported service-account data:
//!
//! 1. The federated trust binding (IAM role-policy / IAM binding /
//!    federated identity credential).
//! 2. Shared locals that let the platform import service-account annotations
//!    and labels, then render chart values from deployment state. Terraform
//!    intentionally does not create Kubernetes runtime objects; Helm owns
//!    those resources.

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

    let cluster_label = kubernetes_cluster_label(stack, resource_labels);
    let mut service_accounts: Vec<(String, String, String)> = Vec::new();
    let mut extension_identities: Vec<(String, String)> = Vec::new();
    for (resource_id, entry) in stack.resources() {
        let Some(service_account) = entry.config.downcast_ref::<ServiceAccount>() else {
            continue;
        };
        let Some(label) = resource_labels.get(resource_id) else {
            continue;
        };
        if service_account.extension.is_some() {
            extension_identities.push((resource_id.clone(), label.clone()));
            continue;
        }
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
            add_target_cluster_data(fragment, target, cluster_label.as_deref());
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
            "kubernetes_namespace".to_string(),
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
        cluster_label.as_deref(),
    );
    shared_locals.insert(
        "helm_manager_service_account".to_string(),
        helm_manager_service_account.unwrap_or_else(|| service_account_values([], [])),
    );
    // The Operator mints an extension's credentials from its own pod, so an extension's identity
    // trusts the Operator's service account, and no pod or Helm value runs as the identity. The
    // identity waits for the management identity, so the cluster's data goes with that one.
    // The Google account of the Operator's management identity, when it has one here.
    let gke_manager_account = stack
        .resources()
        .find(|(_, entry)| {
            entry
                .config
                .downcast_ref::<RemoteStackManagement>()
                .is_some()
        })
        .filter(|(resource_id, _)| {
            per_resource
                .get(*resource_id)
                .is_some_and(|fragment| has_block(fragment, "google_service_account"))
        })
        .and_then(|(resource_id, _)| resource_labels.get(resource_id))
        .cloned();
    for (resource_id, label) in &extension_identities {
        let Some(fragment) = per_resource.get_mut(resource_id) else {
            continue;
        };
        if !target_cluster_data_added {
            add_target_cluster_data(fragment, target, cluster_label.as_deref());
            target_cluster_data_added = true;
        }
        let operator = terraform_service_account_name_expr("manager");
        match target {
            TerraformTarget::Eks => {
                apply_eks(fragment, label, &operator);
            }
            TerraformTarget::Gke => {
                apply_gke(fragment, label, &operator);
                // The Operator calls Google as its own account, not as its Kubernetes service
                // account, so that account may mint this identity's tokens too.
                if let Some(manager) = &gke_manager_account {
                    grant_gke_token_minting(fragment, label, manager);
                }
            }
            TerraformTarget::Aks => {
                apply_aks(fragment, label, &operator);
            }
            _ => {}
        }
    }
    if target == TerraformTarget::Eks && target_cluster_data_added {
        shared_locals.insert(
            "eks_oidc_issuer_host_path".to_string(),
            expr::raw(
                "trimprefix(data.aws_eks_cluster.target.identity[0].oidc[0].issuer, \"https://\")",
            ),
        );
        shared_locals.insert(
            "eks_oidc_provider_arn".to_string(),
            expr::raw(
                "var.kubernetes_cluster_mode == \"create\" ? aws_iam_openid_connect_provider.eks[0].arn : data.aws_iam_openid_connect_provider.eks_existing[0].arn",
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
    cluster_label: Option<&str>,
) -> Option<Expression> {
    let manager_service_account_name = terraform_service_account_name_expr("manager");
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
            add_target_cluster_data(fragment, target, cluster_label);
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
    // Match kubernetes_service_account_name: normalize each part, retain a fallback,
    // then truncate the combined DNS label. The prefix is selected at install time.
    let profile = Expression::String(permission_profile.to_string()).to_string();
    let normalize = |input: &str| {
        format!(
            "coalesce(trim(lower(replace({input}, \"/[^a-zA-Z0-9]+/\", \"-\")), \"-\"), \"alien\")"
        )
    };
    format!(
        "${{trim(substr(format(\"%s-%s-sa\", {}, {}), 0, 63), \"-\")}}",
        normalize("local.resource_prefix"),
        normalize(profile.trim()),
    )
}

fn kubernetes_cluster_label(
    stack: &Stack,
    resource_labels: &IndexMap<String, String>,
) -> Option<String> {
    stack.resources().find_map(|(resource_id, entry)| {
        entry
            .config
            .downcast_ref::<alien_core::KubernetesCluster>()
            .and_then(|_| resource_labels.get(resource_id).cloned())
    })
}

fn add_target_cluster_data(
    fragment: &mut TfFragment,
    target: TerraformTarget,
    cluster_label: Option<&str>,
) {
    match target {
        TerraformTarget::Eks => add_eks_cluster_data(fragment, cluster_label),
        TerraformTarget::Gke => add_gke_cluster_data(fragment, cluster_label),
        TerraformTarget::Aks => add_aks_cluster_data(fragment, cluster_label),
        _ => {}
    }
}

fn add_eks_cluster_data(fragment: &mut TfFragment, cluster_label: Option<&str>) {
    if cluster_label.is_some() {
        return;
    }
    fragment.data_blocks.push(data_block(
        "aws_eks_cluster",
        "target",
        [attr("name", expr::raw("var.eks_cluster_name"))],
    ));
    fragment.data_blocks.push(data_block(
        "tls_certificate",
        "eks_oidc",
        [attr(
            "url",
            expr::raw("data.aws_eks_cluster.target.identity[0].oidc[0].issuer"),
        )],
    ));
    fragment.data_blocks.push(data_block(
        "aws_iam_openid_connect_provider",
        "eks_existing",
        [
            attr(
                "count",
                expr::raw("var.kubernetes_cluster_mode == \"existing\" ? 1 : 0"),
            ),
            attr(
                "url",
                expr::raw("data.aws_eks_cluster.target.identity[0].oidc[0].issuer"),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "aws_iam_openid_connect_provider",
        "eks",
        [
            attr(
                "count",
                expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
            ),
            attr(
                "url",
                expr::raw("data.aws_eks_cluster.target.identity[0].oidc[0].issuer"),
            ),
            attr(
                "client_id_list",
                Expression::Array(vec![Expression::String("sts.amazonaws.com".to_string())]),
            ),
            attr(
                "thumbprint_list",
                Expression::Array(vec![expr::raw(
                    "data.tls_certificate.eks_oidc.certificates[0].sha1_fingerprint",
                )]),
            ),
            attr(
                "tags",
                expr::object([
                    ("Name", expr::template("${local.resource_prefix}-eks-oidc")),
                    (
                        "deployment-resource-prefix",
                        expr::raw("local.resource_prefix"),
                    ),
                ]),
            ),
        ],
    ));
}

fn add_gke_cluster_data(fragment: &mut TfFragment, cluster_label: Option<&str>) {
    if cluster_label.is_some() {
        return;
    }
    let cluster_name = cluster_label
        .map(|label| format!("local.{label}_cluster_name"))
        .unwrap_or_else(|| "var.gke_cluster_name".to_string());
    fragment.data_blocks.push(data_block(
        "google_container_cluster",
        "target",
        [
            attr("name", expr::raw(cluster_name)),
            attr("location", expr::raw("var.gke_cluster_location")),
        ],
    ));
}

fn add_aks_cluster_data(fragment: &mut TfFragment, cluster_label: Option<&str>) {
    if cluster_label.is_some() {
        return;
    }
    let cluster_name = cluster_label
        .map(|label| format!("local.{label}_cluster_name"))
        .unwrap_or_else(|| "var.aks_cluster_name".to_string());
    fragment.data_blocks.push(data_block(
        "azurerm_kubernetes_cluster",
        "target",
        [
            attr("name", expr::raw(cluster_name)),
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
                            Expression::Array(vec![expr::raw("local.eks_oidc_provider_arn")]),
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

/// Takes the federated principal setup's customer federation granted
/// `roles/iam.workloadIdentityUser` on this service account out of the fragment, when it has
/// one: `(member expression, depends_on expression)`.
///
/// `google_service_account_iam_binding` is authoritative for the role, so a separate
/// `google_service_account_iam_member` for the same role is overwritten by it on every apply
/// and the manager loses access. The Workload Identity binding lists both principals instead.
/// The member resource's address is forgotten without destroying it: a destroy would remove
/// the principal the binding has just set.
fn take_federated_workload_identity_member(
    fragment: &mut TfFragment,
    label: &str,
) -> Option<(Expression, Option<Expression>)> {
    let member_label = format!("{label}_manager");
    let index = fragment.resource_blocks.iter().position(|block| {
        let labels: Vec<&str> = block.labels.iter().map(|l| l.as_str()).collect();
        labels == ["google_service_account_iam_member", member_label.as_str()]
    })?;
    let block = &fragment.resource_blocks[index];
    let find = |name: &str| {
        block
            .body
            .attributes()
            .find(|attribute| attribute.key.as_str() == name)
            .map(|attribute| attribute.expr.clone())
    };
    let member = find("member")?;
    let depends_on = find("depends_on");
    fragment.resource_blocks.remove(index);
    fragment.moved_blocks.push(crate::block::block(
        "removed",
        [
            attr(
                "from",
                expr::traversal(["google_service_account_iam_member", &member_label]),
            ),
            nested(crate::block::block(
                "lifecycle",
                [attr("destroy", Expression::Bool(false))],
            )),
        ],
    ));
    Some((member, depends_on))
}

fn apply_gke(fragment: &mut TfFragment, label: &str, service_account_name: &str) -> Expression {
    let federated = take_federated_workload_identity_member(fragment, label);
    let mut members = vec![expr::template(format!(
        "serviceAccount:${{data.google_container_cluster.target.workload_identity_config[0].workload_pool}}[${{var.kubernetes_namespace}}/{service_account_name}]"
    ))];
    let mut depends_on = None;
    if let Some((member, federated_depends_on)) = federated {
        members.push(member);
        depends_on = federated_depends_on;
    }
    let mut binding = vec![
        attr(
            "service_account_id",
            expr::traversal(["google_service_account", label, "name"]),
        ),
        attr(
            "role",
            Expression::String("roles/iam.workloadIdentityUser".to_string()),
        ),
        attr("members", Expression::Array(members)),
    ];
    if let Some(depends_on) = depends_on {
        binding.push(attr("depends_on", depends_on));
    }
    // Workload Identity binding: the IAM service account allows the
    // Kubernetes service account (and the manager's federated principal, when setup
    // federates it) to impersonate it.
    fragment.resource_blocks.push(resource_block(
        "google_service_account_iam_binding",
        &format!("{label}_workload_identity"),
        binding,
    ));

    service_account_values(
        [(
            "iam.gke.io/gcp-service-account",
            expr::traversal(["google_service_account", label, "email"]),
        )],
        [],
    )
}

/// Adds the Operator's Google account (`manager`'s) to the members of `label`'s Workload
/// Identity binding, whose role carries only `getAccessToken` and `getOpenIdToken`.
fn grant_gke_token_minting(fragment: &mut TfFragment, label: &str, manager: &str) {
    let name = format!("{label}_workload_identity");
    let Some(binding) = fragment.resource_blocks.iter_mut().find(|block| {
        block.identifier.as_str() == "resource"
            && block.labels.first().map(|label| label.as_str())
                == Some("google_service_account_iam_binding")
            && block.labels.get(1).map(|label| label.as_str()) == Some(name.as_str())
    }) else {
        return;
    };
    for structure in &mut binding.body.0 {
        let hcl::structure::Structure::Attribute(attribute) = structure else {
            continue;
        };
        if attribute.key.as_str() == "members" {
            if let Expression::Array(members) = &mut attribute.expr {
                members.push(expr::template(format!(
                    "serviceAccount:${{google_service_account.{manager}.email}}"
                )));
            }
            return;
        }
    }
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

#[cfg(test)]
mod naming_tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn terraform_identity_name_matches_runtime_dns_name() {
        let directory = tempfile::tempdir().unwrap();
        for (prefix, profile) in [
            ("e2e-01".to_string(), "execution".to_string()),
            ("My_App!".to_string(), "Writer#Profile".to_string()),
            ("--".to_string(), "!!!".to_string()),
            ("a".repeat(55), "b".repeat(40)),
            ("prefix-".repeat(12), "profile".to_string()),
            ("İ-é".to_string(), "team_Ü".to_string()),
        ] {
            std::fs::write(
                directory.path().join("main.tf"),
                format!(
                    "locals {{ resource_prefix = {} }}\n",
                    serde_json::json!(prefix)
                ),
            )
            .unwrap();
            let expression = terraform_service_account_name_expr(&profile);
            let raw_expression = expression
                .strip_prefix("${")
                .unwrap()
                .strip_suffix('}')
                .unwrap();
            let mut child = Command::new("terraform")
                .arg("console")
                .current_dir(directory.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("Terraform is required for identity contract test");
            writeln!(child.stdin.take().unwrap(), "{raw_expression}").unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: String = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                actual,
                alien_core::kubernetes_service_account_name(&prefix, &profile)
            );
        }
    }
}

#[cfg(test)]
mod federated_member_tests {
    use super::*;

    #[test]
    fn a_member_block_without_a_member_attribute_is_left_in_place() {
        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "google_service_account_iam_member",
            "management_manager",
            [attr(
                "role",
                Expression::String("roles/iam.workloadIdentityUser".into()),
            )],
        ));

        assert!(take_federated_workload_identity_member(&mut fragment, "management").is_none());
        assert_eq!(fragment.resource_blocks.len(), 1, "the block is untouched");
        assert!(
            fragment.moved_blocks.is_empty(),
            "no removed block is added"
        );
    }

    #[test]
    fn a_member_block_with_a_member_is_taken_and_its_address_forgotten() {
        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(resource_block(
            "google_service_account_iam_member",
            "management_manager",
            [attr("member", Expression::String("principal://x".into()))],
        ));

        let (member, depends_on) =
            take_federated_workload_identity_member(&mut fragment, "management").expect("taken");
        assert_eq!(member, Expression::String("principal://x".into()));
        assert!(depends_on.is_none());
        assert!(fragment.resource_blocks.is_empty());
        assert_eq!(fragment.moved_blocks.len(), 1);
    }
}
