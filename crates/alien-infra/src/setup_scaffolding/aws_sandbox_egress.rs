//! What keeps an AWS `egress: deny` sandbox's sessions inside the VPC: an operator role Lambda
//! assumes to place interfaces, a security group permitting only loopback, and the network
//! connector sessions start with.
//!
//! Created in that order, one mutating call per invocation, each recorded as soon as it exists.
//! A group or role this step did not record is adopted only after it is verified, because the
//! connector would carry whatever it permits to every session.

use alien_aws_clients::cloudcontrol::{CloudControlApi, CreateResourceRequest};
use alien_aws_clients::ec2::{
    AuthorizeSecurityGroupEgressRequest, CreateSecurityGroupRequest, DescribeSecurityGroupsRequest,
    Ec2Api, Filter, IpPermission, IpPermissionResponse, IpRange, Ipv6Range,
    RevokeSecurityGroupEgressRequest, SecurityGroup, Tag, TagSpecification, UserIdGroupPair,
};
use alien_aws_clients::iam::{CreateRoleRequest, IamApi};
use alien_aws_clients::AwsClientConfig;
use alien_core::sandbox_egress::{
    sandbox_egress_connector_name, sandbox_egress_name, sandbox_egress_operator_policy,
    sandbox_egress_operator_trust_policy, SandboxEgressConnector, LOOPBACK_ONLY_CIDR,
    NETWORK_CONNECTOR_TYPE_NAME, SANDBOX_EGRESS_POLICY_NAME,
};
use alien_core::{
    AwsSandboxEgressScaffolding, Network, NetworkSettings, ResourceLifecycle, ResourceStatus,
    Sandbox, SandboxEgress, Stack, StackState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use serde_json::Value;
use tracing::info;

use super::aws_sandbox::{
    adoption_mismatches, applied_policy, is_conflict, is_not_found, setup_tags,
};
use super::{ScaffoldingProgress, SetupScaffoldingContext};
use crate::network::AwsNetworkController;
use crate::sandbox::aws_partition;
use crate::{ErrorData, Result};

const IAM_ROLE_NAME_MAX_LEN: usize = 64;

/// The network a deny sandbox's connector attaches to, or `None` for a sandbox that needs none.
///
/// Refuses what the template emitters refuse: a mode with no connector configuration to render,
/// and a network with no private subnets, where a session would start with no connector and
/// reach the internet.
pub(super) fn egress_network<'a>(stack: &'a Stack, sandbox: &Sandbox) -> Result<Option<&'a str>> {
    let refuse = |reason: &str| {
        Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "an AWS sandbox routes session traffic through a VPC egress connector; {reason}"
            ),
            resource_id: Some(sandbox.id.clone()),
        }))
    };
    match &sandbox.egress {
        SandboxEgress::Allow => return Ok(None),
        SandboxEgress::AllowDomains { .. } => {
            return refuse(
                "egress 'allowDomains' has no connector configuration to render into. Declare \
                 egress: deny or egress: allow",
            )
        }
        SandboxEgress::Deny => {}
    }
    let Some((network_id, entry, network)) = stack
        .resources()
        .find_map(|(id, entry)| Some((id, entry, entry.config.downcast_ref::<Network>()?)))
    else {
        return refuse("this stack declares no network for it to attach to");
    };
    // Setup waits for the network to run before creating the connector; a network only the
    // runtime creates would never run during setup.
    if entry.lifecycle != ResourceLifecycle::Frozen {
        return refuse("its network must be created by setup, and this one is created at runtime");
    }
    match &network.settings {
        NetworkSettings::Create { .. } | NetworkSettings::ByoVpcAws { .. } => {
            Ok(Some(network_id.as_str()))
        }
        NetworkSettings::UseDefault => refuse(
            "the account's default VPC has only public subnets. Set the network to create or \
             byo-vpc-aws",
        ),
        _ => refuse("this stack's network settings are for another cloud"),
    }
}

pub(super) async fn reconcile(
    ctx: &SetupScaffoldingContext<'_>,
    aws: &AwsClientConfig,
    sandbox_id: &str,
    network_id: &str,
    stack_state: &StackState,
    record: &mut Option<AwsSandboxEgressScaffolding>,
) -> Result<ScaffoldingProgress> {
    let Some((vpc_id, private_subnet_ids)) = network_ready(stack_state, network_id, sandbox_id)?
    else {
        info!(
            sandbox_id,
            network_id, "Waiting for the network before sandbox egress"
        );
        return Ok(ScaffoldingProgress::InProgress);
    };
    let name = sandbox_egress_name(ctx.resource_prefix, sandbox_id);
    if name.len() > IAM_ROLE_NAME_MAX_LEN {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "the egress operator role name '{name}' is longer than IAM's \
                 {IAM_ROLE_NAME_MAX_LEN} characters; shorten the sandbox id or the prefix"
            ),
            resource_id: Some(sandbox_id.to_string()),
        }));
    }

    let iam = ctx.service_provider.get_aws_iam_client(aws).await?;
    if operator_role(ctx, aws, iam.as_ref(), sandbox_id, &name, record).await?
        == ScaffoldingProgress::InProgress
    {
        return Ok(ScaffoldingProgress::InProgress);
    }

    let ec2 = ctx.service_provider.get_aws_ec2_client(aws).await?;
    let Some(security_group_id) =
        deny_security_group(ctx, ec2.as_ref(), sandbox_id, &name, &vpc_id, record).await?
    else {
        return Ok(ScaffoldingProgress::InProgress);
    };

    let operator_role_arn = operator_role_arn(aws, &name);
    let desired = SandboxEgressConnector::builder()
        .resource_prefix(ctx.resource_prefix)
        .sandbox_id(sandbox_id)
        .operator_role_arn(&operator_role_arn)
        .private_subnet_ids(&private_subnet_ids)
        .security_group_id(&security_group_id)
        .build()
        .desired_state();
    let cloudcontrol = ctx
        .service_provider
        .get_aws_cloudcontrol_client(aws)
        .await?;
    connector(ctx, cloudcontrol.as_ref(), sandbox_id, &desired, record).await
}

/// The VPC and private subnets of a network setup has finished creating, or `None` while it has
/// not.
fn network_ready(
    stack_state: &StackState,
    network_id: &str,
    sandbox_id: &str,
) -> Result<Option<(String, Vec<String>)>> {
    let Some(state) = stack_state.resources.get(network_id) else {
        return Ok(None);
    };
    if state.status != ResourceStatus::Running {
        return Ok(None);
    }
    let Some(internal_state) = &state.internal_state else {
        return Ok(None);
    };
    let network: AwsNetworkController = serde_json::from_value(internal_state.clone())
        .into_alien_error()
        .context(ErrorData::ControllerStateTypeMismatch {
            expected: std::any::type_name::<AwsNetworkController>().to_string(),
            resource_id: network_id.to_string(),
        })?;
    match network.vpc_id {
        Some(vpc_id) if !network.private_subnet_ids.is_empty() => {
            Ok(Some((vpc_id, network.private_subnet_ids)))
        }
        _ => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!(
                "network '{network_id}' is running with no VPC or no private subnets, so the \
                 sandbox's egress connector has nowhere to attach"
            ),
            resource_id: Some(sandbox_id.to_string()),
        })),
    }
}

async fn operator_role(
    ctx: &SetupScaffoldingContext<'_>,
    aws: &AwsClientConfig,
    iam: &dyn IamApi,
    sandbox_id: &str,
    name: &str,
    record: &mut Option<AwsSandboxEgressScaffolding>,
) -> Result<ScaffoldingProgress> {
    let trust = sandbox_egress_operator_trust_policy();
    let role = match iam.get_role(name).await {
        Ok(response) => response.get_role_result.role,
        Err(error) if is_not_found(&error) => {
            let created = iam
                .create_role(
                    CreateRoleRequest::builder()
                        .role_name(name.to_string())
                        .assume_role_policy_document(trust.to_string())
                        .tags(setup_tags(ctx.resource_prefix, sandbox_id))
                        .build(),
                )
                .await;
            return match created {
                Ok(_) => {
                    info!(sandbox_id, role = %name, "Created sandbox egress operator role");
                    record_operator_role(record, name);
                    Ok(ScaffoldingProgress::InProgress)
                }
                Err(error) if is_conflict(&error) => Ok(ScaffoldingProgress::InProgress),
                Err(error) => Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create sandbox egress operator role '{name}'"),
                    resource_id: Some(sandbox_id.to_string()),
                }),
            };
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to read sandbox egress operator role '{name}'"),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    };

    let mismatches = adoption_mismatches(
        iam,
        &role,
        &operator_role_arn(aws, name),
        &trust,
        SANDBOX_EGRESS_POLICY_NAME,
    )
    .await?;
    if !mismatches.is_empty() {
        return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
            resource_id: sandbox_id.to_string(),
            object: format!("IAM role '{name}'"),
            reason: format!(
                "{}. Delete or rename it, then run setup again.",
                mismatches.join("; ")
            ),
        }));
    }
    record_operator_role(record, name);

    let policy =
        sandbox_egress_operator_policy(aws_partition(&aws.region), &aws.account_id, &aws.region);
    let applied = applied_policy(iam, name, SANDBOX_EGRESS_POLICY_NAME, sandbox_id).await?;
    if applied.as_ref() == Some(&policy) {
        return Ok(ScaffoldingProgress::Done);
    }
    iam.put_role_policy(name, SANDBOX_EGRESS_POLICY_NAME, &policy.to_string())
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to apply policy '{SANDBOX_EGRESS_POLICY_NAME}' to role '{name}'"
            ),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    Ok(ScaffoldingProgress::InProgress)
}

/// The group's id once its egress is exactly loopback, or `None` after a mutating call.
///
/// EC2 gives every new group an allow-all egress rule. Only a group this step recorded is
/// repaired toward loopback-only; any other same-named group must already be exactly that.
async fn deny_security_group(
    ctx: &SetupScaffoldingContext<'_>,
    ec2: &dyn Ec2Api,
    sandbox_id: &str,
    name: &str,
    vpc_id: &str,
    record: &mut Option<AwsSandboxEgressScaffolding>,
) -> Result<Option<String>> {
    let described = ec2
        .describe_security_groups(
            DescribeSecurityGroupsRequest::builder()
                .filters(vec![
                    Filter {
                        name: "group-name".to_string(),
                        values: vec![name.to_string()],
                    },
                    Filter {
                        name: "vpc-id".to_string(),
                        values: vec![vpc_id.to_string()],
                    },
                ])
                .build(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to look up security group '{name}' in VPC '{vpc_id}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    let group: Option<SecurityGroup> = described
        .security_group_info
        .and_then(|groups| groups.items.into_iter().next());

    let recorded_id = record
        .as_ref()
        .and_then(|egress| egress.security_group_id.clone());
    if let Some(recorded_id) = recorded_id {
        if group.as_ref().and_then(|group| group.group_id.as_deref()) != Some(&recorded_id) {
            forget_a_recorded_group_that_is_gone(ec2, sandbox_id, &recorded_id, vpc_id, record)
                .await?;
        }
    }

    let Some(group) = group else {
        let created = ec2
            .create_security_group(
                CreateSecurityGroupRequest::builder()
                    .group_name(name.to_string())
                    .description(format!("Sandbox {sandbox_id} session egress"))
                    .vpc_id(vpc_id.to_string())
                    .tag_specifications(vec![TagSpecification {
                        resource_type: "security-group".to_string(),
                        tags: setup_tags(ctx.resource_prefix, sandbox_id)
                            .into_iter()
                            .map(|tag| Tag {
                                key: tag.key,
                                value: tag.value,
                            })
                            .collect(),
                    }])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create security group '{name}' in VPC '{vpc_id}'"),
                resource_id: Some(sandbox_id.to_string()),
            })?;
        let group_id = created.group_id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("CreateSecurityGroup for '{name}' returned no group id"),
                resource_id: Some(sandbox_id.to_string()),
            })
        })?;
        info!(sandbox_id, security_group = %group_id, "Created sandbox deny security group");
        record_group(record, group_id);
        return Ok(None);
    };

    let group_id = group.group_id.clone().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("DescribeSecurityGroups returned '{name}' with no group id"),
            resource_id: Some(sandbox_id.to_string()),
        })
    })?;
    let rules: Vec<IpPermissionResponse> = group
        .ip_permissions_egress
        .clone()
        .map(|set| set.items)
        .unwrap_or_default();
    let recorded = record
        .as_ref()
        .and_then(|egress| egress.security_group_id.as_deref())
        == Some(group_id.as_str());
    // CreateSecurityGroup applies its tags atomically, so a group carrying setup's tags for this
    // sandbox is one this step created before its id could be recorded. Recording it lets the
    // repair below close EC2's default allow-all egress, which verification would refuse.
    let tagged = !recorded && carries_setup_tags(&group, ctx.resource_prefix, sandbox_id);
    if tagged {
        info!(sandbox_id, security_group = %group_id, "Recognised an unrecorded deny group by its setup tags");
        record_group(record, group_id.clone());
    }

    if !recorded && !tagged {
        if !is_loopback_only(&rules) {
            return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
                resource_id: sandbox_id.to_string(),
                object: format!("security group '{group_id}' ('{name}')"),
                reason: format!(
                    "its egress is not exactly {LOOPBACK_ONLY_CIDR} for all protocols, so \
                     sessions would reach past the deny. Delete it, then run setup again."
                ),
            }));
        }
        record_group(record, group_id.clone());
        return Ok(Some(group_id));
    }

    let foreign: Vec<&IpPermissionResponse> = rules
        .iter()
        .filter(|rule| !is_loopback_rule(rule))
        .collect();
    if !foreign.is_empty() {
        let revoke = foreign
            .into_iter()
            .map(|rule| revocable(rule, &group_id, sandbox_id))
            .collect::<Result<Vec<_>>>()?;
        ec2.revoke_security_group_egress(RevokeSecurityGroupEgressRequest {
            group_id: group_id.clone(),
            ip_permissions: revoke,
        })
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to revoke the open egress of security group '{group_id}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
        info!(sandbox_id, security_group = %group_id, "Revoked open egress from deny group");
        return Ok(None);
    }
    if rules.is_empty() {
        ec2.authorize_security_group_egress(AuthorizeSecurityGroupEgressRequest {
            group_id: group_id.clone(),
            ip_permissions: vec![IpPermission {
                ip_protocol: "-1".to_string(),
                from_port: None,
                to_port: None,
                ip_ranges: Some(vec![IpRange {
                    cidr_ip: LOOPBACK_ONLY_CIDR.to_string(),
                    description: Some("Sandbox sessions reach nothing outbound".to_string()),
                }]),
                ipv6_ranges: None,
                user_id_group_pairs: None,
            }],
        })
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to allow loopback egress on security group '{group_id}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
        return Ok(None);
    }
    Ok(Some(group_id))
}

