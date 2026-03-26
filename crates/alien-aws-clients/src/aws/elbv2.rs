//! AWS Elastic Load Balancing v2 (ELBv2) Client
//!
//! This module provides a client for interacting with AWS ELBv2 APIs, including
//! Application Load Balancers, Target Groups, and Listeners.
//!
//! # Example
//!
//! ```rust,ignore
//! use alien_aws_clients::elbv2::{Elbv2Client, Elbv2Api, CreateLoadBalancerRequest};
//! use reqwest::Client;
//!
//! let elb_client = Elbv2Client::new(Client::new(), aws_config);
//! elb_client.create_load_balancer(
//!     CreateLoadBalancerRequest::builder()
//!         .name("my-alb".to_string())
//!         .subnets(vec!["subnet-12345".to_string()])
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
// ELBv2 Error Response Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Elbv2ErrorResponse {
    pub error: Elbv2ErrorWrapper,
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Elbv2ErrorWrapper {
    #[serde(rename = "Code")]
    pub code: String,
    #[serde(rename = "Message")]
    pub message: String,
}

// ---------------------------------------------------------------------------
// ELBv2 API Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Elbv2Api: Send + Sync + std::fmt::Debug {
    // Load Balancer Operations
    async fn create_load_balancer(
        &self,
        request: CreateLoadBalancerRequest,
    ) -> Result<CreateLoadBalancerResponse>;
    async fn describe_load_balancers(
        &self,
        request: DescribeLoadBalancersRequest,
    ) -> Result<DescribeLoadBalancersResponse>;
    async fn delete_load_balancer(&self, load_balancer_arn: &str) -> Result<()>;

    // Target Group Operations
    async fn create_target_group(
        &self,
        request: CreateTargetGroupRequest,
    ) -> Result<CreateTargetGroupResponse>;
    async fn describe_target_groups(
        &self,
        request: DescribeTargetGroupsRequest,
    ) -> Result<DescribeTargetGroupsResponse>;
    async fn modify_target_group(
        &self,
        request: ModifyTargetGroupRequest,
    ) -> Result<ModifyTargetGroupResponse>;
    async fn delete_target_group(&self, target_group_arn: &str) -> Result<()>;

    // Target Operations
    async fn register_targets(&self, request: RegisterTargetsRequest) -> Result<()>;
    async fn deregister_targets(&self, request: DeregisterTargetsRequest) -> Result<()>;
    async fn describe_target_health(
        &self,
        request: DescribeTargetHealthRequest,
    ) -> Result<DescribeTargetHealthResponse>;

    // Listener Operations
    async fn create_listener(
        &self,
        request: CreateListenerRequest,
    ) -> Result<CreateListenerResponse>;
    async fn describe_listeners(
        &self,
        request: DescribeListenersRequest,
    ) -> Result<DescribeListenersResponse>;
    async fn modify_listener(
        &self,
        request: ModifyListenerRequest,
    ) -> Result<ModifyListenerResponse>;
    async fn delete_listener(&self, listener_arn: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// ELBv2 Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Elbv2Client {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl Elbv2Client {
    /// Create a new ELBv2 client.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self { client, credentials }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "elasticloadbalancing".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self
            .credentials
            .get_service_endpoint_option("elasticloadbalancing")
        {
            override_url.to_string()
        } else {
            format!(
                "https://elasticloadbalancing.{}.amazonaws.com",
                self.credentials.region()
            )
        }
    }

    fn get_host(&self) -> String {
        format!("elasticloadbalancing.{}.amazonaws.com", self.credentials.region())
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
                        Self::map_elbv2_error(status, text, operation, resource, request_body)
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

    fn map_elbv2_error(
        status: StatusCode,
        body: &str,
        _operation: &str,
        resource: &str,
        request_body: Option<&str>,
    ) -> Option<ErrorData> {
        if body.trim().is_empty() {
            return match status {
                StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                    message: "Resource conflict".into(),
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                    Some(ErrorData::RemoteAccessDenied {
                        resource_type: "LoadBalancer".into(),
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

        let parsed: std::result::Result<Elbv2ErrorResponse, _> = quick_xml::de::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => (e.error.code, e.error.message),
            Err(_) => {
                return None;
            }
        };

        // Map ELBv2 error codes
        Some(match code.as_str() {
            // Access / Auth errors
            "AccessDenied" | "UnauthorizedAccess" => ErrorData::RemoteAccessDenied {
                resource_type: "LoadBalancer".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "Throttling" | "RequestLimitExceeded" => ErrorData::RateLimitExceeded { message },
            // Service unavailable
            "ServiceUnavailable" | "InternalFailure" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Load balancer not found
            "LoadBalancerNotFound" | "LoadBalancerNotFoundException" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }
            }
            // Target group not found
            "TargetGroupNotFound" | "TargetGroupNotFoundException" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "TargetGroup".into(),
                    resource_name: resource.into(),
                }
            }
            // Listener not found
            "ListenerNotFound" | "ListenerNotFoundException" => ErrorData::RemoteResourceNotFound {
                resource_type: "Listener".into(),
                resource_name: resource.into(),
            },
            // Already exists
            "DuplicateLoadBalancerName" | "DuplicateLoadBalancerNameException" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                }
            }
            "DuplicateTargetGroupName" | "DuplicateTargetGroupNameException" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "TargetGroup".into(),
                    resource_name: resource.into(),
                }
            }
            "DuplicateListener" | "DuplicateListenerException" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "Listener".into(),
                    resource_name: resource.into(),
                }
            }
            // Resource in use
            "ResourceInUse" | "ResourceInUseException" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "LoadBalancer".into(),
                resource_name: resource.into(),
            },
            // Limit exceeded
            "TooManyLoadBalancers"
            | "TooManyTargetGroups"
            | "TooManyListeners"
            | "TooManyTargets"
            | "TooManyRegistrationsForTargetId"
            | "TooManyTags" => ErrorData::QuotaExceeded { message },
            // Invalid input
            "InvalidConfigurationRequest" | "ValidationError" => ErrorData::InvalidInput {
                message,
                field_name: None,
            },
            "InvalidTarget" | "InvalidTargetException" => ErrorData::InvalidInput {
                message,
                field_name: Some("target".into()),
            },
            "InvalidSubnet" | "SubnetNotFound" => ErrorData::InvalidInput {
                message,
                field_name: Some("subnet".into()),
            },
            "InvalidSecurityGroup" | "SecurityGroupNotFound" => ErrorData::InvalidInput {
                message,
                field_name: Some("security_group".into()),
            },
            // Default fallback
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "LoadBalancer".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("ELBv2 operation failed: {}", message),
                    url: "elasticloadbalancing.amazonaws.com".into(),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }

    fn add_tags(form_data: &mut HashMap<String, String>, tags: &[ElbTag]) {
        for (i, tag) in tags.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Tags.member.{}.Key", idx), tag.key.clone());
            form_data.insert(format!("Tags.member.{}.Value", idx), tag.value.clone());
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Elbv2Api for Elbv2Client {
    // ---------------------------------------------------------------------------
    // Load Balancer Operations
    // ---------------------------------------------------------------------------

    async fn create_load_balancer(
        &self,
        request: CreateLoadBalancerRequest,
    ) -> Result<CreateLoadBalancerResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateLoadBalancer".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("Name".to_string(), request.name.clone());

        for (i, subnet) in request.subnets.iter().enumerate() {
            form_data.insert(format!("Subnets.member.{}", i + 1), subnet.clone());
        }

        if let Some(ref subnet_mappings) = request.subnet_mappings {
            for (i, mapping) in subnet_mappings.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("SubnetMappings.member.{}.SubnetId", idx),
                    mapping.subnet_id.clone(),
                );
                if let Some(ref allocation_id) = mapping.allocation_id {
                    form_data.insert(
                        format!("SubnetMappings.member.{}.AllocationId", idx),
                        allocation_id.clone(),
                    );
                }
                if let Some(ref private_ipv4_address) = mapping.private_ipv4_address {
                    form_data.insert(
                        format!("SubnetMappings.member.{}.PrivateIPv4Address", idx),
                        private_ipv4_address.clone(),
                    );
                }
            }
        }

        if let Some(ref security_groups) = request.security_groups {
            for (i, sg) in security_groups.iter().enumerate() {
                form_data.insert(format!("SecurityGroups.member.{}", i + 1), sg.clone());
            }
        }

        if let Some(ref scheme) = request.scheme {
            form_data.insert("Scheme".to_string(), scheme.clone());
        }

        if let Some(ref lb_type) = request.load_balancer_type {
            form_data.insert("Type".to_string(), lb_type.clone());
        }

        if let Some(ref ip_address_type) = request.ip_address_type {
            form_data.insert("IpAddressType".to_string(), ip_address_type.clone());
        }

        if let Some(ref tags) = request.tags {
            Self::add_tags(&mut form_data, tags);
        }

        self.send_form(form_data, "CreateLoadBalancer", &request.name)
            .await
    }

    async fn describe_load_balancers(
        &self,
        request: DescribeLoadBalancersRequest,
    ) -> Result<DescribeLoadBalancersResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeLoadBalancers".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());

        if let Some(ref arns) = request.load_balancer_arns {
            for (i, arn) in arns.iter().enumerate() {
                form_data.insert(format!("LoadBalancerArns.member.{}", i + 1), arn.clone());
            }
        }

        if let Some(ref names) = request.names {
            for (i, name) in names.iter().enumerate() {
                form_data.insert(format!("Names.member.{}", i + 1), name.clone());
            }
        }

        if let Some(ref marker) = request.marker {
            form_data.insert("Marker".to_string(), marker.clone());
        }

        if let Some(page_size) = request.page_size {
            form_data.insert("PageSize".to_string(), page_size.to_string());
        }

        self.send_form(form_data, "DescribeLoadBalancers", "LoadBalancer")
            .await
    }

    async fn delete_load_balancer(&self, load_balancer_arn: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteLoadBalancer".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("LoadBalancerArn".to_string(), load_balancer_arn.to_string());

        self.send_form_no_body(form_data, "DeleteLoadBalancer", load_balancer_arn)
            .await
    }

    // ---------------------------------------------------------------------------
    // Target Group Operations
    // ---------------------------------------------------------------------------

    async fn create_target_group(
        &self,
        request: CreateTargetGroupRequest,
    ) -> Result<CreateTargetGroupResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateTargetGroup".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("Name".to_string(), request.name.clone());

        if let Some(ref protocol) = request.protocol {
            form_data.insert("Protocol".to_string(), protocol.clone());
        }

        if let Some(ref protocol_version) = request.protocol_version {
            form_data.insert("ProtocolVersion".to_string(), protocol_version.clone());
        }

        if let Some(port) = request.port {
            form_data.insert("Port".to_string(), port.to_string());
        }

        if let Some(ref vpc_id) = request.vpc_id {
            form_data.insert("VpcId".to_string(), vpc_id.clone());
        }

        if let Some(ref health_check_protocol) = request.health_check_protocol {
            form_data.insert(
                "HealthCheckProtocol".to_string(),
                health_check_protocol.clone(),
            );
        }

        if let Some(ref health_check_port) = request.health_check_port {
            form_data.insert("HealthCheckPort".to_string(), health_check_port.clone());
        }

        if let Some(health_check_enabled) = request.health_check_enabled {
            form_data.insert(
                "HealthCheckEnabled".to_string(),
                health_check_enabled.to_string(),
            );
        }

        if let Some(ref health_check_path) = request.health_check_path {
            form_data.insert("HealthCheckPath".to_string(), health_check_path.clone());
        }

        if let Some(health_check_interval_seconds) = request.health_check_interval_seconds {
            form_data.insert(
                "HealthCheckIntervalSeconds".to_string(),
                health_check_interval_seconds.to_string(),
            );
        }

        if let Some(health_check_timeout_seconds) = request.health_check_timeout_seconds {
            form_data.insert(
                "HealthCheckTimeoutSeconds".to_string(),
                health_check_timeout_seconds.to_string(),
            );
        }

        if let Some(healthy_threshold_count) = request.healthy_threshold_count {
            form_data.insert(
                "HealthyThresholdCount".to_string(),
                healthy_threshold_count.to_string(),
            );
        }

        if let Some(unhealthy_threshold_count) = request.unhealthy_threshold_count {
            form_data.insert(
                "UnhealthyThresholdCount".to_string(),
                unhealthy_threshold_count.to_string(),
            );
        }

        if let Some(ref matcher) = request.matcher {
            if let Some(ref http_code) = matcher.http_code {
                form_data.insert("Matcher.HttpCode".to_string(), http_code.clone());
            }
            if let Some(ref grpc_code) = matcher.grpc_code {
                form_data.insert("Matcher.GrpcCode".to_string(), grpc_code.clone());
            }
        }

        if let Some(ref target_type) = request.target_type {
            form_data.insert("TargetType".to_string(), target_type.clone());
        }

        if let Some(ref ip_address_type) = request.ip_address_type {
            form_data.insert("IpAddressType".to_string(), ip_address_type.clone());
        }

        if let Some(ref tags) = request.tags {
            Self::add_tags(&mut form_data, tags);
        }

        self.send_form(form_data, "CreateTargetGroup", &request.name)
            .await
    }

    async fn describe_target_groups(
        &self,
        request: DescribeTargetGroupsRequest,
    ) -> Result<DescribeTargetGroupsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeTargetGroups".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());

        if let Some(ref lb_arn) = request.load_balancer_arn {
            form_data.insert("LoadBalancerArn".to_string(), lb_arn.clone());
        }

        if let Some(ref arns) = request.target_group_arns {
            for (i, arn) in arns.iter().enumerate() {
                form_data.insert(format!("TargetGroupArns.member.{}", i + 1), arn.clone());
            }
        }

        if let Some(ref names) = request.names {
            for (i, name) in names.iter().enumerate() {
                form_data.insert(format!("Names.member.{}", i + 1), name.clone());
            }
        }

        if let Some(ref marker) = request.marker {
            form_data.insert("Marker".to_string(), marker.clone());
        }

        if let Some(page_size) = request.page_size {
            form_data.insert("PageSize".to_string(), page_size.to_string());
        }

        self.send_form(form_data, "DescribeTargetGroups", "TargetGroup")
            .await
    }

    async fn modify_target_group(
        &self,
        request: ModifyTargetGroupRequest,
    ) -> Result<ModifyTargetGroupResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ModifyTargetGroup".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        if let Some(ref health_check_protocol) = request.health_check_protocol {
            form_data.insert(
                "HealthCheckProtocol".to_string(),
                health_check_protocol.clone(),
            );
        }

        if let Some(ref health_check_port) = request.health_check_port {
            form_data.insert("HealthCheckPort".to_string(), health_check_port.clone());
        }

        if let Some(ref health_check_path) = request.health_check_path {
            form_data.insert("HealthCheckPath".to_string(), health_check_path.clone());
        }

        if let Some(health_check_enabled) = request.health_check_enabled {
            form_data.insert(
                "HealthCheckEnabled".to_string(),
                health_check_enabled.to_string(),
            );
        }

        if let Some(health_check_interval_seconds) = request.health_check_interval_seconds {
            form_data.insert(
                "HealthCheckIntervalSeconds".to_string(),
                health_check_interval_seconds.to_string(),
            );
        }

        if let Some(health_check_timeout_seconds) = request.health_check_timeout_seconds {
            form_data.insert(
                "HealthCheckTimeoutSeconds".to_string(),
                health_check_timeout_seconds.to_string(),
            );
        }

        if let Some(healthy_threshold_count) = request.healthy_threshold_count {
            form_data.insert(
                "HealthyThresholdCount".to_string(),
                healthy_threshold_count.to_string(),
            );
        }

        if let Some(unhealthy_threshold_count) = request.unhealthy_threshold_count {
            form_data.insert(
                "UnhealthyThresholdCount".to_string(),
                unhealthy_threshold_count.to_string(),
            );
        }

        if let Some(ref matcher) = request.matcher {
            if let Some(ref http_code) = matcher.http_code {
                form_data.insert("Matcher.HttpCode".to_string(), http_code.clone());
            }
        }

        self.send_form(form_data, "ModifyTargetGroup", &request.target_group_arn)
            .await
    }

    async fn delete_target_group(&self, target_group_arn: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteTargetGroup".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("TargetGroupArn".to_string(), target_group_arn.to_string());

        self.send_form_no_body(form_data, "DeleteTargetGroup", target_group_arn)
            .await
    }

    // ---------------------------------------------------------------------------
    // Target Operations
    // ---------------------------------------------------------------------------

    async fn register_targets(&self, request: RegisterTargetsRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "RegisterTargets".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        for (i, target) in request.targets.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Targets.member.{}.Id", idx), target.id.clone());
            if let Some(port) = target.port {
                form_data.insert(format!("Targets.member.{}.Port", idx), port.to_string());
            }
            if let Some(ref az) = target.availability_zone {
                form_data.insert(
                    format!("Targets.member.{}.AvailabilityZone", idx),
                    az.clone(),
                );
            }
        }

        self.send_form_no_body(form_data, "RegisterTargets", &request.target_group_arn)
            .await
    }

    async fn deregister_targets(&self, request: DeregisterTargetsRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeregisterTargets".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        for (i, target) in request.targets.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Targets.member.{}.Id", idx), target.id.clone());
            if let Some(port) = target.port {
                form_data.insert(format!("Targets.member.{}.Port", idx), port.to_string());
            }
        }

        self.send_form_no_body(form_data, "DeregisterTargets", &request.target_group_arn)
            .await
    }

    async fn describe_target_health(
        &self,
        request: DescribeTargetHealthRequest,
    ) -> Result<DescribeTargetHealthResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeTargetHealth".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "TargetGroupArn".to_string(),
            request.target_group_arn.clone(),
        );

        if let Some(ref targets) = request.targets {
            for (i, target) in targets.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(format!("Targets.member.{}.Id", idx), target.id.clone());
                if let Some(port) = target.port {
                    form_data.insert(format!("Targets.member.{}.Port", idx), port.to_string());
                }
            }
        }

        self.send_form(form_data, "DescribeTargetHealth", &request.target_group_arn)
            .await
    }

    // ---------------------------------------------------------------------------
    // Listener Operations
    // ---------------------------------------------------------------------------

    async fn create_listener(
        &self,
        request: CreateListenerRequest,
    ) -> Result<CreateListenerResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateListener".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert(
            "LoadBalancerArn".to_string(),
            request.load_balancer_arn.clone(),
        );
        form_data.insert("Port".to_string(), request.port.to_string());
        form_data.insert("Protocol".to_string(), request.protocol.clone());

        // Add default actions
        for (i, action) in request.default_actions.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(
                format!("DefaultActions.member.{}.Type", idx),
                action.action_type.clone(),
            );

            if let Some(ref tg_arn) = action.target_group_arn {
                form_data.insert(
                    format!("DefaultActions.member.{}.TargetGroupArn", idx),
                    tg_arn.clone(),
                );
            }

            if let Some(order) = action.order {
                form_data.insert(
                    format!("DefaultActions.member.{}.Order", idx),
                    order.to_string(),
                );
            }

            if let Some(ref forward_config) = action.forward_config {
                if let Some(ref target_groups) = forward_config.target_groups {
                    for (j, tg) in target_groups.iter().enumerate() {
                        let tg_idx = j + 1;
                        form_data.insert(
                            format!("DefaultActions.member.{}.ForwardConfig.TargetGroups.member.{}.TargetGroupArn", idx, tg_idx),
                            tg.target_group_arn.clone(),
                        );
                        if let Some(weight) = tg.weight {
                            form_data.insert(
                                format!("DefaultActions.member.{}.ForwardConfig.TargetGroups.member.{}.Weight", idx, tg_idx),
                                weight.to_string(),
                            );
                        }
                    }
                }
            }

            if let Some(ref redirect_config) = action.redirect_config {
                form_data.insert(
                    format!("DefaultActions.member.{}.RedirectConfig.StatusCode", idx),
                    redirect_config.status_code.clone(),
                );
                if let Some(ref protocol) = redirect_config.protocol {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Protocol", idx),
                        protocol.clone(),
                    );
                }
                if let Some(ref port) = redirect_config.port {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Port", idx),
                        port.clone(),
                    );
                }
                if let Some(ref host) = redirect_config.host {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Host", idx),
                        host.clone(),
                    );
                }
                if let Some(ref path) = redirect_config.path {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Path", idx),
                        path.clone(),
                    );
                }
                if let Some(ref query) = redirect_config.query {
                    form_data.insert(
                        format!("DefaultActions.member.{}.RedirectConfig.Query", idx),
                        query.clone(),
                    );
                }
            }

            if let Some(ref fixed_response) = action.fixed_response_config {
                form_data.insert(
                    format!(
                        "DefaultActions.member.{}.FixedResponseConfig.StatusCode",
                        idx
                    ),
                    fixed_response.status_code.clone(),
                );
                if let Some(ref content_type) = fixed_response.content_type {
                    form_data.insert(
                        format!(
                            "DefaultActions.member.{}.FixedResponseConfig.ContentType",
                            idx
                        ),
                        content_type.clone(),
                    );
                }
                if let Some(ref message_body) = fixed_response.message_body {
                    form_data.insert(
                        format!(
                            "DefaultActions.member.{}.FixedResponseConfig.MessageBody",
                            idx
                        ),
                        message_body.clone(),
                    );
                }
            }
        }

        if let Some(ref ssl_policy) = request.ssl_policy {
            form_data.insert("SslPolicy".to_string(), ssl_policy.clone());
        }

        if let Some(ref certificates) = request.certificates {
            for (i, cert) in certificates.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("Certificates.member.{}.CertificateArn", idx),
                    cert.certificate_arn.clone(),
                );
            }
        }

        if let Some(ref alpn_policy) = request.alpn_policy {
            for (i, policy) in alpn_policy.iter().enumerate() {
                form_data.insert(format!("AlpnPolicy.member.{}", i + 1), policy.clone());
            }
        }

        if let Some(ref tags) = request.tags {
            Self::add_tags(&mut form_data, tags);
        }

        self.send_form(form_data, "CreateListener", &request.load_balancer_arn)
            .await
    }

    async fn describe_listeners(
        &self,
        request: DescribeListenersRequest,
    ) -> Result<DescribeListenersResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeListeners".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());

        if let Some(ref lb_arn) = request.load_balancer_arn {
            form_data.insert("LoadBalancerArn".to_string(), lb_arn.clone());
        }

        if let Some(ref listener_arns) = request.listener_arns {
            for (i, arn) in listener_arns.iter().enumerate() {
                form_data.insert(format!("ListenerArns.member.{}", i + 1), arn.clone());
            }
        }

        if let Some(ref marker) = request.marker {
            form_data.insert("Marker".to_string(), marker.clone());
        }

        if let Some(page_size) = request.page_size {
            form_data.insert("PageSize".to_string(), page_size.to_string());
        }

        self.send_form(form_data, "DescribeListeners", "Listener")
            .await
    }

    async fn modify_listener(
        &self,
        request: ModifyListenerRequest,
    ) -> Result<ModifyListenerResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ModifyListener".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("ListenerArn".to_string(), request.listener_arn.clone());

        if let Some(port) = request.port {
            form_data.insert("Port".to_string(), port.to_string());
        }

        if let Some(ref protocol) = request.protocol {
            form_data.insert("Protocol".to_string(), protocol.clone());
        }

        if let Some(ref ssl_policy) = request.ssl_policy {
            form_data.insert("SslPolicy".to_string(), ssl_policy.clone());
        }

        if let Some(ref certificates) = request.certificates {
            for (i, cert) in certificates.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("Certificates.member.{}.CertificateArn", idx),
                    cert.certificate_arn.clone(),
                );
            }
        }

        if let Some(ref default_actions) = request.default_actions {
            for (i, action) in default_actions.iter().enumerate() {
                let idx = i + 1;
                form_data.insert(
                    format!("DefaultActions.member.{}.Type", idx),
                    action.action_type.clone(),
                );
                if let Some(ref tg_arn) = action.target_group_arn {
                    form_data.insert(
                        format!("DefaultActions.member.{}.TargetGroupArn", idx),
                        tg_arn.clone(),
                    );
                }
            }
        }

        self.send_form(form_data, "ModifyListener", &request.listener_arn)
            .await
    }

    async fn delete_listener(&self, listener_arn: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteListener".to_string());
        form_data.insert("Version".to_string(), "2015-12-01".to_string());
        form_data.insert("ListenerArn".to_string(), listener_arn.to_string());

        self.send_form_no_body(form_data, "DeleteListener", listener_arn)
            .await
    }
}

