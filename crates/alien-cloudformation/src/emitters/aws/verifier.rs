//! AWS verifier: the Lambda function `<prefix>-verifier` and its role.
//!
//! Setup creates it and only setup changes it; the management role's limits keep every other
//! principal off what it owns. Its image and the machine-image publisher are literals from the
//! CLI build ([`alien_core::SetupPins`]), so the template's digest covers them.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        default_network, private_subnet_ids_expr, resource_config, security_group_ids_expr,
        sub_json, CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, ComputeCluster, ErrorData, Network, NetworkSettings, Result,
    SigningVerifier, AWS_LOG_RETENTION_DAYS,
};
use alien_error::AlienError;
use alien_permissions::generators::signing_limits;

/// Logical IDs of the verifier's function and role.
pub const VERIFIER_FUNCTION_ID: &str = "SigningVerifier";
pub const VERIFIER_ROLE_ID: &str = "SigningVerifierRole";
pub const VERIFIER_LOG_GROUP_ID: &str = "SigningVerifierLogGroup";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsVerifierEmitter;

impl CfEmitter for AwsVerifierEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<SigningVerifier>(ctx, SigningVerifier::RESOURCE_TYPE)?;
        let missing = |what: &str| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "render a protected AWS setup".to_string(),
                reason: format!(
                    "this CLI build has no {what}; protected setups come from released CLI builds"
                ),
            })
        };
        let publisher = ctx
            .pins
            .aws_publisher
            .as_deref()
            .ok_or_else(|| missing("machine-image publisher"))?;
        let image = ctx
            .pins
            .aws_verifier_image
            .as_deref()
            .filter(|image| alien_core::is_pinned_image(image))
            .ok_or_else(|| missing("verifier image pinned by digest"))?;
        let prefix = "${AWS::StackName}";
        let account = "${AWS::AccountId}";
        let region = "${AWS::Region}";
        let verifier_name = signing_limits::aws_verifier_name(prefix);

        let mut role = CfResource::new(VERIFIER_ROLE_ID.to_string(), "AWS::IAM::Role".to_string());
        role.properties
            .insert("RoleName".to_string(), CfExpression::sub(&verifier_name));
        role.properties.insert(
            "Description".to_string(),
            CfExpression::from("The verifier's role. Only setup changes it."),
        );
        role.properties.insert(
            "AssumeRolePolicyDocument".to_string(),
            CfExpression::object([
                ("Version", CfExpression::from("2012-10-17")),
                (
                    "Statement",
                    CfExpression::list([CfExpression::object([
                        ("Effect", CfExpression::from("Allow")),
                        (
                            "Principal",
                            CfExpression::object([(
                                "Service",
                                CfExpression::from("lambda.amazonaws.com"),
                            )]),
                        ),
                        ("Action", CfExpression::from("sts:AssumeRole")),
                    ])]),
                ),
            ]),
        );
        role.properties.insert(
            "Policies".to_string(),
            CfExpression::list([CfExpression::object([
                ("PolicyName", CfExpression::from("verifier")),
                (
                    "PolicyDocument",
                    sub_json(signing_limits::aws_verifier_policy(
                        prefix, account, region, publisher,
                    )),
                ),
            ])]),
        );
        role.properties
            .insert("Tags".to_string(), setup_tags(&verifier_name));

        // Lambda would otherwise create `/aws/lambda/<function name>` on the first call, outside
        // the stack, and a stack delete would leave it behind.
        let mut log_group = CfResource::new(
            VERIFIER_LOG_GROUP_ID.to_string(),
            "AWS::Logs::LogGroup".to_string(),
        );
        log_group.properties.insert(
            "LogGroupName".to_string(),
            CfExpression::sub(format!("/aws/lambda/{verifier_name}")),
        );
        log_group.properties.insert(
            "RetentionInDays".to_string(),
            CfExpression::Integer(AWS_LOG_RETENTION_DAYS.into()),
        );
        log_group
            .properties
            .insert("Tags".to_string(), setup_tags(&verifier_name));

        let mut function = CfResource::new(
            VERIFIER_FUNCTION_ID.to_string(),
            "AWS::Lambda::Function".to_string(),
        );
        function.depends_on.push(VERIFIER_LOG_GROUP_ID.to_string());
        function.properties.insert(
            "FunctionName".to_string(),
            CfExpression::sub(&verifier_name),
        );
        function.properties.insert(
            "Description".to_string(),
            CfExpression::from(
                "Applies only releases the vendor signed. The management role can't change what runs here.",
            ),
        );
        function
            .properties
            .insert("PackageType".to_string(), CfExpression::from("Image"));
        function.properties.insert(
            "Code".to_string(),
            CfExpression::object([(
                "ImageUri",
                CfExpression::sub(format!(
                    "{publisher}.dkr.ecr.${{AWS::Region}}.amazonaws.com/{image}"
                )),
            )]),
        );
        function.properties.insert(
            "Role".to_string(),
            CfExpression::get_att(VERIFIER_ROLE_ID, "Arn"),
        );
        function
            .properties
            .insert("Timeout".to_string(), CfExpression::from(300u32));
        function
            .properties
            .insert("MemorySize".to_string(), CfExpression::from(512u32));
        // The verifier image is built for x86_64; say so rather than lean on Lambda's default.
        function.properties.insert(
            "Architectures".to_string(),
            CfExpression::list([CfExpression::from("x86_64")]),
        );
        let mut environment = vec![("VERIFIER_CLOUD", CfExpression::from("aws"))];
        if let Some(network) = function_network(ctx) {
            environment.push(("VERIFIER_FUNCTION_VPC", network));
        }
        if let Some(network) = machine_network(ctx)? {
            environment.push(("VERIFIER_MACHINE_NETWORK", network));
        }
        function.properties.insert(
            "Environment".to_string(),
            CfExpression::object([("Variables", CfExpression::object(environment))]),
        );
        function
            .properties
            .insert("Tags".to_string(), setup_tags(&verifier_name));
        Ok(vec![role, log_group, function])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        resource_config::<SigningVerifier>(ctx, SigningVerifier::RESOURCE_TYPE)?;
        Ok(CfExpression::object([(
            "name",
            CfExpression::ref_(VERIFIER_FUNCTION_ID),
        )]))
    }
}

