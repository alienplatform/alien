//! Direct setup's flow log for the VPC it creates: a log group that keeps records for
//! the retention in [`AWS_VPC_FLOW_LOG`], a role the flow logs service assumes to write to that
//! group, and the flow log itself. The template setups render the same three objects.
//!
//! Each step makes at most one create call and records it before the next step. The log group
//! and the role are named after the resource prefix and carry this network's tags. An object
//! with that name belongs to this network only when it carries those tags: a setup run takes it
//! over even when the state that recorded it was lost (an older binary that rewrites controller
//! state drops fields it does not know), and an object made by anything else, such as a
//! Terraform setup with the same prefix, is refused and never deleted. The flow log is found on
//! the VPC by its log group and tags, and created with a recorded idempotency token.
use super::*;
use alien_aws_clients::cloudwatch_logs::{CreateLogGroupRequest, PutRetentionPolicyRequest};
use alien_aws_clients::ec2::{CreateFlowLogsRequest, DescribeFlowLogsRequest};
use alien_aws_clients::iam::{CreateRoleRequest, CreateRoleTag};
use alien_core::{ALIEN_MANAGED_BY_TAG_KEY, ALIEN_STACK_TAG_KEY, AWS_VPC_FLOW_LOG};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const DELIVERY_POLICY_NAME: &str = "flow-logs-delivery";

/// IAM is eventually consistent: give a new role policy time to reach the flow logs service
/// before the flow log is created.
const ROLE_PROPAGATION_DELAY: Duration = Duration::from_secs(10);

/// What setup created toward the VPC's flow log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FlowLogSetup {
    /// The group exists, is this network's, and keeps records for a year.
    #[serde(default)]
    log_group_ready: bool,
    /// The delivery role, once it exists and is this network's.
    #[serde(default)]
    role_arn: Option<String>,
    /// The role's delivery policy is in place.
    #[serde(default)]
    role_policy_ready: bool,
    /// Idempotency token of the `CreateFlowLogs` call, recorded before the call.
    #[serde(default)]
    create_token: Option<String>,
    #[serde(default)]
    flow_log_id: Option<String>,
    /// The flow logs service reported delivering to the group.
    #[serde(default)]
    delivering: bool,
    /// Reads of the flow log that did not report a delivery result yet.
    #[serde(default)]
    delivery_checks: u32,
}

impl FlowLogSetup {
    pub(super) fn is_complete(&self) -> bool {
        self.flow_log_id.is_some() && self.delivering
    }
}

/// How many reads a new flow log gets to report its delivery result, 5 s apart.
const DELIVERY_CHECKS: u32 = 12;

/// Where creating the flow log got to.
pub(super) enum FlowLogStep {
    Ready,
    /// Step again after this delay.
    Again(Duration),
}

const AGAIN: FlowLogStep = FlowLogStep::Again(Duration::from_secs(1));

/// The name of the log group.
pub(super) fn log_group_name(resource_prefix: &str, resource_id: &str) -> String {
    format!("{resource_prefix}-{resource_id}-flow-logs")
}

/// The name of the delivery role. IAM caps role names at 64 characters; a name that would be
/// longer replaces the resource ID with a hash of it, keeping the prefix and the suffix the
/// setup permissions name.
pub(super) fn role_name(resource_prefix: &str, resource_id: &str) -> String {
    const IAM_ROLE_NAME_MAX_LEN: usize = 64;
    let name = log_group_name(resource_prefix, resource_id);
    if name.len() <= IAM_ROLE_NAME_MAX_LEN {
        return name;
    }
    let hash: String = Sha256::digest(resource_id.as_bytes())[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{resource_prefix}-{hash}-flow-logs")
}

/// A delete AWS refused. The object stays recorded and the delete fails: completing it would
/// leave the object behind with nothing that remembers it.
fn denied(
    resource_id: &str,
    object: &str,
    action: &str,
    error: AlienError<CloudClientErrorData>,
) -> AlienError<ErrorData> {
    error.context(ErrorData::CloudPlatformError {
        message: format!("Access denied deleting {object} ({action})"),
        resource_id: Some(resource_id.to_string()),
    })
}

/// Whether `tags` include every tag this controller puts on what it creates for the network.
fn owned<'a>(
    tags: impl IntoIterator<Item = (&'a str, &'a str)>,
    resource_prefix: &str,
    resource_id: &str,
) -> bool {
    let tags: HashMap<&str, &str> = tags.into_iter().collect();
    standard_resource_tags(resource_prefix, resource_id)
        .iter()
        .all(|(key, value)| tags.get(key.as_str()) == Some(&value.as_str()))
}