// ---------------------------------------------------------------------------
// Common Types
// ---------------------------------------------------------------------------

/// A tag for an ELB resource.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ElbTag {
    /// The tag key.
    pub key: String,
    /// The tag value.
    pub value: String,
}

/// A subnet mapping for a load balancer.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct SubnetMapping {
    /// The subnet ID.
    pub subnet_id: String,
    /// The allocation ID of the Elastic IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_id: Option<String>,
    /// The private IPv4 address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ipv4_address: Option<String>,
}

/// A target description.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TargetDescription {
    /// The ID of the target (instance ID, IP address, or Lambda ARN).
    pub id: String,
    /// The port on which the target is listening.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// The Availability Zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
}

/// A certificate for a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Certificate {
    /// The ARN of the certificate.
    pub certificate_arn: String,
    /// Whether this is the default certificate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

/// A matcher for health checks.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Matcher {
    /// The HTTP codes for success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_code: Option<String>,
    /// The gRPC codes for success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grpc_code: Option<String>,
}

// ---------------------------------------------------------------------------
// Load Balancer Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a load balancer.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateLoadBalancerRequest {
    /// The name of the load balancer.
    pub name: String,
    /// The IDs of the subnets.
    pub subnets: Vec<String>,
    /// The subnet mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_mappings: Option<Vec<SubnetMapping>>,
    /// The IDs of the security groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_groups: Option<Vec<String>>,
    /// The scheme: internet-facing or internal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// The type: application, network, or gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_type: Option<String>,
    /// The IP address type: ipv4 or dualstack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address_type: Option<String>,
    /// Tags for the load balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ElbTag>>,
}

