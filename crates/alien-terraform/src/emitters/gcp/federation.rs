//! Setup-owned customer federation shared by management and published-resource identities.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    expr,
};
use alien_core::ResourceType;
use hcl::expr::Expression;

fn scope_hash(purpose: &ResourceType) -> String {
    format!(
        r#"sha256("gcp-deployment-access-v1:${{base64encode(var.deployment_id)}}:${{base64encode("{}")}}")"#,
        purpose
    )
}

pub(super) fn emit(fragment: &mut TfFragment, label: &str, purpose: &ResourceType) {
    let hash = scope_hash(purpose);
    let pool = format!("{label}_manager");
    fragment.resource_blocks.push(resource_block(
        "google_iam_workload_identity_pool",
        &pool,
        [
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "workload_identity_pool_id",
                expr::raw(format!("\"deployment-${{substr({hash}, 0, 20)}}\"")),
            ),
            attr(
                "display_name",
                Expression::String("Deployment access".into()),
            ),
            attr(
                "description",
                expr::raw(format!(
                    "\"Deployment identity pool. Scope: urn:deployment-access:v1:${{{hash}}}\""
                )),
            ),
        ],
    ));
    fragment.resource_blocks.push(resource_block(
        "google_iam_workload_identity_pool_provider",
        &pool,
        [
            attr("project", expr::raw("var.gcp_project")),
            attr(
                "workload_identity_pool_id",
                expr::traversal([
                    "google_iam_workload_identity_pool",
                    &pool,
                    "workload_identity_pool_id",
                ]),
            ),
            attr(
                "workload_identity_pool_provider_id",
                Expression::String("manager".into()),
            ),
            attr(
                "attribute_mapping",
                expr::raw("{ \"google.subject\" = \"assertion.sub\" }"),
            ),
            attr(
                "attribute_condition",
                expr::raw("\"assertion.sub == ${jsonencode(var.managing_oidc_subject)}\""),
            ),
            nested(block(
                "oidc",
                [
                    attr("issuer_uri", expr::raw("var.managing_oidc_issuer")),
                    attr(
                        "allowed_audiences",
                        expr::raw(format!("[\"urn:deployment-access:v1:${{{hash}}}\"]")),
                    ),
                ],
            )),
        ],
    ));
    fragment.resource_blocks.push(resource_block("google_service_account_iam_member", &pool, [
        attr("service_account_id", expr::traversal(["google_service_account", label, "id"])),
        attr("role", Expression::String("roles/iam.workloadIdentityUser".into())),
        attr("member", expr::raw(format!("\"principal://iam.googleapis.com/${{google_iam_workload_identity_pool.{pool}.name}}/subject/${{urlencode(var.managing_oidc_subject)}}\""))),
        attr("depends_on", expr::raw(format!("[google_iam_workload_identity_pool_provider.{pool}]"))),
    ]));
}

pub(super) fn import_ref(label: &str, purpose: &ResourceType) -> Expression {
    let pool = format!("{label}_manager");
    expr::object([
        (
            "projectNumber",
            expr::raw(format!(
                "split(\"/\", google_iam_workload_identity_pool.{pool}.name)[1]"
            )),
        ),
        (
            "poolId",
            expr::traversal([
                "google_iam_workload_identity_pool",
                &pool,
                "workload_identity_pool_id",
            ]),
        ),
        ("providerId", Expression::String("manager".into())),
        (
            "serviceAccountEmail",
            expr::traversal(["google_service_account", label, "email"]),
        ),
        (
            "assertionAudience",
            expr::raw(format!(
                "\"urn:deployment-access:v1:${{{}}}\"",
                scope_hash(purpose)
            )),
        ),
        (
            "trust",
            expr::object([
                ("issuer", expr::raw("var.managing_oidc_issuer")),
                ("subject", expr::raw("var.managing_oidc_subject")),
            ]),
        ),
    ])
}
