//! AWS Auto Scaling Client
//!
//! This module provides a client for interacting with AWS EC2 Auto Scaling APIs,
//! including creating, managing, and scaling Auto Scaling groups.
//!
//! # Example
//!
//! ```rust,ignore
//! use alien_aws_clients::autoscaling::{AutoScalingClient, AutoScalingApi, CreateAutoScalingGroupRequest};
//! use reqwest::Client;
//!
//! let asg_client = AutoScalingClient::new(Client::new(), aws_config);
//! asg_client.create_auto_scaling_group(
//!     CreateAutoScalingGroupRequest::builder()
//!         .auto_scaling_group_name("my-asg".to_string())
//!         .launch_template(LaunchTemplateSpecification::builder()
//!             .launch_template_id("lt-12345".to_string())
//!             .build())
//!         .min_size(1)
//!         .max_size(10)
//!         .build()
//! ).await?;
//! ```

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::ContextError;
use async_trait::async_trait;
use bon::Builder;
use form_urlencoded;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

// ---------------------------------------------------------------------------
// Auto Scaling Error Response Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AutoScalingErrorResponse {
    pub error: AutoScalingErrorWrapper,
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AutoScalingErrorWrapper {
    #[serde(rename = "Code")]
    pub code: String,
    #[serde(rename = "Message")]
    pub message: String,
}

// ---------------------------------------------------------------------------
// Auto Scaling API Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait AutoScalingApi: Send + Sync + std::fmt::Debug {
    // Auto Scaling Group Operations
    async fn create_auto_scaling_group(&self, request: CreateAutoScalingGroupRequest)
        -> Result<()>;
    async fn update_auto_scaling_group(&self, request: UpdateAutoScalingGroupRequest)
        -> Result<()>;
    async fn delete_auto_scaling_group(&self, request: DeleteAutoScalingGroupRequest)
        -> Result<()>;
    async fn describe_auto_scaling_groups(
        &self,
        request: DescribeAutoScalingGroupsRequest,
    ) -> Result<DescribeAutoScalingGroupsResponse>;
    async fn set_desired_capacity(&self, request: SetDesiredCapacityRequest) -> Result<()>;

    // Instance Operations
    async fn describe_auto_scaling_instances(
        &self,
        request: DescribeAutoScalingInstancesRequest,
    ) -> Result<DescribeAutoScalingInstancesResponse>;
    async fn terminate_instance_in_auto_scaling_group(
        &self,
        request: TerminateInstanceInAutoScalingGroupRequest,
    ) -> Result<TerminateInstanceInAutoScalingGroupResponse>;

    // Instance Refresh Operations
    /// Starts a rolling instance refresh on an Auto Scaling group.
    /// See: https://docs.aws.amazon.com/autoscaling/ec2/APIReference/API_StartInstanceRefresh.html
    async fn start_instance_refresh(
        &self,
        request: StartInstanceRefreshRequest,
    ) -> Result<StartInstanceRefreshResponse>;
    /// Describes instance refreshes for an Auto Scaling group.
    /// See: https://docs.aws.amazon.com/autoscaling/ec2/APIReference/API_DescribeInstanceRefreshes.html
    async fn describe_instance_refreshes(
        &self,
        request: DescribeInstanceRefreshesRequest,
    ) -> Result<DescribeInstanceRefreshesResponse>;
}