/// Response from creating a load balancer.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateLoadBalancerResponse {
    pub create_load_balancer_result: CreateLoadBalancerResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateLoadBalancerResult {
    pub load_balancers: Option<LoadBalancersWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancersWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<LoadBalancer>,
}

/// Request to describe load balancers.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeLoadBalancersRequest {
    /// The ARNs of the load balancers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_arns: Option<Vec<String>>,
    /// The names of the load balancers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    /// The marker for the next set of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// The maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
}

/// Response from describing load balancers.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeLoadBalancersResponse {
    pub describe_load_balancers_result: DescribeLoadBalancersResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeLoadBalancersResult {
    pub load_balancers: Option<LoadBalancersWrapper>,
    pub next_marker: Option<String>,
}

/// A load balancer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancer {
    pub load_balancer_arn: Option<String>,
    pub load_balancer_name: Option<String>,
    #[serde(rename = "DNSName")]
    pub dns_name: Option<String>,
    pub canonical_hosted_zone_id: Option<String>,
    pub created_time: Option<String>,
    pub scheme: Option<String>,
    pub state: Option<LoadBalancerState>,
    #[serde(rename = "Type")]
    pub load_balancer_type: Option<String>,
    pub availability_zones: Option<AvailabilityZonesWrapper>,
    pub security_groups: Option<SecurityGroupsWrapper>,
    pub ip_address_type: Option<String>,
    pub vpc_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerState {
    pub code: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AvailabilityZonesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<AvailabilityZone>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AvailabilityZone {
    pub zone_name: Option<String>,
    pub subnet_id: Option<String>,
    pub outpost_id: Option<String>,
    pub load_balancer_addresses: Option<LoadBalancerAddressesWrapper>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerAddressesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<LoadBalancerAddress>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerAddress {
    pub ip_address: Option<String>,
    pub allocation_id: Option<String>,
    pub private_i_pv4_address: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SecurityGroupsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}

// ---------------------------------------------------------------------------
// Target Group Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a target group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateTargetGroupRequest {
    /// The name of the target group.
    pub name: String,
    /// The protocol for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// The protocol version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    /// The port for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// The VPC ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_id: Option<String>,
    /// The health check protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_protocol: Option<String>,
    /// The health check port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_port: Option<String>,
    /// Whether health checks are enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_enabled: Option<bool>,
    /// The health check path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_path: Option<String>,
    /// The health check interval in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_interval_seconds: Option<i32>,
    /// The health check timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_timeout_seconds: Option<i32>,
    /// The number of consecutive successful health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold_count: Option<i32>,
    /// The number of consecutive failed health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold_count: Option<i32>,
    /// The matcher for health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<Matcher>,
    /// The target type: instance, ip, lambda, or alb.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_type: Option<String>,
    /// The IP address type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address_type: Option<String>,
    /// Tags for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ElbTag>>,
}

/// Response from creating a target group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateTargetGroupResponse {
    pub create_target_group_result: CreateTargetGroupResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateTargetGroupResult {
    pub target_groups: Option<TargetGroupsWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<TargetGroup>,
}

/// Request to describe target groups.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeTargetGroupsRequest {
    /// The ARN of the load balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_arn: Option<String>,
    /// The ARNs of the target groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_group_arns: Option<Vec<String>>,
    /// The names of the target groups.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    /// The marker for the next set of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// The maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
}

