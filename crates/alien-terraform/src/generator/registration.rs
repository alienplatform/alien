//! Terraform `registration.tf` / `helm.tf` / `outputs.tf` generation and the
//! stack-input value expression handed to the registration provider.

use super::variables::terraform_stack_input_variable_name;
use super::{TerraformHelmInstall, TerraformRegistration};
use crate::{
    block::{attr, nested, resource_block},
    expr,
    target::TerraformTarget,
};
use alien_core::{import::CURRENT_SETUP_IMPORT_FORMAT_VERSION, StackInputDefinition};
use hcl::{
    expr::Expression,
    structure::{Block, BlockLabel, Body, Structure},
    Identifier,
};

pub(super) fn registration_body(
    target: TerraformTarget,
    registration: Option<&TerraformRegistration>,
    depends_on: &[Expression],
    input_values: Expression,
) -> Body {
    let depends_on_attr = (!depends_on.is_empty()).then(|| {
        attr(
            "depends_on",
            Expression::Array(depends_on.iter().cloned().collect()),
        )
    });
    if let Some(registration) = registration {
        let mut body = vec![
            attr("token", expr::raw("var.token")),
            attr("name", expr::raw("local.deployment_name")),
            attr("resource_prefix", expr::raw("local.resource_prefix")),
            attr(
                "setup_target",
                Expression::String(registration.setup_target.clone()),
            ),
            attr(
                "setup_import_format_version",
                Expression::Number(hcl::Number::from(i64::from(
                    CURRENT_SETUP_IMPORT_FORMAT_VERSION,
                ))),
            ),
            attr(
                "setup_fingerprint",
                Expression::String(registration.setup_fingerprint.clone()),
            ),
            attr(
                "setup_fingerprint_version",
                Expression::Number(hcl::Number::from(i64::from(
                    registration.setup_fingerprint_version,
                ))),
            ),
            attr("platform", expr::raw("local.deployment_platform")),
            attr("region", expr::raw("local.deployment_region")),
            attr("management_url", expr::raw("var.management_url")),
            attr(
                "management_config",
                expr::raw("jsondecode(jsonencode(local.deployment_management_config))"),
            ),
            attr(
                "stack_settings",
                expr::raw("jsondecode(jsonencode(local.deployment_settings))"),
            ),
            attr("resources", expr::raw("local.deployment_resources")),
        ];
        if !expression_is_empty_object(&input_values) {
            body.push(attr("input_values", input_values));
        }
        if let Some(release_id) = &registration.release_id {
            body.push(attr("release_id", Expression::String(release_id.clone())));
        }
        if target.is_kubernetes() {
            body.push(attr(
                "base_platform",
                expr::raw("local.deployment_base_platform"),
            ));
        }
        if let Some(depends_on_attr) = depends_on_attr {
            body.push(depends_on_attr);
        }
        return Body::from(vec![Structure::Block(resource_block(
            &registration.provider_resource_type(),
            "this",
            body,
        ))]);
    }

    let mut body = vec![attr(
        "input",
        expr::object(
            [
                ("platform", expr::raw("local.deployment_platform")),
                ("token", expr::raw("var.token")),
                ("name", expr::raw("var.name")),
                ("resource_prefix", expr::raw("local.resource_prefix")),
                (
                    "setup_target",
                    Expression::String(
                        registration
                            .map(|r| r.setup_target.clone())
                            .unwrap_or_default(),
                    ),
                ),
                (
                    "setup_import_format_version",
                    Expression::Number(hcl::Number::from(i64::from(
                        CURRENT_SETUP_IMPORT_FORMAT_VERSION,
                    ))),
                ),
                (
                    "setup_fingerprint",
                    Expression::String(
                        registration
                            .map(|r| r.setup_fingerprint.clone())
                            .unwrap_or_default(),
                    ),
                ),
                (
                    "setup_fingerprint_version",
                    Expression::Number(hcl::Number::from(i64::from(
                        registration
                            .map(|r| r.setup_fingerprint_version)
                            .unwrap_or_default(),
                    ))),
                ),
                ("management_url", expr::raw("var.management_url")),
                (
                    "management_config",
                    expr::raw("local.deployment_management_config"),
                ),
                ("stack_settings", expr::raw("local.deployment_settings")),
                ("resources", expr::raw("local.deployment_resources")),
                ("inputValues", input_values),
            ]
            .into_iter()
            .chain(
                target
                    .is_kubernetes()
                    .then(|| ("basePlatform", expr::raw("local.deployment_base_platform"))),
            ),
        ),
    )];
    if let Some(depends_on_attr) = depends_on_attr {
        body.push(depends_on_attr);
    }

    Body::from(vec![Structure::Block(resource_block(
        "terraform_data",
        "deployment_registration",
        body,
    ))])
}

pub(super) fn terraform_input_values_expression(inputs: &[StackInputDefinition]) -> Expression {
    if inputs.is_empty() {
        return expr::raw("{}");
    }

    let mut required_entries = Vec::new();
    let mut optional_maps = Vec::new();
    for input in inputs {
        let variable = format!("var.{}", terraform_stack_input_variable_name(input));
        let entry = format!("{} = {variable}", input.id);
        if input.required || input.default.is_some() {
            required_entries.push(entry);
        } else {
            optional_maps.push(format!("{variable} == null ? {{}} : {{ {entry} }}"));
        }
    }

    let required_map = format!("{{ {} }}", required_entries.join(", "));
    if optional_maps.is_empty() {
        expr::raw(required_map)
    } else {
        let mut maps = vec![required_map];
        maps.extend(optional_maps);
        expr::raw(format!("merge({})", maps.join(", ")))
    }
}