fn log_group_arn(
    partition: &str,
    aws_cfg: &alien_aws_clients::AwsClientConfig,
    name: &str,
) -> String {
    format!(
        "arn:{partition}:logs:{}:{}:log-group:{name}",
        aws_cfg.region, aws_cfg.account_id
    )
}

fn create_failed(resource_id: &str, message: String) -> ErrorData {
    ErrorData::CloudPlatformError {
        message,
        resource_id: Some(resource_id.to_string()),
    }
}

fn not_owned(resource_id: &str, object: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: format!(
            "{object} already exists and does not carry this network's tags. \
             Delete or rename it, then run setup again."
        ),
        resource_id: Some(resource_id.to_string()),
    })
}

fn trust_policy(partition: &str, region: &str, account_id: &str) -> String {
    serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "Service": AWS_VPC_FLOW_LOG.delivery_service },
            "Action": "sts:AssumeRole",
            "Condition": {
                "StringEquals": { "aws:SourceAccount": account_id },
                "ArnLike": {
                    "aws:SourceArn": format!("arn:{partition}:ec2:{region}:{account_id}:vpc-flow-log/*")
                }
            }
        }]
    })
    .to_string()
}

fn delivery_policy(log_group_arn: &str) -> String {
    serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Action": AWS_VPC_FLOW_LOG.delivery_actions,
            "Resource": [log_group_arn, format!("{log_group_arn}:*")]
        }]
    })
    .to_string()
}

impl AwsNetworkController {
    /// Whether a setup run should add the flow log to the VPC this controller created. Only
    /// a direct setup holds the credentials that create the role.
    pub(super) fn flow_log_missing(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let config = ctx.desired_resource_config::<Network>()?;
        Ok(
            ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
                && ctx.deployment_config.setup_run
                && matches!(config.settings, NetworkSettings::Create { .. })
                && !self.is_byo_vpc
                && self.vpc_id.is_some()
                && !self
                    .flow_logs
                    .as_ref()
                    .is_some_and(FlowLogSetup::is_complete),
        )
    }

    /// Whether this run may create or delete the flow log objects. Only a run with the
    /// installer's credentials holds what creates the delivery role. An automatic setup also
    /// carries direct setup's authority (the manager records it on every automatic import), but
    /// unless an installer's setup run is making the call, it runs as the management identity.
    /// That identity may only create roles under the setup's permissions boundary, which does not
    /// allow log delivery, and holds no flow log grants, so it creates none of these objects. A
    /// flow log an installer's run created earlier is still recorded and deleted.
    pub(super) fn flow_log_owned_here(ctx: &ResourceControllerContext<'_>) -> bool {
        ctx.initial_setup_authority == alien_core::InitialSetupAuthority::DirectSetup
            && (ctx.deployment_config.setup_run
                || ctx.deployment_config.stack_settings.setup_management
                    != alien_core::SetupManagement::Automatic)
    }