// ---------------------------------------------------------------------------
// Auto Scaling Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AutoScalingClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl AutoScalingClient {
    /// Create a new Auto Scaling client.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "autoscaling".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("autoscaling") {
            override_url.to_string()
        } else {
            format!(
                "https://autoscaling.{}.amazonaws.com",
                self.credentials.region()
            )
        }
    }

    fn get_host(&self) -> String {
        format!("autoscaling.{}.amazonaws.com", self.credentials.region())
    }

    // ------------------------- Internal Helpers -------------------------

    async fn send_form<T: DeserializeOwned + Send + 'static>(
        &self,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;
        let url = self.get_base_url();

        let form_body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&self.get_host())
            .content_type_form()
            .content_sha256(&form_body)
            .body(form_body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_xml(builder, &self.sign_config()).await;

        Self::map_result(result, operation, resource, Some(&form_body))
    }

    async fn send_form_no_body(
        &self,
        form_data: HashMap<String, String>,
        operation: &str,
        resource: &str,
    ) -> Result<()> {
        self.credentials.ensure_fresh().await?;
        let url = self.get_base_url();

        let form_body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form_data.iter())
            .finish();

        let builder = self
            .client
            .request(Method::POST, &url)
            .host(&self.get_host())
            .content_type_form()
            .content_sha256(&form_body)
            .body(form_body.clone());

        let result =
            crate::aws::aws_request_utils::sign_send_no_response(builder, &self.sign_config())
                .await;

        Self::map_result(result, operation, resource, Some(&form_body))
    }

    fn map_result<T>(
        result: Result<T>,
        operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Result<T> {
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(ErrorData::HttpResponseError {
                    http_status,
                    http_response_text: Some(ref text),
                    ..
                }) = &e.error
                {
                    let status = StatusCode::from_u16(*http_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    if let Some(mapped) =
                        Self::map_autoscaling_error(status, text, operation, resource, request_body)
                    {
                        Err(e.context(mapped))
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    fn map_autoscaling_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        // Handle empty response bodies
        if body.trim().is_empty() {
            return match status {
                StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                    resource_type: "AutoScalingGroup".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                    message: "Resource conflict".into(),
                    resource_type: "AutoScalingGroup".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                    Some(ErrorData::RemoteAccessDenied {
                        resource_type: "AutoScalingGroup".into(),
                        resource_name: resource.into(),
                    })
                }
                StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded {
                    message: "Too many requests".into(),
                }),
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => Some(ErrorData::RemoteServiceUnavailable {
                    message: "Service unavailable".into(),
                }),
                _ => None,
            };
        }

        // Try to parse Auto Scaling error XML
        let parsed: std::result::Result<AutoScalingErrorResponse, _> =
            quick_xml::de::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => (e.error.code, e.error.message),
            Err(_) => {
                return None;
            }
        };

        // Map Auto Scaling error codes
        // Reference: https://docs.aws.amazon.com/autoscaling/ec2/APIReference/CommonErrors.html
        Some(match code.as_str() {
            // Access / Auth errors
            "AccessDenied" | "UnauthorizedAccess" => ErrorData::RemoteAccessDenied {
                resource_type: "AutoScalingGroup".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "Throttling" | "RequestLimitExceeded" => ErrorData::RateLimitExceeded { message },
            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Resource not found
            "ResourceNotFound" | "ValidationError" if message.contains("not found") => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "AutoScalingGroup".into(),
                    resource_name: resource.into(),
                }
            }
            // Already exists
            "AlreadyExists" | "AlreadyExistsFault" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "AutoScalingGroup".into(),
                resource_name: resource.into(),
            },
            // Limit exceeded
            "LimitExceeded" | "LimitExceededFault" => ErrorData::QuotaExceeded { message },
            // Resource in use
            "ResourceInUse" | "ResourceInUseFault" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "AutoScalingGroup".into(),
                resource_name: resource.into(),
            },
            // Scaling activity in progress
            "ScalingActivityInProgress" | "ScalingActivityInProgressFault" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "AutoScalingGroup".into(),
                    resource_name: resource.into(),
                }
            }
            // Instance refresh in progress
            "InstanceRefreshInProgress" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "AutoScalingGroup".into(),
                resource_name: resource.into(),
            },
            // Invalid input
            "ValidationError" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            // Service linked role failure
            "ServiceLinkedRoleFailure" => ErrorData::RemoteServiceUnavailable { message },
            // Default fallback based on status code
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "AutoScalingGroup".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "AutoScalingGroup".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "AutoScalingGroup".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("Auto Scaling operation failed: {}", message),
                    url: "autoscaling.amazonaws.com".into(),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }

    /// Add tag parameters to the form data.
    fn add_tags(form_data: &mut HashMap<String, String>, tags: &[AsgTag]) {
        for (i, tag) in tags.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Tags.member.{}.Key", idx), tag.key.clone());
            form_data.insert(format!("Tags.member.{}.Value", idx), tag.value.clone());
            if let Some(ref resource_id) = tag.resource_id {
                form_data.insert(
                    format!("Tags.member.{}.ResourceId", idx),
                    resource_id.clone(),
                );
            }
            if let Some(ref resource_type) = tag.resource_type {
                form_data.insert(
                    format!("Tags.member.{}.ResourceType", idx),
                    resource_type.clone(),
                );
            }
            if let Some(propagate_at_launch) = tag.propagate_at_launch {
                form_data.insert(
                    format!("Tags.member.{}.PropagateAtLaunch", idx),
                    propagate_at_launch.to_string(),
                );
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl AutoScalingApi for AutoScalingClient {
    // ---------------------------------------------------------------------------
    // Auto Scaling Group Operations
    // ---------------------------------------------------------------------------

    async fn create_auto_scaling_group(
        &self,
        request: CreateAutoScalingGroupRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateAutoScalingGroup".to_string());
        form_data.insert("Version".to_string(), "2011-01-01".to_string());
        form_data.insert(
            "AutoScalingGroupName".to_string(),
            request.auto_scaling_group_name.clone(),
        );
        form_data.insert("MinSize".to_string(), request.min_size.to_string());
        form_data.insert("MaxSize".to_string(), request.max_size.to_string());

        if let Some(desired_capacity) = request.desired_capacity {
            form_data.insert("DesiredCapacity".to_string(), desired_capacity.to_string());
        }

        if let Some(ref launch_template) = request.launch_template {
            if let Some(ref id) = launch_template.launch_template_id {
                form_data.insert("LaunchTemplate.LaunchTemplateId".to_string(), id.clone());
            }
            if let Some(ref name) = launch_template.launch_template_name {
                form_data.insert(
                    "LaunchTemplate.LaunchTemplateName".to_string(),
                    name.clone(),
                );
            }
            if let Some(ref version) = launch_template.version {
                form_data.insert("LaunchTemplate.Version".to_string(), version.clone());
            }
        }

        if let Some(ref launch_config_name) = request.launch_configuration_name {
            form_data.insert(
                "LaunchConfigurationName".to_string(),
                launch_config_name.clone(),
            );
        }

        if let Some(ref vpc_zone_identifier) = request.vpc_zone_identifier {
            form_data.insert("VPCZoneIdentifier".to_string(), vpc_zone_identifier.clone());
        }

        if let Some(ref availability_zones) = request.availability_zones {
            for (i, az) in availability_zones.iter().enumerate() {
                form_data.insert(format!("AvailabilityZones.member.{}", i + 1), az.clone());
            }
        }

        if let Some(default_cooldown) = request.default_cooldown {
            form_data.insert("DefaultCooldown".to_string(), default_cooldown.to_string());
        }

        if let Some(health_check_grace_period) = request.health_check_grace_period {
            form_data.insert(
                "HealthCheckGracePeriod".to_string(),
                health_check_grace_period.to_string(),
            );
        }

        if let Some(ref health_check_type) = request.health_check_type {
            form_data.insert("HealthCheckType".to_string(), health_check_type.clone());
        }

        if let Some(ref target_group_arns) = request.target_group_arns {
            for (i, arn) in target_group_arns.iter().enumerate() {
                form_data.insert(format!("TargetGroupARNs.member.{}", i + 1), arn.clone());
            }
        }

        if let Some(ref service_linked_role_arn) = request.service_linked_role_arn {
            form_data.insert(
                "ServiceLinkedRoleARN".to_string(),
                service_linked_role_arn.clone(),
            );
        }

        if let Some(ref tags) = request.tags {
            Self::add_tags(&mut form_data, tags);
        }

        if let Some(capacity_rebalance) = request.capacity_rebalance {
            form_data.insert(
                "CapacityRebalance".to_string(),
                capacity_rebalance.to_string(),
            );
        }

        if let Some(default_instance_warmup) = request.default_instance_warmup {
            form_data.insert(
                "DefaultInstanceWarmup".to_string(),
                default_instance_warmup.to_string(),
            );
        }

        if let Some(new_instances_protected) = request.new_instances_protected_from_scale_in {
            form_data.insert(
                "NewInstancesProtectedFromScaleIn".to_string(),
                new_instances_protected.to_string(),
            );
        }

        self.send_form_no_body(
            form_data,
            "CreateAutoScalingGroup",
            &request.auto_scaling_group_name,
        )
        .await
    }

    async fn update_auto_scaling_group(
        &self,
        request: UpdateAutoScalingGroupRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "UpdateAutoScalingGroup".to_string());
        form_data.insert("Version".to_string(), "2011-01-01".to_string());
        form_data.insert(
            "AutoScalingGroupName".to_string(),
            request.auto_scaling_group_name.clone(),
        );

        if let Some(min_size) = request.min_size {
            form_data.insert("MinSize".to_string(), min_size.to_string());
        }

        if let Some(max_size) = request.max_size {
            form_data.insert("MaxSize".to_string(), max_size.to_string());
        }

        if let Some(desired_capacity) = request.desired_capacity {
            form_data.insert("DesiredCapacity".to_string(), desired_capacity.to_string());
        }

        if let Some(ref launch_template) = request.launch_template {
            if let Some(ref id) = launch_template.launch_template_id {
                form_data.insert("LaunchTemplate.LaunchTemplateId".to_string(), id.clone());
            }
            if let Some(ref name) = launch_template.launch_template_name {
                form_data.insert(
                    "LaunchTemplate.LaunchTemplateName".to_string(),
                    name.clone(),
                );
            }
            if let Some(ref version) = launch_template.version {
                form_data.insert("LaunchTemplate.Version".to_string(), version.clone());
            }
        }

        if let Some(ref vpc_zone_identifier) = request.vpc_zone_identifier {
            form_data.insert("VPCZoneIdentifier".to_string(), vpc_zone_identifier.clone());
        }

        if let Some(default_cooldown) = request.default_cooldown {
            form_data.insert("DefaultCooldown".to_string(), default_cooldown.to_string());
        }

        if let Some(health_check_grace_period) = request.health_check_grace_period {
            form_data.insert(
                "HealthCheckGracePeriod".to_string(),
                health_check_grace_period.to_string(),
            );
        }

        if let Some(ref health_check_type) = request.health_check_type {
            form_data.insert("HealthCheckType".to_string(), health_check_type.clone());
        }

        if let Some(capacity_rebalance) = request.capacity_rebalance {
            form_data.insert(
                "CapacityRebalance".to_string(),
                capacity_rebalance.to_string(),
            );
        }

        if let Some(default_instance_warmup) = request.default_instance_warmup {
            form_data.insert(
                "DefaultInstanceWarmup".to_string(),
                default_instance_warmup.to_string(),
            );
        }

        self.send_form_no_body(
            form_data,
            "UpdateAutoScalingGroup",
            &request.auto_scaling_group_name,
        )
        .await
    }

    async fn delete_auto_scaling_group(
        &self,
        request: DeleteAutoScalingGroupRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteAutoScalingGroup".to_string());
        form_data.insert("Version".to_string(), "2011-01-01".to_string());
        form_data.insert(
            "AutoScalingGroupName".to_string(),
            request.auto_scaling_group_name.clone(),
        );

        if let Some(force_delete) = request.force_delete {
            form_data.insert("ForceDelete".to_string(), force_delete.to_string());
        }

        self.send_form_no_body(
            form_data,
            "DeleteAutoScalingGroup",
            &request.auto_scaling_group_name,
        )
        .await
    }

    async fn describe_auto_scaling_groups(
        &self,
        request: DescribeAutoScalingGroupsRequest,
    ) -> Result<DescribeAutoScalingGroupsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "DescribeAutoScalingGroups".to_string(),
        );
        form_data.insert("Version".to_string(), "2011-01-01".to_string());

        if let Some(ref names) = request.auto_scaling_group_names {
            for (i, name) in names.iter().enumerate() {
                form_data.insert(
                    format!("AutoScalingGroupNames.member.{}", i + 1),
                    name.clone(),
                );
            }
        }

        if let Some(max_records) = request.max_records {
            form_data.insert("MaxRecords".to_string(), max_records.to_string());
        }

        if let Some(ref next_token) = request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeAutoScalingGroups", "AutoScalingGroup")
            .await
    }

    async fn set_desired_capacity(&self, request: SetDesiredCapacityRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "SetDesiredCapacity".to_string());
        form_data.insert("Version".to_string(), "2011-01-01".to_string());
        form_data.insert(
            "AutoScalingGroupName".to_string(),
            request.auto_scaling_group_name.clone(),
        );
        form_data.insert(
            "DesiredCapacity".to_string(),
            request.desired_capacity.to_string(),
        );

        if let Some(honor_cooldown) = request.honor_cooldown {
            form_data.insert("HonorCooldown".to_string(), honor_cooldown.to_string());
        }

        self.send_form_no_body(
            form_data,
            "SetDesiredCapacity",
            &request.auto_scaling_group_name,
        )
        .await
    }

    // ---------------------------------------------------------------------------
    // Instance Operations
    // ---------------------------------------------------------------------------

    async fn describe_auto_scaling_instances(
        &self,
        request: DescribeAutoScalingInstancesRequest,
    ) -> Result<DescribeAutoScalingInstancesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "DescribeAutoScalingInstances".to_string(),
        );
        form_data.insert("Version".to_string(), "2011-01-01".to_string());

        if let Some(ref instance_ids) = request.instance_ids {
            for (i, id) in instance_ids.iter().enumerate() {
                form_data.insert(format!("InstanceIds.member.{}", i + 1), id.clone());
            }
        }

        if let Some(max_records) = request.max_records {
            form_data.insert("MaxRecords".to_string(), max_records.to_string());
        }

        if let Some(ref next_token) = request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeAutoScalingInstances", "Instance")
            .await
    }

    async fn terminate_instance_in_auto_scaling_group(
        &self,
        request: TerminateInstanceInAutoScalingGroupRequest,
    ) -> Result<TerminateInstanceInAutoScalingGroupResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "TerminateInstanceInAutoScalingGroup".to_string(),
        );
        form_data.insert("Version".to_string(), "2011-01-01".to_string());
        form_data.insert("InstanceId".to_string(), request.instance_id.clone());
        form_data.insert(
            "ShouldDecrementDesiredCapacity".to_string(),
            request.should_decrement_desired_capacity.to_string(),
        );

        self.send_form(
            form_data,
            "TerminateInstanceInAutoScalingGroup",
            &request.instance_id,
        )
        .await
    }

    async fn start_instance_refresh(
        &self,
        request: StartInstanceRefreshRequest,
    ) -> Result<StartInstanceRefreshResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "StartInstanceRefresh".to_string());
        form_data.insert("Version".to_string(), "2011-01-01".to_string());
        form_data.insert(
            "AutoScalingGroupName".to_string(),
            request.auto_scaling_group_name.clone(),
        );
        if let Some(ref strategy) = request.strategy {
            form_data.insert("Strategy".to_string(), strategy.clone());
        }
        if let Some(ref prefs) = request.preferences {
            if let Some(min_healthy) = prefs.min_healthy_percentage {
                form_data.insert(
                    "Preferences.MinHealthyPercentage".to_string(),
                    min_healthy.to_string(),
                );
            }
            if let Some(max_healthy) = prefs.max_healthy_percentage {
                form_data.insert(
                    "Preferences.MaxHealthyPercentage".to_string(),
                    max_healthy.to_string(),
                );
            }
            if let Some(warmup) = prefs.instance_warmup {
                form_data.insert("Preferences.InstanceWarmup".to_string(), warmup.to_string());
            }
        }
        self.send_form(
            form_data,
            "StartInstanceRefresh",
            &request.auto_scaling_group_name,
        )
        .await
    }

    async fn describe_instance_refreshes(
        &self,
        request: DescribeInstanceRefreshesRequest,
    ) -> Result<DescribeInstanceRefreshesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "DescribeInstanceRefreshes".to_string(),
        );
        form_data.insert("Version".to_string(), "2011-01-01".to_string());
        form_data.insert(
            "AutoScalingGroupName".to_string(),
            request.auto_scaling_group_name.clone(),
        );
        if let Some(ref ids) = request.instance_refresh_ids {
            for (i, id) in ids.iter().enumerate() {
                form_data.insert(format!("InstanceRefreshIds.member.{}", i + 1), id.clone());
            }
        }
        self.send_form(
            form_data,
            "DescribeInstanceRefreshes",
            &request.auto_scaling_group_name,
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Request/Response Types
// ---------------------------------------------------------------------------

/// Launch template specification for an Auto Scaling group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateSpecification {
    /// The ID of the launch template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_id: Option<String>,
    /// The name of the launch template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_name: Option<String>,
    /// The version of the launch template. Use "$Latest" or "$Default".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// A tag for an Auto Scaling group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AsgTag {
    /// The tag key.
    pub key: String,
    /// The tag value.
    pub value: String,
    /// The ID of the resource (e.g., the Auto Scaling group name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    /// The type of resource (auto-scaling-group).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    /// Whether to propagate the tag to instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub propagate_at_launch: Option<bool>,
}

