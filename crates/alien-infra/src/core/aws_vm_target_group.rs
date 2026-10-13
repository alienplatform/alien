//! Attempt-owned target groups for independent VM ingress frontends.

use std::collections::BTreeMap;

use alien_aws_clients::elbv2::{
    CreateTargetGroupRequest, DescribeTargetGroupsRequest, ElbTag, Elbv2Api, TargetGroup,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::aws_tag_scoped::{is_remote_not_found, CREATE_ATTEMPT_TAG};
use crate::{ErrorData, Result};

/// Each frontend needs its own target group: AWS cannot attach one target group
/// to two load balancers. The caller owns workload registration and must register
/// every prepared group before waiting for target health.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsVmTargetGroup {
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
}

impl AwsVmTargetGroup {
    /// Performs at most one mutation. Checkpoint every pending result. Names
    /// include the recorded attempt, so a retry never adopts an older lifetime.
    /// A new plan requires a new child; preserve this one until its frontend and
    /// workload registrations have been retired.
    pub async fn step_create(
        &mut self,
        elb: &dyn Elbv2Api,
        mut request: CreateTargetGroupRequest,
    ) -> Result<Option<String>> {
        if request.name.is_empty()
            || !matches!(request.protocol.as_deref(), Some("HTTP" | "TCP"))
            || !request.port.is_some_and(|port| (1..=65535).contains(&port))
            || request.vpc_id.as_deref().is_none_or(str::is_empty)
            || request
                .target_type
                .as_deref()
                .is_some_and(|kind| kind != "instance")
        {
            return Err(invalid("VM target group requires a name, HTTP/TCP protocol, valid port, VPC and instance targets"));
        }
        request
            .target_type
            .get_or_insert_with(|| "instance".to_string());
        let tags = normalized_tags(request.tags.take().unwrap_or_default())?;
        let encoded = serde_json::to_vec(&request)
            .into_alien_error()
            .context(invalid_data("Encode VM target group configuration"))?;
        let fingerprint = format!("{:x}", Sha256::digest(encoded));
        if self.token.is_none() {
            if self.arn.is_some()
                || self.name.is_some()
                || self.fingerprint.is_some()
                || !self.ownership_tags.is_empty()
            {
                return Err(invalid("Target group ownership has no attempt token"));
            }
            let token = Uuid::new_v4().to_string();
            self.name = Some(attempt_name(&request.name, &token));
            self.token = Some(token);
            self.fingerprint = Some(fingerprint);
            self.ownership_tags = tags;
            return Ok(None);
        }
        if self.fingerprint.as_ref() != Some(&fingerprint) || self.ownership_tags != tags {
            return Err(invalid(
                "Target group configuration changed during its lifetime",
            ));
        }
        if let Some(group) = self.find(elb).await? {
            let arn = required_arn(&group)?;
            self.verify_owned(elb, &arn).await?;
            if group.target_group_name != self.name
                || group.protocol != request.protocol
                || group.port != request.port
                || group.vpc_id != request.vpc_id
                || group.target_type != request.target_type
                || !matches_requested_settings(&group, &request)
            {
                return Err(invalid(
                    "Discovered target group does not match its recorded plan",
                ));
            }
            if let Some(recorded) = &self.arn {
                if recorded != &arn {
                    return Err(invalid("Recorded target group was replaced"));
                }
                return Ok(Some(arn));
            }
            self.arn = Some(arn);
            return Ok(None);
        }
        if self.arn.is_some() {
            return Err(invalid(
                "Recorded target group disappeared; refusing implicit replacement",
            ));
        }
        request.name = self.name.clone().expect("intent was recorded");
        let mut tags: Vec<ElbTag> = tags
            .into_iter()
            .map(|(key, value)| ElbTag { key, value })
            .collect();
        tags.push(ElbTag {
            key: CREATE_ATTEMPT_TAG.to_string(),
            value: self.token.clone().expect("intent was recorded"),
        });
        request.tags = Some(tags);
        let response = elb
            .create_target_group(request)
            .await
            .context(cloud_data("Create VM frontend target group"))?;
        let groups = response
            .create_target_group_result
            .target_groups
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if groups.len() != 1 {
            return Err(invalid(
                "CreateTargetGroup did not return exactly one target group",
            ));
        }
        self.arn = Some(required_arn(&groups[0])?);
        Ok(None)
    }

    /// The load balancer and workload registration must already be retired.
    /// Retain ownership through delete responses until a later read proves
    /// absence. Denied cleanup never escapes as best-effort resource deletion.
    pub async fn step_delete(&mut self, elb: &dyn Elbv2Api) -> Result<bool> {
        if self.token.is_none() {
            if self.name.is_some() || self.arn.is_some() {
                return Err(invalid(
                    "Cannot delete target group without attempt ownership",
                ));
            }
            return Ok(true);
        }
        let group = self.find(elb).await.map_err(keep_cleanup_failure)?;
        let Some(group) = group else {
            *self = Self::default();
            return Ok(true);
        };
        let arn = required_arn(&group)?;
        self.verify_owned(elb, &arn)
            .await
            .map_err(keep_cleanup_failure)?;
        if self.arn.as_ref().is_some_and(|recorded| recorded != &arn) {
            return Err(invalid("Refusing cleanup of a replacement target group"));
        }
        if self.arn.is_none() {
            self.arn = Some(arn);
            return Ok(false);
        }
        match elb.delete_target_group(&arn).await {
            Ok(_) => Ok(false),
            Err(error) if is_remote_not_found(&error) => Ok(false),
            Err(error) if matches!(&error.error, Some(ClientErrorData::RemoteResourceConflict {message,..}) if message.contains("ResourceInUse") || message.contains("in use")) => {
                Ok(false)
            }
            Err(error) => Err(AlienError::new(cloud_data(&format!(
                "Delete VM frontend target group: {}",
                error.message
            )))),
        }
    }

