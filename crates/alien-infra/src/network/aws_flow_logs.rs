//! Direct setup's flow log for the VPC it creates: a log group that keeps records for
//! [`AWS_VPC_FLOW_LOG_RETENTION_DAYS`], a role the flow logs service assumes to write to that
//! group, and the flow log itself. The template setups render the same three objects.
//!
//! Each step makes at most one create call and records it before the next step. The log group
//! and the role have names derived from the resource prefix; one that already exists is taken
//! as this network's only when an earlier create call of this controller may have made it, so
//! a foreign object is never adopted and later deleted. The flow log is created with a recorded
//! idempotency token.
use super::*;
use alien_aws_clients::cloudwatch_logs::{CreateLogGroupRequest, PutRetentionPolicyRequest};
use alien_aws_clients::ec2::{CreateFlowLogsRequest, DescribeFlowLogsRequest};
use alien_aws_clients::iam::{CreateRoleRequest, CreateRoleTag};
use alien_core::AWS_VPC_FLOW_LOG_RETENTION_DAYS;

const DELIVERY_POLICY_NAME: &str = "flow-logs-delivery";

/// Seconds a flow is aggregated into one record. 600 is the AWS default and the cheapest.
const MAX_AGGREGATION_INTERVAL: u32 = 600;

/// IAM is eventually consistent: give a new role policy time to reach the flow logs service
/// before the flow log is created.
const ROLE_PROPAGATION_DELAY: Duration = Duration::from_secs(10);

/// What setup created toward the VPC's flow log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FlowLogSetup {
    /// A `CreateLogGroup` call was made, so the group may exist.
    #[serde(default)]
    log_group_requested: bool,
    /// The group exists and keeps records for a year.
    #[serde(default)]
    log_group_ready: bool,
    /// A `CreateRole` call was made, so the role may exist.
    #[serde(default)]
    role_requested: bool,
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
}

impl FlowLogSetup {
    pub(super) fn is_complete(&self) -> bool {
        self.flow_log_id.is_some()
    }
}

/// Where creating the flow log got to.
pub(super) enum FlowLogStep {
    Ready,
    /// Step again after this delay.
    Again(Duration),
}

const AGAIN: FlowLogStep = FlowLogStep::Again(Duration::from_secs(1));

/// The name of the log group and of the role.
pub(super) fn object_name(resource_prefix: &str, resource_id: &str) -> String {
    format!("{resource_prefix}-{resource_id}-flow-logs")
}

fn create_failed(resource_id: &str, message: String) -> ErrorData {
    ErrorData::CloudPlatformError {
        message,
        resource_id: Some(resource_id.to_string()),
    }
}

fn already_exists(resource_id: &str, object: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: format!(
            "{object} already exists and was not created by this network's setup. \
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
            "Principal": { "Service": "vpc-flow-logs.amazonaws.com" },
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
            "Action": [
                "logs:CreateLogStream",
                "logs:PutLogEvents",
                "logs:DescribeLogGroups",
                "logs:DescribeLogStreams"
            ],
            "Resource": [log_group_arn, format!("{log_group_arn}:*")]
        }]
    })
    .to_string()
}

impl AwsNetworkController {
    /// Whether a setup run should add the flow log to the VPC this controller created. Only
    /// a direct setup holds the credentials that creates the role.
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