/// Request to create an Auto Scaling group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateAutoScalingGroupRequest {
    /// The name of the Auto Scaling group.
    pub auto_scaling_group_name: String,
    /// The minimum size of the group.
    pub min_size: i32,
    /// The maximum size of the group.
    pub max_size: i32,
    /// The desired capacity of the group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_capacity: Option<i32>,
    /// The launch template to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template: Option<LaunchTemplateSpecification>,
    /// The name of the launch configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_configuration_name: Option<String>,
    /// A comma-separated list of subnet IDs for the VPC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_zone_identifier: Option<String>,
    /// A list of Availability Zones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zones: Option<Vec<String>>,
    /// The default cooldown period in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_cooldown: Option<i32>,
    /// The health check grace period in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_grace_period: Option<i32>,
    /// The health check type: EC2 or ELB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_type: Option<String>,
    /// The ARNs of the target groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_group_arns: Option<Vec<String>>,
    /// The ARN of the service-linked role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_linked_role_arn: Option<String>,
    /// Tags for the group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<AsgTag>>,
    /// Enable Capacity Rebalancing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_rebalance: Option<bool>,
    /// The default instance warmup in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_instance_warmup: Option<i32>,
    /// Whether instances are protected from scale in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_instances_protected_from_scale_in: Option<bool>,
}