/// Keeps the group and connector already recorded: a role recreated after it went missing must
/// not drop objects teardown still has to delete.
fn record_operator_role(record: &mut Option<AwsSandboxEgressScaffolding>, name: &str) {
    match record {
        Some(egress) => egress.operator_role_name = name.to_string(),
        None => {
            *record = Some(AwsSandboxEgressScaffolding {
                operator_role_name: name.to_string(),
                security_group_id: None,
                connector_arn: None,
                connector_request: None,
            })
        }
    }
}

fn carries_setup_tags(group: &SecurityGroup, resource_prefix: &str, sandbox_id: &str) -> bool {
    let tags: Vec<(&str, &str)> = group
        .tag_set
        .iter()
        .flat_map(|set| &set.items)
        .map(|tag| (tag.key.as_str(), tag.value.as_str()))
        .collect();
    setup_tags(resource_prefix, sandbox_id)
        .iter()
        .all(|expected| tags.contains(&(expected.key.as_str(), expected.value.as_str())))
}

/// A recorded group that is gone leaves the record, so setup makes a new one. One that
/// still exists outside this network's VPC cannot follow the sandbox there, and is refused.
async fn forget_a_recorded_group_that_is_gone(
    ec2: &dyn Ec2Api,
    sandbox_id: &str,
    recorded_id: &str,
    vpc_id: &str,
    record: &mut Option<AwsSandboxEgressScaffolding>,
) -> Result<()> {
    let described = ec2
        .describe_security_groups(
            DescribeSecurityGroupsRequest::builder()
                .filters(vec![Filter {
                    name: "group-id".to_string(),
                    values: vec![recorded_id.to_string()],
                }])
                .build(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to look up recorded security group '{recorded_id}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?;
    let still_there = described
        .security_group_info
        .and_then(|groups| groups.items.into_iter().next());
    if let Some(group) = still_there {
        return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
            resource_id: sandbox_id.to_string(),
            object: format!("security group '{recorded_id}'"),
            reason: format!(
                "setup recorded it for this sandbox in VPC '{}', and the sandbox's network is \
                 now VPC '{vpc_id}'. Its egress objects cannot move to another network: destroy \
                 the deployment and deploy it again.",
                group.vpc_id.as_deref().unwrap_or("unknown")
            ),
        }));
    }
    info!(sandbox_id, security_group = %recorded_id, "Recorded deny group is gone");
    if let Some(egress) = record {
        egress.security_group_id = None;
    }
    Ok(())
}

fn record_group(record: &mut Option<AwsSandboxEgressScaffolding>, group_id: String) {
    if let Some(egress) = record {
        egress.security_group_id = Some(group_id);
    }
}

/// One all-protocol rule to `127.0.0.1/32` and nothing else on it.
fn is_loopback_rule(rule: &IpPermissionResponse) -> bool {
    let cidrs: Vec<Option<&str>> = rule
        .ip_ranges
        .iter()
        .flat_map(|set| &set.items)
        .map(|range| range.cidr_ip.as_deref())
        .collect();
    rule.ip_protocol.as_deref() == Some("-1")
        && cidrs == [Some(LOOPBACK_ONLY_CIDR)]
        && rule
            .ipv6_ranges
            .as_ref()
            .is_none_or(|set| set.items.is_empty())
        && rule.groups.as_ref().is_none_or(|set| set.items.is_empty())
        && rule
            .prefix_list_ids
            .as_ref()
            .is_none_or(|set| set.items.is_empty())
}

fn is_loopback_only(rules: &[IpPermissionResponse]) -> bool {
    matches!(rules, [rule] if is_loopback_rule(rule))
}

/// The request form of a described rule, so revoking removes exactly it.
fn revocable(
    rule: &IpPermissionResponse,
    group_id: &str,
    sandbox_id: &str,
) -> Result<IpPermission> {
    if rule
        .prefix_list_ids
        .as_ref()
        .is_some_and(|set| !set.items.is_empty())
    {
        return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
            resource_id: sandbox_id.to_string(),
            object: format!("security group '{group_id}'"),
            reason: "an egress rule names a prefix list, which setup cannot revoke. Remove the \
                     rule or delete the group, then run setup again."
                .to_string(),
        }));
    }
    Ok(IpPermission {
        ip_protocol: rule.ip_protocol.clone().unwrap_or_else(|| "-1".to_string()),
        from_port: rule.from_port,
        to_port: rule.to_port,
        ip_ranges: rule.ip_ranges.as_ref().map(|set| {
            set.items
                .iter()
                .filter_map(|range| range.cidr_ip.clone())
                .map(|cidr_ip| IpRange {
                    cidr_ip,
                    description: None,
                })
                .collect()
        }),
        ipv6_ranges: rule.ipv6_ranges.as_ref().map(|set| {
            set.items
                .iter()
                .filter_map(|range| range.cidr_ipv6.clone())
                .map(|cidr_ipv6| Ipv6Range {
                    cidr_ipv6,
                    description: None,
                })
                .collect()
        }),
        user_id_group_pairs: rule.groups.as_ref().map(|set| {
            set.items
                .iter()
                .map(|pair| UserIdGroupPair {
                    group_id: pair.group_id.clone(),
                    user_id: pair.user_id.clone(),
                    description: None,
                })
                .collect()
        }),
    })
}

async fn connector(
    ctx: &SetupScaffoldingContext<'_>,
    cloudcontrol: &dyn CloudControlApi,
    sandbox_id: &str,
    desired: &Value,
    record: &mut Option<AwsSandboxEgressScaffolding>,
) -> Result<ScaffoldingProgress> {
    let name = sandbox_egress_connector_name(ctx.resource_prefix, sandbox_id);
    let Some(egress) = record.as_mut() else {
        unreachable!("the operator role step records the egress objects first")
    };
    if settle_connector_request(cloudcontrol, egress, &name, sandbox_id).await?
        == ScaffoldingProgress::InProgress
    {
        return Ok(ScaffoldingProgress::InProgress);
    }
    let existing = find_connector(
        cloudcontrol,
        &name,
        egress.connector_arn.as_deref(),
        sandbox_id,
    )
    .await?;

    let Some((arn, properties)) = existing else {
        let created = cloudcontrol
            .create_resource(
                CreateResourceRequest::builder()
                    .type_name(NETWORK_CONNECTOR_TYPE_NAME.to_string())
                    .desired_state(desired.to_string())
                    .build(),
            )
            .await;
        return match created {
            Ok(event) => {
                info!(sandbox_id, connector = %name, "Requested sandbox egress connector");
                egress.connector_request = Some(event.request_token);
                Ok(ScaffoldingProgress::InProgress)
            }
            // The name is unique per account and Region: a create that loses a race with an
            // earlier one is found by name on the next call and verified there.
            Err(error) if is_conflict(&error) => Ok(ScaffoldingProgress::InProgress),
            Err(error) => Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to create network connector '{name}'"),
                resource_id: Some(sandbox_id.to_string()),
            }),
        };
    };

    match properties["State"].as_str() {
        Some("PENDING") => return Ok(ScaffoldingProgress::InProgress),
        Some("ACTIVE") => {}
        other => {
            return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
                resource_id: sandbox_id.to_string(),
                object: format!("network connector '{arn}' ('{name}')"),
                reason: format!(
                    "it is in state {}, not ACTIVE. Delete it, then run setup again.",
                    other.unwrap_or("unknown")
                ),
            }))
        }
    }
    let mismatches = connector_mismatches(&properties, desired);
    if !mismatches.is_empty() {
        return Err(AlienError::new(ErrorData::SetupScaffoldingNotAdoptable {
            resource_id: sandbox_id.to_string(),
            object: format!("network connector '{arn}' ('{name}')"),
            reason: format!(
                "{}. Delete it, then run setup again.",
                mismatches.join("; ")
            ),
        }));
    }
    egress.connector_arn = Some(arn);
    Ok(ScaffoldingProgress::Done)
}

/// Reads the outcome of the connector's pending Cloud Control request, once. `InProgress` while
/// AWS still runs it; `Done` when there is none left and the connector itself can be read.
///
/// A FAILED request is an error carrying AWS's code and message. Its token is cleared first, and
/// callers persist the record on error, so the next run starts a new request instead of reading
/// the same failure again. `AlreadyExists` and `NotFound` outcomes are what reading the connector
/// settles anyway. Cloud Control forgets old requests; a token it no longer knows is dropped.
async fn settle_connector_request(
    cloudcontrol: &dyn CloudControlApi,
    egress: &mut AwsSandboxEgressScaffolding,
    name: &str,
    sandbox_id: &str,
) -> Result<ScaffoldingProgress> {
    let Some(token) = egress.connector_request.clone() else {
        return Ok(ScaffoldingProgress::Done);
    };
    let event = match cloudcontrol.get_resource_request_status(&token).await {
        Ok(event) => event,
        Err(error) if is_not_found(&error) => {
            egress.connector_request = None;
            return Ok(ScaffoldingProgress::Done);
        }
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to read Cloud Control request '{token}'"),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    };
    if !event.is_terminal() {
        return Ok(ScaffoldingProgress::InProgress);
    }
    egress.connector_request = None;
    match event.failure() {
        None => Ok(ScaffoldingProgress::Done),
        Some(failure) if is_conflict(&failure) || is_not_found(&failure) => {
            Ok(ScaffoldingProgress::Done)
        }
        Some(failure) => Err(failure).context(ErrorData::CloudPlatformError {
            message: format!("Cloud Control request for network connector '{name}' failed"),
            resource_id: Some(sandbox_id.to_string()),
        }),
    }
}

/// Tags are not compared: AWS adds its own, and they grant nothing.
fn connector_mismatches(found: &Value, desired: &Value) -> Vec<String> {
    let found_vpc = &found["Configuration"]["VpcEgressConfiguration"];
    let desired_vpc = &desired["Configuration"]["VpcEgressConfiguration"];
    let mut mismatches = Vec::new();
    if found["OperatorRole"] != desired["OperatorRole"] {
        mismatches.push(format!(
            "its operator role is {}, not {}",
            found["OperatorRole"], desired["OperatorRole"]
        ));
    }
    for key in [
        "SecurityGroupIds",
        "SubnetIds",
        "AssociatedComputeResourceTypes",
    ] {
        if sorted(&found_vpc[key]) != sorted(&desired_vpc[key]) {
            mismatches.push(format!(
                "its {key} are {}, not {}",
                found_vpc[key], desired_vpc[key]
            ));
        }
    }
    mismatches
}

fn sorted(list: &Value) -> Option<Vec<String>> {
    let mut items: Vec<String> = list
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect::<Option<_>>()?;
    items.sort();
    Some(items)
}

/// The connector by its recorded ARN, else by its unique name, with its properties.
async fn find_connector(
    cloudcontrol: &dyn CloudControlApi,
    name: &str,
    recorded_arn: Option<&str>,
    sandbox_id: &str,
) -> Result<Option<(String, Value)>> {
    if let Some(arn) = recorded_arn {
        if let Some(found) = read_connector(cloudcontrol, arn, sandbox_id).await? {
            return Ok(Some(found));
        }
    }
    let mut next_token = None;
    loop {
        let page = cloudcontrol
            .list_resources(NETWORK_CONNECTOR_TYPE_NAME, next_token)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to list network connectors".to_string(),
                resource_id: Some(sandbox_id.to_string()),
            })?;
        for description in page.resource_descriptions {
            let listed_name = description
                .properties
                .as_deref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .and_then(|properties| properties["Name"].as_str().map(str::to_string));
            if listed_name.as_deref().is_some_and(|listed| listed != name) {
                continue;
            }
            if let Some((arn, properties)) =
                read_connector(cloudcontrol, &description.identifier, sandbox_id).await?
            {
                if properties["Name"].as_str() == Some(name) {
                    return Ok(Some((arn, properties)));
                }
            }
        }
        match page.next_token {
            Some(token) => next_token = Some(token),
            None => return Ok(None),
        }
    }
}

async fn read_connector(
    cloudcontrol: &dyn CloudControlApi,
    identifier: &str,
    sandbox_id: &str,
) -> Result<Option<(String, Value)>> {
    let description = match cloudcontrol
        .get_resource(NETWORK_CONNECTOR_TYPE_NAME, identifier)
        .await
    {
        Ok(description) => description,
        Err(error) if is_not_found(&error) => return Ok(None),
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to read network connector '{identifier}'"),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    };
    let properties = description
        .properties
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Network connector '{identifier}' has unreadable properties"),
            resource_id: Some(sandbox_id.to_string()),
        })?
        .unwrap_or(Value::Null);
    Ok(Some((description.identifier, properties)))
}