    /// One create call per step until the flow log exists.
    pub(super) async fn reconcile_flow_log(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<FlowLogStep> {
        let config = ctx.desired_resource_config::<Network>()?;
        let id = config.id.as_str();
        let vpc_id = self.vpc_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not set in state".to_string(),
                resource_id: Some(id.to_string()),
            })
        })?;
        let aws_cfg = ctx.get_aws_config()?;
        let name = object_name(ctx.resource_prefix, id);
        let tags = standard_resource_tags(ctx.resource_prefix, id);
        let setup = self.flow_logs.get_or_insert_with(FlowLogSetup::default);

        if setup.flow_log_id.is_some() {
            return Ok(FlowLogStep::Ready);
        }

        if !setup.log_group_ready {
            let logs = ctx.service_provider.get_aws_logs_client(aws_cfg).await?;
            let requested_before = setup.log_group_requested;
            setup.log_group_requested = true;
            match logs
                .create_log_group(
                    CreateLogGroupRequest::builder()
                        .log_group_name(name.clone())
                        .tags(tags.clone().into_iter().collect())
                        .build(),
                )
                .await
            {
                Ok(()) => info!(log_group = %name, "Created VPC flow log group"),
                Err(error) if is_conflict(&error) && requested_before => {
                    info!(log_group = %name, "VPC flow log group already created")
                }
                Err(error) if is_conflict(&error) => {
                    // Nothing of ours to delete: the group is someone else's.
                    setup.log_group_requested = false;
                    return Err(already_exists(id, format!("Log group '{name}'")));
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
                    .retention_in_days(AWS_VPC_FLOW_LOG_RETENTION_DAYS)
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

        let partition = crate::sandbox::aws_partition(&aws_cfg.region);
        if setup.role_arn.is_none() {
            let iam = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
            let requested_before = setup.role_requested;
            setup.role_requested = true;
            let role_arn = match iam
                .create_role(
                    CreateRoleRequest::builder()
                        .role_name(name.clone())
                        .assume_role_policy_document(trust_policy(
                            partition,
                            &aws_cfg.region,
                            &aws_cfg.account_id,
                        ))
                        .tags(
                            tags.into_iter()
                                .map(|(key, value)| CreateRoleTag { key, value })
                                .collect(),
                        )
                        .build(),
                )
                .await
            {
                Ok(response) => response.create_role_result.role.arn,
                Err(error) if is_conflict(&error) && requested_before => {
                    iam.get_role(&name)
                        .await
                        .context(create_failed(
                            id,
                            format!("Failed to read IAM role '{name}'"),
                        ))?
                        .get_role_result
                        .role
                        .arn
                }
                Err(error) if is_conflict(&error) => {
                    setup.role_requested = false;
                    return Err(already_exists(id, format!("IAM role '{name}'")));
                }
                Err(error) => {
                    return Err(error.context(create_failed(
                        id,
                        format!("Failed to create IAM role '{name}'"),
                    )))
                }
            };
            info!(role = %role_arn, "Created VPC flow log delivery role");
            setup.role_arn = Some(role_arn);
            return Ok(AGAIN);
        }

        if !setup.role_policy_ready {
            let iam = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
            let log_group_arn = format!(
                "arn:{partition}:logs:{}:{}:log-group:{name}",
                aws_cfg.region, aws_cfg.account_id
            );
            iam.put_role_policy(
                &name,
                DELIVERY_POLICY_NAME,
                &delivery_policy(&log_group_arn),
            )
            .await
            .context(create_failed(
                id,
                format!("Failed to set the policy of IAM role '{name}'"),
            ))?;
            setup.role_policy_ready = true;
            return Ok(FlowLogStep::Again(ROLE_PROPAGATION_DELAY));
        }

        let role_arn = setup.role_arn.clone().expect("checked above");
        let token = setup
            .create_token
            .get_or_insert_with(new_create_attempt_token)
            .clone();
        let ec2 = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let created = ec2
            .create_flow_logs(
                CreateFlowLogsRequest::builder()
                    .resource_ids(vec![vpc_id.clone()])
                    .resource_type("VPC".to_string())
                    .traffic_type("ALL".to_string())
                    .log_group_name(name.clone())
                    .deliver_logs_permission_arn(role_arn)
                    .max_aggregation_interval(MAX_AGGREGATION_INTERVAL)
                    .tag_specifications(self.create_tags(ctx.resource_prefix, id, "vpc-flow-log"))
                    .client_token(token)
                    .build(),
            )
            .await;
        let flow_log_id = match created {
            Ok(response) => response
                .flow_log_id_set
                .and_then(|set| set.items.into_iter().next())
                .ok_or_else(|| {
                    AlienError::new(create_failed(
                        id,
                        format!("CreateFlowLogs returned no flow log ID for VPC '{vpc_id}'"),
                    ))
                })?,
            // This VPC already sends to this network's group: a create whose response was lost.
            Err(error) if is_conflict(&error) => self
                .flow_log_ids(ctx, &vpc_id, &name, id)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    error.context(create_failed(
                        id,
                        format!("Flow log of VPC '{vpc_id}' exists but could not be found"),
                    ))
                })?,
            Err(error) => {
                return Err(error.context(create_failed(
                    id,
                    format!("Failed to create the flow log of VPC '{vpc_id}'"),
                )))
            }
        };
        info!(flow_log_id = %flow_log_id, vpc_id = %vpc_id, "Created VPC flow log");
        if let Some(setup) = self.flow_logs.as_mut() {
            setup.flow_log_id = Some(flow_log_id);
        }
        Ok(FlowLogStep::Ready)
    }

    /// Flow logs of `vpc_id` that send to this network's log group.
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
        let mut ids = Vec::new();
        let mut next_token = None;
        loop {
            let response = ec2
                .describe_flow_logs(
                    DescribeFlowLogsRequest::builder()
                        .filters(vec![
                            Filter {
                                name: "resource-id".to_string(),
                                values: vec![vpc_id.to_string()],
                            },
                            Filter {
                                name: "log-group-name".to_string(),
                                values: vec![log_group_name.to_string()],
                            },
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

    /// Deletes the flow log, then the role, then the log group, recording each removal. The
    /// flow log goes before the VPC. The log group goes with the network: its records were
    /// about a VPC that no longer exists, and keeping it would make a later setup with the
    /// same prefix find a group it did not create.
    pub(super) async fn delete_flow_log(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let Some(setup) = self.flow_logs.clone() else {
            return Ok(());
        };
        let config = ctx.desired_resource_config::<Network>()?;
        let id = config.id.as_str();
        let aws_cfg = ctx.get_aws_config()?;
        let name = object_name(ctx.resource_prefix, id);

        if setup.flow_log_id.is_some() || setup.create_token.is_some() {
            let ids = match (&setup.flow_log_id, &self.vpc_id) {
                (Some(flow_log_id), _) => vec![flow_log_id.clone()],
                // A create whose response was lost: find it on the VPC.
                (None, Some(vpc_id)) => self
                    .denied_lookup(
                        ctx,
                        id,
                        "VPC flow logs",
                        self.flow_log_ids(ctx, vpc_id, &name, id).await.map(Some),
                    )?
                    .unwrap_or_default(),
                (None, None) => Vec::new(),
            };
            let ec2 = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
            for flow_log_id in ids {
                match ec2.delete_flow_log(&flow_log_id).await {
                    Ok(()) => info!(flow_log_id = %flow_log_id, "Deleted VPC flow log"),
                    Err(error) if is_not_found(&error) => {
                        info!(flow_log_id = %flow_log_id, "VPC flow log already deleted")
                    }
                    Err(error) if is_access_denied(&error) => self.denied_delete(
                        ctx,
                        id,
                        &format!("flow log '{flow_log_id}'"),
                        "ec2:DeleteFlowLogs",
                        error,
                    )?,
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete flow log '{flow_log_id}'"),
                            resource_id: Some(id.to_string()),
                        }))
                    }
                }
            }
            self.delete_removed_something = true;
            if let Some(setup) = self.flow_logs.as_mut() {
                setup.flow_log_id = None;
                setup.create_token = None;
            }
        }

        if setup.role_requested {
            let iam = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
            match iam.delete_role_policy(&name, DELIVERY_POLICY_NAME).await {
                Ok(()) => {}
                Err(error) if is_not_found(&error) => {}
                Err(error) if is_access_denied(&error) => self.denied_delete(
                    ctx,
                    id,
                    &format!("policy of IAM role '{name}'"),
                    "iam:DeleteRolePolicy",
                    error,
                )?,
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete the policy of IAM role '{name}'"),
                        resource_id: Some(id.to_string()),
                    }))
                }
            }
            match iam.delete_role(&name).await {
                Ok(()) => info!(role = %name, "Deleted VPC flow log delivery role"),
                Err(error) if is_not_found(&error) => {}
                Err(error) if is_access_denied(&error) => self.denied_delete(
                    ctx,
                    id,
                    &format!("IAM role '{name}'"),
                    "iam:DeleteRole",
                    error,
                )?,
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete IAM role '{name}'"),
                        resource_id: Some(id.to_string()),
                    }))
                }
            }
            self.delete_removed_something = true;
            if let Some(setup) = self.flow_logs.as_mut() {
                setup.role_requested = false;
                setup.role_arn = None;
                setup.role_policy_ready = false;
            }
        }

        if setup.log_group_requested {
            let logs = ctx.service_provider.get_aws_logs_client(aws_cfg).await?;
            match logs.delete_log_group(&name).await {
                Ok(()) => info!(log_group = %name, "Deleted VPC flow log group"),
                Err(error) if is_not_found(&error) => {}
                Err(error) if is_access_denied(&error) => self.denied_delete(
                    ctx,
                    id,
                    &format!("log group '{name}'"),
                    "logs:DeleteLogGroup",
                    error,
                )?,
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete log group '{name}'"),
                        resource_id: Some(id.to_string()),
                    }))
                }
            }
            self.delete_removed_something = true;
        }

        self.flow_logs = None;
        Ok(())
    }
}

#[cfg(test)]
#[path = "aws_flow_logs_tests.rs"]
mod tests;