/// Request to update an Auto Scaling group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct UpdateAutoScalingGroupRequest {
    /// The name of the Auto Scaling group.
    pub auto_scaling_group_name: String,
    /// The minimum size of the group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size: Option<i32>,
    /// The maximum size of the group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<i32>,
    /// The desired capacity of the group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_capacity: Option<i32>,
    /// The launch template to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template: Option<LaunchTemplateSpecification>,
    /// A comma-separated list of subnet IDs for the VPC.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_zone_identifier: Option<String>,
    /// The default cooldown period in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_cooldown: Option<i32>,
    /// The health check grace period in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_grace_period: Option<i32>,
    /// The health check type: EC2 or ELB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_type: Option<String>,
    /// Enable Capacity Rebalancing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_rebalance: Option<bool>,
    /// The default instance warmup in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_instance_warmup: Option<i32>,
}

/// Request to delete an Auto Scaling group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DeleteAutoScalingGroupRequest {
    /// The name of the Auto Scaling group.
    pub auto_scaling_group_name: String,
    /// Whether to also terminate the EC2 instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_delete: Option<bool>,
}

/// Request to describe Auto Scaling groups.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeAutoScalingGroupsRequest {
    /// The names of the Auto Scaling groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_scaling_group_names: Option<Vec<String>>,
    /// The maximum number of items to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_records: Option<i32>,
    /// The token for the next set of items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing Auto Scaling groups.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeAutoScalingGroupsResponse {
    pub describe_auto_scaling_groups_result: DescribeAutoScalingGroupsResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeAutoScalingGroupsResult {
    pub auto_scaling_groups: Option<AutoScalingGroupsWrapper>,
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AutoScalingGroupsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<AutoScalingGroup>,
}

