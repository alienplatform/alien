//! Shared helpers used across AWS CloudFormation emitters.
//!
//! Anything that touches more than one resource (intrinsic-function helpers,
//! tag block, JSON-to-CFN conversion, network expression helpers, IAM
//! trust-policy boilerplate) lives here so the per-resource emitters stay
//! focused on what they emit, not how they emit it.

use crate::template::{CfExpression, CfResource};
use alien_core::{
    import::EmitContext, ErrorData, Function, Network, NetworkSettings, Queue, ResourceDefinition,
    ResourceRef, ResourceType, Result, ServiceAccount, Storage, Vault, ALIEN_MANAGED_BY_TAG_KEY,
    ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_RESOURCE_TAG_KEY, ALIEN_STACK_TAG_KEY,
};
use alien_error::AlienError;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;

pub const PARAM_DEPLOYMENT_GROUP_TOKEN: &str = "DeploymentGroupToken";
pub const PARAM_MANAGING_ROLE_ARN: &str = "ManagingRoleArn";
pub const PARAM_MANAGING_ACCOUNT_ID: &str = "ManagingAccountId";
pub const PARAM_VPC_CIDR: &str = "VpcCidr";
pub const PARAM_PUBLIC_SUBNET_IDS: &str = "PublicSubnetIds";
pub const PARAM_PRIVATE_SUBNET_IDS: &str = "PrivateSubnetIds";
pub const PARAM_SECURITY_GROUP_IDS: &str = "SecurityGroupIds";

pub const CONDITION_NETWORK_CREATE_AZ2: &str = "NetworkCreateUseAz2";
pub const CONDITION_NETWORK_CREATE_AZ3: &str = "NetworkCreateUseAz3";
pub const CONDITION_HAS_VPC_CIDR: &str = "HasVpcCidr";

pub const INLINE_POLICY_NAME: &str = "alien-managed-policy";

/// Downcast `ctx.resource.config` to the typed resource definition or return
/// a typed `UnexpectedResourceType` error.
pub fn resource_config<'a, T>(ctx: &'a EmitContext<'_>, expected: ResourceType) -> Result<&'a T>
where
    T: ResourceDefinition,
{
    ctx.resource.config.downcast_ref::<T>().ok_or_else(|| {
        AlienError::new(ErrorData::UnexpectedResourceType {
            resource_id: ctx.resource_id.to_string(),
            expected,
            actual: ctx.resource.config.resource_type(),
        })
    })
}

/// Look up the precomputed CloudFormation logical id for the current
/// emitter context.
pub fn required_logical_id<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.name_for(ctx.resource_id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!(
                "missing CloudFormation logical id for resource '{}'",
                ctx.resource_id
            ),
        })
    })
}

/// Look up the precomputed logical id for a referenced resource.
pub fn logical_id_for_ref<'a>(
    ctx: &'a EmitContext<'_>,
    reference: &ResourceRef,
) -> Result<&'a str> {
    ctx.name_for(reference.id()).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!(
                "missing CloudFormation logical id for referenced resource '{}'",
                reference.id()
            ),
        })
    })
}

/// `${AWS::StackName}-{suffix}` template expression.
pub fn stack_name(suffix: &str) -> CfExpression {
    CfExpression::sub(format!("${{AWS::StackName}}-{suffix}"))
}

/// Standard Alien resource tags.
pub fn tags(ctx: &EmitContext<'_>) -> CfExpression {
    CfExpression::list([
        tag(ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE),
        tag_expr(ALIEN_STACK_TAG_KEY, CfExpression::ref_("AWS::StackName")),
        tag(ALIEN_RESOURCE_TAG_KEY, ctx.resource_id),
        tag(
            "AlienResourceType",
            ctx.resource.config.resource_type().as_ref(),
        ),
    ])
}

pub fn tag(key: &str, value: &str) -> CfExpression {
    tag_expr(key, CfExpression::from(value))
}

pub fn tag_expr(key: &str, value: CfExpression) -> CfExpression {
    CfExpression::object([("Key", CfExpression::from(key)), ("Value", value)])
}

/// `aws:PrincipalArn` plus `Service` trust policy block.
pub fn service_trust_policy<I, S>(services: I) -> CfExpression
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let services: Vec<CfExpression> = services
        .into_iter()
        .map(|service| CfExpression::from(service.into()))
        .collect();
    let principal = if services.len() == 1 {
        services.first().cloned().expect("len checked")
    } else {
        CfExpression::list(services)
    };

    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([CfExpression::object([
                ("Effect", CfExpression::from("Allow")),
                ("Principal", CfExpression::object([("Service", principal)])),
                ("Action", CfExpression::from("sts:AssumeRole")),
            ])]),
        ),
    ])
}