/// Response from describing target groups.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetGroupsResponse {
    pub describe_target_groups_result: DescribeTargetGroupsResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetGroupsResult {
    pub target_groups: Option<TargetGroupsWrapper>,
    pub next_marker: Option<String>,
}

/// Request to modify a target group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyTargetGroupRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The health check protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_protocol: Option<String>,
    /// The health check port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_port: Option<String>,
    /// The health check path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_path: Option<String>,
    /// Whether health checks are enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_enabled: Option<bool>,
    /// The health check interval in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_interval_seconds: Option<i32>,
    /// The health check timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_timeout_seconds: Option<i32>,
    /// The number of consecutive successful health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy_threshold_count: Option<i32>,
    /// The number of consecutive failed health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unhealthy_threshold_count: Option<i32>,
    /// The matcher for health checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<Matcher>,
}

/// Response from modifying a target group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyTargetGroupResponse {
    pub modify_target_group_result: ModifyTargetGroupResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyTargetGroupResult {
    pub target_groups: Option<TargetGroupsWrapper>,
}

/// A target group.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroup {
    pub target_group_arn: Option<String>,
    pub target_group_name: Option<String>,
    pub protocol: Option<String>,
    pub protocol_version: Option<String>,
    pub port: Option<i32>,
    pub vpc_id: Option<String>,
    pub health_check_protocol: Option<String>,
    pub health_check_port: Option<String>,
    pub health_check_enabled: Option<bool>,
    pub health_check_interval_seconds: Option<i32>,
    pub health_check_timeout_seconds: Option<i32>,
    pub healthy_threshold_count: Option<i32>,
    pub unhealthy_threshold_count: Option<i32>,
    pub health_check_path: Option<String>,
    pub matcher: Option<MatcherResponse>,
    pub load_balancer_arns: Option<LoadBalancerArnsWrapper>,
    pub target_type: Option<String>,
    pub ip_address_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MatcherResponse {
    pub http_code: Option<String>,
    pub grpc_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadBalancerArnsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}