/// Represents an Auto Scaling group.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AutoScalingGroup {
    pub auto_scaling_group_name: Option<String>,
    #[serde(rename = "AutoScalingGroupARN")]
    pub auto_scaling_group_arn: Option<String>,
    pub launch_template: Option<LaunchTemplateSpecificationResponse>,
    pub launch_configuration_name: Option<String>,
    pub min_size: Option<i32>,
    pub max_size: Option<i32>,
    pub desired_capacity: Option<i32>,
    pub default_cooldown: Option<i32>,
    pub availability_zones: Option<AvailabilityZonesWrapper>,
    #[serde(rename = "VPCZoneIdentifier")]
    pub vpc_zone_identifier: Option<String>,
    pub health_check_type: Option<String>,
    pub health_check_grace_period: Option<i32>,
    pub instances: Option<InstancesWrapper>,
    pub created_time: Option<String>,
    pub status: Option<String>,
    pub tags: Option<TagsWrapper>,
    #[serde(rename = "TargetGroupARNs")]
    pub target_group_arns: Option<TargetGroupArnsWrapper>,
    pub service_linked_role_arn: Option<String>,
    pub capacity_rebalance: Option<bool>,
    pub default_instance_warmup: Option<i32>,
    pub new_instances_protected_from_scale_in: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LaunchTemplateSpecificationResponse {
    pub launch_template_id: Option<String>,
    pub launch_template_name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AvailabilityZonesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupArnsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InstancesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<AsgInstance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TagsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<TagDescription>,
}