/// `Fn::GetAZs` → list of availability zones for the active region.
pub fn get_azs() -> CfExpression {
    CfExpression::object([("Fn::GetAZs", CfExpression::ref_("AWS::Region"))])
}

/// `Fn::Select [n, value]`.
pub fn select(index: usize, values: CfExpression) -> CfExpression {
    CfExpression::object([(
        "Fn::Select",
        CfExpression::list([CfExpression::Integer(index as i64), values]),
    )])
}

/// `Fn::Select [0, value]`. Useful for list-typed parameters.
pub fn first_or_null(values: CfExpression) -> CfExpression {
    select(0, values)
}

/// AZ-conditional name used by `Network::Create` subnets / route tables.
pub fn az_condition(index: usize) -> Option<&'static str> {
    match index {
        0 => None,
        1 => Some(CONDITION_NETWORK_CREATE_AZ2),
        2 => Some(CONDITION_NETWORK_CREATE_AZ3),
        _ => unreachable!("only three AZ slots are emitted"),
    }
}

/// Per-AZ subnet refs for the created VPC, with `Fn::If` masking AZ 2/3
/// when the customer requested fewer.
pub fn subnet_refs(prefix: &str, kind: &str) -> CfExpression {
    CfExpression::list([
        CfExpression::ref_(format!("{prefix}{kind}1")),
        CfExpression::if_(
            CONDITION_NETWORK_CREATE_AZ2,
            CfExpression::ref_(format!("{prefix}{kind}2")),
            CfExpression::no_value(),
        ),
        CfExpression::if_(
            CONDITION_NETWORK_CREATE_AZ3,
            CfExpression::ref_(format!("{prefix}{kind}3")),
            CfExpression::no_value(),
        ),
    ])
}

/// Per-AZ availability-zone names with the same `Fn::If` masking.
pub fn availability_zone_names() -> CfExpression {
    CfExpression::list([
        select(0, get_azs()),
        CfExpression::if_(
            CONDITION_NETWORK_CREATE_AZ2,
            select(1, get_azs()),
            CfExpression::no_value(),
        ),
        CfExpression::if_(
            CONDITION_NETWORK_CREATE_AZ3,
            select(2, get_azs()),
            CfExpression::no_value(),
        ),
    ])
}

/// VPC ID expression — created VPC ref, BYO VPC parameter, or nothing.
pub fn vpc_id_expr(ctx: &EmitContext<'_>) -> CfExpression {
    let Some((network_id, network)) = default_network(ctx) else {
        return CfExpression::ref_("VpcId");
    };
    match &network.settings {
        NetworkSettings::Create { .. } => CfExpression::ref_(format!("{network_id}Vpc")),
        NetworkSettings::UseDefault
        | NetworkSettings::ByoVpcAws { .. }
        | NetworkSettings::ByoVpcGcp { .. }
        | NetworkSettings::ByoVnetAzure { .. } => CfExpression::ref_("VpcId"),
    }
}

/// Private subnet IDs expression — uses created VPC subnets when this
/// stack creates the VPC, BYO parameter otherwise.
pub fn private_subnet_ids_expr(ctx: &EmitContext<'_>) -> CfExpression {
    let Some((network_id, network)) = default_network(ctx) else {
        return CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS);
    };
    match &network.settings {
        NetworkSettings::Create { .. } => subnet_refs(network_id, "PrivateSubnet"),
        NetworkSettings::UseDefault
        | NetworkSettings::ByoVpcAws { .. }
        | NetworkSettings::ByoVpcGcp { .. }
        | NetworkSettings::ByoVnetAzure { .. } => CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
    }
}

/// Security-group IDs expression — created SG ref or BYO parameter.
pub fn security_group_ids_expr(ctx: &EmitContext<'_>) -> CfExpression {
    let Some((network_id, network)) = default_network(ctx) else {
        return CfExpression::ref_(PARAM_SECURITY_GROUP_IDS);
    };
    match &network.settings {
        NetworkSettings::Create { .. } => {
            CfExpression::list([CfExpression::ref_(format!("{network_id}SecurityGroup"))])
        }
        NetworkSettings::UseDefault
        | NetworkSettings::ByoVpcAws { .. }
        | NetworkSettings::ByoVpcGcp { .. }
        | NetworkSettings::ByoVnetAzure { .. } => CfExpression::ref_(PARAM_SECURITY_GROUP_IDS),
    }
}

/// First [`Network`] resource in the stack, with its logical id. Used
/// across compute emitters that need to know whether a VPC exists.
pub fn default_network<'a>(ctx: &EmitContext<'a>) -> Option<(&'a str, &'a Network)> {
    ctx.stack.resources().find_map(|(id, entry)| {
        let network = entry.config.downcast_ref::<Network>()?;
        let logical_id = ctx.name_for(id)?;
        Some((logical_id, network))
    })
}