/// The stack's network, unless it is the default VPC, which setup fixes no subnets in.
fn named_network<'a>(ctx: &EmitContext<'a>) -> Option<&'a Network> {
    default_network(ctx)
        .map(|(_, network)| network)
        .filter(|network| !matches!(network.settings, NetworkSettings::UseDefault))
}

/// Where the verifier runs functions, when the stack has a network functions join: the
/// private subnets and workload security group the worker controller puts an unprotected
/// install's functions in. Functions stay out of the default VPC, so it gives none for that.
fn function_network(ctx: &EmitContext<'_>) -> Option<CfExpression> {
    named_network(ctx)?;
    Some(when_subnets_are_named(ctx, subnets_and_groups(ctx)))
}

/// Where the verifier launches machines, when the stack has any: the private subnets and
/// security group setup created, or the ones the customer named. Machines in the default VPC
/// have no subnets setup fixes, so a protected stack refuses them.
fn machine_network(ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
    if !ctx
        .stack
        .resources()
        .any(|(_, entry)| entry.config.resource_type() == ComputeCluster::RESOURCE_TYPE)
    {
        return Ok(None);
    }
    if named_network(ctx).is_none() {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "protect machines in the default VPC".to_string(),
            reason: "the verifier launches machines only into subnets setup fixes; create a network or name existing subnets".to_string(),
        }));
    }
    Ok(Some(when_subnets_are_named(ctx, subnets_and_groups(ctx))))
}

/// `value` when the stack's network mode names subnets, nothing otherwise. A template whose
/// `NetworkMode` parameter can choose the default VPC has no subnets to name then, so the value
/// is left out rather than name none.
fn when_subnets_are_named(ctx: &EmitContext<'_>, value: CfExpression) -> CfExpression {
    let parameterized = named_network(ctx).is_some_and(|network| {
        matches!(network.settings, NetworkSettings::Create { .. })
            && !ctx.targets_kubernetes
            && !crate::emitters::aws::helpers::network_mode_names_subnets(ctx.stack)
    });
    if parameterized {
        CfExpression::if_(
            CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS,
            value,
            CfExpression::no_value(),
        )
    } else {
        value
    }
}

/// JSON `{"subnetIds": [..], "securityGroupIds": [..]}` of the private subnets and security
/// group of the stack's network.
fn subnets_and_groups(ctx: &EmitContext<'_>) -> CfExpression {
    CfExpression::sub_with(
        r#"{"subnetIds":["${Subnets}"],"securityGroupIds":["${Groups}"]}"#,
        [
            (
                "Subnets",
                CfExpression::join("\",\"", always_a_list(private_subnet_ids_expr(ctx))),
            ),
            (
                "Groups",
                CfExpression::join("\",\"", always_a_list(security_group_ids_expr(ctx))),
            ),
        ],
    )
}

/// `list`, a list whose `Fn::If` branches may be `AWS::NoValue`, with those branches an empty
/// list. CloudFormation refuses an `Fn::Join` over a branch that can be nothing, even where an
/// outer condition leaves the joined value out, so what is joined must always be a list.
fn always_a_list(list: CfExpression) -> CfExpression {
    match list {
        CfExpression::Object(mut fields) if fields.len() == 1 => {
            if let Some(CfExpression::List(parts)) = fields.get_mut("Fn::If") {
                for branch in parts.iter_mut().skip(1) {
                    let taken = std::mem::replace(branch, CfExpression::Null);
                    *branch = if taken == CfExpression::no_value() {
                        CfExpression::List(Vec::new())
                    } else {
                        always_a_list(taken)
                    };
                }
            }
            CfExpression::Object(fields)
        }
        other => other,
    }
}

fn setup_tags(name: &str) -> CfExpression {
    CfExpression::list([
        CfExpression::object([
            ("Key", CfExpression::from("managed-by")),
            ("Value", CfExpression::from("setup")),
        ]),
        CfExpression::object([
            ("Key", CfExpression::from("Name")),
            ("Value", CfExpression::sub(name)),
        ]),
    ])
}