/// An instance in an Auto Scaling group.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AsgInstance {
    pub instance_id: Option<String>,
    pub instance_type: Option<String>,
    pub availability_zone: Option<String>,
    pub lifecycle_state: Option<String>,
    pub health_status: Option<String>,
    pub launch_template: Option<LaunchTemplateSpecificationResponse>,
    pub launch_configuration_name: Option<String>,
    pub protected_from_scale_in: Option<bool>,
    pub weighted_capacity: Option<String>,
}

/// A tag description.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TagDescription {
    pub key: Option<String>,
    pub value: Option<String>,
    pub resource_id: Option<String>,
    pub resource_type: Option<String>,
    pub propagate_at_launch: Option<bool>,
}

/// Request to set the desired capacity.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct SetDesiredCapacityRequest {
    /// The name of the Auto Scaling group.
    pub auto_scaling_group_name: String,
    /// The desired capacity.
    pub desired_capacity: i32,
    /// Whether to honor the default cooldown period.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub honor_cooldown: Option<bool>,
}

/// Request to describe Auto Scaling instances.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeAutoScalingInstancesRequest {
    /// The instance IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_ids: Option<Vec<String>>,
    /// The maximum number of items to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_records: Option<i32>,
    /// The token for the next set of items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing Auto Scaling instances.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeAutoScalingInstancesResponse {
    pub describe_auto_scaling_instances_result: DescribeAutoScalingInstancesResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeAutoScalingInstancesResult {
    pub auto_scaling_instances: Option<AutoScalingInstancesWrapper>,
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AutoScalingInstancesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<AutoScalingInstanceDetails>,
}

