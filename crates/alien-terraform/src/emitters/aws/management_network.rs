//! The stack's network in the management role's policy.
//!
//! The VPC and subnets are only known when Terraform applies (the network mode is a variable, a
//! created VPC gets its ID on apply), so the policy is rendered with literal placeholders of the
//! same length as real IDs, which keeps policy packing exact, and [`fill_network_scope`] swaps
//! them for HCL expressions. A placeholder left in place names a VPC and subnets that do not
//! exist, so the policy fails closed.

use crate::{
    emitters::aws::{
        helpers::{default_network, stack_has_setup_postgres},
        network::gateway_subnet_ids,
    },
    expr,
};
use alien_core::{import::EmitContext, Network, NetworkSettings, Platform, Result};
use alien_permissions::AwsNetworkScope;
use hcl::expr::{Expression, FuncCall, TemplateExpr};

const VPC_PLACEHOLDER: &str = "vpc-0a1e0000000000000";
/// The most subnets a created network has: three public, three private and three sandbox
/// gateway subnets.
const SUBNET_PLACEHOLDERS: usize = 9;

fn subnet_placeholders() -> Vec<String> {
    (1..=SUBNET_PLACEHOLDERS)
        .map(|index| format!("subnet-0a1e000000000000{index}"))
        .collect()
}

/// The stack's network when setup does not create or name it: an automatic package leaves the
/// network to the runtime, so the module has no network resources or inputs to name.
fn network_left_to_runtime<'a>(ctx: &EmitContext<'a>) -> Option<&'a Network> {
    let (_, entry) = ctx
        .stack
        .resources()
        .find(|(_, entry)| entry.config.downcast_ref::<Network>().is_some())?;
    (!alien_core::emits_package_scaffolding(
        entry,
        ctx.stack_settings.setup_management,
        Platform::Aws,
    ))
    .then(|| entry.config.downcast_ref::<Network>())
    .flatten()
}

/// The network scope to render the management policy with, when the stack has an AWS network.
pub(crate) fn network_scope(ctx: &EmitContext<'_>) -> Option<AwsNetworkScope> {
    if let Some(network) = network_left_to_runtime(ctx) {
        // Management creates a new network with the stack's tags. An existing or default VPC
        // named only in the runtime's settings keeps the previous wildcards.
        return matches!(network.settings, NetworkSettings::Create { .. })
            .then_some(AwsNetworkScope::StackTagged);
    }
    let (_, network) = default_network(ctx)?;
    match network.settings {
        NetworkSettings::Create { .. }
        | NetworkSettings::ByoVpcAws { .. }
        | NetworkSettings::UseDefault => Some(AwsNetworkScope::Vpc {
            vpc_id: VPC_PLACEHOLDER.to_string(),
            subnet_ids: subnet_placeholders(),
        }),
        NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => None,
    }
}

/// The VPC and subnet expressions [`fill_network_scope`] writes, per network mode.
pub(crate) struct NetworkExpressions {
    /// Locals the expressions read, to add to the module.
    pub locals: Vec<(String, Expression)>,
    vpc: Expression,
    subnets: Expression,
}

/// Longest VPC ID: `vpc-` and 17 hexadecimal digits.
const VPC_ID_CHARS: i64 = 21;
/// Longest subnet ID: `subnet-` and 17 hexadecimal digits.
const SUBNET_ID_CHARS: i64 = 24;

