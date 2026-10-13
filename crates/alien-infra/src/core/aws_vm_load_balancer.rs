//! Independently owned public or private VM load balancers.

use std::collections::BTreeMap;

use alien_aws_clients::elbv2::{
    CreateLoadBalancerRequest, DescribeLoadBalancersRequest, ElbTag, Elbv2Api, LoadBalancer,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::aws_tag_scoped::{is_remote_not_found, CREATE_ATTEMPT_TAG};
use crate::{ErrorData, Result};

#[path = "aws_vm_load_balancer_listener.rs"]
mod listener;

/// An additional frontend never replaces a retained frontend's load balancer.
/// The caller owns its security group, target groups, listeners and DNS records.
/// Checkpoint every pending result; retire DNS before calling `step_delete` and
/// retain target groups and certificates until deletion is confirmed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsVmLoadBalancer {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    ownership_tags: BTreeMap<String, String>,
    #[serde(default)]
    arn: Option<String>,
    #[serde(default)]
    listener: Option<listener::ListenerReceipt>,
}

impl AwsVmLoadBalancer {
    /// Records intent, creates once by attempt-derived name, then verifies
    /// ownership and provider configuration. Returns an active frontend only
    /// after a separate read; this does not prove listener or target health.
    pub async fn step_create(
        &mut self,
        elb: &dyn Elbv2Api,
        request: CreateLoadBalancerRequest,
    ) -> Result<Option<LoadBalancer>> {
        let (mut request, tags, fingerprint) = prepare_request(request)?;
        if self.token.is_none() {
            if self.arn.is_some()
                || self.name.is_some()
                || self.fingerprint.is_some()
                || !self.ownership_tags.is_empty()
                || self.listener.is_some()
            {
                return Err(invalid("Load balancer ownership has no attempt token"));
            }
            let token = Uuid::new_v4().to_string();
            let stem: String = request
                .name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .take(12)
                .collect();
            self.name = Some(format!(
                "lb-{stem}-{}",
                &format!("{:x}", Sha256::digest(token.as_bytes()))[..16]
            ));
            self.token = Some(token);
            self.fingerprint = Some(fingerprint);
            self.ownership_tags = tags;
            return Ok(None);
        }
        if self.fingerprint.as_ref() != Some(&fingerprint) || self.ownership_tags != tags {
            return Err(invalid(
                "Load balancer configuration changed during its lifetime",
            ));
        }
        if let Some(lb) = self.find(elb).await? {
            let arn = required_arn(&lb)?;
            self.verify_owned(elb, &arn).await?;
            let mut subnets = lb
                .availability_zones
                .as_ref()
                .map(|zones| {
                    zones
                        .members
                        .iter()
                        .filter_map(|zone| zone.subnet_id.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            subnets.sort();
            let mut security_groups = lb
                .security_groups
                .as_ref()
                .map(|groups| groups.members.clone())
                .unwrap_or_default();
            security_groups.sort();
            if lb.load_balancer_name != self.name
                || lb.scheme != request.scheme
                || lb.load_balancer_type != request.load_balancer_type
                || lb.ip_address_type != request.ip_address_type
                || subnets != request.subnets
                || security_groups != request.security_groups.unwrap_or_default()
            {
                return Err(invalid(
                    "Discovered load balancer does not match its recorded plan",
                ));
            }
            if let Some(recorded) = &self.arn {
                if recorded != &arn {
                    return Err(invalid("Recorded load balancer was replaced"));
                }
            } else {
                self.arn = Some(arn);
                return Ok(None);
            }
            match lb.state.as_ref().and_then(|state| state.code.as_deref()) {
                Some("active") => {
                    if lb.dns_name.as_deref().is_none_or(str::is_empty)
                        || lb
                            .canonical_hosted_zone_id
                            .as_deref()
                            .is_none_or(str::is_empty)
                    {
                        return Err(invalid("Active load balancer has no DNS binding"));
                    }
                    return Ok(Some(lb));
                }
                Some("provisioning" | "active_impaired") => return Ok(None),
                _ => return Err(invalid("Load balancer is not provisioning or active")),
            }
        }
        if self.arn.is_some() {
            return Err(invalid(
                "Recorded load balancer disappeared; refusing implicit replacement",
            ));
        }
        request.name = self.name.clone().expect("intent was recorded");
        request.tags = Some(
            tags.into_iter()
                .map(|(key, value)| ElbTag { key, value })
                .chain(std::iter::once(ElbTag {
                    key: CREATE_ATTEMPT_TAG.to_string(),
                    value: self.token.clone().expect("intent was recorded"),
                }))
                .collect(),
        );
        let response = elb
            .create_load_balancer(request)
            .await
            .context(cloud_data("Create VM frontend load balancer"))?;
        let lbs = response
            .create_load_balancer_result
            .load_balancers
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if lbs.len() != 1 {
            return Err(invalid(
                "CreateLoadBalancer did not return exactly one load balancer",
            ));
        }
        self.arn = Some(required_arn(&lbs[0])?);
        Ok(None)
    }

    /// Deletes only the recorded lifetime. Retains ownership until a subsequent
    /// read confirms absence, including when the delete response is lost.
    pub async fn step_delete(&mut self, elb: &dyn Elbv2Api) -> Result<bool> {
        if self.token.is_none() {
            if self.name.is_some() || self.arn.is_some() {
                return Err(invalid(
                    "Cannot delete a load balancer without attempt ownership",
                ));
            }
            return Ok(true);
        }
        let Some(lb) = self.find(elb).await.map_err(cleanup_failure)? else {
            *self = Self::default();
            return Ok(true);
        };
        let arn = required_arn(&lb)?;
        self.verify_owned(elb, &arn)
            .await
            .map_err(cleanup_failure)?;
        if self.arn.as_ref().is_some_and(|recorded| recorded != &arn) {
            return Err(invalid("Refusing cleanup of a replacement load balancer"));
        }
        if self.arn.is_none() {
            self.arn = Some(arn);
            return Ok(false);
        }
        match elb.delete_load_balancer(&arn).await {
            Ok(_) => Ok(false),
            Err(error) if is_remote_not_found(&error) => Ok(false),
            Err(error) if matches!(&error.error, Some(ClientErrorData::RemoteResourceConflict { message, .. }) if message.contains("ResourceInUse") || message.contains("in use")) => {
                Ok(false)
            }
            Err(error) => Err(cleanup_failure(
                error.context(cloud_data("Delete VM frontend load balancer")),
            )),
        }
    }

    async fn find(&self, elb: &dyn Elbv2Api) -> Result<Option<LoadBalancer>> {
        let name = self
            .name
            .as_ref()
            .ok_or_else(|| invalid("Load balancer attempt has no recorded name"))?;
        let response = match elb
            .describe_load_balancers(
                DescribeLoadBalancersRequest::builder()
                    .names(vec![name.clone()])
                    .build(),
            )
            .await
        {
            Ok(response) => response.describe_load_balancers_result,
            Err(error) if is_remote_not_found(&error) => return Ok(None),
            Err(error) => {
                return Err(error.context(cloud_data("Discover VM frontend load balancer")))
            }
        };
        let lbs = response
            .load_balancers
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if response.next_marker.is_some() || lbs.len() > 1 {
            return Err(invalid(
                "Load balancer name discovery returned an ambiguous collection",
            ));
        }
        Ok(lbs.into_iter().next())
    }

    async fn verify_owned(&self, elb: &dyn Elbv2Api, arn: &str) -> Result<()> {
        if self.ownership_tags.is_empty() {
            return Err(invalid("Load balancer ownership boundary is missing"));
        }
        let response = elb
            .describe_tags(vec![arn.to_string()])
            .await
            .context(cloud_data("Verify VM frontend load balancer ownership"))?;
        let tags = response
            .describe_tags_result
            .tag_descriptions
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if tags.len() != 1 || tags[0].resource_arn.as_deref() != Some(arn) {
            return Err(invalid(
                "Load balancer ownership metadata is missing or ambiguous",
            ));
        }
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| invalid("Load balancer attempt token is missing"))?;
        if tags[0].tag_value(CREATE_ATTEMPT_TAG) != Some(token)
            || self
                .ownership_tags
                .iter()
                .any(|(key, value)| tags[0].tag_value(key) != Some(value.as_str()))
        {
            return Err(invalid(
                "Refusing a load balancer with foreign ownership tags",
            ));
        }
        Ok(())
    }
}

pub(super) fn prepare_request(
    mut request: CreateLoadBalancerRequest,
) -> Result<(CreateLoadBalancerRequest, BTreeMap<String, String>, String)> {
    if request.name.is_empty()
        || request.subnets.is_empty()
        || request.subnets.iter().any(String::is_empty)
        || request.subnet_mappings.is_some()
        || !matches!(
            request.scheme.as_deref(),
            Some("internet-facing" | "internal")
        )
        || !matches!(
            request.load_balancer_type.as_deref(),
            Some("application" | "network")
        )
    {
        return Err(invalid("VM load balancer requires a name, subnets, explicit public/private scheme and application/network type"));
    }
    request.subnets.sort();
    request.subnets.dedup();
    if let Some(groups) = request.security_groups.as_mut() {
        groups.sort();
        groups.dedup();
    }
    request
        .ip_address_type
        .get_or_insert_with(|| "ipv4".to_string());
    let mut tags = BTreeMap::new();
    for tag in request.tags.take().unwrap_or_default() {
        if tag.key.is_empty()
            || tag.key == CREATE_ATTEMPT_TAG
            || tags.insert(tag.key, tag.value).is_some()
        {
            return Err(invalid(
                "Load balancer ownership tags are empty, reserved or duplicated",
            ));
        }
    }
    if tags.is_empty() {
        return Err(invalid("Load balancer requires ownership tags"));
    }
    let encoded = serde_json::to_vec(&request)
        .into_alien_error()
        .context(config_data("Encode VM load balancer configuration"))?;
    let fingerprint = format!("{:x}", Sha256::digest(encoded));
    Ok((request, tags, fingerprint))
}

fn required_arn(lb: &LoadBalancer) -> Result<String> {
    lb.load_balancer_arn
        .clone()
        .filter(|arn| !arn.is_empty())
        .ok_or_else(|| invalid("Load balancer has no provider ARN"))
}
fn config_data(message: &str) -> ErrorData {
    ErrorData::ResourceConfigInvalid {
        message: message.to_string(),
        resource_id: None,
    }
}
fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(config_data(message))
}
fn cloud_data(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: None,
    }
}
// The generic executor must not interpret a denied discovery/metadata read as
// best-effort deletion of the entire parent and skip later child cleanup.
fn cleanup_failure(error: AlienError<ErrorData>) -> AlienError<ErrorData> {
    crate::core::aws_tag_scoped::preserve_composite_cleanup_error(error)
}

#[cfg(test)]
#[path = "aws_vm_load_balancer_tests.rs"]
mod tests;