/// Details about an Auto Scaling instance.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AutoScalingInstanceDetails {
    pub instance_id: Option<String>,
    pub instance_type: Option<String>,
    pub auto_scaling_group_name: Option<String>,
    pub availability_zone: Option<String>,
    pub lifecycle_state: Option<String>,
    pub health_status: Option<String>,
    pub launch_template: Option<LaunchTemplateSpecificationResponse>,
    pub launch_configuration_name: Option<String>,
    pub protected_from_scale_in: Option<bool>,
    pub weighted_capacity: Option<String>,
}

/// Request to terminate an instance in an Auto Scaling group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TerminateInstanceInAutoScalingGroupRequest {
    /// The instance ID.
    pub instance_id: String,
    /// Whether to decrement the desired capacity.
    pub should_decrement_desired_capacity: bool,
}

/// Response from terminating an instance.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TerminateInstanceInAutoScalingGroupResponse {
    pub terminate_instance_in_auto_scaling_group_result: TerminateInstanceInAutoScalingGroupResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TerminateInstanceInAutoScalingGroupResult {
    pub activity: Option<Activity>,
}

/// An Auto Scaling activity.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Activity {
    pub activity_id: Option<String>,
    pub auto_scaling_group_name: Option<String>,
    pub description: Option<String>,
    pub cause: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub status_code: Option<String>,
    pub status_message: Option<String>,
    pub progress: Option<i32>,
    pub details: Option<String>,
}

