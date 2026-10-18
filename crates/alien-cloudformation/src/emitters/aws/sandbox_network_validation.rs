//! Setup reads effective routes, including implicit main-table associations.

use crate::{
    emitters::aws::{
        helpers::{
            private_subnet_ids_expr, required_logical_id, service_trust_policy, tags, vpc_id_expr,
        },
        network,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, Network, NetworkSettings, Result};

pub(crate) fn resources(ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
    let prefix = required_logical_id(ctx)?;
    let role_id = format!("{prefix}SandboxNetworkValidationRole");
    let function_id = format!("{prefix}SandboxNetworkValidationFunction");
    let mut role = CfResource::new(role_id.clone(), "AWS::IAM::Role".into());
    role.properties.insert(
        "AssumeRolePolicyDocument".into(),
        service_trust_policy(["lambda.amazonaws.com"]),
    );
    role.properties.insert(
        "Policies".into(),
        CfExpression::list([CfExpression::object([
            (
                "PolicyName",
                CfExpression::from("read-sandbox-subnet-routes"),
            ),
            (
                "PolicyDocument",
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list([CfExpression::object([
                            ("Effect", CfExpression::from("Allow")),
                            (
                                "Action",
                                CfExpression::list([
                                    CfExpression::from("ec2:DescribeVpcs"),
                                    CfExpression::from("ec2:DescribeSubnets"),
                                    CfExpression::from("ec2:DescribeRouteTables"),
                                ]),
                            ),
                            ("Resource", CfExpression::from("*")),
                        ])]),
                    ),
                ]),
            ),
        ])]),
    );
    role.properties.insert("Tags".into(), tags(ctx));
    let mut function = CfResource::new(function_id.clone(), "AWS::Lambda::Function".into());
    function
        .properties
        .insert("Runtime".into(), CfExpression::from("python3.13"));
    function
        .properties
        .insert("Handler".into(), CfExpression::from("index.handler"));
    function
        .properties
        .insert("Timeout".into(), CfExpression::Integer(60));
    function
        .properties
        .insert("Role".into(), CfExpression::get_att(role_id, "Arn"));
    function.properties.insert(
        "Code".into(),
        CfExpression::object([(
            "ZipFile",
            CfExpression::from(include_str!("sandbox_network_validation.py")),
        )]),
    );
    function.properties.insert("Tags".into(), tags(ctx));
    let mut validation = CfResource::new(
        format!("{prefix}SandboxNetworkValidation"),
        "Custom::SandboxNetworkValidation".into(),
    );
    validation.properties.insert(
        "ServiceToken".into(),
        CfExpression::get_att(function_id, "Arn"),
    );
    validation
        .properties
        .insert("ServiceTimeout".into(), CfExpression::Integer(120));
    validation
        .properties
        .insert("VpcId".into(), vpc_id_expr(ctx));
    validation
        .properties
        .insert("SandboxSubnetIds".into(), network::sandbox_subnet_ids(ctx)?);
    validation.properties.insert(
        "GatewaySubnetIds".into(),
        if network::needs_gateway(ctx) {
            network::gateway_subnet_ids(ctx)?
        } else {
            CfExpression::list([])
        },
    );
    validation
        .properties
        .insert("WorkloadSubnetIds".into(), private_subnet_ids_expr(ctx));
    if ctx
        .resource
        .config
        .downcast_ref::<Network>()
        .is_some_and(|network| matches!(network.settings, NetworkSettings::Create { .. }))
    {
        let mut ready = vec![CfExpression::if_(
            "NetworkModeCreate",
            CfExpression::ref_(format!("{prefix}PrivateDefaultRoute")),
            CfExpression::no_value(),
        )];
        for index in 0..3 {
            let condition = super::helpers::az_condition(index).unwrap_or("NetworkModeCreate");
            for kind in ["Sandbox", "Private"] {
                ready.push(CfExpression::if_(
                    condition,
                    CfExpression::ref_(format!(
                        "{prefix}{kind}Subnet{}RouteTableAssociation",
                        index + 1
                    )),
                    CfExpression::no_value(),
                ));
            }
            if network::needs_gateway(ctx) {
                ready.push(CfExpression::if_(
                    condition,
                    CfExpression::ref_(format!(
                        "{prefix}SandboxGatewaySubnet{}RouteTableAssociation",
                        index + 1
                    )),
                    CfExpression::no_value(),
                ));
            }
        }
        if network::needs_gateway(ctx) {
            ready.push(CfExpression::if_(
                "NetworkModeCreate",
                CfExpression::ref_(format!("{prefix}SandboxGatewayDefaultRoute")),
                CfExpression::no_value(),
            ));
        }
        validation
            .properties
            .insert("TopologyReady".into(), CfExpression::list(ready));
    }
    Ok(vec![role, function, validation])
}