// ---------------------------------------------------------------------------
// Target Operations Request/Response Types
// ---------------------------------------------------------------------------

/// Request to register targets.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RegisterTargetsRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The targets to register.
    pub targets: Vec<TargetDescription>,
}

/// Request to deregister targets.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DeregisterTargetsRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The targets to deregister.
    pub targets: Vec<TargetDescription>,
}

/// Request to describe target health.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DescribeTargetHealthRequest {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The targets to describe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<TargetDescription>>,
}

/// Response from describing target health.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetHealthResponse {
    pub describe_target_health_result: DescribeTargetHealthResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeTargetHealthResult {
    pub target_health_descriptions: Option<TargetHealthDescriptionsWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetHealthDescriptionsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<TargetHealthDescription>,
}

/// A target health description.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetHealthDescription {
    pub target: Option<TargetDescriptionResponse>,
    pub health_check_port: Option<String>,
    pub target_health: Option<TargetHealth>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetDescriptionResponse {
    pub id: Option<String>,
    pub port: Option<i32>,
    pub availability_zone: Option<String>,
}

/// The health of a target.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetHealth {
    pub state: Option<String>,
    pub reason: Option<String>,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Listener Request/Response Types
// ---------------------------------------------------------------------------

/// An action for a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Action {
    /// The action type.
    pub action_type: String,
    /// The ARN of the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_group_arn: Option<String>,
    /// The order for the action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    /// The forward configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_config: Option<ForwardActionConfig>,
    /// The redirect configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_config: Option<RedirectActionConfig>,
    /// The fixed response configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_response_config: Option<FixedResponseActionConfig>,
}