/// Look up the IAM role logical id for a service account by permissions
/// profile (the `<profile>-sa` convention used across AWS resources).
pub fn service_account_role_id(ctx: &EmitContext<'_>, profile_name: &str) -> Option<String> {
    let service_account_id = format!("{profile_name}-sa");
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    Some(format!("{}Role", ctx.name_for(&service_account_id)?))
}

/// IAM permissions for a function's link to another resource. Returns one
/// or more statement objects ready to splice into a policy document.
pub fn link_permission_statements(
    ctx: &EmitContext<'_>,
    link: &ResourceRef,
) -> Result<Vec<CfExpression>> {
    let logical_id = logical_id_for_ref(ctx, link)?;
    if link.resource_type == Storage::RESOURCE_TYPE {
        Ok(vec![CfExpression::object([
            (
                "Sid",
                CfExpression::from(format!("AccessStorage{}", logical_id)),
            ),
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("s3:GetObject"),
                    CfExpression::from("s3:PutObject"),
                    CfExpression::from("s3:DeleteObject"),
                    CfExpression::from("s3:ListBucket"),
                ]),
            ),
            (
                "Resource",
                CfExpression::list([
                    CfExpression::get_att(logical_id, "Arn"),
                    CfExpression::sub(format!("${{{logical_id}.Arn}}/*")),
                ]),
            ),
        ])])
    } else if link.resource_type == Queue::RESOURCE_TYPE {
        Ok(vec![CfExpression::object([
            (
                "Sid",
                CfExpression::from(format!("AccessQueue{}", logical_id)),
            ),
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("sqs:SendMessage"),
                    CfExpression::from("sqs:ReceiveMessage"),
                    CfExpression::from("sqs:DeleteMessage"),
                    CfExpression::from("sqs:GetQueueAttributes"),
                ]),
            ),
            ("Resource", CfExpression::get_att(logical_id, "Arn")),
        ])])
    } else if link.resource_type == alien_core::Kv::RESOURCE_TYPE {
        Ok(vec![CfExpression::object([
            (
                "Sid",
                CfExpression::from(format!("AccessTable{}", logical_id)),
            ),
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("dynamodb:GetItem"),
                    CfExpression::from("dynamodb:PutItem"),
                    CfExpression::from("dynamodb:UpdateItem"),
                    CfExpression::from("dynamodb:DeleteItem"),
                    CfExpression::from("dynamodb:Query"),
                    CfExpression::from("dynamodb:Scan"),
                ]),
            ),
            ("Resource", CfExpression::get_att(logical_id, "Arn")),
        ])])
    } else if link.resource_type == Vault::RESOURCE_TYPE {
        Ok(vec![CfExpression::object([
            (
                "Sid",
                CfExpression::from(format!("AccessVault{}", logical_id)),
            ),
            ("Effect", CfExpression::from("Allow")),
            (
                "Action",
                CfExpression::list([
                    CfExpression::from("ssm:GetParameter"),
                    CfExpression::from("ssm:GetParameters"),
                    CfExpression::from("ssm:PutParameter"),
                    CfExpression::from("ssm:DeleteParameter"),
                ]),
            ),
            (
                "Resource",
                CfExpression::sub(format!(
                    "arn:${{AWS::Partition}}:ssm:${{AWS::Region}}:${{AWS::AccountId}}:parameter/${{AWS::StackName}}-{}/*",
                    link.id
                )),
            ),
        ])])
    } else {
        Ok(vec![])
    }
}

/// Result of [`role_for_profile_or_fallback`] — either the SA role's ARN
/// expression and no extra resources, or a fallback role's ARN expression
/// with the role resource that backs it.
#[derive(Debug)]
pub struct RoleSelection {
    pub arn: CfExpression,
    pub resource_id: Option<String>,
    pub resources: Vec<CfResource>,
}

/// If the stack has a `<profile>-sa` ServiceAccount, return its role ARN.
/// Otherwise emit a fallback service role with the given trust principal
/// and inline policy.
pub fn role_for_profile_or_fallback(
    ctx: &EmitContext<'_>,
    profile_name: &str,
    fallback_role_id: &str,
    service_principal: &str,
    fallback_policy: CfExpression,
) -> Result<RoleSelection> {
    if let Some(role_id) = service_account_role_id(ctx, profile_name) {
        return Ok(RoleSelection {
            arn: CfExpression::get_att(role_id, "Arn"),
            resource_id: None,
            resources: vec![],
        });
    }

    let role = fallback_service_role(ctx, fallback_role_id, service_principal, fallback_policy);
    Ok(RoleSelection {
        arn: CfExpression::get_att(fallback_role_id, "Arn"),
        resource_id: Some(fallback_role_id.to_string()),
        resources: vec![role],
    })
}