fn expression_is_empty_object(expression: &Expression) -> bool {
    matches!(expression, Expression::Object(object) if object.is_empty())
}

pub(super) fn helm_install_body(
    registration: &TerraformRegistration,
    _helm_install: &TerraformHelmInstall,
) -> Body {
    Body::from(vec![Structure::Block(resource_block(
        "helm_release",
        "runtime",
        [
            attr("count", expr::raw("var.helm_install_enabled ? 1 : 0")),
            attr("name", expr::raw("var.helm_release_name")),
            attr("namespace", expr::raw("var.kubernetes_namespace")),
            attr("create_namespace", Expression::Bool(true)),
            attr("chart", expr::raw("var.helm_chart")),
            attr(
                "values",
                Expression::Array(vec![expr::raw(format!(
                    "{}.this.helm_values",
                    registration.provider_resource_type()
                ))]),
            ),
            attr(
                "depends_on",
                Expression::Array(vec![expr::raw(format!(
                    "{}.this",
                    registration.provider_resource_type()
                ))]),
            ),
        ],
    ))])
}

pub(super) fn outputs_body(
    target: TerraformTarget,
    registration: Option<&TerraformRegistration>,
) -> Body {
    let mut outputs = vec![
        (
            "deployment_target",
            Expression::String(target.name().to_string()),
            "Terraform module target.",
        ),
        (
            "deployment_resource_prefix",
            expr::raw("local.resource_prefix"),
            "Physical resource prefix.",
        ),
        (
            "deployment_platform",
            expr::raw("local.deployment_platform"),
            "Target platform.",
        ),
        (
            "deployment_region",
            expr::raw("local.deployment_region"),
            "Target cloud region or location.",
        ),
        (
            "deployment_setup_target",
            Expression::String(
                registration
                    .map(|registration| registration.setup_target.clone())
                    .unwrap_or_default(),
            ),
            "Setup target.",
        ),
        (
            "deployment_setup_import_format_version",
            Expression::Number(hcl::Number::from(i64::from(
                CURRENT_SETUP_IMPORT_FORMAT_VERSION,
            ))),
            "Setup registration payload format version.",
        ),
        (
            "deployment_setup_fingerprint",
            Expression::String(
                registration
                    .map(|registration| registration.setup_fingerprint.clone())
                    .unwrap_or_default(),
            ),
            "Setup compatibility fingerprint.",
        ),
        (
            "deployment_setup_fingerprint_version",
            Expression::Number(hcl::Number::from(i64::from(
                registration
                    .map(|registration| registration.setup_fingerprint_version)
                    .unwrap_or_default(),
            ))),
            "Setup fingerprint algorithm version.",
        ),
        (
            "deployment_management_config",
            expr::raw("jsonencode(local.deployment_management_config)"),
            "Deployment registration management configuration JSON.",
        ),
        (
            "deployment_stack_settings",
            expr::raw("jsonencode(local.deployment_settings)"),
            "Deployment registration settings JSON.",
        ),
        (
            "deployment_resources",
            expr::raw("jsonencode(local.deployment_resources)"),
            "Deployment registration resource metadata JSON.",
        ),
    ];
    if let Some(registration) = registration {
        outputs.push((
            "deployment_id",
            expr::raw(format!(
                "{}.this.deployment_id",
                registration.provider_resource_type()
            )),
            "Deployment id assigned by the Terraform registration provider.",
        ));
        outputs.push((
            "deployment_token",
            expr::raw(format!(
                "{}.this.deployment_token",
                registration.provider_resource_type()
            )),
            "Deployment token assigned by the Terraform registration provider.",
        ));
    }
    if target.is_kubernetes() {
        outputs.push((
            "deployment_base_platform",
            expr::raw("local.deployment_base_platform"),
            "Base cloud platform for Kubernetes targets.",
        ));
        outputs.push((
            "kubernetes_namespace",
            expr::raw("var.kubernetes_namespace"),
            "Kubernetes namespace for runtime resources.",
        ));
        outputs.push((
            "kubernetes_kubeconfig",
            expr::raw("local.kubernetes_kubeconfig"),
            "Kubeconfig for managed Kubernetes clusters created by this module.",
        ));
        outputs.push((
            "kubernetes_kube_context",
            expr::raw("local.kubernetes_kube_context"),
            "Kube context for managed Kubernetes clusters created by this module.",
        ));
        if target == TerraformTarget::Eks {
            outputs.push((
                "kubernetes_update_kubeconfig_command",
                expr::template(
                    "AWS_PROFILE=<target-profile> aws eks update-kubeconfig --region ${local.deployment_region} --name ${local.kubernetes_kube_context} --alias ${local.kubernetes_kube_context}"
                        .to_string(),
                ),
                "AWS CLI command template for configuring kubectl access to the target EKS cluster.",
            ));
        }
    }

    let blocks: Vec<Structure> = outputs
        .into_iter()
        .map(|(name, value, description)| {
            let mut body = vec![
                attr("value", value),
                attr("description", Expression::String(description.to_string())),
            ];
            if name == "deployment_stack_settings"
                || name == "deployment_token"
                || name == "kubernetes_kubeconfig"
            {
                body.push(attr("sensitive", Expression::Bool(true)));
            }

            nested(Block {
                identifier: Identifier::sanitized("output"),
                labels: vec![BlockLabel::String(name.to_string())],
                body: Body::from(body),
            })
        })
        .collect();

    Body::from(blocks)
}