/// The egress objects setup created for this sandbox, found by their names and setup's tags, for
/// a record that never learned of them. `None` without the operator role, which setup creates
/// first and teardown deletes by name.
pub(super) async fn recover(
    ctx: &SetupScaffoldingContext<'_>,
    aws: &AwsClientConfig,
    iam: &dyn IamApi,
    sandbox_id: &str,
) -> Result<Option<AwsSandboxEgressScaffolding>> {
    let name = sandbox_egress_name(ctx.resource_prefix, sandbox_id);
    if !carries_setup_role_tags(iam, &name, ctx.resource_prefix, sandbox_id).await? {
        return Ok(None);
    }

    let ec2 = ctx.service_provider.get_aws_ec2_client(aws).await?;
    let mut filters = vec![Filter {
        name: "group-name".to_string(),
        values: vec![name.clone()],
    }];
    filters.extend(
        setup_tags(ctx.resource_prefix, sandbox_id)
            .into_iter()
            .map(|tag| Filter {
                name: format!("tag:{}", tag.key),
                values: vec![tag.value],
            }),
    );
    let security_group_id = ec2
        .describe_security_groups(
            DescribeSecurityGroupsRequest::builder()
                .filters(filters)
                .build(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to look up security group '{name}'"),
            resource_id: Some(sandbox_id.to_string()),
        })?
        .security_group_info
        .and_then(|groups| groups.items.into_iter().next())
        .and_then(|group| group.group_id);

    let cloudcontrol = ctx
        .service_provider
        .get_aws_cloudcontrol_client(aws)
        .await?;
    let connector_name = sandbox_egress_connector_name(ctx.resource_prefix, sandbox_id);
    let operator_role_arn = operator_role_arn(aws, &name);
    let connector_arn = find_connector(cloudcontrol.as_ref(), &connector_name, None, sandbox_id)
        .await?
        .filter(|(_, properties)| {
            is_setups_connector(
                properties,
                ctx.resource_prefix,
                sandbox_id,
                &operator_role_arn,
            )
        })
        .map(|(arn, _)| arn);

    Ok(Some(AwsSandboxEgressScaffolding {
        operator_role_name: name,
        security_group_id,
        connector_arn,
        connector_request: None,
    }))
}

fn operator_role_arn(aws: &AwsClientConfig, name: &str) -> String {
    format!(
        "arn:{}:iam::{}:role/{name}",
        aws_partition(&aws.region),
        aws.account_id
    )
}

/// Setup's tags, or the operator role setup made for this sandbox: either marks a same-named
/// connector as setup's. The role holds whether or not Cloud Control reads the tags back.
fn is_setups_connector(
    properties: &Value,
    resource_prefix: &str,
    sandbox_id: &str,
    operator_role_arn: &str,
) -> bool {
    let tags = properties["Tags"].as_array().cloned().unwrap_or_default();
    properties["OperatorRole"].as_str() == Some(operator_role_arn)
        || setup_tags(resource_prefix, sandbox_id)
            .into_iter()
            .all(|tag| tags.contains(&serde_json::json!({ "Key": tag.key, "Value": tag.value })))
}

