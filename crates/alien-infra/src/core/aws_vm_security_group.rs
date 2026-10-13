//! Generation-specific security-group receipt shared by VM workload parents.

use crate::{ErrorData, Result};
use alien_aws_clients::ec2::{
    CreateSecurityGroupRequest, DescribeSecurityGroupsRequest, Ec2Api, Filter, IpPermission,
    RevokeSecurityGroupIngressRequest, Tag, TagSpecification, UserIdGroupPair,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_core::{
    ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE, ALIEN_RESOURCE_TAG_KEY,
    ALIEN_STACK_TAG_KEY,
};
use alien_error::{AlienError, Context};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsVmSecurityGroup {
    name: String,
    vpc_id: String,
    stack: String,
    resource: String,
    #[serde(default)]
    id: Option<String>,
    // Older receipts may already have issued a create whose response was lost.
    #[serde(default = "create_may_have_started")]
    create_attempted: bool,
    #[serde(default)]
    delete_attempted: bool,
    /// Exact compute groups whose rule authorization may have committed.
    /// None identifies older receipts without destination tracking.
    #[serde(default)]
    compute_groups: Option<Vec<String>>,
}

fn create_may_have_started() -> bool {
    true
}

impl AwsVmSecurityGroup {
    /// The immutable EC2 group name contains the attempt token. Its uniqueness
    /// within the VPC makes repeating CreateSecurityGroup safe after a lost reply.
    pub fn new(vpc_id: String, stack: &str, resource: &str) -> Self {
        Self {
            name: format!("alien-ingress-{}", Uuid::new_v4()),
            vpc_id,
            stack: stack.to_string(),
            resource: resource.to_string(),
            id: None,
            create_attempted: false,
            delete_attempted: false,
            compute_groups: Some(Vec::new()),
        }
    }

    pub fn validate_owner(&self, vpc_id: &str, stack: &str, resource: &str) -> Result<()> {
        if self.vpc_id != vpc_id || self.stack != stack || self.resource != resource {
            return Err(self.invalid("saved generation belongs to a different network or resource"));
        }
        Ok(())
    }

    fn tags(&self) -> Vec<Tag> {
        [
            ("Name", self.name.as_str()),
            (ALIEN_STACK_TAG_KEY, self.stack.as_str()),
            (ALIEN_RESOURCE_TAG_KEY, self.resource.as_str()),
            (ALIEN_MANAGED_BY_TAG_KEY, ALIEN_MANAGED_BY_TAG_VALUE),
        ]
        .into_iter()
        .map(|(key, value)| Tag {
            key: key.to_string(),
            value: value.to_string(),
        })
        .collect()
    }

    fn invalid(&self, message: &str) -> AlienError<ErrorData> {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message: format!("Security group '{}': {message}", self.name),
            resource_id: Some(self.resource.clone()),
        })
    }

    async fn discover(&self, ec2: &dyn Ec2Api) -> Result<Option<String>> {
        let response = ec2
            .describe_security_groups(
                DescribeSecurityGroupsRequest::builder()
                    .filters(vec![
                        Filter {
                            name: "group-name".to_string(),
                            values: vec![self.name.clone()],
                        },
                        Filter {
                            name: "vpc-id".to_string(),
                            values: vec![self.vpc_id.clone()],
                        },
                    ])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Read security-group generation '{}'", self.name),
                resource_id: Some(self.resource.clone()),
            })?;
        let groups = response
            .security_group_info
            .map(|set| set.items)
            .unwrap_or_default();
        if response.next_token.is_some() || groups.len() > 1 {
            return Err(self.invalid("discovery is incomplete or ambiguous"));
        }
        let Some(group) = groups.first() else {
            return Ok(None);
        };
        let tags = group
            .tag_set
            .as_ref()
            .map(|set| set.items.as_slice())
            .unwrap_or_default();
        if group.group_name.as_ref() != Some(&self.name)
            || group.vpc_id.as_ref() != Some(&self.vpc_id)
            || self.tags().iter().any(|expected| {
                tags.iter().filter(|tag| tag.key == expected.key).count() != 1
                    || !tags
                        .iter()
                        .any(|tag| tag.key == expected.key && tag.value == expected.value)
            })
        {
            return Err(self.invalid("provider object does not match its recorded ownership"));
        }
        let id = group
            .group_id
            .as_ref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| self.invalid("provider object has no ID"))?;
        if self.id.as_ref().is_some_and(|recorded| recorded != id) {
            return Err(self.invalid("provider ID differs from its recorded receipt"));
        }
        Ok(Some(id.clone()))
    }

    /// A successful create only records the receipt. A later read must confirm
    /// it before the parent authorizes traffic or publishes a frontend.
    pub async fn step_prepare(&mut self, ec2: &dyn Ec2Api) -> Result<Option<String>> {
        if let Some(id) = self.discover(ec2).await? {
            self.id = Some(id.clone());
            return Ok(Some(id));
        }
        if self.id.is_some() {
            // Eventual visibility cannot justify replacing a known ID.
            return Ok(None);
        }
        self.create_attempted = true;
        let response = ec2
            .create_security_group(
                CreateSecurityGroupRequest::builder()
                    .group_name(self.name.clone())
                    .description("Alien managed ingress security group".to_string())
                    .vpc_id(self.vpc_id.clone())
                    .tag_specifications(vec![TagSpecification {
                        resource_type: "security-group".to_string(),
                        tags: self.tags(),
                    }])
                    .build(),
            )
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) if matches!(&error.error, Some(ClientErrorData::RemoteResourceConflict { message, .. }) if message.contains("InvalidGroup.Duplicate")) =>
            {
                // EC2 can hide the committed create from Describe briefly. Keep
                // the same immutable attempt; the next bounded step rediscovers it.
                return Ok(None);
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudPlatformError {
                    message: format!("Create security-group generation '{}'", self.name),
                    resource_id: Some(self.resource.clone()),
                })
            }
        };
        self.id = Some(
            response
                .group_id
                .filter(|id| !id.is_empty())
                .ok_or_else(|| self.invalid("create returned no ID"))?,
        );
        Ok(None)
    }

    /// Resolve a possibly lost create response before the parent's existing
    /// compute-rule revocation and dependency-ordered deletion.
    pub async fn recover(&mut self, ec2: &dyn Ec2Api) -> Result<Option<String>> {
        let discovered = self.discover(ec2).await?;
        if let Some(id) = discovered.as_ref() {
            self.id = Some(id.clone());
            return Ok(discovered);
        }
        if self.delete_attempted || !self.create_attempted {
            return Ok(None);
        }
        if self.id.is_some() {
            // A known ID can be deleted even while name discovery lags. Keep it
            // until a read after the delete attempt confirms absence.
            return Ok(self.id.clone());
        }
        // An empty eventually consistent read cannot settle a lost create.
        // Let the executor bound retries and retain this receipt on failure.
        Err(AlienError::new(ErrorData::ExecutionStepFailed {
            message: format!(
                "Security-group creation '{}' is not yet resolved",
                self.name
            ),
            resource_id: Some(self.resource.clone()),
        }))
    }

    /// Record the delete boundary before sending the exact-ID request.
    pub fn record_delete_attempt(&mut self) {
        self.delete_attempted = true;
    }

    pub fn record_compute_group(&mut self, id: &str) {
        let groups = self.compute_groups.get_or_insert_with(Vec::new);
        if !groups.iter().any(|group| group == id) {
            groups.push(id.to_string());
        }
    }

    /// Retire one recorded destination per step. None requires the legacy
    /// parent's dependency-based cleanup because the old state has no receipt.
    pub async fn retire_compute_groups(
        &mut self,
        ec2: &dyn Ec2Api,
        keep: Option<&str>,
    ) -> Result<Option<bool>> {
        let Some(groups) = &self.compute_groups else {
            return Ok(None);
        };
        let Some(destination) = groups
            .iter()
            .find(|group| Some(group.as_str()) != keep)
            .cloned()
        else {
            return Ok(Some(true));
        };
        let source = self
            .id
            .clone()
            .ok_or_else(|| self.invalid("compute rule has no source group receipt"))?;
        let result = ec2
            .revoke_security_group_ingress(
                RevokeSecurityGroupIngressRequest::builder()
                    .group_id(destination.clone())
                    .ip_permissions(vec![IpPermission {
                        ip_protocol: "tcp".to_string(),
                        from_port: Some(8001),
                        to_port: Some(8999),
                        ip_ranges: None,
                        ipv6_ranges: None,
                        user_id_group_pairs: Some(vec![UserIdGroupPair {
                            group_id: Some(source),
                            user_id: None,
                            description: None,
                        }]),
                    }])
                    .build(),
            )
            .await;
        match result {
            Ok(()) => {}
            Err(error)
                if matches!(
                    &error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) => {}
            Err(error) if matches!(&error.error, Some(ClientErrorData::InvalidInput { message, .. }) if message.contains("InvalidPermission.NotFound")) =>
                {}
            Err(error) => {
                return Err(error)
                    .context(ErrorData::CloudPlatformError {
                        message: "Revoke recorded compute ingress".to_string(),
                        resource_id: Some(self.resource.clone()),
                    })
                    .map_err(crate::preserve_composite_cleanup_error)
            }
        }
        if let Some(groups) = &mut self.compute_groups {
            groups.retain(|group| group != &destination);
        }
        Ok(Some(false))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex};

    use alien_aws_clients::ec2::MockEc2Api;
    use alien_client_core::ErrorData as ClientErrorData;
    use serde_json::json;

    use super::*;

    pub(crate) fn mock_discovery(
        ec2: &mut MockEc2Api,
    ) -> Arc<Mutex<Option<CreateSecurityGroupRequest>>> {
        let group = Arc::new(Mutex::new(None::<CreateSecurityGroupRequest>));
        let stored = group.clone();
        ec2.expect_describe_security_groups().returning(move |request| {
            let filters = request.filters.expect("exact generation filters");
            assert_eq!(filters.len(), 2);
            let stored = stored.lock().unwrap();
            let groups = stored.as_ref().map(|created| {
                assert!(filters.iter().any(|f| f.name == "group-name" && f.values == vec![created.group_name.clone()]));
                assert!(filters.iter().any(|f| f.name == "vpc-id" && f.values == vec![created.vpc_id.clone()]));
                vec![json!({
                    "groupId": "sg-alb", "groupName": created.group_name,
                    "vpcId": created.vpc_id, "tagSet": {"item": created.tag_specifications.as_ref().unwrap()[0].tags},
                })]
            }).unwrap_or_default();
            Ok(serde_json::from_value(json!({"securityGroupInfo": {"item": groups}})).unwrap())
        });
        group
    }

    #[tokio::test]
    async fn lost_create_response_reloads_and_recovers_the_exact_generation() {
        let mut ec2 = MockEc2Api::new();
        let stored = mock_discovery(&mut ec2);
        ec2.expect_create_security_group()
            .times(1)
            .returning(move |request| {
                *stored.lock().unwrap() = Some(request);
                Err(AlienError::new(ClientErrorData::Timeout {
                    message: "create committed; reply lost".to_string(),
                }))
            });
        let mut receipt = AwsVmSecurityGroup::new("vpc-test".to_string(), "test", "api");
        let name = receipt.name.clone();
        let error = receipt
            .step_prepare(&ec2)
            .await
            .expect_err("lost reply surfaces");
        assert!(error.retryable);
        let mut restored: AwsVmSecurityGroup =
            serde_json::from_value(serde_json::to_value(receipt).unwrap()).unwrap();
        assert_eq!(restored.name, name);
        assert_eq!(
            restored.step_prepare(&ec2).await.unwrap().as_deref(),
            Some("sg-alb")
        );
        assert_eq!(
            restored.recover(&ec2).await.unwrap().as_deref(),
            Some("sg-alb")
        );
    }

    #[tokio::test]
    async fn confirm_rejects_foreign_incomplete_and_changed_receipts() {
        for fault in [
            "vpc",
            "name",
            "tags",
            "duplicate-tag",
            "missing-id",
            "changed-id",
            "pagination",
            "ambiguous",
        ] {
            let mut receipt = AwsVmSecurityGroup::new("vpc-test".to_string(), "test", "api");
            receipt.id = Some("sg-alb".to_string());
            let mut group = json!({
                "groupId": "sg-alb", "groupName": receipt.name, "vpcId": receipt.vpc_id,
                "tagSet": {"item": receipt.tags()},
            });
            match fault {
                "vpc" => group["vpcId"] = json!("vpc-foreign"),
                "name" => group["groupName"] = json!("older-generation"),
                "tags" => group["tagSet"]["item"] = json!([]),
                "duplicate-tag" => {
                    let duplicate = group["tagSet"]["item"][0].clone();
                    group["tagSet"]["item"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                "missing-id" => group["groupId"] = json!(null),
                "changed-id" => group["groupId"] = json!("sg-other"),
                _ => {}
            }
            let mut groups = vec![group.clone()];
            if fault == "ambiguous" {
                groups.push(group);
            }
            let response = json!({"securityGroupInfo": {"item": groups}, "nextToken": if fault == "pagination" { Some("more") } else { None }});
            let mut ec2 = MockEc2Api::new();
            ec2.expect_describe_security_groups()
                .times(1)
                .return_once(move |_| Ok(serde_json::from_value(response).unwrap()));
            ec2.expect_create_security_group().times(0);
            receipt.step_prepare(&ec2).await.expect_err(fault);
            assert_eq!(receipt.id.as_deref(), Some("sg-alb"));
        }
    }

    #[tokio::test]
    async fn missing_recorded_id_never_creates_a_replacement() {
        let mut ec2 = MockEc2Api::new();
        mock_discovery(&mut ec2);
        ec2.expect_create_security_group().times(0);
        let mut receipt = AwsVmSecurityGroup::new("vpc-test".to_string(), "test", "api");
        receipt.id = Some("sg-alb".to_string());
        assert!(receipt.step_prepare(&ec2).await.unwrap().is_none());
        assert_eq!(receipt.id.as_deref(), Some("sg-alb"));
    }

    #[tokio::test]
    async fn delete_recovery_retains_an_uncertain_create_across_empty_reads_and_reload() {
        let mut ec2 = MockEc2Api::new();
        let stored = mock_discovery(&mut ec2);
        let created = Arc::new(Mutex::new(None));
        let saved = created.clone();
        ec2.expect_create_security_group()
            .times(1)
            .returning(move |request| {
                *saved.lock().unwrap() = Some(request);
                Err(AlienError::new(ClientErrorData::Timeout {
                    message: "committed but not visible".to_string(),
                }))
            });
        let mut receipt = AwsVmSecurityGroup::new("vpc-test".to_string(), "test", "api");
        assert!(receipt.recover(&ec2).await.unwrap().is_none());
        receipt.step_prepare(&ec2).await.expect_err("lost reply");
        let mut receipt: AwsVmSecurityGroup =
            serde_json::from_value(serde_json::to_value(receipt).unwrap()).unwrap();
        let error = receipt
            .recover(&ec2)
            .await
            .expect_err("absence is inconclusive");
        assert!(error.retryable);
        *stored.lock().unwrap() = created.lock().unwrap().take();
        assert_eq!(
            receipt.recover(&ec2).await.unwrap().as_deref(),
            Some("sg-alb")
        );
        receipt.record_delete_attempt();
        let mut receipt: AwsVmSecurityGroup =
            serde_json::from_value(serde_json::to_value(receipt).unwrap()).unwrap();
        assert_eq!(
            receipt.recover(&ec2).await.unwrap().as_deref(),
            Some("sg-alb")
        );
        *stored.lock().unwrap() = None;
        assert!(receipt.recover(&ec2).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn duplicate_create_waits_for_the_same_generation() {
        let mut ec2 = MockEc2Api::new();
        let stored = mock_discovery(&mut ec2);
        ec2.expect_create_security_group()
            .times(1)
            .returning(move |request| {
                *stored.lock().unwrap() = Some(request);
                Err(AlienError::new(ClientErrorData::RemoteResourceConflict {
                    message: "InvalidGroup.Duplicate: already exists".to_string(),
                    resource_type: "security-group".to_string(),
                    resource_name: "generation".to_string(),
                }))
            });
        let mut receipt = AwsVmSecurityGroup::new("vpc-test".to_string(), "test", "api");
        assert!(receipt.step_prepare(&ec2).await.unwrap().is_none());
        assert_eq!(
            receipt.step_prepare(&ec2).await.unwrap().as_deref(),
            Some("sg-alb")
        );
    }

    #[tokio::test]
    async fn older_receipt_without_create_progress_cannot_be_forgotten() {
        let receipt = AwsVmSecurityGroup::new("vpc-test".to_string(), "test", "api");
        let mut value = serde_json::to_value(receipt).unwrap();
        value.as_object_mut().unwrap().remove("createAttempted");
        let mut receipt: AwsVmSecurityGroup = serde_json::from_value(value).unwrap();
        let mut ec2 = MockEc2Api::new();
        mock_discovery(&mut ec2);
        assert!(
            receipt
                .recover(&ec2)
                .await
                .expect_err("unknown create progress")
                .retryable
        );
    }
}