// ---------------------------------------------------------------------------
// Instance Refresh Types
// ---------------------------------------------------------------------------

/// Request to start an instance refresh.
#[derive(Debug, Clone, Builder)]
pub struct StartInstanceRefreshRequest {
    pub auto_scaling_group_name: String,
    pub strategy: Option<String>,
    pub preferences: Option<RefreshPreferences>,
}

/// Preferences for an instance refresh.
#[derive(Debug, Clone, Default, Builder)]
pub struct RefreshPreferences {
    pub min_healthy_percentage: Option<i32>,
    pub max_healthy_percentage: Option<i32>,
    pub instance_warmup: Option<i32>,
}

/// Response from starting an instance refresh.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StartInstanceRefreshResponse {
    pub start_instance_refresh_result: StartInstanceRefreshResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StartInstanceRefreshResult {
    pub instance_refresh_id: Option<String>,
}

/// Request to describe instance refreshes.
#[derive(Debug, Clone, Builder)]
pub struct DescribeInstanceRefreshesRequest {
    pub auto_scaling_group_name: String,
    pub instance_refresh_ids: Option<Vec<String>>,
    pub max_records: Option<i32>,
}

/// Response from describing instance refreshes.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeInstanceRefreshesResponse {
    pub describe_instance_refreshes_result: DescribeInstanceRefreshesResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeInstanceRefreshesResult {
    pub instance_refreshes: Option<InstanceRefreshesWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InstanceRefreshesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<InstanceRefresh>,
}

/// An instance refresh record.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InstanceRefresh {
    pub instance_refresh_id: Option<String>,
    pub auto_scaling_group_name: Option<String>,
    /// Status: Pending, InProgress, Successful, Failed, Cancelling, Cancelled.
    pub status: Option<String>,
    pub status_reason: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub percentage_complete: Option<i32>,
    pub instances_to_update: Option<i32>,
}