/// Forward action configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ForwardActionConfig {
    /// The target groups to forward to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_groups: Option<Vec<TargetGroupTuple>>,
    /// The target group stickiness configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_group_stickiness_config: Option<TargetGroupStickinessConfig>,
}

/// A target group tuple for forwarding.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TargetGroupTuple {
    /// The ARN of the target group.
    pub target_group_arn: String,
    /// The weight for the target group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<i32>,
}

/// Target group stickiness configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TargetGroupStickinessConfig {
    /// Whether stickiness is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The duration in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<i32>,
}

/// Redirect action configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RedirectActionConfig {
    /// The status code: HTTP_301 or HTTP_302.
    pub status_code: String,
    /// The protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// The port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// The host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The query string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Fixed response action configuration.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct FixedResponseActionConfig {
    /// The HTTP status code.
    pub status_code: String,
    /// The content type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// The message body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_body: Option<String>,
}

/// Request to create a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateListenerRequest {
    /// The ARN of the load balancer.
    pub load_balancer_arn: String,
    /// The port for the listener.
    pub port: i32,
    /// The protocol for the listener.
    pub protocol: String,
    /// The default actions.
    pub default_actions: Vec<Action>,
    /// The SSL policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_policy: Option<String>,
    /// The certificates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificates: Option<Vec<Certificate>>,
    /// The ALPN policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpn_policy: Option<Vec<String>>,
    /// Tags for the listener.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ElbTag>>,
}