fn fallback_service_role(
    ctx: &EmitContext<'_>,
    role_id: &str,
    service_principal: &str,
    policy: CfExpression,
) -> CfResource {
    let mut role = CfResource::new(role_id.to_string(), "AWS::IAM::Role".to_string());
    role.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        service_trust_policy([service_principal]),
    );
    role.properties.insert(
        "Policies".to_string(),
        CfExpression::list([CfExpression::object([
            ("PolicyName", CfExpression::from(INLINE_POLICY_NAME)),
            ("PolicyDocument", policy),
        ])]),
    );
    role.properties.insert("Tags".to_string(), tags(ctx));
    role
}

/// Convert `serde_json::Value` (e.g. an IAM policy generated by
/// `alien-permissions`) into [`CfExpression`].
pub fn cf_from_json(value: JsonValue) -> Result<CfExpression> {
    Ok(match value {
        JsonValue::Null => CfExpression::Null,
        JsonValue::Bool(value) => CfExpression::from(value),
        JsonValue::Number(value) => {
            if let Some(integer) = value.as_i64() {
                CfExpression::Integer(integer)
            } else if let Some(number) = value.as_f64() {
                CfExpression::Number(number)
            } else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation expression".to_string(),
                    reason: format!("unsupported JSON number '{value}'"),
                }));
            }
        }
        JsonValue::String(value) => CfExpression::from(value),
        JsonValue::Array(values) => CfExpression::list(
            values
                .into_iter()
                .map(cf_from_json)
                .collect::<Result<Vec<_>>>()?,
        ),
        JsonValue::Object(values) => CfExpression::Object(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, cf_from_json(value)?)))
                .collect::<Result<IndexMap<_, _>>>()?,
        ),
    })
}

/// Notification configuration for a storage resource based on the
/// stack's function triggers. Returns `None` if no functions trigger on
/// this storage.
pub fn storage_notification_configuration(ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
    let mut lambda_configurations = Vec::new();
    for (_id, entry) in ctx.stack.resources() {
        let Some(function) = entry.config.downcast_ref::<Function>() else {
            continue;
        };
        let Some(function_logical_id) = ctx.name_for(function.id()) else {
            continue;
        };
        for trigger in &function.triggers {
            let alien_core::FunctionTrigger::Storage { storage, events } = trigger else {
                continue;
            };
            if storage.resource_type == Storage::RESOURCE_TYPE && storage.id == ctx.resource_id {
                for event in storage_events(events) {
                    lambda_configurations.push(CfExpression::object([
                        ("Event", CfExpression::from(event)),
                        (
                            "Function",
                            CfExpression::get_att(function_logical_id, "Arn"),
                        ),
                    ]));
                }
            }
        }
    }

    if lambda_configurations.is_empty() {
        Ok(None)
    } else {
        Ok(Some(CfExpression::object([(
            "LambdaConfigurations",
            CfExpression::list(lambda_configurations),
        )])))
    }
}

/// Logical ids of `AWS::Lambda::Permission` resources that the storage
/// notification configuration must depend on (so the bucket isn't created
/// before the permission grants are in place).
pub fn storage_notification_dependencies(ctx: &EmitContext<'_>) -> Vec<String> {
    let mut dependencies = Vec::new();
    for (_id, entry) in ctx.stack.resources() {
        let Some(function) = entry.config.downcast_ref::<Function>() else {
            continue;
        };
        let Some(function_logical_id) = ctx.name_for(function.id()) else {
            continue;
        };
        if function.triggers.iter().any(|trigger| {
            matches!(
                trigger,
                alien_core::FunctionTrigger::Storage { storage, .. }
                    if storage.resource_type == Storage::RESOURCE_TYPE && storage.id == ctx.resource_id
            )
        }) {
            dependencies.push(format!(
                "{function_logical_id}StoragePermission{}",
                ctx.resource_id
            ));
        }
    }
    dependencies.sort();
    dependencies
}

fn storage_events(events: &[String]) -> Vec<String> {
    if events.is_empty() {
        return vec!["s3:ObjectCreated:*".to_string()];
    }

    events
        .iter()
        .map(|event| match event.as_str() {
            "created" | "object-created" | "ObjectCreated" => "s3:ObjectCreated:*".to_string(),
            "deleted" | "object-deleted" | "ObjectDeleted" => "s3:ObjectRemoved:*".to_string(),
            other if other.starts_with("s3:") => other.to_string(),
            other => format!("s3:{other}"),
        })
        .collect()
}
