use std::collections::BTreeMap;

use alien_aws_clients::elbv2::{
    DescribeListenersRequest, DescribeLoadBalancersRequest, DescribeTargetGroupsRequest,
    ModifyListenerRequest,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{Context, ContextError};

use super::*;
use crate::core::aws_tag_scoped::is_remote_not_found;

/// Exact provider identifiers from a previously saved workload controller.
/// Never construct this receipt from a name-only lookup. Ownership tags must
/// be derived from that controller's original stack/resource identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsVmFrontendReceipt {
    pub target_group_arn: String,
    pub load_balancer_arn: String,
    pub listener_arn: String,
    pub ownership_tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecordedFrontend {
    receipt: AwsVmFrontendReceipt,
    #[serde(default)]
    pub certificate_arns: Vec<String>,
    #[serde(default)]
    load_balancer_deleted: bool,
    #[serde(default)]
    target_group_deleted: bool,
}

impl AwsVmFrontend {
    /// Preserves an older controller's recorded lifetime without inventing an
    /// attempt token. No output is confirmed here: preparation verifies every
    /// provider identifier, ownership tag and forwarding edge before use.
    pub fn from_recorded(receipt: AwsVmFrontendReceipt) -> Result<Self> {
        if receipt.target_group_arn.is_empty()
            || receipt.load_balancer_arn.is_empty()
            || receipt.listener_arn.is_empty()
            || receipt.ownership_tags.is_empty()
            || receipt.ownership_tags.keys().any(String::is_empty)
        {
            return Err(invalid(
                "Recorded frontend requires exact IDs and original ownership tags",
            ));
        }
        Ok(Self {
            recorded: Some(RecordedFrontend {
                receipt,
                certificate_arns: vec![],
                load_balancer_deleted: false,
                target_group_deleted: false,
            }),
            ..Self::default()
        })
    }
}

impl RecordedFrontend {
    pub(super) async fn step_prepare(
        &mut self,
        elb: &dyn Elbv2Api,
        plan: AwsVmFrontendPlan,
    ) -> Result<Option<AwsPreparedVmFrontend>> {
        if self.load_balancer_deleted || self.target_group_deleted {
            return Err(invalid(
                "A retiring recorded frontend cannot be implicitly recreated",
            ));
        }
        for tags in [&plan.target_group.tags, &plan.load_balancer.tags] {
            if self.receipt.ownership_tags.iter().any(|(key, value)| {
                !tags.as_ref().is_some_and(|tags| {
                    tags.iter()
                        .any(|tag| &tag.key == key && &tag.value == value)
                })
            }) {
                return Err(invalid(
                    "Desired frontend changed its recorded ownership boundary",
                ));
            }
        }
        let response = elb
            .describe_target_groups(
                DescribeTargetGroupsRequest::builder()
                    .target_group_arns(vec![self.receipt.target_group_arn.clone()])
                    .build(),
            )
            .await
            .context(cloud_data("Read recorded VM target group"))?
            .describe_target_groups_result;
        let groups = response
            .target_groups
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if response.next_marker.is_some() || groups.len() != 1 {
            return Err(invalid("Recorded target group is missing or ambiguous"));
        }
        let group = &groups[0];
        self.verify_tags(elb, &self.receipt.target_group_arn)
            .await?;
        if group.target_group_arn.as_ref() != Some(&self.receipt.target_group_arn)
            || group.protocol != plan.target_group.protocol
            || group.port != plan.target_group.port
            || group.vpc_id != plan.target_group.vpc_id
            || group.target_type.as_deref() != Some("instance")
            || !crate::core::aws_vm_target_group::matches_requested_settings(
                group,
                &plan.target_group,
            )
        {
            return Err(invalid(
                "Recorded target group does not match its desired VM workload",
            ));
        }
        let response = elb
            .describe_load_balancers(
                DescribeLoadBalancersRequest::builder()
                    .load_balancer_arns(vec![self.receipt.load_balancer_arn.clone()])
                    .build(),
            )
            .await
            .context(cloud_data("Read recorded VM load balancer"))?
            .describe_load_balancers_result;
        let lbs = response
            .load_balancers
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if response.next_marker.is_some() || lbs.len() != 1 {
            return Err(invalid("Recorded load balancer is missing or ambiguous"));
        }
        let lb = &lbs[0];
        self.verify_tags(elb, &self.receipt.load_balancer_arn)
            .await?;
        let mut subnets: Vec<String> = lb
            .availability_zones
            .as_ref()
            .map(|zones| {
                zones
                    .members
                    .iter()
                    .filter_map(|zone| zone.subnet_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        subnets.sort();
        let mut desired_subnets = plan.load_balancer.subnets.clone();
        desired_subnets.sort();
        desired_subnets.dedup();
        let mut groups = lb
            .security_groups
            .as_ref()
            .map(|groups| groups.members.clone())
            .unwrap_or_default();
        groups.sort();
        let mut desired_groups = plan
            .load_balancer
            .security_groups
            .clone()
            .unwrap_or_default();
        desired_groups.sort();
        desired_groups.dedup();
        if lb.load_balancer_arn.as_ref() != Some(&self.receipt.load_balancer_arn)
            || lb.scheme != plan.load_balancer.scheme
            || lb.load_balancer_type != plan.load_balancer.load_balancer_type
            || lb.ip_address_type.as_deref()
                != Some(
                    plan.load_balancer
                        .ip_address_type
                        .as_deref()
                        .unwrap_or("ipv4"),
                )
            || subnets != desired_subnets
            || groups != desired_groups
        {
            return Err(invalid(
                "Recorded load balancer does not match its retained access/network plan",
            ));
        }
        match lb.state.as_ref().and_then(|state| state.code.as_deref()) {
            Some("active") => {}
            Some("provisioning" | "active_impaired") => return Ok(None),
            _ => {
                return Err(invalid(
                    "Recorded load balancer is not provisioning or active",
                ))
            }
        }
        let endpoint = LoadBalancerEndpoint {
            dns_name: lb
                .dns_name
                .clone()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| invalid("Recorded load balancer has no DNS name"))?,
            hosted_zone_id: Some(
                lb.canonical_hosted_zone_id
                    .clone()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| invalid("Recorded load balancer has no hosted zone"))?,
            ),
        };
        let response = elb
            .describe_listeners(
                DescribeListenersRequest::builder()
                    .listener_arns(vec![self.receipt.listener_arn.clone()])
                    .build(),
            )
            .await
            .context(cloud_data("Read recorded VM listener"))?
            .describe_listeners_result;
        let listeners = response
            .listeners
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if response.next_marker.is_some() || listeners.len() != 1 {
            return Err(invalid("Recorded listener is missing or ambiguous"));
        }
        let listener = &listeners[0];
        self.verify_tags(elb, &self.receipt.listener_arn).await?;
        let actions = listener
            .default_actions
            .as_ref()
            .map(|wrapper| wrapper.members.as_slice())
            .unwrap_or_default();
        if listener.listener_arn.as_ref() != Some(&self.receipt.listener_arn)
            || listener.load_balancer_arn.as_ref() != Some(&self.receipt.load_balancer_arn)
            || listener.port != Some(plan.listener_port)
            || listener.protocol.as_deref()
                != Some(if plan.certificate_arn.is_some() {
                    "HTTPS"
                } else {
                    "TCP"
                })
            || plan
                .ssl_policy
                .as_ref()
                .is_some_and(|policy| listener.ssl_policy.as_ref() != Some(policy))
            || actions.len() != 1
            || actions[0].action_type.as_deref() != Some("forward")
            || actions[0].target_group_arn.as_ref() != Some(&self.receipt.target_group_arn)
            || actions[0].redirect_config.is_some()
            || actions[0].fixed_response_config.is_some()
            || actions[0].forward_config.as_ref().is_some_and(|forward| {
                forward.target_groups.as_ref().is_none_or(|groups| {
                    groups.members.len() != 1
                        || groups.members[0].target_group_arn.as_ref()
                            != Some(&self.receipt.target_group_arn)
                })
            })
        {
            return Err(invalid("Recorded listener's forwarding plan was changed"));
        }
        let certificates: Vec<String> = listener
            .certificates
            .as_ref()
            .map(|wrapper| {
                wrapper
                    .members
                    .iter()
                    .filter_map(|certificate| certificate.certificate_arn.clone())
                    .collect()
            })
            .unwrap_or_default();
        if self.certificate_arns != certificates {
            self.certificate_arns = certificates;
            return Ok(None); // Checkpoint observed serving references before mutation.
        }
        let desired: Vec<String> = plan.certificate_arn.iter().cloned().collect();
        if self.certificate_arns != desired {
            let certificate_arn = plan
                .certificate_arn
                .ok_or_else(|| invalid("Recorded TCP listener unexpectedly has a certificate"))?;
            elb.modify_listener(
                ModifyListenerRequest::builder()
                    .listener_arn(self.receipt.listener_arn.clone())
                    .certificates(vec![Certificate {
                        certificate_arn,
                        is_default: None,
                    }])
                    .build(),
            )
            .await
            .context(cloud_data("Rotate recorded VM listener certificate"))?;
            return Ok(None);
        }
        Ok(Some(AwsPreparedVmFrontend {
            target_group_arn: self.receipt.target_group_arn.clone(),
            load_balancer_arn: self.receipt.load_balancer_arn.clone(),
            listener_arn: self.receipt.listener_arn.clone(),
            endpoint,
        }))
    }

    pub(super) async fn step_delete(&mut self, elb: &dyn Elbv2Api) -> Result<bool> {
        if !self.load_balancer_deleted {
            let response = match elb
                .describe_load_balancers(
                    DescribeLoadBalancersRequest::builder()
                        .load_balancer_arns(vec![self.receipt.load_balancer_arn.clone()])
                        .build(),
                )
                .await
            {
                Ok(response) => Some(response.describe_load_balancers_result),
                Err(error) if is_remote_not_found(&error) => None,
                Err(error) => {
                    return Err(cleanup_failure(
                        error.context(cloud_data("Read recorded load balancer during cleanup")),
                    ))
                }
            };
            let lbs = response
                .as_ref()
                .and_then(|response| response.load_balancers.as_ref())
                .map(|wrapper| wrapper.members.as_slice())
                .unwrap_or_default();
            if response
                .as_ref()
                .is_some_and(|response| response.next_marker.is_some())
                || lbs.len() > 1
                || lbs.first().is_some_and(|lb| {
                    lb.load_balancer_arn.as_ref() != Some(&self.receipt.load_balancer_arn)
                })
            {
                return Err(invalid(
                    "Recorded load balancer cleanup discovery is ambiguous",
                ));
            }
            if lbs.is_empty() {
                self.load_balancer_deleted = true;
                self.certificate_arns.clear();
                return Ok(false);
            }
            self.verify_tags(elb, &self.receipt.load_balancer_arn)
                .await
                .map_err(cleanup_failure)?;
            return delete_result(
                elb.delete_load_balancer(&self.receipt.load_balancer_arn)
                    .await,
            );
        }
        if !self.target_group_deleted {
            let response = match elb
                .describe_target_groups(
                    DescribeTargetGroupsRequest::builder()
                        .target_group_arns(vec![self.receipt.target_group_arn.clone()])
                        .build(),
                )
                .await
            {
                Ok(response) => Some(response.describe_target_groups_result),
                Err(error) if is_remote_not_found(&error) => None,
                Err(error) => {
                    return Err(cleanup_failure(
                        error.context(cloud_data("Read recorded target group during cleanup")),
                    ))
                }
            };
            let groups = response
                .as_ref()
                .and_then(|response| response.target_groups.as_ref())
                .map(|wrapper| wrapper.members.as_slice())
                .unwrap_or_default();
            if response
                .as_ref()
                .is_some_and(|response| response.next_marker.is_some())
                || groups.len() > 1
                || groups.first().is_some_and(|group| {
                    group.target_group_arn.as_ref() != Some(&self.receipt.target_group_arn)
                })
            {
                return Err(invalid(
                    "Recorded target group cleanup discovery is ambiguous",
                ));
            }
            if groups.is_empty() {
                self.target_group_deleted = true;
                return Ok(true);
            }
            self.verify_tags(elb, &self.receipt.target_group_arn)
                .await
                .map_err(cleanup_failure)?;
            return delete_result(
                elb.delete_target_group(&self.receipt.target_group_arn)
                    .await,
            );
        }
        Ok(true)
    }

    async fn verify_tags(&self, elb: &dyn Elbv2Api, arn: &str) -> Result<()> {
        if self.receipt.ownership_tags.is_empty() {
            return Err(invalid("Recorded frontend ownership boundary is missing"));
        }
        let response = elb
            .describe_tags(vec![arn.to_string()])
            .await
            .context(cloud_data("Verify recorded VM frontend ownership"))?;
        let tags = response
            .describe_tags_result
            .tag_descriptions
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if tags.len() != 1
            || tags[0].resource_arn.as_deref() != Some(arn)
            || self
                .receipt
                .ownership_tags
                .iter()
                .any(|(key, value)| tags[0].tag_value(key) != Some(value.as_str()))
        {
            return Err(invalid(
                "Refusing a recorded frontend with missing or foreign ownership metadata",
            ));
        }
        Ok(())
    }
}

fn delete_result(result: alien_client_core::Result<()>) -> Result<bool> {
    match result {
        Ok(()) => Ok(false),
        Err(error) if is_remote_not_found(&error) => Ok(false),
        Err(error) if matches!(&error.error, Some(ClientErrorData::RemoteResourceConflict { message, .. }) if message.contains("ResourceInUse") || message.contains("in use")) => {
            Ok(false)
        }
        Err(error) => Err(AlienError::new(cloud_data(&format!(
            "Delete recorded VM frontend: {}",
            error.message
        )))),
    }
}
fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.to_string(),
        resource_id: None,
    })
}
fn cloud_data(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: None,
    }
}
fn cleanup_failure(error: AlienError<ErrorData>) -> AlienError<ErrorData> {
    AlienError::new(cloud_data(&error.message))
}