    async fn find(&self, elb: &dyn Elbv2Api) -> Result<Option<TargetGroup>> {
        let name = self
            .name
            .as_ref()
            .ok_or_else(|| invalid("Target group attempt has no recorded name"))?;
        let response = match elb
            .describe_target_groups(
                DescribeTargetGroupsRequest::builder()
                    .names(vec![name.clone()])
                    .build(),
            )
            .await
        {
            Ok(response) => response.describe_target_groups_result,
            Err(error) if is_remote_not_found(&error) => return Ok(None),
            Err(error) => {
                return Err(error.context(cloud_data("Discover VM frontend target group")))
            }
        };
        let groups = response
            .target_groups
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if response.next_marker.is_some() || groups.len() > 1 {
            return Err(invalid(
                "Target group name discovery returned an ambiguous collection",
            ));
        }
        Ok(groups.into_iter().next())
    }

    async fn verify_owned(&self, elb: &dyn Elbv2Api, arn: &str) -> Result<()> {
        if self.ownership_tags.is_empty() {
            return Err(invalid("Target group ownership boundary is missing"));
        }
        let response = elb
            .describe_tags(vec![arn.to_string()])
            .await
            .context(cloud_data("Verify VM frontend target group ownership"))?;
        let tags = response
            .describe_tags_result
            .tag_descriptions
            .map(|wrapper| wrapper.members)
            .unwrap_or_default();
        if tags.len() != 1 || tags[0].resource_arn.as_deref() != Some(arn) {
            return Err(invalid(
                "Target group ownership metadata is missing or ambiguous",
            ));
        }
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| invalid("Target group attempt token is missing"))?;
        if tags[0].tag_value(CREATE_ATTEMPT_TAG) != Some(token)
            || self
                .ownership_tags
                .iter()
                .any(|(key, value)| tags[0].tag_value(key) != Some(value.as_str()))
        {
            return Err(invalid(
                "Refusing a target group with foreign ownership tags",
            ));
        }
        Ok(())
    }
}

// AWS fills provider defaults for omitted options; only explicit settings
// belong to the recorded plan and must match during recovery.
fn matches_requested_settings(group: &TargetGroup, request: &CreateTargetGroupRequest) -> bool {
    request
        .protocol_version
        .as_ref()
        .is_none_or(|value| group.protocol_version.as_ref() == Some(value))
        && request
            .health_check_protocol
            .as_ref()
            .is_none_or(|value| group.health_check_protocol.as_ref() == Some(value))
        && request
            .health_check_port
            .as_ref()
            .is_none_or(|value| group.health_check_port.as_ref() == Some(value))
        && request
            .health_check_enabled
            .as_ref()
            .is_none_or(|value| group.health_check_enabled.as_ref() == Some(value))
        && request
            .health_check_path
            .as_ref()
            .is_none_or(|value| group.health_check_path.as_ref() == Some(value))
        && request
            .health_check_interval_seconds
            .as_ref()
            .is_none_or(|value| group.health_check_interval_seconds.as_ref() == Some(value))
        && request
            .health_check_timeout_seconds
            .as_ref()
            .is_none_or(|value| group.health_check_timeout_seconds.as_ref() == Some(value))
        && request
            .healthy_threshold_count
            .as_ref()
            .is_none_or(|value| group.healthy_threshold_count.as_ref() == Some(value))
        && request
            .unhealthy_threshold_count
            .as_ref()
            .is_none_or(|value| group.unhealthy_threshold_count.as_ref() == Some(value))
        && request
            .ip_address_type
            .as_ref()
            .is_none_or(|value| group.ip_address_type.as_ref() == Some(value))
        && request.matcher.as_ref().is_none_or(|matcher| {
            group.matcher.as_ref().is_some_and(|actual| {
                matcher
                    .http_code
                    .as_ref()
                    .is_none_or(|value| actual.http_code.as_ref() == Some(value))
                    && matcher
                        .grpc_code
                        .as_ref()
                        .is_none_or(|value| actual.grpc_code.as_ref() == Some(value))
            })
        })
}

fn required_arn(group: &TargetGroup) -> Result<String> {
    group
        .target_group_arn
        .clone()
        .filter(|arn| !arn.is_empty())
        .ok_or_else(|| invalid("Target group has no provider ARN"))
}

fn normalized_tags(tags: Vec<ElbTag>) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for tag in tags {
        if tag.key == CREATE_ATTEMPT_TAG
            || tag.key.is_empty()
            || result.insert(tag.key, tag.value).is_some()
        {
            return Err(invalid(
                "Target group ownership tags are empty, reserved, or duplicated",
            ));
        }
    }
    if result.is_empty() {
        return Err(invalid("Target group requires ownership tags"));
    }
    Ok(result)
}

fn attempt_name(base: &str, token: &str) -> String {
    let stem: String = base
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .take(12)
        .collect();
    let suffix = format!("{:x}", Sha256::digest(token.as_bytes()));
    format!("tg-{stem}-{}", &suffix[..16])
}

fn invalid_data(message: &str) -> ErrorData {
    ErrorData::ResourceConfigInvalid {
        message: message.to_string(),
        resource_id: None,
    }
}
fn invalid(message: &str) -> AlienError<ErrorData> {
    AlienError::new(invalid_data(message))
}
fn cloud_data(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: None,
    }
}
fn keep_cleanup_failure(error: AlienError<ErrorData>) -> AlienError<ErrorData> {
    AlienError::new(cloud_data(&error.message))
}

#[cfg(test)]
#[path = "aws_vm_target_group_tests.rs"]
mod tests;