/// Response from creating a listener.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateListenerResponse {
    pub create_listener_result: CreateListenerResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CreateListenerResult {
    pub listeners: Option<ListenersWrapper>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListenersWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<Listener>,
}

/// Request to describe listeners.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeListenersRequest {
    /// The ARN of the load balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_balancer_arn: Option<String>,
    /// The ARNs of the listeners.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listener_arns: Option<Vec<String>>,
    /// The marker for the next set of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    /// The maximum number of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
}

/// Response from describing listeners.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeListenersResponse {
    pub describe_listeners_result: DescribeListenersResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DescribeListenersResult {
    pub listeners: Option<ListenersWrapper>,
    pub next_marker: Option<String>,
}

/// Request to modify a listener.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyListenerRequest {
    /// The ARN of the listener.
    pub listener_arn: String,
    /// The new port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// The new protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// The new SSL policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssl_policy: Option<String>,
    /// The new certificates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificates: Option<Vec<Certificate>>,
    /// The new default actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_actions: Option<Vec<Action>>,
}

/// Response from modifying a listener.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyListenerResponse {
    pub modify_listener_result: ModifyListenerResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModifyListenerResult {
    pub listeners: Option<ListenersWrapper>,
}

/// A listener.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Listener {
    pub listener_arn: Option<String>,
    pub load_balancer_arn: Option<String>,
    pub port: Option<i32>,
    pub protocol: Option<String>,
    pub certificates: Option<CertificatesWrapper>,
    pub ssl_policy: Option<String>,
    pub default_actions: Option<ActionsWrapper>,
    pub alpn_policy: Option<AlpnPolicyWrapper>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CertificatesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<CertificateResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CertificateResponse {
    pub certificate_arn: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ActionsWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<ActionResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ActionResponse {
    #[serde(rename = "Type")]
    pub action_type: Option<String>,
    pub target_group_arn: Option<String>,
    pub order: Option<i32>,
    pub forward_config: Option<ForwardActionConfigResponse>,
    pub redirect_config: Option<RedirectActionConfigResponse>,
    pub fixed_response_config: Option<FixedResponseActionConfigResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ForwardActionConfigResponse {
    pub target_groups: Option<TargetGroupTuplesWrapper>,
    pub target_group_stickiness_config: Option<TargetGroupStickinessConfigResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupTuplesWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<TargetGroupTupleResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupTupleResponse {
    pub target_group_arn: Option<String>,
    pub weight: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TargetGroupStickinessConfigResponse {
    pub enabled: Option<bool>,
    pub duration_seconds: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RedirectActionConfigResponse {
    pub protocol: Option<String>,
    pub port: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub status_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FixedResponseActionConfigResponse {
    pub status_code: Option<String>,
    pub content_type: Option<String>,
    pub message_body: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AlpnPolicyWrapper {
    #[serde(rename = "member", default)]
    pub members: Vec<String>,
}