pub(super) async fn carries_setup_role_tags(
    iam: &dyn IamApi,
    role_name: &str,
    resource_prefix: &str,
    sandbox_id: &str,
) -> Result<bool> {
    let role = match iam.get_role(role_name).await {
        Ok(response) => response.get_role_result.role,
        Err(error) if is_not_found(&error) => return Ok(false),
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to read role '{role_name}'"),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    };
    let tags: Vec<(String, String)> = role
        .tags
        .map(|tags| tags.member)
        .unwrap_or_default()
        .into_iter()
        .map(|tag| (tag.key, tag.value))
        .collect();
    Ok(setup_tags(resource_prefix, sandbox_id)
        .into_iter()
        .all(|expected| tags.contains(&(expected.key, expected.value))))
}

/// Connector, then group, then role: the connector's interfaces hold the group, and the group
/// cannot go until AWS releases them, which it does some time after the connector is gone.
pub(super) async fn teardown(
    ctx: &SetupScaffoldingContext<'_>,
    aws: &AwsClientConfig,
    sandbox_id: &str,
    egress: &mut AwsSandboxEgressScaffolding,
) -> Result<ScaffoldingProgress> {
    let cloudcontrol = ctx
        .service_provider
        .get_aws_cloudcontrol_client(aws)
        .await?;
    let name = sandbox_egress_connector_name(ctx.resource_prefix, sandbox_id);
    if settle_connector_request(cloudcontrol.as_ref(), egress, &name, sandbox_id).await?
        == ScaffoldingProgress::InProgress
    {
        return Ok(ScaffoldingProgress::InProgress);
    }
    // Found by name, a connector is deleted only if it is the recorded one or setup's by
    // `is_setups_connector`: the name alone does not make it setup's.
    let operator_role_arn = operator_role_arn(aws, &egress.operator_role_name);
    let found = find_connector(
        cloudcontrol.as_ref(),
        &name,
        egress.connector_arn.as_deref(),
        sandbox_id,
    )
    .await?
    .filter(|(arn, properties)| {
        egress.connector_arn.as_deref() == Some(arn.as_str())
            || is_setups_connector(
                properties,
                ctx.resource_prefix,
                sandbox_id,
                &operator_role_arn,
            )
    });
    if let Some((arn, _)) = found {
        match cloudcontrol
            .delete_resource(NETWORK_CONNECTOR_TYPE_NAME, &arn)
            .await
        {
            Ok(event) => {
                info!(sandbox_id, connector = %arn, "Requested sandbox egress connector deletion");
                egress.connector_request = Some(event.request_token);
                return Ok(ScaffoldingProgress::InProgress);
            }
            Err(error) if is_not_found(&error) => {}
            // Another delete of the same connector is still running.
            Err(error) if is_conflict(&error) => return Ok(ScaffoldingProgress::InProgress),
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete network connector '{arn}'"),
                    resource_id: Some(sandbox_id.to_string()),
                })
            }
        }
    }
    egress.connector_arn = None;

    if let Some(group_id) = egress.security_group_id.clone() {
        let ec2 = ctx.service_provider.get_aws_ec2_client(aws).await?;
        match ec2.delete_security_group(&group_id).await {
            Ok(()) => info!(sandbox_id, security_group = %group_id, "Deleted deny group"),
            Err(error) if is_not_found(&error) => {}
            // EC2's DependencyViolation, which the client maps to a conflict.
            Err(error) if is_conflict(&error) => {
                info!(
                    sandbox_id,
                    security_group = %group_id,
                    "Deny group still has network interfaces; retrying on the next call"
                );
                return Ok(ScaffoldingProgress::InProgress);
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to delete security group '{group_id}'"),
                    resource_id: Some(sandbox_id.to_string()),
                })
            }
        }
        egress.security_group_id = None;
    }

    let iam = ctx.service_provider.get_aws_iam_client(aws).await?;
    let role = egress.operator_role_name.as_str();
    match iam
        .delete_role_policy(role, SANDBOX_EGRESS_POLICY_NAME)
        .await
    {
        Ok(()) => {}
        Err(error) if is_not_found(&error) => {}
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to delete policy '{SANDBOX_EGRESS_POLICY_NAME}' from role '{role}'"
                ),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    }
    match iam.delete_role(role).await {
        Ok(()) => {}
        Err(error) if is_not_found(&error) => {}
        Err(error) => {
            return Err(error).context(ErrorData::CloudPlatformError {
                message: format!("Failed to delete sandbox egress operator role '{role}'"),
                resource_id: Some(sandbox_id.to_string()),
            })
        }
    }
    info!(sandbox_id, role = %role, "Deleted sandbox egress operator role");
    Ok(ScaffoldingProgress::Done)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::controller_test::SingleControllerExecutor;
    use crate::core::{MockPlatformServiceProvider, ResourceController as _};
    use crate::sandbox::AwsSandboxController;
    use crate::setup_scaffolding::{
        apply_seeds, reconcile as reconcile_all, seeds, teardown as teardown_all, SeedContext,
    };
    use alien_aws_clients::cloudcontrol::{
        ListResourcesResponse, MockCloudControlApi, OperationStatus, ProgressEvent,
        ResourceDescription,
    };
    use alien_aws_clients::ec2::{
        CreateSecurityGroupResponse, DescribeSecurityGroupsResponse, IpPermissionSet,
        IpRangeResponse, IpRangeSet, Ipv6RangeResponse, Ipv6RangeSet, MockEc2Api,
        PrefixListIdResponse, PrefixListIdSet, SecurityGroupSet, UserIdGroupPairResponse,
        UserIdGroupPairSet,
    };
    use alien_aws_clients::iam::{
        AttachedPolicies, CreateRoleResponse, CreateRoleResult, GetRolePolicyResponse,
        GetRolePolicyResult, GetRoleResponse, GetRoleResult, ListAttachedRolePoliciesResponse,
        ListAttachedRolePoliciesResult, ListRolePoliciesResponse, ListRolePoliciesResult,
        MockIamApi, PolicyNames, Role,
    };
    use alien_aws_clients::lambda_microvms::{
        CreateMicrovmImageResponse, MicrovmImage, MicrovmImageVersion, MockLambdaMicrovmsApi,
    };
    use alien_aws_clients::AwsClientConfigExt as _;
    use alien_bindings::{BindingsProvider, BindingsProviderApi};
    use alien_client_core::ErrorData as CloudError;
    use alien_core::bindings::SandboxBinding;
    use alien_core::import::ImportContext;
    use alien_core::{
        ClientConfig, Platform, Resource, ResourceLifecycle, ResourceRef, SandboxCode,
        SandboxLifecyclePolicy, SetupScaffolding, StackResourceState,
    };
    use serde_json::json;

    const PREFIX: &str = "test";
    const ACCOUNT: &str = "123456789012";
    const BUILD_ROLE: &str = "test-agents-build";
    const EGRESS_NAME: &str = "test-agents-egress";
    const OPERATOR_ARN: &str = "arn:aws:iam::123456789012:role/test-agents-egress";
    const VPC: &str = "vpc-0sandbox";

    fn subnets() -> Vec<String> {
        vec!["subnet-a".to_string(), "subnet-b".to_string()]
    }

    /// One egress rule as EC2 describes it.
    #[derive(Debug, Clone, PartialEq)]
    struct Rule {
        protocol: String,
        cidrs: Vec<String>,
        ipv6_cidrs: Vec<String>,
        groups: Vec<String>,
        prefix_lists: Vec<String>,
    }

    fn rule(cidr: &str) -> Rule {
        Rule {
            protocol: "-1".to_string(),
            cidrs: vec![cidr.to_string()],
            ipv6_cidrs: vec![],
            groups: vec![],
            prefix_lists: vec![],
        }
    }

    #[derive(Debug, Clone, Default)]
    struct Group {
        id: String,
        name: String,
        vpc: String,
        egress: Vec<Rule>,
        tags: Vec<(String, String)>,
    }

    /// An in-memory account: what exists, and every mutating call made against it, in order.
    #[derive(Default)]
    struct Cloud {
        roles: BTreeMap<String, Value>,
        role_tags: BTreeMap<String, Vec<(String, String)>>,
        inline: BTreeMap<(String, String), String>,
        attached: BTreeMap<String, Vec<String>>,
        groups: Vec<Group>,
        connectors: Vec<(String, Value)>,
        requests: BTreeMap<String, ProgressEvent>,
        mutations: Vec<String>,
        dependency_violations: usize,
        /// The handler code and message the next create's request ends FAILED with.
        failing_create: Option<(String, String)>,
        /// The state a created connector reports.
        created_state: Option<&'static str>,
        next_id: usize,
        /// The 1-based mutating call whose effect lands but whose response is lost.
        lose_response_to: Option<usize>,
        /// Cloud Control reports every request as still running.
        requests_in_flight: bool,
    }

    impl Cloud {
        /// Called after a mutating call has taken effect.
        fn respond(&mut self) -> std::result::Result<(), AlienError<CloudError>> {
            if self.lose_response_to == Some(self.mutations.len()) {
                self.lose_response_to = None;
                return Err(AlienError::new(CloudError::Timeout {
                    message: format!("no response to {}", self.mutations.last().unwrap()),
                }));
            }
            Ok(())
        }
    }

    type Shared = Arc<Mutex<Cloud>>;

    fn not_found(name: &str) -> AlienError<CloudError> {
        AlienError::new(CloudError::RemoteResourceNotFound {
            resource_type: "fake".to_string(),
            resource_name: name.to_string(),
        })
    }

    fn event(token: &str, status: OperationStatus, arn: Option<&str>) -> ProgressEvent {
        ProgressEvent {
            type_name: Some(NETWORK_CONNECTOR_TYPE_NAME.to_string()),
            identifier: arn.map(str::to_string),
            request_token: token.to_string(),
            operation: None,
            operation_status: status,
            status_message: None,
            error_code: None,
        }
    }

    fn tagged_role(name: &str, trust: &Value, tags: &[(String, String)]) -> Role {
        Role {
            tags: Some(alien_aws_clients::iam::Tags {
                member: tags
                    .iter()
                    .map(|(key, value)| alien_aws_clients::iam::Tag {
                        key: key.clone(),
                        value: value.clone(),
                    })
                    .collect(),
            }),
            ..role(name, trust)
        }
    }

    fn role(name: &str, trust: &Value) -> Role {
        Role {
            path: "/".to_string(),
            role_name: name.to_string(),
            role_id: "AROAEXAMPLE".to_string(),
            arn: format!("arn:aws:iam::{ACCOUNT}:role/{name}"),
            create_date: "2026-09-23T00:00:00Z".to_string(),
            assume_role_policy_document: Some(urlencoding::encode(&trust.to_string()).into()),
            description: None,
            max_session_duration: None,
            permissions_boundary: None,
            tags: None,
            role_last_used: None,
        }
    }

    fn iam(cloud: &Shared) -> MockIamApi {
        let mut iam = MockIamApi::new();
        let c = cloud.clone();
        iam.expect_get_role().returning(move |name| {
            let cloud = c.lock().unwrap();
            let trust = cloud.roles.get(name).ok_or_else(|| not_found(name))?;
            let tags = cloud.role_tags.get(name).cloned().unwrap_or_default();
            Ok(GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: tagged_role(name, trust, &tags),
                },
            })
        });
        let c = cloud.clone();
        iam.expect_create_role().returning(move |request| {
            let mut cloud = c.lock().unwrap();
            let trust: Value = serde_json::from_str(&request.assume_role_policy_document).unwrap();
            cloud
                .mutations
                .push(format!("iam:CreateRole {}", request.role_name));
            cloud.roles.insert(request.role_name.clone(), trust.clone());
            let tags = request
                .tags
                .iter()
                .flatten()
                .map(|tag| (tag.key.clone(), tag.value.clone()))
                .collect();
            cloud.role_tags.insert(request.role_name.clone(), tags);
            cloud.respond()?;
            Ok(CreateRoleResponse {
                create_role_result: CreateRoleResult {
                    role: role(&request.role_name, &trust),
                },
            })
        });
        let c = cloud.clone();
        iam.expect_list_role_policies().returning(move |name| {
            let cloud = c.lock().unwrap();
            Ok(ListRolePoliciesResponse {
                list_role_policies_result: ListRolePoliciesResult {
                    policy_names: Some(PolicyNames {
                        member: cloud
                            .inline
                            .keys()
                            .filter(|(r, _)| r == name)
                            .map(|(_, p)| p.clone())
                            .collect(),
                    }),
                    is_truncated: Some(false),
                    marker: None,
                },
            })
        });
        let c = cloud.clone();
        iam.expect_list_attached_role_policies()
            .returning(move |name| {
                let cloud = c.lock().unwrap();
                Ok(ListAttachedRolePoliciesResponse {
                    list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                        attached_policies: Some(AttachedPolicies {
                            member: cloud
                                .attached
                                .get(name)
                                .into_iter()
                                .flatten()
                                .map(|arn| alien_aws_clients::iam::AttachedPolicy {
                                    policy_name: "p".to_string(),
                                    policy_arn: arn.clone(),
                                })
                                .collect(),
                        }),
                        is_truncated: Some(false),
                        marker: None,
                    },
                })
            });
        let c = cloud.clone();
        iam.expect_get_role_policy().returning(move |role, policy| {
            let cloud = c.lock().unwrap();
            let document = cloud
                .inline
                .get(&(role.to_string(), policy.to_string()))
                .ok_or_else(|| not_found(policy))?;
            Ok(GetRolePolicyResponse {
                get_role_policy_result: GetRolePolicyResult {
                    role_name: role.to_string(),
                    policy_name: policy.to_string(),
                    policy_document: urlencoding::encode(document).into(),
                },
            })
        });
        let c = cloud.clone();
        iam.expect_put_role_policy()
            .returning(move |role, policy, document| {
                let mut cloud = c.lock().unwrap();
                cloud
                    .mutations
                    .push(format!("iam:PutRolePolicy {role} {policy}"));
                cloud
                    .inline
                    .insert((role.to_string(), policy.to_string()), document.to_string());
                cloud.respond()
            });
        let c = cloud.clone();
        iam.expect_delete_role_policy()
            .returning(move |role, policy| {
                let mut cloud = c.lock().unwrap();
                cloud
                    .mutations
                    .push(format!("iam:DeleteRolePolicy {role} {policy}"));
                cloud
                    .inline
                    .remove(&(role.to_string(), policy.to_string()))
                    .ok_or_else(|| not_found(policy))?;
                cloud.respond()
            });
        let c = cloud.clone();
        iam.expect_delete_role().returning(move |role| {
            let mut cloud = c.lock().unwrap();
            cloud.mutations.push(format!("iam:DeleteRole {role}"));
            cloud.roles.remove(role).ok_or_else(|| not_found(role))?;
            cloud.role_tags.remove(role);
            cloud.respond()
        });
        iam
    }

    fn described(group: &Group) -> SecurityGroup {
        SecurityGroup {
            group_id: Some(group.id.clone()),
            group_name: Some(group.name.clone()),
            vpc_id: Some(group.vpc.clone()),
            owner_id: None,
            group_description: None,
            ip_permissions: None,
            ip_permissions_egress: Some(IpPermissionSet {
                items: group
                    .egress
                    .iter()
                    .map(|rule| IpPermissionResponse {
                        ip_protocol: Some(rule.protocol.clone()),
                        from_port: None,
                        to_port: None,
                        ip_ranges: Some(IpRangeSet {
                            items: rule
                                .cidrs
                                .iter()
                                .map(|cidr| IpRangeResponse {
                                    cidr_ip: Some(cidr.clone()),
                                    description: None,
                                })
                                .collect(),
                        }),
                        ipv6_ranges: Some(Ipv6RangeSet {
                            items: rule
                                .ipv6_cidrs
                                .iter()
                                .map(|cidr| Ipv6RangeResponse {
                                    cidr_ipv6: Some(cidr.clone()),
                                    description: None,
                                })
                                .collect(),
                        }),
                        groups: Some(UserIdGroupPairSet {
                            items: rule
                                .groups
                                .iter()
                                .map(|id| UserIdGroupPairResponse {
                                    group_id: Some(id.clone()),
                                    user_id: None,
                                    description: None,
                                })
                                .collect(),
                        }),
                        prefix_list_ids: Some(PrefixListIdSet {
                            items: rule
                                .prefix_lists
                                .iter()
                                .map(|id| PrefixListIdResponse {
                                    prefix_list_id: Some(id.clone()),
                                    description: None,
                                })
                                .collect(),
                        }),
                    })
                    .collect(),
            }),
            tag_set: Some(alien_aws_clients::ec2::TagSet {
                items: group
                    .tags
                    .iter()
                    .map(|(key, value)| Tag {
                        key: key.clone(),
                        value: value.clone(),
                    })
                    .collect(),
            }),
        }
    }

    fn requested(permissions: &[IpPermission]) -> Vec<Rule> {
        permissions
            .iter()
            .map(|permission| Rule {
                protocol: permission.ip_protocol.clone(),
                cidrs: permission
                    .ip_ranges
                    .iter()
                    .flatten()
                    .map(|range| range.cidr_ip.clone())
                    .collect(),
                ipv6_cidrs: vec![],
                groups: vec![],
                prefix_lists: vec![],
            })
            .collect()
    }

    fn ec2(cloud: &Shared) -> MockEc2Api {
        let mut ec2 = MockEc2Api::new();
        let c = cloud.clone();
        ec2.expect_describe_security_groups()
            .returning(move |request| {
                let filter = |key: &str| {
                    request
                        .filters
                        .iter()
                        .flatten()
                        .find(|f| f.name == key)
                        .map(|f| f.values.clone())
                };
                assert!(
                    request.group_ids.is_none(),
                    "GroupIds fails on a missing group; look it up by the group-id filter"
                );
                let cloud = c.lock().unwrap();
                let matching: Vec<SecurityGroup> = match filter("group-id") {
                    Some(ids) => cloud
                        .groups
                        .iter()
                        .filter(|g| ids.contains(&g.id))
                        .map(described)
                        .collect(),
                    None => {
                        let names = filter("group-name").expect("the lookup filters by name");
                        let vpcs = filter("vpc-id");
                        let tags: Vec<(String, Vec<String>)> = request
                            .filters
                            .iter()
                            .flatten()
                            .filter_map(|f| {
                                Some((f.name.strip_prefix("tag:")?.to_string(), f.values.clone()))
                            })
                            .collect();
                        cloud
                            .groups
                            .iter()
                            .filter(|g| {
                                names.contains(&g.name)
                                    && vpcs.as_ref().is_none_or(|vpcs| vpcs.contains(&g.vpc))
                                    && tags.iter().all(|(key, values)| {
                                        g.tags.iter().any(|(k, v)| k == key && values.contains(v))
                                    })
                            })
                            .map(described)
                            .collect()
                    }
                };
                Ok(DescribeSecurityGroupsResponse {
                    security_group_info: Some(SecurityGroupSet { items: matching }),
                    next_token: None,
                })
            });
        let c = cloud.clone();
        ec2.expect_create_security_group()
            .returning(move |request| {
                let mut cloud = c.lock().unwrap();
                cloud.next_id += 1;
                let id = format!("sg-{}", cloud.next_id);
                cloud
                    .mutations
                    .push(format!("ec2:CreateSecurityGroup {}", request.group_name));
                // EC2's default for any new group.
                cloud.groups.push(Group {
                    id: id.clone(),
                    name: request.group_name,
                    vpc: request.vpc_id,
                    egress: vec![rule("0.0.0.0/0")],
                    tags: request
                        .tag_specifications
                        .iter()
                        .flatten()
                        .flat_map(|spec| &spec.tags)
                        .map(|tag| (tag.key.clone(), tag.value.clone()))
                        .collect(),
                });
                cloud.respond()?;
                Ok(CreateSecurityGroupResponse { group_id: Some(id) })
            });
        let c = cloud.clone();
        ec2.expect_revoke_security_group_egress()
            .returning(move |request| {
                let mut cloud = c.lock().unwrap();
                let revoked = requested(&request.ip_permissions);
                cloud.mutations.push(format!(
                    "ec2:RevokeSecurityGroupEgress {:?}",
                    revoked
                        .iter()
                        .flat_map(|r| r.cidrs.clone())
                        .collect::<Vec<_>>()
                ));
                let group = cloud
                    .groups
                    .iter_mut()
                    .find(|g| g.id == request.group_id)
                    .ok_or_else(|| not_found(&request.group_id))?;
                group.egress.retain(|rule| !revoked.contains(rule));
                cloud.respond()
            });
        let c = cloud.clone();
        ec2.expect_authorize_security_group_egress()
            .returning(move |request| {
                let mut cloud = c.lock().unwrap();
                let added = requested(&request.ip_permissions);
                cloud.mutations.push(format!(
                    "ec2:AuthorizeSecurityGroupEgress {:?}",
                    added
                        .iter()
                        .flat_map(|r| r.cidrs.clone())
                        .collect::<Vec<_>>()
                ));
                let group = cloud
                    .groups
                    .iter_mut()
                    .find(|g| g.id == request.group_id)
                    .ok_or_else(|| not_found(&request.group_id))?;
                group.egress.extend(added);
                cloud.respond()
            });
        let c = cloud.clone();
        ec2.expect_delete_security_group()
            .returning(move |group_id| {
                let mut cloud = c.lock().unwrap();
                cloud
                    .mutations
                    .push(format!("ec2:DeleteSecurityGroup {group_id}"));
                if cloud.dependency_violations > 0 {
                    cloud.dependency_violations -= 1;
                    return Err(AlienError::new(CloudError::RemoteResourceConflict {
                        message: "DependencyViolation: resource has a dependent object".to_string(),
                        resource_type: "SecurityGroup".to_string(),
                        resource_name: group_id.to_string(),
                    }));
                }
                let before = cloud.groups.len();
                cloud.groups.retain(|g| g.id != group_id);
                if cloud.groups.len() == before {
                    return Err(not_found(group_id));
                }
                cloud.respond()
            });
        ec2
    }

    fn cloudcontrol(cloud: &Shared) -> MockCloudControlApi {
        let mut cc = MockCloudControlApi::new();
        let c = cloud.clone();
        cc.expect_create_resource().returning(move |request| {
            let mut cloud = c.lock().unwrap();
            assert_eq!(request.type_name, NETWORK_CONNECTOR_TYPE_NAME);
            cloud.next_id += 1;
            let arn = format!(
                "arn:aws:lambda:us-east-1:{ACCOUNT}:network-connector:nc-{}",
                cloud.next_id
            );
            let token = format!("create-{}", cloud.next_id);
            cloud.mutations.push(format!(
                "cloudcontrol:CreateResource {}",
                request.desired_state
            ));
            if let Some((code, message)) = cloud.failing_create.take() {
                let mut failed = event(&token, OperationStatus::Failed, None);
                failed.error_code = Some(code);
                failed.status_message = Some(message);
                cloud.requests.insert(token.clone(), failed);
                return Ok(event(&token, OperationStatus::InProgress, None));
            }
            let mut properties: Value = serde_json::from_str(&request.desired_state).unwrap();
            properties["Arn"] = json!(arn);
            properties["State"] = json!(cloud.created_state.unwrap_or("ACTIVE"));
            cloud.connectors.push((arn.clone(), properties));
            cloud.requests.insert(
                token.clone(),
                event(&token, OperationStatus::Success, Some(&arn)),
            );
            cloud.respond()?;
            Ok(event(&token, OperationStatus::InProgress, None))
        });
        let c = cloud.clone();
        cc.expect_get_resource_request_status()
            .returning(move |token| {
                let cloud = c.lock().unwrap();
                if cloud.requests_in_flight && cloud.requests.contains_key(token) {
                    return Ok(event(token, OperationStatus::InProgress, None));
                }
                cloud
                    .requests
                    .get(token)
                    .cloned()
                    .ok_or_else(|| not_found(token))
            });
        let c = cloud.clone();
        cc.expect_get_resource().returning(move |_, identifier| {
            let cloud = c.lock().unwrap();
            let (arn, properties) = cloud
                .connectors
                .iter()
                .find(|(arn, _)| arn == identifier)
                .ok_or_else(|| not_found(identifier))?;
            Ok(ResourceDescription {
                identifier: arn.clone(),
                properties: Some(properties.to_string()),
            })
        });
        let c = cloud.clone();
        cc.expect_list_resources().returning(move |_, _| {
            let cloud = c.lock().unwrap();
            // The list handler returns state only, so a caller must read each one for its name.
            Ok(ListResourcesResponse {
                resource_descriptions: cloud
                    .connectors
                    .iter()
                    .map(|(arn, properties)| ResourceDescription {
                        identifier: arn.clone(),
                        properties: Some(
                            json!({ "Arn": arn, "State": properties["State"] }).to_string(),
                        ),
                    })
                    .collect(),
                next_token: None,
            })
        });
        let c = cloud.clone();
        cc.expect_delete_resource().returning(move |_, identifier| {
            let mut cloud = c.lock().unwrap();
            cloud
                .mutations
                .push(format!("cloudcontrol:DeleteResource {identifier}"));
            let before = cloud.connectors.len();
            cloud.connectors.retain(|(arn, _)| arn != identifier);
            if cloud.connectors.len() == before {
                return Err(not_found(identifier));
            }
            let token = format!("delete-{identifier}");
            cloud.requests.insert(
                token.clone(),
                event(&token, OperationStatus::Success, Some(identifier)),
            );
            cloud.respond()?;
            Ok(event(&token, OperationStatus::InProgress, None))
        });
        cc
    }

    fn provider(cloud: &Shared) -> MockPlatformServiceProvider {
        let (iam, ec2, cc) = (
            Arc::new(iam(cloud)),
            Arc::new(ec2(cloud)),
            Arc::new(cloudcontrol(cloud)),
        );
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        provider
            .expect_get_aws_cloudcontrol_client()
            .returning(move |_| Ok(cc.clone()));
        provider
    }

    fn sandbox(egress: SandboxEgress) -> Sandbox {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
            })
            .egress(egress)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build()
    }

    fn stack(egress: SandboxEgress, network: Option<NetworkSettings>) -> Stack {
        stack_with(egress, network, ResourceLifecycle::Frozen, "agents")
    }

    fn stack_with(
        egress: SandboxEgress,
        network: Option<NetworkSettings>,
        network_lifecycle: ResourceLifecycle,
        sandbox_id: &str,
    ) -> Stack {
        let mut stack = Stack::new("acme".to_string());
        if let Some(settings) = network {
            stack = stack.add(
                Network::new("default-network".to_string())
                    .settings(settings)
                    .build(),
                network_lifecycle,
            );
        }
        let sandbox = Sandbox {
            id: sandbox_id.to_string(),
            ..sandbox(egress)
        };
        stack.add(sandbox, ResourceLifecycle::Live).build()
    }

    fn created_network() -> Option<NetworkSettings> {
        Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        })
    }

    fn stack_state(network_status: Option<ResourceStatus>) -> StackState {
        stack_state_with(network_status, subnets())
    }

    fn stack_state_with(
        network_status: Option<ResourceStatus>,
        private_subnet_ids: Vec<String>,
    ) -> StackState {
        let mut state = StackState::new(Platform::Aws);
        state.resource_prefix = PREFIX.to_string();
        if let Some(status) = network_status {
            let controller = AwsNetworkController {
                vpc_id: Some(VPC.to_string()),
                private_subnet_ids,
                ..Default::default()
            };
            state.resources.insert(
                "default-network".to_string(),
                StackResourceState::builder()
                    .resource_type(Network::RESOURCE_TYPE.to_string())
                    .status(status)
                    .config(Resource::new(
                        Network::new("default-network".to_string())
                            .settings(created_network().unwrap())
                            .build(),
                    ))
                    .internal_state(serde_json::to_value(controller).unwrap())
                    .lifecycle(ResourceLifecycle::Frozen)
                    .build(),
            );
        }
        state
    }

    fn client_config() -> ClientConfig {
        ClientConfig::Aws(Box::new(AwsClientConfig::mock()))
    }

    /// Fails a loop that would otherwise spin forever on a step that never settles.
    fn bounded(calls: &mut usize) {
        *calls += 1;
        assert!(*calls <= 20, "setup scaffolding never settled");
    }

    /// One reconcile call, and the mutating calls it made.
    async fn step(
        cloud: &Shared,
        stack: &Stack,
        state: &StackState,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) -> Result<(ScaffoldingProgress, Vec<String>)> {
        let provider = provider(cloud);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let before = cloud.lock().unwrap().mutations.len();
        let progress = reconcile_all(&ctx, stack, state, records).await?;
        let made = cloud.lock().unwrap().mutations[before..].to_vec();
        Ok((progress, made))
    }

    /// Calls reconcile until it reports Done, asserting each call made at most one mutation.
    async fn converge(
        cloud: &Shared,
        stack: &Stack,
        state: &StackState,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) -> Vec<String> {
        let mut made = Vec::new();
        for _ in 0..20 {
            let (progress, calls) = step(cloud, stack, state, records).await.unwrap();
            assert!(
                calls.len() <= 1,
                "one mutating call per invocation: {calls:?}"
            );
            made.extend(calls);
            if progress == ScaffoldingProgress::Done {
                return made;
            }
        }
        panic!("setup scaffolding never converged: {made:?}");
    }

    async fn tear_down(
        cloud: &Shared,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) -> Result<(ScaffoldingProgress, Vec<String>)> {
        let provider = provider(cloud);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let before = cloud.lock().unwrap().mutations.len();
        let progress = teardown_all(&ctx, records).await?;
        let made = cloud.lock().unwrap().mutations[before..].to_vec();
        Ok((progress, made))
    }

    /// Teardown as a destroy runs it: first recovering what the record does not name.
    async fn destroy(
        cloud: &Shared,
        stack: &Stack,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) {
        let provider = provider(cloud);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let mut calls = 0;
        loop {
            bounded(&mut calls);
            crate::setup_scaffolding::recover_unrecorded(&ctx, stack, Platform::Aws, records)
                .await
                .unwrap();
            if teardown_all(&ctx, records).await.unwrap() == ScaffoldingProgress::Done {
                return;
            }
        }
    }

    fn desired_connector(security_group_id: &str) -> Value {
        SandboxEgressConnector::builder()
            .resource_prefix(PREFIX)
            .sandbox_id("agents")
            .operator_role_arn(OPERATOR_ARN)
            .private_subnet_ids(&subnets())
            .security_group_id(security_group_id)
            .build()
            .desired_state()
    }

    fn full_record(security_group_id: &str, connector_arn: &str) -> SetupScaffolding {
        SetupScaffolding::AwsSandbox {
            build_role_name: BUILD_ROLE.to_string(),
            egress: Some(AwsSandboxEgressScaffolding {
                operator_role_name: EGRESS_NAME.to_string(),
                security_group_id: Some(security_group_id.to_string()),
                connector_arn: Some(connector_arn.to_string()),
                connector_request: None,
            }),
        }
    }

    #[tokio::test]
    async fn deny_builds_role_then_a_loopback_only_group_then_the_connector() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();

        let made = converge(&cloud, &stack, &state, &mut records).await;

        let desired = desired_connector("sg-1");
        assert_eq!(
            made,
            vec![
                format!("iam:CreateRole {BUILD_ROLE}"),
                format!("iam:CreateRole {EGRESS_NAME}"),
                format!("iam:PutRolePolicy {EGRESS_NAME} {SANDBOX_EGRESS_POLICY_NAME}"),
                format!("ec2:CreateSecurityGroup {EGRESS_NAME}"),
                "ec2:RevokeSecurityGroupEgress [\"0.0.0.0/0\"]".to_string(),
                "ec2:AuthorizeSecurityGroupEgress [\"127.0.0.1/32\"]".to_string(),
                format!("cloudcontrol:CreateResource {desired}"),
                format!("iam:PutRolePolicy {BUILD_ROLE} sandbox-image-build"),
            ]
        );
        let cloud = cloud.lock().unwrap();
        assert_eq!(cloud.groups.len(), 1);
        assert_eq!(
            cloud.groups[0].egress,
            vec![rule("127.0.0.1/32")],
            "the deny group must reach nothing but loopback"
        );
        assert_eq!(cloud.groups[0].vpc, VPC);
        assert_eq!(
            cloud.roles[EGRESS_NAME],
            sandbox_egress_operator_trust_policy()
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                &cloud.inline[&(
                    EGRESS_NAME.to_string(),
                    SANDBOX_EGRESS_POLICY_NAME.to_string()
                )]
            )
            .unwrap(),
            sandbox_egress_operator_policy("aws", ACCOUNT, "us-east-1")
        );
        assert_eq!(cloud.connectors.len(), 1);
        let connector_arn = cloud.connectors[0].0.clone();
        assert_eq!(
            records,
            BTreeMap::from([("agents".to_string(), full_record("sg-1", &connector_arn))])
        );
    }

    #[tokio::test]
    async fn a_converged_deny_sandbox_changes_nothing() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        converge(&cloud, &stack, &state, &mut records).await;
        let converged = records.clone();

        let (progress, made) = step(&cloud, &stack, &state, &mut records).await.unwrap();

        assert_eq!(progress, ScaffoldingProgress::Done);
        assert_eq!(made, Vec::<String>::new());
        assert_eq!(records, converged);
    }

    #[tokio::test]
    async fn allow_creates_no_egress_objects() {
        let mut provider = MockPlatformServiceProvider::new();
        let cloud = Shared::default();
        let iam = Arc::new(iam(&cloud));
        provider
            .expect_get_aws_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider.expect_get_aws_ec2_client().times(0);
        provider.expect_get_aws_cloudcontrol_client().times(0);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let stack = stack(SandboxEgress::Allow, None);
        let state = stack_state(None);
        let mut records = BTreeMap::new();

        for _ in 0..2 {
            reconcile_all(&ctx, &stack, &state, &mut records)
                .await
                .unwrap();
        }

        assert_eq!(
            records,
            BTreeMap::from([(
                "agents".to_string(),
                SetupScaffolding::AwsSandbox {
                    build_role_name: BUILD_ROLE.to_string(),
                    egress: None
                }
            )])
        );
        assert_eq!(cloud.lock().unwrap().roles.len(), 1, "the build role only");
    }

    #[tokio::test]
    async fn deny_waits_for_its_network_without_creating_egress_objects() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let mut records = BTreeMap::new();
        for network in [None, Some(ResourceStatus::Provisioning)] {
            let state = stack_state(network);
            for _ in 0..3 {
                let (progress, _) = step(&cloud, &stack, &state, &mut records).await.unwrap();
                assert_eq!(progress, ScaffoldingProgress::InProgress);
            }
        }
        let cloud = cloud.lock().unwrap();
        assert_eq!(
            cloud.mutations,
            vec![format!("iam:CreateRole {BUILD_ROLE}")],
            "nothing past the build role before the network is running"
        );
    }

    async fn assert_refused_before_any_call(stack: Stack, expected: &str) {
        let provider = MockPlatformServiceProvider::new();
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let mut records = BTreeMap::new();
        let error = reconcile_all(
            &ctx,
            &stack,
            &stack_state(Some(ResourceStatus::Running)),
            &mut records,
        )
        .await
        .expect_err("the direct path must refuse what the templates refuse");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert!(error.message.contains(expected), "{}", error.message);
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn deny_is_refused_on_a_network_with_no_private_subnets() {
        assert_refused_before_any_call(
            stack(SandboxEgress::Deny, Some(NetworkSettings::UseDefault)),
            "only public subnets",
        )
        .await;
        assert_refused_before_any_call(stack(SandboxEgress::Deny, None), "declares no network")
            .await;
        assert_refused_before_any_call(
            stack(
                SandboxEgress::AllowDomains {
                    domains: vec!["example.com".to_string()],
                },
                created_network(),
            ),
            "allowDomains",
        )
        .await;
    }

    async fn assert_not_adoptable(cloud: &Shared, expected: &str) {
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        let connectors_before = cloud.lock().unwrap().connectors.len();
        let mut calls = 0;
        let error = loop {
            bounded(&mut calls);
            match step(cloud, &stack, &state, &mut records).await {
                Ok((ScaffoldingProgress::InProgress, _)) => continue,
                Ok((ScaffoldingProgress::Done, made)) => {
                    panic!("adopted what it must refuse: {made:?}")
                }
                Err(error) => break error,
            }
        };
        assert_eq!(error.code, "SETUP_SCAFFOLDING_NOT_ADOPTABLE");
        assert!(error.message.contains(expected), "{}", error.message);
        assert_eq!(
            cloud.lock().unwrap().connectors.len(),
            connectors_before,
            "no connector may carry an unverified object"
        );
    }

    fn preexisting_group(egress: Vec<Rule>) -> Shared {
        let cloud = Shared::default();
        cloud.lock().unwrap().groups.push(Group {
            id: "sg-foreign".to_string(),
            name: EGRESS_NAME.to_string(),
            vpc: VPC.to_string(),
            egress,
            tags: vec![],
        });
        cloud
    }

    #[tokio::test]
    async fn an_unrecorded_group_with_open_egress_is_refused() {
        let cloud = preexisting_group(vec![rule("0.0.0.0/0")]);
        assert_not_adoptable(&cloud, "sg-foreign").await;
        let cloud = cloud.lock().unwrap();
        assert_eq!(
            cloud.groups[0].egress,
            vec![rule("0.0.0.0/0")],
            "a group setup did not create is never rewritten"
        );
    }

    #[tokio::test]
    async fn an_unrecorded_group_reaching_a_prefix_list_is_refused() {
        let mut loopback_and_s3 = rule("127.0.0.1/32");
        loopback_and_s3.prefix_lists = vec!["pl-63a5400a".to_string()];
        assert_not_adoptable(&preexisting_group(vec![loopback_and_s3]), "sg-foreign").await;
    }

    #[tokio::test]
    async fn an_unrecorded_group_also_reaching_ipv6_is_refused() {
        let mut loopback_and_v6 = rule("127.0.0.1/32");
        loopback_and_v6.ipv6_cidrs = vec!["::/0".to_string()];
        assert_not_adoptable(&preexisting_group(vec![loopback_and_v6]), "sg-foreign").await;
    }

    #[tokio::test]
    async fn an_unrecorded_group_also_reaching_another_group_is_refused() {
        let mut loopback_and_peer = rule("127.0.0.1/32");
        loopback_and_peer.groups = vec!["sg-peer".to_string()];
        assert_not_adoptable(&preexisting_group(vec![loopback_and_peer]), "sg-foreign").await;
    }

    fn tags_for(prefix: &str, sandbox_id: &str) -> Vec<(String, String)> {
        setup_tags(prefix, sandbox_id)
            .into_iter()
            .map(|tag| (tag.key, tag.value))
            .collect()
    }

    /// Setup created this group and lost the response, so its id never reached the record.
    fn open_group_tagged_for(prefix: &str, sandbox_id: &str) -> Shared {
        let cloud = preexisting_group(vec![rule("0.0.0.0/0")]);
        cloud.lock().unwrap().groups[0].tags = tags_for(prefix, sandbox_id);
        cloud
    }

    #[tokio::test]
    async fn an_unrecorded_group_carrying_setup_tags_is_recorded_and_repaired() {
        let cloud = open_group_tagged_for(PREFIX, "agents");
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();

        let made = converge(&cloud, &stack, &state, &mut records).await;

        assert_eq!(
            made.iter()
                .filter(|call| call.starts_with("ec2:CreateSecurityGroup"))
                .count(),
            0,
            "the tagged group is the sandbox's, not a reason to make another"
        );
        let cloud = cloud.lock().unwrap();
        assert_eq!(cloud.groups.len(), 1);
        assert_eq!(cloud.groups[0].egress, vec![rule("127.0.0.1/32")]);
        let connector_arn = cloud.connectors[0].0.clone();
        assert_eq!(
            records,
            BTreeMap::from([(
                "agents".to_string(),
                full_record("sg-foreign", &connector_arn)
            )])
        );
    }

    #[tokio::test]
    async fn an_open_group_tagged_for_another_sandbox_or_stack_is_refused() {
        for (prefix, sandbox_id) in [(PREFIX, "other"), ("other", "agents")] {
            let cloud = open_group_tagged_for(prefix, sandbox_id);
            assert_not_adoptable(&cloud, "sg-foreign").await;
            assert_eq!(
                cloud.lock().unwrap().groups[0].egress,
                vec![rule("0.0.0.0/0")],
                "a group tagged for anything else is never rewritten"
            );
        }
    }

    fn record_with_group(security_group_id: &str) -> BTreeMap<String, SetupScaffolding> {
        BTreeMap::from([(
            "agents".to_string(),
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: Some(AwsSandboxEgressScaffolding {
                    operator_role_name: EGRESS_NAME.to_string(),
                    security_group_id: Some(security_group_id.to_string()),
                    connector_arn: None,
                    connector_request: None,
                }),
            },
        )])
    }

    #[tokio::test]
    async fn a_recorded_group_left_in_another_vpc_is_refused() {
        let cloud = Shared::default();
        cloud.lock().unwrap().groups.push(Group {
            id: "sg-old".to_string(),
            name: EGRESS_NAME.to_string(),
            vpc: "vpc-0previous".to_string(),
            egress: vec![rule("127.0.0.1/32")],
            tags: tags_for(PREFIX, "agents"),
        });
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = record_with_group("sg-old");
        let mut calls = 0;
        let error = loop {
            bounded(&mut calls);
            match step(&cloud, &stack, &state, &mut records).await {
                Ok((ScaffoldingProgress::InProgress, _)) => continue,
                Ok((ScaffoldingProgress::Done, made)) => panic!("converged: {made:?}"),
                Err(error) => break error,
            }
        };
        assert_eq!(error.code, "SETUP_SCAFFOLDING_NOT_ADOPTABLE");
        assert!(
            error.message.contains("vpc-0previous")
                && error.message.contains(VPC)
                && error
                    .message
                    .contains("destroy the deployment and deploy it again"),
            "{}",
            error.message
        );
        let cloud = cloud.lock().unwrap();
        assert!(
            !cloud
                .mutations
                .iter()
                .any(|call| call.starts_with("ec2:") || call.starts_with("cloudcontrol:")),
            "no second group or connector is made while the first is still recorded: {:?}",
            cloud.mutations
        );
        assert_eq!(records, record_with_group("sg-old"));
    }

    #[tokio::test]
    async fn a_recorded_group_that_is_gone_is_replaced() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = record_with_group("sg-deleted-by-hand");

        let made = converge(&cloud, &stack, &state, &mut records).await;

        assert_eq!(
            made.iter()
                .filter(|call| call.starts_with("ec2:CreateSecurityGroup"))
                .count(),
            1
        );
        let connector_arn = cloud.lock().unwrap().connectors[0].0.clone();
        assert_eq!(
            records,
            BTreeMap::from([("agents".to_string(), full_record("sg-1", &connector_arn))])
        );
    }

    /// Each mutating call in turn takes effect and then loses its response, as a crash between
    /// the call and the checkpoint would. Setup and teardown must each still finish with one of
    /// every object while set up, and none after, without refusing their own objects.
    #[tokio::test]
    async fn a_lost_response_at_any_mutating_call_still_converges() {
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let total = {
            let cloud = Shared::default();
            let mut records = BTreeMap::new();
            converge(&cloud, &stack, &state, &mut records).await;
            while tear_down(&cloud, &mut records).await.unwrap().0 != ScaffoldingProgress::Done {}
            let total = cloud.lock().unwrap().mutations.len();
            total
        };
        assert_eq!(total, 14, "8 setup calls and 6 teardown calls");

        for lost in 1..=total {
            let cloud = Shared::default();
            cloud.lock().unwrap().lose_response_to = Some(lost);
            let mut records = BTreeMap::new();
            let mut failures = 0;
            let mut calls = 0;
            loop {
                bounded(&mut calls);
                match step(&cloud, &stack, &state, &mut records).await {
                    Ok((ScaffoldingProgress::Done, _)) => break,
                    Ok((ScaffoldingProgress::InProgress, _)) => {}
                    Err(error) => {
                        failures += 1;
                        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR", "call {lost}: {error}");
                    }
                }
            }
            {
                let cloud = cloud.lock().unwrap();
                let created = |prefix: &str| {
                    cloud
                        .mutations
                        .iter()
                        .filter(|call| call.starts_with(prefix))
                        .count()
                };
                assert_eq!(
                    created(&format!("iam:CreateRole {BUILD_ROLE}")),
                    1,
                    "call {lost}"
                );
                assert_eq!(
                    created(&format!("iam:CreateRole {EGRESS_NAME}")),
                    1,
                    "call {lost}"
                );
                assert_eq!(created("ec2:CreateSecurityGroup"), 1, "call {lost}");
                assert_eq!(created("cloudcontrol:CreateResource"), 1, "call {lost}");
                assert_eq!(cloud.groups.len(), 1, "call {lost}");
                assert_eq!(
                    cloud.groups[0].egress,
                    vec![rule("127.0.0.1/32")],
                    "call {lost}"
                );
                assert_eq!(cloud.inline.len(), 2, "call {lost}");
                let connector_arn = cloud.connectors[0].0.clone();
                assert_eq!(
                    records,
                    BTreeMap::from([(
                        "agents".to_string(),
                        full_record(&cloud.groups[0].id, &connector_arn)
                    )]),
                    "call {lost}"
                );
            }

            let mut calls = 0;
            loop {
                bounded(&mut calls);
                match tear_down(&cloud, &mut records).await {
                    Ok((ScaffoldingProgress::Done, _)) => break,
                    Ok((ScaffoldingProgress::InProgress, _)) => {}
                    Err(error) => {
                        failures += 1;
                        assert_eq!(error.code, "CLOUD_PLATFORM_ERROR", "call {lost}: {error}");
                    }
                }
            }
            assert_eq!(failures, 1, "call {lost}: the lost response surfaces once");
            let cloud = cloud.lock().unwrap();
            assert!(records.is_empty(), "call {lost}");
            assert!(cloud.roles.is_empty(), "call {lost}: {:?}", cloud.roles);
            assert!(cloud.inline.is_empty(), "call {lost}");
            assert!(cloud.groups.is_empty(), "call {lost}");
            assert!(cloud.connectors.is_empty(), "call {lost}");
        }
    }

    #[tokio::test]
    async fn an_unrecorded_loopback_only_group_is_adopted() {
        let cloud = preexisting_group(vec![rule("127.0.0.1/32")]);
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();

        let made = converge(&cloud, &stack, &state, &mut records).await;

        assert!(
            !made.iter().any(|call| call.starts_with("ec2:")),
            "an adopted group is used as is: {made:?}"
        );
        let arn = cloud.lock().unwrap().connectors[0].0.clone();
        assert_eq!(records["agents"], full_record("sg-foreign", &arn));
    }

    #[tokio::test]
    async fn an_operator_role_with_a_foreign_policy_is_refused() {
        let cloud = Shared::default();
        {
            let mut c = cloud.lock().unwrap();
            c.roles.insert(
                EGRESS_NAME.to_string(),
                sandbox_egress_operator_trust_policy(),
            );
            c.inline.insert(
                (EGRESS_NAME.to_string(), "admin".to_string()),
                "{}".to_string(),
            );
        }
        assert_not_adoptable(
            &cloud,
            "inline policies other than 'sandbox-egress-connector'",
        )
        .await;
    }

    #[tokio::test]
    async fn an_operator_role_trusting_another_principal_is_refused() {
        let cloud = Shared::default();
        let mut trust = sandbox_egress_operator_trust_policy();
        trust["Statement"][0]["Principal"]["AWS"] = json!("arn:aws:iam::999999999999:root");
        cloud
            .lock()
            .unwrap()
            .roles
            .insert(EGRESS_NAME.to_string(), trust);
        assert_not_adoptable(&cloud, "trust policy").await;
    }

    /// The connector's ARN is assigned by AWS, so a crash between its create and the record
    /// leaves only its name to find it by.
    #[tokio::test]
    async fn a_connector_created_before_a_crash_is_found_not_created_again() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        let mut calls = 0;
        loop {
            bounded(&mut calls);
            let (_, made) = step(&cloud, &stack, &state, &mut records).await.unwrap();
            if made
                .iter()
                .any(|call| call.starts_with("cloudcontrol:CreateResource"))
            {
                break;
            }
        }
        let SetupScaffolding::AwsSandbox { egress, .. } = records.get_mut("agents").unwrap();
        egress.as_mut().unwrap().connector_arn = None;

        let made = converge(&cloud, &stack, &state, &mut records).await;

        assert_eq!(
            made,
            vec![format!(
                "iam:PutRolePolicy {BUILD_ROLE} sandbox-image-build"
            )]
        );
        let cloud = cloud.lock().unwrap();
        assert_eq!(cloud.connectors.len(), 1, "one connector, not two");
        assert_eq!(
            records["agents"],
            full_record("sg-1", &cloud.connectors[0].0)
        );
    }

    #[tokio::test]
    async fn a_same_named_connector_on_another_group_is_refused() {
        let cloud = Shared::default();
        {
            let mut c = cloud.lock().unwrap();
            c.roles.insert(
                EGRESS_NAME.to_string(),
                sandbox_egress_operator_trust_policy(),
            );
            c.groups.push(Group {
                id: "sg-1".to_string(),
                name: EGRESS_NAME.to_string(),
                vpc: VPC.to_string(),
                egress: vec![rule("127.0.0.1/32")],
                tags: vec![],
            });
            let mut properties = desired_connector("sg-open");
            properties["State"] = json!("ACTIVE");
            c.connectors.push((
                format!("arn:aws:lambda:us-east-1:{ACCOUNT}:network-connector:other"),
                properties,
            ));
        }
        assert_not_adoptable(&cloud, "SecurityGroupIds").await;
    }

    #[tokio::test]
    async fn teardown_deletes_connector_then_group_then_roles_and_waits_out_the_group() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        converge(&cloud, &stack, &state, &mut records).await;
        let connector_arn = cloud.lock().unwrap().connectors[0].0.clone();
        cloud.lock().unwrap().dependency_violations = 1;

        let (progress, made) = tear_down(&cloud, &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert_eq!(
            made,
            vec![format!("cloudcontrol:DeleteResource {connector_arn}")]
        );
        assert_eq!(
            records["agents"],
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: Some(AwsSandboxEgressScaffolding {
                    operator_role_name: EGRESS_NAME.to_string(),
                    security_group_id: Some("sg-1".to_string()),
                    connector_arn: Some(connector_arn.clone()),
                    connector_request: Some(format!("delete-{connector_arn}")),
                }),
            },
            "the delete request is recorded until its outcome is read"
        );

        let (progress, made) = tear_down(&cloud, &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert_eq!(made, vec!["ec2:DeleteSecurityGroup sg-1".to_string()]);
        assert_eq!(
            records["agents"],
            SetupScaffolding::AwsSandbox {
                build_role_name: BUILD_ROLE.to_string(),
                egress: Some(AwsSandboxEgressScaffolding {
                    operator_role_name: EGRESS_NAME.to_string(),
                    security_group_id: Some("sg-1".to_string()),
                    connector_arn: None,
                    connector_request: None,
                }),
            },
            "a group still held by interfaces stays recorded"
        );

        let (progress, made) = tear_down(&cloud, &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::Done);
        assert_eq!(
            made,
            vec![
                "ec2:DeleteSecurityGroup sg-1".to_string(),
                format!("iam:DeleteRolePolicy {EGRESS_NAME} {SANDBOX_EGRESS_POLICY_NAME}"),
                format!("iam:DeleteRole {EGRESS_NAME}"),
                format!("iam:DeleteRolePolicy {BUILD_ROLE} sandbox-image-build"),
                format!("iam:DeleteRole {BUILD_ROLE}"),
            ]
        );
        assert!(records.is_empty());
        let cloud = cloud.lock().unwrap();
        assert!(cloud.connectors.is_empty() && cloud.groups.is_empty() && cloud.roles.is_empty());
    }

    #[tokio::test]
    async fn teardown_of_egress_already_gone_succeeds() {
        let cloud = Shared::default();
        let mut records = BTreeMap::from([(
            "agents".to_string(),
            full_record(
                "sg-gone",
                &format!("arn:aws:lambda:us-east-1:{ACCOUNT}:network-connector:gone"),
            ),
        )]);

        let (progress, made) = tear_down(&cloud, &mut records).await.unwrap();

        assert_eq!(progress, ScaffoldingProgress::Done);
        assert_eq!(
            made,
            vec![
                "ec2:DeleteSecurityGroup sg-gone".to_string(),
                format!("iam:DeleteRolePolicy {EGRESS_NAME} {SANDBOX_EGRESS_POLICY_NAME}"),
                format!("iam:DeleteRole {EGRESS_NAME}"),
                format!("iam:DeleteRolePolicy {BUILD_ROLE} sandbox-image-build"),
                format!("iam:DeleteRole {BUILD_ROLE}"),
            ],
            "a connector no longer listed is not deleted again"
        );
        assert!(records.is_empty());
    }

    async fn drive_to_connector_create(
        cloud: &Shared,
        records: &mut BTreeMap<String, SetupScaffolding>,
    ) -> Result<ScaffoldingProgress> {
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut calls = 0;
        loop {
            bounded(&mut calls);
            let (progress, made) = step(cloud, &stack, &state, records).await?;
            if made
                .iter()
                .any(|call| call.starts_with("cloudcontrol:CreateResource"))
            {
                return Ok(progress);
            }
        }
    }

    /// The failure is read from the recorded request on the next call, and the request is dropped
    /// from the record, so a run after the cause is fixed creates again rather than re-reading it.
    #[tokio::test]
    async fn a_failed_connector_create_surfaces_aws_status_message() {
        let cloud = Shared::default();
        cloud.lock().unwrap().failing_create = Some((
            "InvalidRequest".to_string(),
            "unable to assume the provided NetworkConnectorOperatorRole".to_string(),
        ));
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();

        let progress = drive_to_connector_create(&cloud, &mut records)
            .await
            .unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert!(
            pending_request(&records).is_some_and(|token| token.starts_with("create-")),
            "the create request is recorded until its outcome is read"
        );

        let error = step(&cloud, &stack, &state, &mut records)
            .await
            .expect_err("a FAILED create is an error");
        let chain = format!("{error:?}");
        assert!(
            chain.contains("InvalidRequest")
                && chain.contains("unable to assume the provided NetworkConnectorOperatorRole"),
            "{chain}"
        );
        assert!(cloud.lock().unwrap().connectors.is_empty());
        assert_eq!(pending_request(&records), None);

        let (_, made) = step(&cloud, &stack, &state, &mut records).await.unwrap();
        assert!(
            made[0].starts_with("cloudcontrol:CreateResource"),
            "the next run creates again: {made:?}"
        );
    }

    #[tokio::test]
    async fn a_connector_request_still_running_is_waited_for_not_repeated() {
        let cloud = Shared::default();
        cloud.lock().unwrap().requests_in_flight = true;
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        drive_to_connector_create(&cloud, &mut records)
            .await
            .unwrap();
        let token = pending_request(&records).unwrap().to_string();

        let (progress, made) = step(&cloud, &stack, &state, &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::InProgress);
        assert!(made.is_empty(), "nothing is created again: {made:?}");
        assert_eq!(pending_request(&records), Some(token.as_str()));

        cloud.lock().unwrap().requests_in_flight = false;
        let made = converge(&cloud, &stack, &state, &mut records).await;
        assert!(
            !made
                .iter()
                .any(|call| call.starts_with("cloudcontrol:CreateResource")),
            "{made:?}"
        );
        assert_eq!(pending_request(&records), None);
        assert_eq!(cloud.lock().unwrap().connectors.len(), 1);
    }

    /// Cloud Control forgets old requests; the connector itself is then the only record.
    #[tokio::test]
    async fn a_request_cloud_control_no_longer_knows_is_dropped() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        converge(&cloud, &stack, &state, &mut records).await;
        let Some(SetupScaffolding::AwsSandbox {
            egress: Some(egress),
            ..
        }) = records.get_mut("agents")
        else {
            panic!("a deny sandbox records its egress objects");
        };
        egress.connector_request = Some("forgotten".to_string());

        let (progress, made) = step(&cloud, &stack, &state, &mut records).await.unwrap();

        assert_eq!(progress, ScaffoldingProgress::Done);
        assert!(made.is_empty(), "{made:?}");
        assert_eq!(pending_request(&records), None);
    }

    fn pending_request(records: &BTreeMap<String, SetupScaffolding>) -> Option<&str> {
        let SetupScaffolding::AwsSandbox {
            egress: Some(egress),
            ..
        } = &records["agents"]
        else {
            panic!("a deny sandbox records its egress objects");
        };
        egress.connector_request.as_deref()
    }

    /// Names are unique per account and Region, so this is an earlier create of the same one.
    #[tokio::test]
    async fn a_create_refused_as_already_existing_waits_for_the_next_call() {
        let cloud = Shared::default();
        cloud.lock().unwrap().failing_create =
            Some(("AlreadyExists".to_string(), "exists".to_string()));
        let mut records = BTreeMap::new();

        let progress = drive_to_connector_create(&cloud, &mut records)
            .await
            .unwrap();

        assert_eq!(progress, ScaffoldingProgress::InProgress);
    }

    #[tokio::test]
    async fn a_pending_connector_is_waited_for_not_recreated() {
        let cloud = Shared::default();
        cloud.lock().unwrap().created_state = Some("PENDING");
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        drive_to_connector_create(&cloud, &mut records)
            .await
            .unwrap();

        for _ in 0..3 {
            let (progress, made) = step(&cloud, &stack, &state, &mut records).await.unwrap();
            assert_eq!(progress, ScaffoldingProgress::InProgress);
            assert!(made.is_empty(), "{made:?}");
        }

        cloud.lock().unwrap().connectors[0].1["State"] = json!("ACTIVE");
        let (progress, _) = step(&cloud, &stack, &state, &mut records).await.unwrap();
        assert_eq!(progress, ScaffoldingProgress::Done);
    }

    #[tokio::test]
    async fn an_operator_role_name_past_iams_limit_is_refused() {
        let cloud = Shared::default();
        let stack = stack_with(
            SandboxEgress::Deny,
            created_network(),
            ResourceLifecycle::Frozen,
            "a-sandbox-id-long-enough-to-push-the-egress-name-over",
        );
        let state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();

        let mut calls = 0;
        let error = loop {
            bounded(&mut calls);
            match step(&cloud, &stack, &state, &mut records).await {
                Ok((ScaffoldingProgress::InProgress, _)) => continue,
                Ok((ScaffoldingProgress::Done, _)) => panic!("an over-long name was used"),
                Err(error) => break error,
            }
        };

        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert!(
            error.message.contains("longer than IAM's 64"),
            "{}",
            error.message
        );
        assert!(cloud
            .lock()
            .unwrap()
            .roles
            .keys()
            .all(|name| name.ends_with("-build")));
    }

    #[tokio::test]
    async fn a_running_network_with_no_private_subnets_is_refused() {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state_with(Some(ResourceStatus::Running), vec![]);
        let mut records = BTreeMap::new();

        let mut calls = 0;
        let error = loop {
            bounded(&mut calls);
            match step(&cloud, &stack, &state, &mut records).await {
                Ok((ScaffoldingProgress::InProgress, _)) => continue,
                Ok((ScaffoldingProgress::Done, _)) => panic!("a connector with no subnets"),
                Err(error) => break error,
            }
        };

        assert!(
            error.message.contains("no private subnets"),
            "{}",
            error.message
        );
        assert!(cloud.lock().unwrap().groups.is_empty());
    }

    #[tokio::test]
    async fn deny_is_refused_on_a_network_the_runtime_creates() {
        assert_refused_before_any_call(
            stack_with(
                SandboxEgress::Deny,
                created_network(),
                ResourceLifecycle::Live,
                "agents",
            ),
            "created at runtime",
        )
        .await;
    }

    const REGIONAL_BUNDLE: &str = "s3://acme-artifacts-{region}/sandbox-bundle/f00dcafe/bundle.zip";

    struct Served {
        seed: crate::setup_scaffolding::ScaffoldingSeed,
        build_role_arn: String,
        bundle_uri: String,
        binding: Value,
    }

    /// Converges direct setup, seeds the sandbox through the registered importer, then runs its
    /// controller from that seed alone until the image is ACTIVE.
    async fn serve_from_seed(sandbox: Sandbox, network: Option<NetworkSettings>) -> Served {
        use crate::core::ResourceController as _;
        use crate::setup_scaffolding::{apply_seeds, seeds, SeedContext};
        const IMAGE_ARN: &str = "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents";
        let cloud = Shared::default();
        let mut stack = Stack::new("acme".to_string());
        let network_status = network.as_ref().map(|_| ResourceStatus::Running);
        if let Some(settings) = network {
            stack = stack.add(
                Network::new("default-network".to_string())
                    .settings(settings)
                    .build(),
                ResourceLifecycle::Frozen,
            );
        }
        let stack = stack.add(sandbox.clone(), ResourceLifecycle::Live).build();
        let mut state = stack_state(network_status);
        let mut records = BTreeMap::new();
        converge(&cloud, &stack, &state, &mut records).await;

        let provider = provider(&cloud);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let mut seeds = seeds(&ctx, &stack, &state, &records).unwrap();
        assert_eq!(seeds.len(), 1, "one seed per scaffolded sandbox");
        let seed = seeds[0].clone();
        apply_seeds(
            &SeedContext {
                registry: &crate::ImporterRegistry::built_in(),
                stack_settings: &alien_core::StackSettings::default(),
                management_config: None,
            },
            &stack,
            &mut state,
            std::mem::take(&mut seeds),
        )
        .unwrap();
        let seeded = &state.resources["agents"];
        assert_eq!(seeded.status, ResourceStatus::Provisioning);
        assert_eq!(seeded.controller_platform, Some(Platform::Aws));
        let controller =
            AwsSandboxController::from_persisted(seeded.internal_state.clone().unwrap()).unwrap();

        let requested = Arc::new(Mutex::new(None::<(String, String)>));
        let capture = requested.clone();
        let mut microvms = MockLambdaMicrovmsApi::new();
        let built = Arc::new(Mutex::new(false));
        let probe = built.clone();
        microvms.expect_get_microvm_image().returning(move |_| {
            if !*probe.lock().unwrap() {
                return Err(not_found("test-agents"));
            }
            Ok(MicrovmImage {
                image_identifier: None,
                image_arn: Some(IMAGE_ARN.to_string()),
                image_version: Some("1.0".to_string()),
                state: Some("CREATED".to_string()),
            })
        });
        microvms
            .expect_create_microvm_image()
            .times(1)
            .returning(move |request| {
                *capture.lock().unwrap() = Some((
                    request.build_role_arn.clone(),
                    request.code_artifact.uri.clone(),
                ));
                *built.lock().unwrap() = true;
                Ok(CreateMicrovmImageResponse {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    name: Some("test-agents".to_string()),
                    state: Some("CREATING".to_string()),
                    image_version: Some("1.0".to_string()),
                })
            });
        microvms
            .expect_get_microvm_image_version()
            .returning(|_, _| {
                Ok(MicrovmImageVersion {
                    image_arn: Some(IMAGE_ARN.to_string()),
                    image_version: Some("1.0".to_string()),
                    state: Some("SUCCESSFUL".to_string()),
                    status: Some("ACTIVE".to_string()),
                    state_reason: None,
                })
            });
        let microvms = Arc::new(microvms);
        let mut controller_provider = MockPlatformServiceProvider::new();
        controller_provider
            .expect_get_aws_microvms_client()
            .returning(move |_| Ok(microvms.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(sandbox)
            .controller(controller)
            .platform(Platform::Aws)
            .resource_lifecycle(ResourceLifecycle::Live)
            .service_provider(Arc::new(controller_provider))
            .build()
            .await
            .unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        let binding = executor
            .internal_state::<AwsSandboxController>()
            .unwrap()
            .get_binding_params()
            .unwrap()
            .expect("an ACTIVE image publishes a binding");
        let (build_role_arn, bundle_uri) = requested.lock().unwrap().clone().unwrap();
        Served {
            seed,
            build_role_arn,
            bundle_uri,
            binding,
        }
    }

    async fn load(binding: &Value) -> std::result::Result<(), String> {
        let env = std::collections::HashMap::from([
            (
                alien_core::ENV_ALIEN_DEPLOYMENT_TYPE.to_string(),
                Platform::Aws.as_str().to_string(),
            ),
            ("AWS_REGION".to_string(), "us-east-1".to_string()),
            ("AWS_ACCOUNT_ID".to_string(), ACCOUNT.to_string()),
            ("AWS_ACCESS_KEY_ID".to_string(), "test".to_string()),
            ("AWS_SECRET_ACCESS_KEY".to_string(), "test".to_string()),
            ("ALIEN_AGENTS_BINDING".to_string(), binding.to_string()),
        ]);
        BindingsProvider::from_env(env)
            .await
            .map_err(|error| error.to_string())?
            .load_sandbox("agents")
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn seed_field<'a>(served: &'a Served, field: &str) -> &'a Value {
        &served.seed.import_data[field]
    }

    /// The binding as the runtime deserializes it: (allowEgress, connectors, preview ports).
    fn egress_facts(binding: &Value) -> (bool, Vec<String>, Vec<u16>) {
        let SandboxBinding::Aws(aws) = serde_json::from_value(binding.clone()).unwrap() else {
            panic!("an AWS sandbox publishes an AWS binding: {binding}");
        };
        let connectors = aws
            .egress_connector_arns
            .into_iter()
            .map(|arn| arn.into_value("agents", "egressConnectorArns").unwrap())
            .collect();
        (aws.allow_egress, connectors, aws.preview_ports)
    }

    #[tokio::test]
    async fn a_deny_sandbox_seeded_by_direct_setup_serves_a_binding_the_runtime_loads() {
        let sandbox = Sandbox {
            preview_ports: vec![8080],
            ..sandbox(SandboxEgress::Deny)
        };
        let served = serve_from_seed(sandbox, created_network()).await;

        load(&served.binding).await.unwrap_or_else(|error| {
            panic!("the deny binding must load: {error}\n{}", served.binding)
        });
        let (allow_egress, connectors, preview_ports) = egress_facts(&served.binding);
        assert!(!allow_egress);
        assert_eq!(
            connectors,
            vec!["arn:aws:lambda:us-east-1:123456789012:network-connector:nc-2".to_string()],
            "the session starts on the connector setup created"
        );
        assert_eq!(preview_ports, vec![8080]);
        assert_eq!(
            seed_field(&served, "buildRoleArn"),
            &json!(served.build_role_arn)
        );
        assert_eq!(seed_field(&served, "bundleUri"), &json!(served.bundle_uri));
    }

    #[tokio::test]
    async fn an_allow_sandbox_seeded_by_direct_setup_serves_a_binding_the_runtime_loads() {
        let sandbox = Sandbox {
            preview_ports: vec![3000, 8080],
            code: SandboxCode::Image {
                image: REGIONAL_BUNDLE.to_string(),
            },
            ..sandbox(SandboxEgress::Allow)
        };
        let served = serve_from_seed(sandbox, None).await;

        load(&served.binding).await.unwrap_or_else(|error| {
            panic!("the allow binding must load: {error}\n{}", served.binding)
        });
        assert_eq!(
            egress_facts(&served.binding),
            (true, vec![], vec![3000, 8080])
        );
        // The seed resolves the region token as the controller does, so the bundle it records
        // is the one the image is built from and no roll is mistaken for a change.
        assert_eq!(
            served.bundle_uri,
            "s3://acme-artifacts-us-east-1/sandbox-bundle/f00dcafe/bundle.zip"
        );
        assert_eq!(seed_field(&served, "bundleUri"), &json!(served.bundle_uri));
        assert_eq!(
            seed_field(&served, "buildRoleArn"),
            &json!(served.build_role_arn)
        );
    }

    /// A serving deny sandbox that setup runs over again keeps its image and version and starts
    /// its sessions on the connector setup recorded, not on the empty list it was serving with.
    #[tokio::test]
    async fn a_second_seed_keeps_a_serving_deny_sandbox_and_hands_it_the_connector() {
        use crate::sandbox::AwsSandboxController;

        let (cloud, stack, mut state, records) = serving_deny_sandbox().await;
        seed_again(&cloud, &stack, &mut state, &records).unwrap();

        let sandbox = &state.resources["agents"];
        assert_eq!(sandbox.status, ResourceStatus::Running);
        let controller =
            AwsSandboxController::from_persisted(sandbox.internal_state.clone().unwrap()).unwrap();
        assert_eq!(controller.state, crate::sandbox::AwsSandboxState::Ready);
        assert_eq!(controller.image_arn.as_deref(), Some(SERVING_IMAGE_ARN));
        assert_eq!(controller.active_version.as_deref(), Some("1.0"));
        let SetupScaffolding::AwsSandbox {
            egress: Some(egress),
            ..
        } = &records["agents"]
        else {
            panic!("a deny sandbox records its egress objects");
        };
        assert_eq!(
            controller.egress_connector_arns,
            vec![egress.connector_arn.clone().unwrap()]
        );
        let binding = controller.get_binding_params().unwrap().unwrap();
        load(&binding)
            .await
            .unwrap_or_else(|error| panic!("the corrected binding must load: {error}\n{binding}"));
    }

    #[tokio::test]
    async fn a_second_seed_takes_its_dependencies_from_the_stack() {
        let (cloud, stack, mut state, records) = serving_deny_sandbox().await;
        state.resources.get_mut("agents").unwrap().dependencies =
            vec![ResourceRef::new(Sandbox::RESOURCE_TYPE, "removed-since")];

        seed_again(&cloud, &stack, &mut state, &records).unwrap();

        assert_eq!(
            state.resources["agents"].dependencies,
            stack.resources["agents"].combined_dependencies(),
            "teardown orders by these, so a stale list must not survive the seed"
        );
    }

    #[tokio::test]
    async fn a_seed_refuses_state_of_another_resource_type() {
        let (cloud, stack, mut state, records) = serving_deny_sandbox().await;
        state.resources.get_mut("agents").unwrap().resource_type = "worker".to_string();

        let error = seed_again(&cloud, &stack, &mut state, &records).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("another resource type or platform"),
            "{error}"
        );
        assert_eq!(state.resources["agents"].resource_type, "worker");
    }

    /// Setup makes each object, then the checkpoint holding its record never lands and the
    /// deployment is destroyed rather than set up again. Teardown still finds and deletes it.
    #[tokio::test]
    async fn a_destroy_after_a_lost_checkpoint_deletes_what_setup_made() {
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        for made in 1..=8 {
            let cloud = Shared::default();
            let mut records = BTreeMap::new();
            while cloud.lock().unwrap().mutations.len() < made {
                step(&cloud, &stack, &state, &mut records).await.unwrap();
            }

            destroy(&cloud, &stack, &mut BTreeMap::new()).await;

            let cloud = cloud.lock().unwrap();
            assert!(
                cloud.roles.is_empty(),
                "after call {made}: {:?}",
                cloud.roles
            );
            assert!(cloud.inline.is_empty(), "after call {made}");
            assert!(cloud.groups.is_empty(), "after call {made}");
            assert!(cloud.connectors.is_empty(), "after call {made}");
        }
    }

    /// Should Cloud Control not read a connector's tags back, its operator role still marks it.
    #[tokio::test]
    async fn a_destroy_deletes_an_unrecorded_connector_by_its_operator_role() {
        let stack = stack(SandboxEgress::Deny, created_network());
        let state = stack_state(Some(ResourceStatus::Running));
        let cloud = Shared::default();
        converge(&cloud, &stack, &state, &mut BTreeMap::new()).await;
        for (_, properties) in cloud.lock().unwrap().connectors.iter_mut() {
            properties.as_object_mut().unwrap().remove("Tags");
        }

        destroy(&cloud, &stack, &mut BTreeMap::new()).await;

        let cloud = cloud.lock().unwrap();
        assert!(cloud.connectors.is_empty());
        assert!(cloud.groups.is_empty() && cloud.roles.is_empty());
    }

    /// Recovery claims by name only what also carries setup's tags for this sandbox.
    #[tokio::test]
    async fn a_destroy_leaves_same_named_objects_setup_did_not_tag() {
        let stack = stack(SandboxEgress::Deny, created_network());
        let with_roles = |tagged: &[&str]| {
            let cloud = preexisting_group(vec![rule("127.0.0.1/32")]);
            let mut c = cloud.lock().unwrap();
            for name in [BUILD_ROLE, EGRESS_NAME] {
                c.roles.insert(name.to_string(), json!({}));
                if tagged.contains(&name) {
                    c.role_tags
                        .insert(name.to_string(), tags_for(PREFIX, "agents"));
                }
            }
            drop(c);
            cloud
        };

        let foreign_build_role = with_roles(&[EGRESS_NAME]);
        destroy(&foreign_build_role, &stack, &mut BTreeMap::new()).await;
        assert_eq!(
            foreign_build_role.lock().unwrap().mutations,
            Vec::<String>::new()
        );

        let foreign_group = with_roles(&[BUILD_ROLE, EGRESS_NAME]);
        foreign_group.lock().unwrap().connectors.push((
            format!("arn:aws:lambda:us-east-1:{ACCOUNT}:network-connector:foreign"),
            json!({
                "Name": sandbox_egress_connector_name(PREFIX, "agents"),
                "State": "ACTIVE",
            }),
        ));
        destroy(&foreign_group, &stack, &mut BTreeMap::new()).await;
        let cloud = foreign_group.lock().unwrap();
        assert!(cloud.roles.is_empty(), "both roles carry setup's tags");
        assert_eq!(cloud.groups.len(), 1, "the untagged group is not setup's");
        assert_eq!(
            cloud.connectors.len(),
            1,
            "the untagged connector is not setup's"
        );
    }

    /// A serving sandbox's egress mode changes only through setup running again. Each run leaves
    /// a binding the runtime loads with the declared mode, and the egress objects a switch to
    /// allow leaves unused stay recorded, so teardown still removes them.
    #[tokio::test]
    async fn setup_run_again_switches_a_serving_sandbox_between_allow_and_deny() {
        use crate::sandbox::AwsSandboxController;
        let cloud = Shared::default();
        let allow = stack(SandboxEgress::Allow, created_network());
        let deny = stack(SandboxEgress::Deny, created_network());
        let mut state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        converge(&cloud, &allow, &state, &mut records).await;
        let settings = alien_core::StackSettings::default();
        let mut serving = crate::ImporterRegistry::built_in()
            .run(
                &Sandbox::RESOURCE_TYPE,
                Platform::Aws,
                json!({
                    "imageIdentifier": SERVING_IMAGE_ARN,
                    "imageArn": SERVING_IMAGE_ARN,
                    "imageVersion": "1.0",
                    "allowEgress": true,
                }),
                &ImportContext {
                    resource_id: "agents",
                    platform: Platform::Aws,
                    region: "us-east-1",
                    stack_settings: &settings,
                    management_config: None,
                    resource: &allow.resources["agents"],
                },
            )
            .unwrap();
        serving.controller_platform = Some(Platform::Aws);
        state.resources.insert("agents".to_string(), serving);

        let binding = |state: &StackState| {
            AwsSandboxController::from_persisted(
                state.resources["agents"].internal_state.clone().unwrap(),
            )
            .unwrap()
            .get_binding_params()
            .unwrap()
            .expect("a serving sandbox keeps its binding")
        };

        converge(&cloud, &deny, &state, &mut records).await;
        seed_again(&cloud, &deny, &mut state, &records).unwrap();
        let connector_arn = cloud.lock().unwrap().connectors[0].0.clone();
        let denied = binding(&state);
        assert_eq!(
            egress_facts(&denied),
            (false, vec![connector_arn.clone()], vec![])
        );
        load(&denied).await.unwrap();

        converge(&cloud, &allow, &state, &mut records).await;
        seed_again(&cloud, &allow, &mut state, &records).unwrap();
        let allowed = binding(&state);
        assert_eq!(egress_facts(&allowed), (true, vec![], vec![]));
        load(&allowed).await.unwrap();
        assert_eq!(
            records["agents"],
            full_record("sg-1", &connector_arn),
            "the unused egress objects stay recorded for teardown"
        );

        let mut calls = 0;
        while tear_down(&cloud, &mut records).await.unwrap().0 != ScaffoldingProgress::Done {
            bounded(&mut calls);
        }
        let cloud = cloud.lock().unwrap();
        assert!(cloud.roles.is_empty() && cloud.groups.is_empty() && cloud.connectors.is_empty());
    }

    const SERVING_IMAGE_ARN: &str =
        "arn:aws:lambda:us-east-1:123456789012:microvm-image:test-agents";

    /// A converged deny sandbox whose state was registered Frozen-style: Ready at version 1.0.
    async fn serving_deny_sandbox() -> (
        Shared,
        Stack,
        StackState,
        BTreeMap<String, SetupScaffolding>,
    ) {
        let cloud = Shared::default();
        let stack = stack(SandboxEgress::Deny, created_network());
        let mut state = stack_state(Some(ResourceStatus::Running));
        let mut records = BTreeMap::new();
        converge(&cloud, &stack, &state, &mut records).await;
        let settings = alien_core::StackSettings::default();
        let mut serving = crate::ImporterRegistry::built_in()
            .run(
                &Sandbox::RESOURCE_TYPE,
                Platform::Aws,
                json!({
                    "imageIdentifier": SERVING_IMAGE_ARN,
                    "imageArn": SERVING_IMAGE_ARN,
                    "imageVersion": "1.0",
                }),
                &ImportContext {
                    resource_id: "agents",
                    platform: Platform::Aws,
                    region: "us-east-1",
                    stack_settings: &settings,
                    management_config: None,
                    resource: &stack.resources["agents"],
                },
            )
            .unwrap();
        serving.controller_platform = Some(Platform::Aws);
        state.resources.insert("agents".to_string(), serving);
        (cloud, stack, state, records)
    }

    fn seed_again(
        cloud: &Shared,
        stack: &Stack,
        state: &mut StackState,
        records: &BTreeMap<String, SetupScaffolding>,
    ) -> Result<()> {
        let provider = provider(cloud);
        let client_config = client_config();
        let ctx = SetupScaffoldingContext {
            client_config: &client_config,
            service_provider: &provider,
            resource_prefix: PREFIX,
        };
        let seeds = seeds(&ctx, stack, state, records)?;
        apply_seeds(
            &SeedContext {
                registry: &crate::ImporterRegistry::built_in(),
                stack_settings: &alien_core::StackSettings::default(),
                management_config: None,
            },
            stack,
            state,
            seeds,
        )
    }
}