    /// One create call per step until the flow log exists.
    pub(super) async fn reconcile_flow_log(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<FlowLogStep> {
        if !Self::flow_log_owned_here(ctx) {
            return Ok(FlowLogStep::Ready);
        }
        let config = ctx.desired_resource_config::<Network>()?;
        let id = config.id.as_str();
        let vpc_id = self.vpc_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(id.to_string()),
            })
        })?;
        let aws_cfg = ctx.get_aws_config()?;
        let partition = crate::sandbox::aws_partition(&aws_cfg.region);
        let prefix = ctx.resource_prefix;
        let name = log_group_name(prefix, id);
        let role = role_name(prefix, id);
        let setup = self.flow_logs.get_or_insert_with(FlowLogSetup::default);

        if setup.is_complete() {
            return Ok(FlowLogStep::Ready);
        }
        if setup.flow_log_id.is_some() {
            return self.check_delivery(ctx, id).await;
        }

        if !setup.log_group_ready {
            let logs = ctx.service_provider.get_aws_logs_client(aws_cfg).await?;
            match logs
                .create_log_group(
                    CreateLogGroupRequest::builder()
                        .log_group_name(name.clone())
                        .tags(standard_resource_tags(prefix, id))
                        .build(),
                )
                .await
            {
                Ok(()) => info!(log_group = %name, "Created VPC flow log group"),
                Err(error) if is_conflict(&error) => {
                    let tags = logs
                        .list_tags_for_resource(&log_group_arn(partition, aws_cfg, &name))
                        .await
                        .context(create_failed(
                            id,
                            format!("Failed to read the tags of log group '{name}'"),
                        ))?;
                    if !owned(
                        tags.iter()
                            .map(|(key, value)| (key.as_str(), value.as_str())),
                        prefix,
                        id,
                    ) {
                        return Err(not_owned(id, format!("Log group '{name}'")));
                    }
                    info!(log_group = %name, "VPC flow log group already created");
                }
                Err(error) => {
                    return Err(error.context(create_failed(
                        id,
                        format!("Failed to create log group '{name}'"),
                    )))
                }
            }
            logs.put_retention_policy(
                PutRetentionPolicyRequest::builder()
                    .log_group_name(name.clone())
                    .retention_in_days(AWS_VPC_FLOW_LOG.retention_in_days)
                    .build(),
            )
            .await
            .context(create_failed(
                id,
                format!("Failed to set the retention of log group '{name}'"),
            ))?;
            setup.log_group_ready = true;
            return Ok(AGAIN);
        }

        if setup.role_arn.is_none() {
            let iam = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
            let role_arn = match iam
                .create_role(
                    CreateRoleRequest::builder()
                        .role_name(role.clone())
                        .assume_role_policy_document(trust_policy(
                            partition,
                            &aws_cfg.region,
                            &aws_cfg.account_id,
                        ))
                        .tags(
                            standard_resource_tags(prefix, id)
                                .into_iter()
                                .map(|(key, value)| CreateRoleTag { key, value })
                                .collect(),
                        )
                        .build(),
                )
                .await
            {
                Ok(response) => response.create_role_result.role.arn,
                Err(error) if is_conflict(&error) => {
                    let existing = iam
                        .get_role(&role)
                        .await
                        .context(create_failed(
                            id,
                            format!("Failed to read IAM role '{role}'"),
                        ))?
                        .get_role_result
                        .role;
                    let tags = existing.tags.iter().flat_map(|tags| &tags.member);
                    if !owned(
                        tags.map(|tag| (tag.key.as_str(), tag.value.as_str())),
                        prefix,
                        id,
                    ) {
                        return Err(not_owned(id, format!("IAM role '{role}'")));
                    }
                    existing.arn
                }
                Err(error) => {
                    return Err(error.context(create_failed(
                        id,
                        format!("Failed to create IAM role '{role}'"),
                    )))
                }
            };
            info!(role = %role_arn, "Created VPC flow log delivery role");
            setup.role_arn = Some(role_arn);
            return Ok(AGAIN);
        }

        if !setup.role_policy_ready {
            let iam = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
            iam.put_role_policy(
                &role,
                DELIVERY_POLICY_NAME,
                &delivery_policy(&log_group_arn(partition, aws_cfg, &name)),
            )
            .await
            .context(create_failed(
                id,
                format!("Failed to set the policy of IAM role '{role}'"),
            ))?;
            setup.role_policy_ready = true;
            return Ok(FlowLogStep::Again(ROLE_PROPAGATION_DELAY));
        }

        let role_arn = setup.role_arn.clone().expect("checked above");
        let token = setup
            .create_token
            .get_or_insert_with(new_create_attempt_token)
            .clone();
        // A flow log this network made earlier, whose ID was not recorded.
        let flow_log_id = match self
            .flow_log_ids(ctx, &vpc_id, &name, id)
            .await?
            .into_iter()
            .next()
        {
            Some(existing) => existing,
            None => {
                self.create_flow_log(ctx, &vpc_id, &name, role_arn, token, id)
                    .await?
            }
        };
        info!(flow_log_id = %flow_log_id, vpc_id = %vpc_id, "VPC flow log in place");
        if let Some(setup) = self.flow_logs.as_mut() {
            setup.flow_log_id = Some(flow_log_id);
            setup.delivery_checks = 0;
        }
        Ok(AGAIN)
    }

    /// Reads the recorded flow log until the flow logs service reports delivering to the group.
    /// A failed delivery, such as a role the service cannot use yet, deletes the flow log and
    /// fails the step; the retry writes the role's policy again and creates a new flow log.
    async fn check_delivery(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        id: &str,
    ) -> Result<FlowLogStep> {
        let setup = self.flow_logs.as_mut().expect("flow log recorded");
        let flow_log_id = setup.flow_log_id.clone().expect("flow log recorded");
        let ec2 = ctx
            .service_provider
            .get_aws_ec2_client(ctx.get_aws_config()?)
            .await?;
        let described = match ec2
            .describe_flow_logs(
                DescribeFlowLogsRequest::builder()
                    .flow_log_ids(vec![flow_log_id.clone()])
                    .build(),
            )
            .await
        {
            Ok(described) => Some(described),
            Err(error) if is_not_found(&error) => None,
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to read flow log '{flow_log_id}'"),
                    resource_id: Some(id.to_string()),
                }))
            }
        };
        let flow_log = described
            .and_then(|described| described.flow_log_set)
            .into_iter()
            .flat_map(|set| set.items)
            .find(|flow_log| flow_log.flow_log_id.as_deref() == Some(flow_log_id.as_str()));
        let Some(flow_log) = flow_log else {
            // Gone: create it again.
            setup.flow_log_id = None;
            setup.create_token = None;
            return Ok(AGAIN);
        };
        match flow_log.deliver_logs_status.as_deref() {
            Some("SUCCESS") => {
                setup.delivering = true;
                Ok(FlowLogStep::Ready)
            }
            Some("FAILED") => {
                let reason = flow_log
                    .deliver_logs_error_message
                    .unwrap_or_else(|| "no reason given".to_string());
                ec2.delete_flow_log(&flow_log_id)
                    .await
                    .or_else(|error| if is_not_found(&error) { Ok(()) } else { Err(error) })
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete flow log '{flow_log_id}' that cannot deliver"),
                        resource_id: Some(id.to_string()),
                    })?;
                setup.flow_log_id = None;
                setup.create_token = None;
                setup.role_policy_ready = false;
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Flow log '{flow_log_id}' could not deliver to its log group ({reason}); it was deleted and is created again on retry"
                    ),
                    resource_id: Some(id.to_string()),
                }))
            }
            _ if setup.delivery_checks < DELIVERY_CHECKS => {
                setup.delivery_checks += 1;
                Ok(FlowLogStep::Again(Duration::from_secs(5)))
            }
            status => Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Flow log '{flow_log_id}' reported no delivery result after {DELIVERY_CHECKS} reads (status {status:?})"
                ),
                resource_id: Some(id.to_string()),
            })),
        }
    }

    async fn create_flow_log(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vpc_id: &str,
        name: &str,
        role_arn: String,
        token: String,
        id: &str,
    ) -> Result<String> {
        let ec2 = ctx
            .service_provider
            .get_aws_ec2_client(ctx.get_aws_config()?)
            .await?;
        let created = ec2
            .create_flow_logs(
                CreateFlowLogsRequest::builder()
                    .resource_ids(vec![vpc_id.to_string()])
                    .resource_type("VPC".to_string())
                    .traffic_type(AWS_VPC_FLOW_LOG.traffic_type.to_string())
                    .log_group_name(name.to_string())
                    .deliver_logs_permission_arn(role_arn)
                    .max_aggregation_interval(AWS_VPC_FLOW_LOG.max_aggregation_interval)
                    .tag_specifications(self.create_tags(ctx.resource_prefix, id, "vpc-flow-log"))
                    .client_token(token)
                    .build(),
            )
            .await;
        match created {
            Ok(response) => response
                .flow_log_id_set
                .and_then(|set| set.items.into_iter().next())
                .ok_or_else(|| {
                    AlienError::new(create_failed(
                        id,
                        format!("CreateFlowLogs returned no flow log ID for VPC '{vpc_id}'"),
                    ))
                }),
            // The VPC already sends to this group: a flow log the lookup did not see yet.
            Err(error) if is_conflict(&error) => self
                .flow_log_ids(ctx, vpc_id, name, id)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    error.context(create_failed(
                        id,
                        format!("Flow log of VPC '{vpc_id}' exists but could not be found"),
                    ))
                }),
            Err(error) => Err(error.context(create_failed(
                id,
                format!("Failed to create the flow log of VPC '{vpc_id}'"),
            ))),
        }
    }

    /// This network's flow logs of `vpc_id`: those that send to its log group and carry its
    /// tags.
    async fn flow_log_ids(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vpc_id: &str,
        log_group_name: &str,
        resource_id: &str,
    ) -> Result<Vec<String>> {
        let ec2 = ctx
            .service_provider
            .get_aws_ec2_client(ctx.get_aws_config()?)
            .await?;
        let filter = |name: String, value: &str| Filter {
            name,
            values: vec![value.to_string()],
        };
        let mut ids = Vec::new();
        let mut next_token = None;
        loop {
            let response = ec2
                .describe_flow_logs(
                    DescribeFlowLogsRequest::builder()
                        .filters(vec![
                            filter("resource-id".to_string(), vpc_id),
                            filter("log-group-name".to_string(), log_group_name),
                            filter(format!("tag:{ALIEN_STACK_TAG_KEY}"), ctx.resource_prefix),
                            filter(
                                format!("tag:{ALIEN_MANAGED_BY_TAG_KEY}"),
                                alien_core::ALIEN_MANAGED_BY_TAG_VALUE,
                            ),
                        ])
                        .maybe_next_token(next_token)
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to list the flow logs of VPC '{vpc_id}'"),
                    resource_id: Some(resource_id.to_string()),
                })?;
            ids.extend(
                response
                    .flow_log_set
                    .into_iter()
                    .flat_map(|set| set.items)
                    .filter_map(|flow_log| flow_log.flow_log_id),
            );
            next_token = response.next_token;
            if next_token.is_none() {
                return Ok(ids);
            }
        }
    }

    /// Deletes the flow log, then the role, then the log group, each only while it carries
    /// this network's tags. The flow log goes before the VPC: deleting a VPC leaves its flow
    /// log behind. The log group goes with the network: its records were about a VPC that no
    /// longer exists, and keeping it would leave a billed group nothing owns.
    pub(super) async fn delete_flow_log(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<Network>()?;
        let id = config.id.as_str();
        let aws_cfg = ctx.get_aws_config()?;
        let partition = crate::sandbox::aws_partition(&aws_cfg.region);
        let prefix = ctx.resource_prefix;
        let name = log_group_name(prefix, id);
        let role_name = role_name(prefix, id);

        if let Some(vpc_id) = self.vpc_id.clone() {
            let mut ids = self.flow_log_ids(ctx, &vpc_id, &name, id).await?;
            if let Some(recorded) = self.flow_logs.as_ref().and_then(|s| s.flow_log_id.clone()) {
                if !ids.contains(&recorded) {
                    ids.push(recorded);
                }
            }
            let ec2 = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
            for flow_log_id in ids {
                match ec2.delete_flow_log(&flow_log_id).await {
                    Ok(()) => info!(flow_log_id = %flow_log_id, "Deleted VPC flow log"),
                    Err(error) if is_not_found(&error) => {
                        info!(flow_log_id = %flow_log_id, "VPC flow log already deleted")
                    }
                    Err(error) if is_access_denied(&error) => {
                        return Err(denied(
                            id,
                            &format!("flow log '{flow_log_id}'"),
                            "ec2:DeleteFlowLogs",
                            error,
                        ))
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete flow log '{flow_log_id}'"),
                            resource_id: Some(id.to_string()),
                        }))
                    }
                }
                self.delete_removed_something = true;
            }
            if let Some(setup) = self.flow_logs.as_mut() {
                setup.flow_log_id = None;
                setup.create_token = None;
            }
        }

        let iam = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
        let role = match iam.get_role(&role_name).await {
            Ok(response) => Some(response.get_role_result.role),
            Err(error) if is_not_found(&error) => None,
            Err(error) if is_access_denied(&error) => {
                return Err(denied(
                    id,
                    &format!("IAM role '{role_name}'"),
                    "iam:GetRole",
                    error,
                ))
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to read IAM role '{role_name}'"),
                    resource_id: Some(id.to_string()),
                }))
            }
        };
        if let Some(role) = role {
            let tags = role.tags.iter().flat_map(|tags| &tags.member);
            if owned(
                tags.map(|tag| (tag.key.as_str(), tag.value.as_str())),
                prefix,
                id,
            ) {
                let deleted = |action: &'static str,
                               result: std::result::Result<
                    (),
                    AlienError<CloudClientErrorData>,
                >| match result {
                    Ok(()) => Ok(()),
                    Err(error) if is_not_found(&error) => Ok(()),
                    Err(error) if is_access_denied(&error) => Err(denied(
                        id,
                        &format!("IAM role '{role_name}'"),
                        action,
                        error,
                    )),
                    Err(error) => Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete IAM role '{role_name}' ({action})"),
                        resource_id: Some(id.to_string()),
                    })),
                };
                deleted(
                    "iam:DeleteRolePolicy",
                    iam.delete_role_policy(&role_name, DELIVERY_POLICY_NAME)
                        .await,
                )?;
                deleted("iam:DeleteRole", iam.delete_role(&role_name).await)?;
                info!(role = %role_name, "Deleted VPC flow log delivery role");
                self.delete_removed_something = true;
            } else {
                info!(role = %role_name, "IAM role does not carry this network's tags; left in place");
            }
        }
        if let Some(setup) = self.flow_logs.as_mut() {
            setup.role_arn = None;
            setup.role_policy_ready = false;
        }

        let logs = ctx.service_provider.get_aws_logs_client(aws_cfg).await?;
        let tags = match logs
            .list_tags_for_resource(&log_group_arn(partition, aws_cfg, &name))
            .await
        {
            Ok(tags) => Some(tags),
            Err(error) if is_not_found(&error) => None,
            Err(error) if is_access_denied(&error) => {
                return Err(denied(
                    id,
                    &format!("log group '{name}'"),
                    "logs:ListTagsForResource",
                    error,
                ))
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to read the tags of log group '{name}'"),
                    resource_id: Some(id.to_string()),
                }))
            }
        };
        if let Some(tags) = tags {
            if owned(
                tags.iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
                prefix,
                id,
            ) {
                match logs.delete_log_group(&name).await {
                    Ok(()) => info!(log_group = %name, "Deleted VPC flow log group"),
                    Err(error) if is_not_found(&error) => {}
                    Err(error) if is_access_denied(&error) => {
                        return Err(denied(
                            id,
                            &format!("log group '{name}'"),
                            "logs:DeleteLogGroup",
                            error,
                        ))
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete log group '{name}'"),
                            resource_id: Some(id.to_string()),
                        }))
                    }
                }
                self.delete_removed_something = true;
            } else {
                info!(log_group = %name, "Log group does not carry this network's tags; left in place");
            }
        }

        self.flow_logs = None;
        Ok(())
    }
}

#[cfg(test)]
#[path = "aws_flow_logs_tests.rs"]
mod tests;
