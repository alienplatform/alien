//! The stack's network in the management role's policy.
//!
//! The network mode is a deploy-time parameter, so the VPC and subnets are `Fn::If` expressions.
//! The permission generator renders literal placeholders of the same length as real IDs, which
//! keeps policy chunking exact, and [`fill_network_scope`] swaps them for the expressions. A
//! placeholder left in place names a VPC and subnets that do not exist, so the policy fails
//! closed.

use crate::{
    emitters::aws::{
        helpers::{
            created_or_provided, default_network, subnet_refs, PARAM_PRIVATE_SUBNET_IDS,
            PARAM_PUBLIC_SUBNET_IDS,
        },
        network::created_sandbox_subnet_values,
    },
    template::CfExpression,
};
use alien_core::{import::EmitContext, Network, NetworkSettings, Platform, Sandbox, SandboxEgress};
use alien_permissions::AwsNetworkScope;

const VPC_PLACEHOLDER: &str = "vpc-0a1e0000000000000";
const VPC_VARIABLE: &str = "StackVpcId";
/// The most subnets a created network has: three public, three private and three sandbox
/// gateway subnets.
const SUBNET_PLACEHOLDERS: usize = 9;
const PARAM_SANDBOX_GATEWAY_SUBNET_IDS: &str = "SandboxGatewaySubnetIds";

fn subnet_placeholders() -> Vec<String> {
    (1..=SUBNET_PLACEHOLDERS)
        .map(|index| format!("subnet-0a1e000000000000{index}"))
        .collect()
}

/// The stack's network when setup does not create or name it: an automatic package leaves the
/// network to the runtime, so the template has no network resources or parameters to name.
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

/// Replace the placeholders [`network_scope`] rendered with the stack's VPC and subnets.
pub(crate) fn fill_network_scope(document: &mut CfExpression, ctx: &EmitContext<'_>) {
    let Some((vpc, subnets)) = network_expressions(ctx) else {
        return;
    };
    let placeholders = CfExpression::List(
        subnet_placeholders()
            .into_iter()
            .map(CfExpression::String)
            .collect(),
    );
    fill(document, &vpc, &placeholders, &subnets);
}

fn fill(
    value: &mut CfExpression,
    vpc: &CfExpression,
    subnet_placeholders: &CfExpression,
    subnets: &CfExpression,
) {
    if value == subnet_placeholders {
        *value = subnets.clone();
        return;
    }
    match value {
        CfExpression::String(text) if text == VPC_PLACEHOLDER => *value = vpc.clone(),
        CfExpression::Object(fields) if fields.len() == 1 => {
            if let Some(CfExpression::String(template)) = fields.get("Fn::Sub") {
                if template.contains(VPC_PLACEHOLDER) {
                    *value = CfExpression::sub_with(
                        template.replace(VPC_PLACEHOLDER, &format!("${{{VPC_VARIABLE}}}")),
                        [(VPC_VARIABLE, vpc.clone())],
                    );
                    return;
                }
            }
            for field in fields.values_mut() {
                fill(field, vpc, subnet_placeholders, subnets);
            }
        }
        CfExpression::Object(fields) => {
            for field in fields.values_mut() {
                fill(field, vpc, subnet_placeholders, subnets);
            }
        }
        CfExpression::List(values) => {
            for value in values {
                fill(value, vpc, subnet_placeholders, subnets);
            }
        }
        _ => {}
    }
}

/// The VPC ID and the subnet ID list, per network mode. The account's default VPC is not known
/// to CloudFormation, so that mode renders `*`.
fn network_expressions(ctx: &EmitContext<'_>) -> Option<(CfExpression, CfExpression)> {
    if network_left_to_runtime(ctx).is_some() {
        return None;
    }
    let (network_id, network) = default_network(ctx)?;
    let any = || CfExpression::from("*");
    let any_list = || CfExpression::list([any()]);
    let gateway = has_sandbox_gateway_subnets(ctx);
    let provided_subnets = || {
        let mut lists = vec![
            CfExpression::ref_(PARAM_PUBLIC_SUBNET_IDS),
            CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
        ];
        if gateway {
            lists.push(CfExpression::ref_(PARAM_SANDBOX_GATEWAY_SUBNET_IDS));
        }
        concat(lists)
    };
    match &network.settings {
        NetworkSettings::Create { .. } => {
            let mut created = vec![
                subnet_refs(network_id, "PublicSubnet"),
                subnet_refs(network_id, "PrivateSubnet"),
            ];
            if gateway {
                created.push(created_sandbox_subnet_values(
                    network_id,
                    "SandboxGatewaySubnet",
                    None,
                ));
            }
            Some((
                or_default(
                    created_or_provided(
                        ctx,
                        CfExpression::ref_(format!("{network_id}Vpc")),
                        CfExpression::ref_("VpcId"),
                    ),
                    any(),
                ),
                or_default(
                    created_or_provided(ctx, concat(created), provided_subnets()),
                    any_list(),
                ),
            ))
        }
        NetworkSettings::ByoVpcAws { .. } => {
            Some((CfExpression::ref_("VpcId"), provided_subnets()))
        }
        NetworkSettings::UseDefault => Some((any(), any_list())),
        NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => None,
    }
}

/// One list of every subnet in `lists`. A blank list parameter contributes an empty ID, which
/// matches no subnet.
fn concat(lists: Vec<CfExpression>) -> CfExpression {
    CfExpression::object([(
        "Fn::Split",
        CfExpression::list([
            CfExpression::from(","),
            CfExpression::join(
                ",",
                CfExpression::List(
                    lists
                        .into_iter()
                        .map(|list| CfExpression::join(",", list))
                        .collect(),
                ),
            ),
        ]),
    )])
}

/// `expression` with the use-default branch of its network-mode `Fn::If` chain set to
/// `fallback`.
fn or_default(expression: CfExpression, fallback: CfExpression) -> CfExpression {
    match expression {
        CfExpression::Object(mut fields) if fields.len() == 1 && fields.contains_key("Fn::If") => {
            if let Some(CfExpression::List(parts)) = fields.get_mut("Fn::If") {
                for branch in parts.iter_mut().skip(1) {
                    let taken = std::mem::replace(branch, CfExpression::Null);
                    *branch = if taken == CfExpression::no_value() {
                        fallback.clone()
                    } else {
                        or_default(taken, fallback.clone())
                    };
                }
            }
            CfExpression::Object(fields)
        }
        other => other,
    }
}

/// Sandbox gateway subnets host the internal load balancers that link sandboxes to containers.
fn has_sandbox_gateway_subnets(ctx: &EmitContext<'_>) -> bool {
    !ctx.targets_kubernetes
        && ctx.stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Sandbox>()
                .is_some_and(|sandbox| matches!(sandbox.egress, SandboxEgress::Internet))
        })
}