/// The stack's VPC ID and subnet IDs as HCL, or `None` when it has no AWS network.
///
/// The policy names exactly [`SUBNET_PLACEHOLDERS`] subnet entries, each cut to the length of a
/// subnet ID, and the VPC cut to the length of a VPC ID: a real ID is never cut, and the policy
/// keeps the size it was packed at. Entries past the stack's subnets are blank and match no
/// subnet; a network with more subnets than that has the extra ones left out, so management
/// cannot use them.
pub(crate) fn network_expressions(
    ctx: &EmitContext<'_>,
    label: &str,
) -> Result<Option<NetworkExpressions>> {
    if network_left_to_runtime(ctx).is_some() {
        return Ok(None);
    }
    let Some((network_label, network)) = default_network(ctx) else {
        return Ok(None);
    };
    let gateway = alien_core::sandbox_egress::has_sandbox_gateway(ctx.stack);
    let gateway_list = || -> Result<Option<String>> {
        Ok(if gateway {
            Some(format!(
                "({})",
                hcl::format::to_string(&gateway_subnet_ids(ctx)?).expect("expression formats")
            ))
        } else {
            None
        })
    };
    // Blank entries drop out.
    let provided = |lists: Vec<String>| {
        format!(
            "compact([for id in concat({}) : trimspace(id)])",
            lists.join(", ")
        )
    };
    let (vpc, subnets) = match &network.settings {
        NetworkSettings::Create { .. } => {
            let (default_vpc, default_subnets) = if stack_has_setup_postgres(ctx) {
                (
                    format!("data.aws_vpcs.{network_label}_default[0].ids[0]"),
                    format!("data.aws_subnets.{network_label}_default[0].ids"),
                )
            } else {
                // Without a database in it, setup does not look the default VPC up, so its
                // policy cannot name it.
                ("\"*\"".to_string(), "[\"*\"]".to_string())
            };
            let mut created = vec![
                format!("aws_subnet.{network_label}_public[*].id"),
                format!("aws_subnet.{network_label}_private[*].id"),
            ];
            let mut existing = vec![
                "var.public_subnet_ids".to_string(),
                "var.private_subnet_ids".to_string(),
            ];
            if let Some(gateway) = gateway_list()? {
                created.push(gateway.clone());
                existing.push(gateway);
            }
            (
                format!(
                    "var.network_mode == \"create-new\" ? aws_vpc.{network_label}[0].id : var.network_mode == \"use-existing\" ? var.vpc_id : {default_vpc}"
                ),
                format!(
                    "var.network_mode == \"create-new\" ? concat({}) : var.network_mode == \"use-existing\" ? {} : {default_subnets}",
                    created.join(", "),
                    provided(existing),
                ),
            )
        }
        // The same default VPC and default subnets the runtime places resources in.
        NetworkSettings::UseDefault => (
            format!("data.aws_vpcs.{network_label}_default.ids[0]"),
            format!("data.aws_subnets.{network_label}_default.ids"),
        ),
        NetworkSettings::ByoVpcAws { .. } => {
            let mut lists = vec![
                format!("var.{network_label}_public_subnet_ids"),
                format!("var.{network_label}_private_subnet_ids"),
            ];
            if let Some(gateway) = gateway_list()? {
                lists.push(gateway);
            }
            (format!("var.{network_label}_vpc_id"), provided(lists))
        }
        NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
            return Ok(None)
        }
    };
    let vpc_local = format!("{label}_network_vpc_id");
    let subnets_local = format!("{label}_network_subnet_ids");
    let padding = vec!["\"\""; SUBNET_PLACEHOLDERS].join(", ");
    Ok(Some(NetworkExpressions {
        locals: vec![
            (vpc_local.clone(), expr::raw(vpc)),
            (
                subnets_local.clone(),
                expr::raw(format!("concat({subnets}, [{padding}])")),
            ),
        ],
        vpc: expr::raw(format!("substr(local.{vpc_local}, 0, {VPC_ID_CHARS})")),
        subnets: Expression::Array(
            (0..SUBNET_PLACEHOLDERS)
                .map(|index| {
                    expr::raw(format!(
                        "substr(local.{subnets_local}[{index}], 0, {SUBNET_ID_CHARS})"
                    ))
                })
                .collect(),
        ),
    }))
}

/// Replace the placeholders [`network_scope`] rendered with the stack's VPC and subnets.
pub(crate) fn fill_network_scope(document: &mut Expression, network: &NetworkExpressions) {
    let placeholders: Vec<Expression> = subnet_placeholders()
        .into_iter()
        .map(Expression::String)
        .collect();
    fill(document, network, &placeholders);
}

fn fill(value: &mut Expression, network: &NetworkExpressions, placeholders: &[Expression]) {
    match value {
        Expression::Array(items) if items.as_slice() == placeholders => {
            *value = network.subnets.clone();
        }
        Expression::Array(items) => {
            for item in items {
                fill(item, network, placeholders);
            }
        }
        Expression::String(text) if text == VPC_PLACEHOLDER => *value = network.vpc.clone(),
        Expression::String(text) if text.contains(VPC_PLACEHOLDER) => {
            *value = replace_vpc(Expression::String(text.clone()), network);
        }
        Expression::TemplateExpr(template) => {
            if let TemplateExpr::QuotedString(text) = template.as_ref() {
                if text.contains(VPC_PLACEHOLDER) {
                    *value = replace_vpc(value.clone(), network);
                }
            }
        }
        Expression::Object(fields) => {
            for field in fields.values_mut() {
                fill(field, network, placeholders);
            }
        }
        Expression::FuncCall(call) => {
            for arg in &mut call.args {
                fill(arg, network, placeholders);
            }
        }
        _ => {}
    }
}

/// `replace(text, placeholder, vpc)`: the text keeps its own interpolations, and the VPC
/// expression is never quoted inside a template.
fn replace_vpc(text: Expression, network: &NetworkExpressions) -> Expression {
    Expression::FuncCall(Box::new(
        FuncCall::builder("replace")
            .arg(text)
            .arg(Expression::String(VPC_PLACEHOLDER.to_string()))
            .arg(network.vpc.clone())
            .build(),
    ))
}
