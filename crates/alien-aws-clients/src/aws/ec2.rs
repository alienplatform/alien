//! AWS EC2 Client
//!
//! This module provides a client for interacting with AWS EC2 APIs, focused on
//! VPC networking operations including VPCs, Subnets, Internet Gateways, NAT Gateways,
//! Route Tables, Security Groups, and Elastic IPs.
//!
//! # Example
//!
//! ```rust,ignore
//! use alien_aws_clients::ec2::{Ec2Client, Ec2Api, CreateVpcRequest};
//! use reqwest::Client;
//!
//! let ec2_client = Ec2Client::new(Client::new(), aws_config);
//! let vpc = ec2_client.create_vpc(
//!     CreateVpcRequest::builder()
//!         .cidr_block("10.0.0.0/16".to_string())
//!         .build()
//! ).await?;
//! ```

use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};

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
// EC2 Error Response Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Ec2ErrorResponse {
    pub errors: Ec2ErrorsWrapper,
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Ec2ErrorsWrapper {
    #[serde(rename = "Error")]
    pub error: Ec2ErrorDetails,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Ec2ErrorDetails {
    pub code: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// EC2 API Trait
// ---------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait Ec2Api: Send + Sync + std::fmt::Debug {
    // VPC Operations
    async fn describe_vpcs(&self, request: DescribeVpcsRequest) -> Result<DescribeVpcsResponse>;
    async fn describe_vpc_attribute(
        &self,
        request: DescribeVpcAttributeRequest,
    ) -> Result<DescribeVpcAttributeResponse>;
    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse>;
    async fn delete_vpc(&self, vpc_id: &str) -> Result<()>;
    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()>;

    // Subnet Operations
    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse>;
    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse>;
    async fn delete_subnet(&self, subnet_id: &str) -> Result<()>;

    // Internet Gateway Operations
    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse>;
    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()>;
    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()>;
    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()>;
    async fn describe_internet_gateways(
        &self,
        request: DescribeInternetGatewaysRequest,
    ) -> Result<DescribeInternetGatewaysResponse>;

    // NAT Gateway Operations
    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse>;
    async fn delete_nat_gateway(&self, nat_gateway_id: &str) -> Result<DeleteNatGatewayResponse>;
    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse>;

    // Elastic IP Operations
    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse>;
    async fn release_address(&self, allocation_id: &str) -> Result<()>;

    // Route Table Operations
    async fn describe_route_tables(
        &self,
        request: DescribeRouteTablesRequest,
    ) -> Result<DescribeRouteTablesResponse>;
    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse>;
    async fn delete_route_table(&self, route_table_id: &str) -> Result<()>;
    async fn create_route(&self, request: CreateRouteRequest) -> Result<()>;
    async fn delete_route(&self, request: DeleteRouteRequest) -> Result<()>;
    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse>;
    async fn disassociate_route_table(&self, association_id: &str) -> Result<()>;

    // Security Group Operations
    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse>;
    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse>;
    async fn delete_security_group(&self, group_id: &str) -> Result<()>;
    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()>;
    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()>;
    async fn revoke_security_group_ingress(
        &self,
        request: RevokeSecurityGroupIngressRequest,
    ) -> Result<()>;
    async fn revoke_security_group_egress(
        &self,
        request: RevokeSecurityGroupEgressRequest,
    ) -> Result<()>;

    // Availability Zone Operations
    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse>;

    // AMI Operations
    async fn describe_images(
        &self,
        request: DescribeImagesRequest,
    ) -> Result<DescribeImagesResponse>;

    // Instance Operations
    async fn terminate_instances(
        &self,
        instance_ids: Vec<String>,
    ) -> Result<TerminateInstancesResponse>;
    async fn describe_instances(
        &self,
        request: DescribeInstancesRequest,
    ) -> Result<DescribeInstancesResponse>;

    // Volume Operations
    async fn create_volume(&self, request: CreateVolumeRequest) -> Result<CreateVolumeResponse>;
    async fn delete_volume(&self, volume_id: &str) -> Result<()>;
    async fn describe_volumes(
        &self,
        request: DescribeVolumesRequest,
    ) -> Result<DescribeVolumesResponse>;
    async fn attach_volume(&self, request: AttachVolumeRequest) -> Result<AttachVolumeResponse>;
    async fn detach_volume(&self, request: DetachVolumeRequest) -> Result<DetachVolumeResponse>;

    // Launch Template Operations
    async fn create_launch_template(
        &self,
        request: CreateLaunchTemplateRequest,
    ) -> Result<CreateLaunchTemplateResponse>;
    /// Creates a new version of an existing launch template with updated launch template data.
    /// ASGs using $Latest will automatically pick up the new version.
    /// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_CreateLaunchTemplateVersion.html
    async fn create_launch_template_version(
        &self,
        request: CreateLaunchTemplateVersionRequest,
    ) -> Result<CreateLaunchTemplateVersionResponse>;
    async fn delete_launch_template(
        &self,
        request: DeleteLaunchTemplateRequest,
    ) -> Result<DeleteLaunchTemplateResponse>;
    async fn describe_launch_templates(
        &self,
        request: DescribeLaunchTemplatesRequest,
    ) -> Result<DescribeLaunchTemplatesResponse>;

    // Console Output
    /// Gets the console output for an EC2 instance (base64-encoded).
    /// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_GetConsoleOutput.html
    async fn get_console_output(&self, instance_id: String) -> Result<GetConsoleOutputResponse>;
}

// ---------------------------------------------------------------------------
// EC2 Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Ec2Client {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl Ec2Client {
    /// Create a new EC2 client.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "ec2".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn get_base_url(&self) -> String {
        if let Some(override_url) = self.credentials.get_service_endpoint_option("ec2") {
            override_url.to_string()
        } else {
            format!("https://ec2.{}.amazonaws.com", self.credentials.region())
        }
    }

    fn get_host(&self) -> String {
        format!("ec2.{}.amazonaws.com", self.credentials.region())
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
                        Self::map_ec2_error(status, text, operation, resource, request_body)
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

    fn map_ec2_error(
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
                    resource_type: "EC2 Resource".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                    message: "Resource conflict".into(),
                    resource_type: "EC2 Resource".into(),
                    resource_name: resource.into(),
                }),
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                    Some(ErrorData::RemoteAccessDenied {
                        resource_type: "EC2 Resource".into(),
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

        // Try to parse EC2 error XML
        let parsed: std::result::Result<Ec2ErrorResponse, _> = quick_xml::de::from_str(body);
        let (code, message) = match parsed {
            Ok(e) => (e.errors.error.code, e.errors.error.message),
            Err(_) => {
                // If we can't parse the response, fall back to status code mapping
                let default_message = "Unknown error".to_string();
                return match status {
                    StatusCode::NOT_FOUND => Some(ErrorData::RemoteResourceNotFound {
                        resource_type: "EC2 Resource".into(),
                        resource_name: resource.into(),
                    }),
                    StatusCode::CONFLICT => Some(ErrorData::RemoteResourceConflict {
                        message: default_message,
                        resource_type: "EC2 Resource".into(),
                        resource_name: resource.into(),
                    }),
                    StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                        Some(ErrorData::RemoteAccessDenied {
                            resource_type: "EC2 Resource".into(),
                            resource_name: resource.into(),
                        })
                    }
                    StatusCode::TOO_MANY_REQUESTS => Some(ErrorData::RateLimitExceeded {
                        message: default_message,
                    }),
                    StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::BAD_GATEWAY
                    | StatusCode::GATEWAY_TIMEOUT => Some(ErrorData::RemoteServiceUnavailable {
                        message: default_message,
                    }),
                    _ => None,
                };
            }
        };

        // Map EC2 error codes to our error types
        // Reference: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/errors-overview.html
        Some(match code.as_str() {
            // Access / Auth errors
            "AuthFailure" | "UnauthorizedOperation" | "Blocked" => ErrorData::RemoteAccessDenied {
                resource_type: "EC2 Resource".into(),
                resource_name: resource.into(),
            },
            // Throttling
            "RequestLimitExceeded" | "ResourceLimitExceeded" => {
                ErrorData::RateLimitExceeded { message }
            }
            // Service unavailable
            "ServiceUnavailable" | "Unavailable" | "InternalError" | "InternalFailure" => {
                ErrorData::RemoteServiceUnavailable { message }
            }
            // Resource not found errors
            "InvalidVpcID.NotFound" | "InvalidVpc.NotFound" => ErrorData::RemoteResourceNotFound {
                resource_type: "VPC".into(),
                resource_name: resource.into(),
            },
            "InvalidSubnetID.NotFound" | "InvalidSubnet.NotFound" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "Subnet".into(),
                    resource_name: resource.into(),
                }
            }
            "InvalidInternetGatewayID.NotFound" | "InvalidInternetGateway.NotFound" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "InternetGateway".into(),
                    resource_name: resource.into(),
                }
            }
            "InvalidNatGatewayID.NotFound" | "NatGatewayNotFound" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "NatGateway".into(),
                    resource_name: resource.into(),
                }
            }
            "InvalidRouteTableID.NotFound" | "InvalidRouteTableId.NotFound" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "RouteTable".into(),
                    resource_name: resource.into(),
                }
            }
            "InvalidGroup.NotFound" | "InvalidSecurityGroupID.NotFound" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "SecurityGroup".into(),
                    resource_name: resource.into(),
                }
            }
            "InvalidAllocationID.NotFound" | "InvalidAddress.NotFound" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "ElasticIP".into(),
                    resource_name: resource.into(),
                }
            }
            "InvalidAssociationID.NotFound" => ErrorData::RemoteResourceNotFound {
                resource_type: "RouteTableAssociation".into(),
                resource_name: resource.into(),
            },
            // Conflict / already exists errors
            "VpcLimitExceeded"
            | "SubnetLimitExceeded"
            | "SecurityGroupLimitExceeded"
            | "AddressLimitExceeded" => ErrorData::QuotaExceeded { message },
            "InvalidVpcState" | "InvalidSubnetID.DuplicateSubnet" | "InvalidGroup.Duplicate" => {
                ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "EC2 Resource".into(),
                    resource_name: resource.into(),
                }
            }
            "DependencyViolation" | "ResourceInUse" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "EC2 Resource".into(),
                resource_name: resource.into(),
            },
            "Gateway.NotAttached" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "InternetGateway".into(),
                resource_name: resource.into(),
            },
            "RouteAlreadyExists" => ErrorData::RemoteResourceConflict {
                message,
                resource_type: "Route".into(),
                resource_name: resource.into(),
            },
            // Invalid input errors
            "InvalidParameterValue" | "InvalidParameter" | "MissingParameter" | "InvalidInput" => {
                ErrorData::InvalidInput {
                    message,
                    field_name: None,
                }
            }
            "InvalidVpcRange" | "InvalidSubnetRange.Conflict" | "InvalidCIDRBlock" => {
                ErrorData::InvalidInput {
                    message,
                    field_name: Some("cidr_block".into()),
                }
            }
            // Default fallback based on status code
            _ => match status {
                StatusCode::NOT_FOUND => ErrorData::RemoteResourceNotFound {
                    resource_type: "EC2 Resource".into(),
                    resource_name: resource.into(),
                },
                StatusCode::CONFLICT => ErrorData::RemoteResourceConflict {
                    message,
                    resource_type: "EC2 Resource".into(),
                    resource_name: resource.into(),
                },
                StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => ErrorData::RemoteAccessDenied {
                    resource_type: "EC2 Resource".into(),
                    resource_name: resource.into(),
                },
                StatusCode::TOO_MANY_REQUESTS => ErrorData::RateLimitExceeded { message },
                StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::BAD_GATEWAY
                | StatusCode::GATEWAY_TIMEOUT => ErrorData::RemoteServiceUnavailable { message },
                _ => ErrorData::HttpResponseError {
                    message: format!("EC2 operation failed: {}", message),
                    url: "ec2.amazonaws.com".into(),
                    http_status: status.as_u16(),
                    http_response_text: Some(body.into()),
                    http_request_text: request_body.map(|s| s.to_string()),
                },
            },
        })
    }

    /// Add filter parameters to the form data.
    fn add_filters(form_data: &mut HashMap<String, String>, filters: &[Filter]) {
        for (i, filter) in filters.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(format!("Filter.{}.Name", idx), filter.name.clone());
            for (j, value) in filter.values.iter().enumerate() {
                form_data.insert(format!("Filter.{}.Value.{}", idx, j + 1), value.clone());
            }
        }
    }

    /// Add tag specifications to the form data.
    fn add_tag_specifications(
        form_data: &mut HashMap<String, String>,
        tag_specs: &[TagSpecification],
    ) {
        for (i, spec) in tag_specs.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(
                format!("TagSpecification.{}.ResourceType", idx),
                spec.resource_type.clone(),
            );
            for (j, tag) in spec.tags.iter().enumerate() {
                form_data.insert(
                    format!("TagSpecification.{}.Tag.{}.Key", idx, j + 1),
                    tag.key.clone(),
                );
                form_data.insert(
                    format!("TagSpecification.{}.Tag.{}.Value", idx, j + 1),
                    tag.value.clone(),
                );
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Ec2Api for Ec2Client {
    // ---------------------------------------------------------------------------
    // VPC Operations
    // ---------------------------------------------------------------------------

    async fn describe_vpcs(&self, request: DescribeVpcsRequest) -> Result<DescribeVpcsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeVpcs".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(vpc_ids) = &request.vpc_ids {
            for (i, vpc_id) in vpc_ids.iter().enumerate() {
                form_data.insert(format!("VpcId.{}", i + 1), vpc_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeVpcs", "VPC").await
    }

    async fn describe_vpc_attribute(
        &self,
        request: DescribeVpcAttributeRequest,
    ) -> Result<DescribeVpcAttributeResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeVpcAttribute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());
        form_data.insert("Attribute".to_string(), request.attribute.clone());

        self.send_form(form_data, "DescribeVpcAttribute", &request.vpc_id)
            .await
    }

    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateVpc".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("CidrBlock".to_string(), request.cidr_block.clone());

        if let Some(instance_tenancy) = &request.instance_tenancy {
            form_data.insert("InstanceTenancy".to_string(), instance_tenancy.clone());
        }

        if let Some(amazon_provided_ipv6_cidr_block) = request.amazon_provided_ipv6_cidr_block {
            form_data.insert(
                "AmazonProvidedIpv6CidrBlock".to_string(),
                amazon_provided_ipv6_cidr_block.to_string(),
            );
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateVpc", &request.cidr_block)
            .await
    }

    async fn delete_vpc(&self, vpc_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteVpc".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), vpc_id.to_string());

        self.send_form_no_body(form_data, "DeleteVpc", vpc_id).await
    }

    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ModifyVpcAttribute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        if let Some(enable_dns_support) = request.enable_dns_support {
            form_data.insert(
                "EnableDnsSupport.Value".to_string(),
                enable_dns_support.to_string(),
            );
        }

        if let Some(enable_dns_hostnames) = request.enable_dns_hostnames {
            form_data.insert(
                "EnableDnsHostnames.Value".to_string(),
                enable_dns_hostnames.to_string(),
            );
        }

        self.send_form_no_body(form_data, "ModifyVpcAttribute", &request.vpc_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Subnet Operations
    // ---------------------------------------------------------------------------

    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeSubnets".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(subnet_ids) = &request.subnet_ids {
            for (i, subnet_id) in subnet_ids.iter().enumerate() {
                form_data.insert(format!("SubnetId.{}", i + 1), subnet_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeSubnets", "Subnet").await
    }

    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateSubnet".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());
        form_data.insert("CidrBlock".to_string(), request.cidr_block.clone());

        if let Some(availability_zone) = &request.availability_zone {
            form_data.insert("AvailabilityZone".to_string(), availability_zone.clone());
        }

        if let Some(availability_zone_id) = &request.availability_zone_id {
            form_data.insert(
                "AvailabilityZoneId".to_string(),
                availability_zone_id.clone(),
            );
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateSubnet", &request.cidr_block)
            .await
    }

    async fn delete_subnet(&self, subnet_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteSubnet".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("SubnetId".to_string(), subnet_id.to_string());

        self.send_form_no_body(form_data, "DeleteSubnet", subnet_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Internet Gateway Operations
    // ---------------------------------------------------------------------------

    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateInternetGateway", "InternetGateway")
            .await
    }

    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "InternetGatewayId".to_string(),
            internet_gateway_id.to_string(),
        );

        self.send_form_no_body(form_data, "DeleteInternetGateway", internet_gateway_id)
            .await
    }

    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AttachInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "InternetGatewayId".to_string(),
            request.internet_gateway_id.clone(),
        );
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        self.send_form_no_body(
            form_data,
            "AttachInternetGateway",
            &request.internet_gateway_id,
        )
        .await
    }

    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DetachInternetGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "InternetGatewayId".to_string(),
            request.internet_gateway_id.clone(),
        );
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        self.send_form_no_body(
            form_data,
            "DetachInternetGateway",
            &request.internet_gateway_id,
        )
        .await
    }

    async fn describe_internet_gateways(
        &self,
        request: DescribeInternetGatewaysRequest,
    ) -> Result<DescribeInternetGatewaysResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeInternetGateways".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(igw_ids) = &request.internet_gateway_ids {
            for (i, igw_id) in igw_ids.iter().enumerate() {
                form_data.insert(format!("InternetGatewayId.{}", i + 1), igw_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeInternetGateways", "InternetGateway")
            .await
    }

    // ---------------------------------------------------------------------------
    // NAT Gateway Operations
    // ---------------------------------------------------------------------------

    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateNatGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("SubnetId".to_string(), request.subnet_id.clone());

        if let Some(allocation_id) = &request.allocation_id {
            form_data.insert("AllocationId".to_string(), allocation_id.clone());
        }

        if let Some(connectivity_type) = &request.connectivity_type {
            form_data.insert("ConnectivityType".to_string(), connectivity_type.clone());
        }

        if let Some(private_ip_address) = &request.private_ip_address {
            form_data.insert("PrivateIpAddress".to_string(), private_ip_address.clone());
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateNatGateway", &request.subnet_id)
            .await
    }

    async fn delete_nat_gateway(&self, nat_gateway_id: &str) -> Result<DeleteNatGatewayResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteNatGateway".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("NatGatewayId".to_string(), nat_gateway_id.to_string());

        self.send_form(form_data, "DeleteNatGateway", nat_gateway_id)
            .await
    }

    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeNatGateways".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(nat_gateway_ids) = &request.nat_gateway_ids {
            for (i, nat_id) in nat_gateway_ids.iter().enumerate() {
                form_data.insert(format!("NatGatewayId.{}", i + 1), nat_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeNatGateways", "NatGateway")
            .await
    }

    // ---------------------------------------------------------------------------
    // Elastic IP Operations
    // ---------------------------------------------------------------------------

    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AllocateAddress".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        // Default to VPC domain
        let domain = request.domain.as_deref().unwrap_or("vpc");
        form_data.insert("Domain".to_string(), domain.to_string());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "AllocateAddress", "ElasticIP")
            .await
    }

    async fn release_address(&self, allocation_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ReleaseAddress".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("AllocationId".to_string(), allocation_id.to_string());

        self.send_form_no_body(form_data, "ReleaseAddress", allocation_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Route Table Operations
    // ---------------------------------------------------------------------------

    async fn describe_route_tables(
        &self,
        request: DescribeRouteTablesRequest,
    ) -> Result<DescribeRouteTablesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeRouteTables".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(rt_ids) = &request.route_table_ids {
            for (i, rt_id) in rt_ids.iter().enumerate() {
                form_data.insert(format!("RouteTableId.{}", i + 1), rt_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeRouteTables", "RouteTable")
            .await
    }

    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateRouteTable", &request.vpc_id)
            .await
    }

    async fn delete_route_table(&self, route_table_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), route_table_id.to_string());

        self.send_form_no_body(form_data, "DeleteRouteTable", route_table_id)
            .await
    }

    async fn create_route(&self, request: CreateRouteRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateRoute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), request.route_table_id.clone());
        form_data.insert(
            "DestinationCidrBlock".to_string(),
            request.destination_cidr_block.clone(),
        );

        if let Some(gateway_id) = &request.gateway_id {
            form_data.insert("GatewayId".to_string(), gateway_id.clone());
        }

        if let Some(nat_gateway_id) = &request.nat_gateway_id {
            form_data.insert("NatGatewayId".to_string(), nat_gateway_id.clone());
        }

        if let Some(instance_id) = &request.instance_id {
            form_data.insert("InstanceId".to_string(), instance_id.clone());
        }

        if let Some(network_interface_id) = &request.network_interface_id {
            form_data.insert(
                "NetworkInterfaceId".to_string(),
                network_interface_id.clone(),
            );
        }

        if let Some(vpc_peering_connection_id) = &request.vpc_peering_connection_id {
            form_data.insert(
                "VpcPeeringConnectionId".to_string(),
                vpc_peering_connection_id.clone(),
            );
        }

        if let Some(transit_gateway_id) = &request.transit_gateway_id {
            form_data.insert("TransitGatewayId".to_string(), transit_gateway_id.clone());
        }

        self.send_form_no_body(form_data, "CreateRoute", &request.route_table_id)
            .await
    }

    async fn delete_route(&self, request: DeleteRouteRequest) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteRoute".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), request.route_table_id.clone());
        form_data.insert(
            "DestinationCidrBlock".to_string(),
            request.destination_cidr_block.clone(),
        );

        self.send_form_no_body(form_data, "DeleteRoute", &request.route_table_id)
            .await
    }

    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AssociateRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("RouteTableId".to_string(), request.route_table_id.clone());
        form_data.insert("SubnetId".to_string(), request.subnet_id.clone());

        self.send_form(form_data, "AssociateRouteTable", &request.route_table_id)
            .await
    }

    async fn disassociate_route_table(&self, association_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DisassociateRouteTable".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("AssociationId".to_string(), association_id.to_string());

        self.send_form_no_body(form_data, "DisassociateRouteTable", association_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Security Group Operations
    // ---------------------------------------------------------------------------

    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeSecurityGroups".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(group_ids) = &request.group_ids {
            for (i, group_id) in group_ids.iter().enumerate() {
                form_data.insert(format!("GroupId.{}", i + 1), group_id.clone());
            }
        }

        if let Some(group_names) = &request.group_names {
            for (i, group_name) in group_names.iter().enumerate() {
                form_data.insert(format!("GroupName.{}", i + 1), group_name.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeSecurityGroups", "SecurityGroup")
            .await
    }

    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateSecurityGroup".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupName".to_string(), request.group_name.clone());
        form_data.insert("GroupDescription".to_string(), request.description.clone());
        form_data.insert("VpcId".to_string(), request.vpc_id.clone());

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateSecurityGroup", &request.group_name)
            .await
    }

    async fn delete_security_group(&self, group_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteSecurityGroup".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), group_id.to_string());

        self.send_form_no_body(form_data, "DeleteSecurityGroup", group_id)
            .await
    }

    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "AuthorizeSecurityGroupIngress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(
            form_data,
            "AuthorizeSecurityGroupIngress",
            &request.group_id,
        )
        .await
    }

    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "AuthorizeSecurityGroupEgress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(form_data, "AuthorizeSecurityGroupEgress", &request.group_id)
            .await
    }

    async fn revoke_security_group_ingress(
        &self,
        request: RevokeSecurityGroupIngressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "RevokeSecurityGroupIngress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(form_data, "RevokeSecurityGroupIngress", &request.group_id)
            .await
    }

    async fn revoke_security_group_egress(
        &self,
        request: RevokeSecurityGroupEgressRequest,
    ) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "RevokeSecurityGroupEgress".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("GroupId".to_string(), request.group_id.clone());

        Self::add_ip_permissions(&mut form_data, &request.ip_permissions);

        self.send_form_no_body(form_data, "RevokeSecurityGroupEgress", &request.group_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Availability Zone Operations
    // ---------------------------------------------------------------------------

    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "DescribeAvailabilityZones".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(zone_names) = &request.zone_names {
            for (i, zone_name) in zone_names.iter().enumerate() {
                form_data.insert(format!("ZoneName.{}", i + 1), zone_name.clone());
            }
        }

        if let Some(zone_ids) = &request.zone_ids {
            for (i, zone_id) in zone_ids.iter().enumerate() {
                form_data.insert(format!("ZoneId.{}", i + 1), zone_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(all_availability_zones) = request.all_availability_zones {
            form_data.insert(
                "AllAvailabilityZones".to_string(),
                all_availability_zones.to_string(),
            );
        }

        self.send_form(form_data, "DescribeAvailabilityZones", "AvailabilityZone")
            .await
    }

    // ---------------------------------------------------------------------------
    // AMI Operations
    // ---------------------------------------------------------------------------

    async fn describe_images(
        &self,
        request: DescribeImagesRequest,
    ) -> Result<DescribeImagesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeImages".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(image_ids) = &request.image_ids {
            for (i, image_id) in image_ids.iter().enumerate() {
                form_data.insert(format!("ImageId.{}", i + 1), image_id.clone());
            }
        }

        if let Some(owners) = &request.owners {
            for (i, owner) in owners.iter().enumerate() {
                form_data.insert(format!("Owner.{}", i + 1), owner.clone());
            }
        }

        if let Some(executable_users) = &request.executable_users {
            for (i, user) in executable_users.iter().enumerate() {
                form_data.insert(format!("ExecutableBy.{}", i + 1), user.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(include_deprecated) = request.include_deprecated {
            form_data.insert(
                "IncludeDeprecated".to_string(),
                include_deprecated.to_string(),
            );
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeImages", "AMI").await
    }

    // ---------------------------------------------------------------------------
    // Instance Operations
    // ---------------------------------------------------------------------------

    async fn terminate_instances(
        &self,
        instance_ids: Vec<String>,
    ) -> Result<TerminateInstancesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "TerminateInstances".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        for (i, instance_id) in instance_ids.iter().enumerate() {
            form_data.insert(format!("InstanceId.{}", i + 1), instance_id.clone());
        }

        let resource = instance_ids.join(",");
        self.send_form(form_data, "TerminateInstances", &resource)
            .await
    }

    async fn describe_instances(
        &self,
        request: DescribeInstancesRequest,
    ) -> Result<DescribeInstancesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeInstances".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(instance_ids) = &request.instance_ids {
            for (i, instance_id) in instance_ids.iter().enumerate() {
                form_data.insert(format!("InstanceId.{}", i + 1), instance_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeInstances", "Instance")
            .await
    }

    // ---------------------------------------------------------------------------
    // Volume Operations
    // ---------------------------------------------------------------------------

    async fn create_volume(&self, request: CreateVolumeRequest) -> Result<CreateVolumeResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "AvailabilityZone".to_string(),
            request.availability_zone.clone(),
        );

        if let Some(size) = request.size {
            form_data.insert("Size".to_string(), size.to_string());
        }

        if let Some(snapshot_id) = &request.snapshot_id {
            form_data.insert("SnapshotId".to_string(), snapshot_id.clone());
        }

        if let Some(volume_type) = &request.volume_type {
            form_data.insert("VolumeType".to_string(), volume_type.clone());
        }

        if let Some(iops) = request.iops {
            form_data.insert("Iops".to_string(), iops.to_string());
        }

        if let Some(throughput) = request.throughput {
            form_data.insert("Throughput".to_string(), throughput.to_string());
        }

        if let Some(encrypted) = request.encrypted {
            form_data.insert("Encrypted".to_string(), encrypted.to_string());
        }

        if let Some(kms_key_id) = &request.kms_key_id {
            form_data.insert("KmsKeyId".to_string(), kms_key_id.clone());
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(form_data, "CreateVolume", &request.availability_zone)
            .await
    }

    async fn delete_volume(&self, volume_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VolumeId".to_string(), volume_id.to_string());

        self.send_form_no_body(form_data, "DeleteVolume", volume_id)
            .await
    }

    async fn describe_volumes(
        &self,
        request: DescribeVolumesRequest,
    ) -> Result<DescribeVolumesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeVolumes".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(volume_ids) = &request.volume_ids {
            for (i, volume_id) in volume_ids.iter().enumerate() {
                form_data.insert(format!("VolumeId.{}", i + 1), volume_id.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeVolumes", "Volume").await
    }

    async fn attach_volume(&self, request: AttachVolumeRequest) -> Result<AttachVolumeResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AttachVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VolumeId".to_string(), request.volume_id.clone());
        form_data.insert("InstanceId".to_string(), request.instance_id.clone());
        form_data.insert("Device".to_string(), request.device.clone());

        let resource = format!("{}:{}", request.volume_id, request.instance_id);
        self.send_form(form_data, "AttachVolume", &resource).await
    }

    async fn detach_volume(&self, request: DetachVolumeRequest) -> Result<DetachVolumeResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DetachVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VolumeId".to_string(), request.volume_id.clone());

        if let Some(instance_id) = &request.instance_id {
            form_data.insert("InstanceId".to_string(), instance_id.clone());
        }

        if let Some(device) = &request.device {
            form_data.insert("Device".to_string(), device.clone());
        }

        if let Some(force) = request.force {
            form_data.insert("Force".to_string(), force.to_string());
        }

        self.send_form(form_data, "DetachVolume", &request.volume_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Launch Template Operations
    // ---------------------------------------------------------------------------

    async fn create_launch_template(
        &self,
        request: CreateLaunchTemplateRequest,
    ) -> Result<CreateLaunchTemplateResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateLaunchTemplate".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "LaunchTemplateName".to_string(),
            request.launch_template_name.clone(),
        );

        if let Some(version_description) = &request.version_description {
            form_data.insert(
                "VersionDescription".to_string(),
                version_description.clone(),
            );
        }

        // Add launch template data
        let data = &request.launch_template_data;

        if let Some(image_id) = &data.image_id {
            form_data.insert("LaunchTemplateData.ImageId".to_string(), image_id.clone());
        }

        if let Some(instance_type) = &data.instance_type {
            form_data.insert(
                "LaunchTemplateData.InstanceType".to_string(),
                instance_type.clone(),
            );
        }

        if let Some(key_name) = &data.key_name {
            form_data.insert("LaunchTemplateData.KeyName".to_string(), key_name.clone());
        }

        if let Some(user_data) = &data.user_data {
            form_data.insert("LaunchTemplateData.UserData".to_string(), user_data.clone());
        }

        if let Some(security_group_ids) = &data.security_group_ids {
            for (i, sg_id) in security_group_ids.iter().enumerate() {
                form_data.insert(
                    format!("LaunchTemplateData.SecurityGroupId.{}", i + 1),
                    sg_id.clone(),
                );
            }
        }

        if let Some(iam_instance_profile) = &data.iam_instance_profile {
            if let Some(arn) = &iam_instance_profile.arn {
                form_data.insert(
                    "LaunchTemplateData.IamInstanceProfile.Arn".to_string(),
                    arn.clone(),
                );
            }
            if let Some(name) = &iam_instance_profile.name {
                form_data.insert(
                    "LaunchTemplateData.IamInstanceProfile.Name".to_string(),
                    name.clone(),
                );
            }
        }

        if let Some(block_device_mappings) = &data.block_device_mappings {
            for (i, bdm) in block_device_mappings.iter().enumerate() {
                let idx = i + 1;
                if let Some(device_name) = &bdm.device_name {
                    form_data.insert(
                        format!("LaunchTemplateData.BlockDeviceMapping.{}.DeviceName", idx),
                        device_name.clone(),
                    );
                }
                if let Some(ebs) = &bdm.ebs {
                    if let Some(volume_size) = ebs.volume_size {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.VolumeSize",
                                idx
                            ),
                            volume_size.to_string(),
                        );
                    }
                    if let Some(volume_type) = &ebs.volume_type {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.VolumeType",
                                idx
                            ),
                            volume_type.clone(),
                        );
                    }
                    if let Some(delete_on_termination) = ebs.delete_on_termination {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.DeleteOnTermination",
                                idx
                            ),
                            delete_on_termination.to_string(),
                        );
                    }
                    if let Some(encrypted) = ebs.encrypted {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.Encrypted",
                                idx
                            ),
                            encrypted.to_string(),
                        );
                    }
                    if let Some(iops) = ebs.iops {
                        form_data.insert(
                            format!("LaunchTemplateData.BlockDeviceMapping.{}.Ebs.Iops", idx),
                            iops.to_string(),
                        );
                    }
                    if let Some(throughput) = ebs.throughput {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.Throughput",
                                idx
                            ),
                            throughput.to_string(),
                        );
                    }
                }
            }
        }

        if let Some(network_interfaces) = &data.network_interfaces {
            for (i, ni) in network_interfaces.iter().enumerate() {
                let idx = i + 1;
                if let Some(device_index) = ni.device_index {
                    form_data.insert(
                        format!("LaunchTemplateData.NetworkInterface.{}.DeviceIndex", idx),
                        device_index.to_string(),
                    );
                }
                if let Some(associate_public_ip) = ni.associate_public_ip_address {
                    form_data.insert(
                        format!(
                            "LaunchTemplateData.NetworkInterface.{}.AssociatePublicIpAddress",
                            idx
                        ),
                        associate_public_ip.to_string(),
                    );
                }
                if let Some(subnet_id) = &ni.subnet_id {
                    form_data.insert(
                        format!("LaunchTemplateData.NetworkInterface.{}.SubnetId", idx),
                        subnet_id.clone(),
                    );
                }
                if let Some(groups) = &ni.groups {
                    for (j, group) in groups.iter().enumerate() {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.NetworkInterface.{}.SecurityGroupId.{}",
                                idx,
                                j + 1
                            ),
                            group.clone(),
                        );
                    }
                }
            }
        }

        if let Some(metadata_options) = &data.metadata_options {
            if let Some(http_tokens) = &metadata_options.http_tokens {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpTokens".to_string(),
                    http_tokens.clone(),
                );
            }
            if let Some(http_endpoint) = &metadata_options.http_endpoint {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpEndpoint".to_string(),
                    http_endpoint.clone(),
                );
            }
            if let Some(http_put_response_hop_limit) = metadata_options.http_put_response_hop_limit
            {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpPutResponseHopLimit".to_string(),
                    http_put_response_hop_limit.to_string(),
                );
            }
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(
            form_data,
            "CreateLaunchTemplate",
            &request.launch_template_name,
        )
        .await
    }

    async fn create_launch_template_version(
        &self,
        request: CreateLaunchTemplateVersionRequest,
    ) -> Result<CreateLaunchTemplateVersionResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "CreateLaunchTemplateVersion".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        let resource_name;
        if let Some(ref id) = request.launch_template_id {
            form_data.insert("LaunchTemplateId".to_string(), id.clone());
            resource_name = id.clone();
        } else if let Some(ref name) = request.launch_template_name {
            form_data.insert("LaunchTemplateName".to_string(), name.clone());
            resource_name = name.clone();
        } else {
            return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
                message: "Either launch_template_id or launch_template_name must be provided"
                    .to_string(),
                field_name: Some("launch_template_id".to_string()),
            }));
        }

        if let Some(ref source_version) = request.source_version {
            form_data.insert("SourceVersion".to_string(), source_version.clone());
        }
        if let Some(ref description) = request.version_description {
            form_data.insert("VersionDescription".to_string(), description.clone());
        }

        let data = &request.launch_template_data;
        if let Some(ref user_data) = data.user_data {
            form_data.insert("LaunchTemplateData.UserData".to_string(), user_data.clone());
        }
        if let Some(ref image_id) = data.image_id {
            form_data.insert("LaunchTemplateData.ImageId".to_string(), image_id.clone());
        }
        if let Some(ref instance_type) = data.instance_type {
            form_data.insert(
                "LaunchTemplateData.InstanceType".to_string(),
                instance_type.clone(),
            );
        }

        self.send_form(form_data, "CreateLaunchTemplateVersion", &resource_name)
            .await
    }

    async fn delete_launch_template(
        &self,
        request: DeleteLaunchTemplateRequest,
    ) -> Result<DeleteLaunchTemplateResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteLaunchTemplate".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        let resource: String;
        if let Some(launch_template_id) = &request.launch_template_id {
            form_data.insert("LaunchTemplateId".to_string(), launch_template_id.clone());
            resource = launch_template_id.clone();
        } else if let Some(launch_template_name) = &request.launch_template_name {
            form_data.insert(
                "LaunchTemplateName".to_string(),
                launch_template_name.clone(),
            );
            resource = launch_template_name.clone();
        } else {
            resource = "unknown".to_string();
        }

        self.send_form(form_data, "DeleteLaunchTemplate", &resource)
            .await
    }

    async fn describe_launch_templates(
        &self,
        request: DescribeLaunchTemplatesRequest,
    ) -> Result<DescribeLaunchTemplatesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeLaunchTemplates".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(launch_template_ids) = &request.launch_template_ids {
            for (i, lt_id) in launch_template_ids.iter().enumerate() {
                form_data.insert(format!("LaunchTemplateId.{}", i + 1), lt_id.clone());
            }
        }

        if let Some(launch_template_names) = &request.launch_template_names {
            for (i, lt_name) in launch_template_names.iter().enumerate() {
                form_data.insert(format!("LaunchTemplateName.{}", i + 1), lt_name.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeLaunchTemplates", "LaunchTemplate")
            .await
    }

    async fn get_console_output(&self, instance_id: String) -> Result<GetConsoleOutputResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "GetConsoleOutput".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("InstanceId".to_string(), instance_id.clone());

        self.send_form(form_data, "GetConsoleOutput", &instance_id)
            .await
    }
}

impl Ec2Client {
    /// Add IP permissions to form data for security group operations.
    fn add_ip_permissions(form_data: &mut HashMap<String, String>, permissions: &[IpPermission]) {
        for (i, perm) in permissions.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(
                format!("IpPermissions.{}.IpProtocol", idx),
                perm.ip_protocol.clone(),
            );

            if let Some(from_port) = perm.from_port {
                form_data.insert(
                    format!("IpPermissions.{}.FromPort", idx),
                    from_port.to_string(),
                );
            }

            if let Some(to_port) = perm.to_port {
                form_data.insert(format!("IpPermissions.{}.ToPort", idx), to_port.to_string());
            }

            if let Some(ip_ranges) = &perm.ip_ranges {
                for (j, range) in ip_ranges.iter().enumerate() {
                    form_data.insert(
                        format!("IpPermissions.{}.IpRanges.{}.CidrIp", idx, j + 1),
                        range.cidr_ip.clone(),
                    );
                    if let Some(description) = &range.description {
                        form_data.insert(
                            format!("IpPermissions.{}.IpRanges.{}.Description", idx, j + 1),
                            description.clone(),
                        );
                    }
                }
            }

            if let Some(ipv6_ranges) = &perm.ipv6_ranges {
                for (j, range) in ipv6_ranges.iter().enumerate() {
                    form_data.insert(
                        format!("IpPermissions.{}.Ipv6Ranges.{}.CidrIpv6", idx, j + 1),
                        range.cidr_ipv6.clone(),
                    );
                    if let Some(description) = &range.description {
                        form_data.insert(
                            format!("IpPermissions.{}.Ipv6Ranges.{}.Description", idx, j + 1),
                            description.clone(),
                        );
                    }
                }
            }

            if let Some(user_id_group_pairs) = &perm.user_id_group_pairs {
                for (j, pair) in user_id_group_pairs.iter().enumerate() {
                    if let Some(group_id) = &pair.group_id {
                        form_data.insert(
                            format!("IpPermissions.{}.Groups.{}.GroupId", idx, j + 1),
                            group_id.clone(),
                        );
                    }
                    if let Some(user_id) = &pair.user_id {
                        form_data.insert(
                            format!("IpPermissions.{}.Groups.{}.UserId", idx, j + 1),
                            user_id.clone(),
                        );
                    }
                    if let Some(description) = &pair.description {
                        form_data.insert(
                            format!("IpPermissions.{}.Groups.{}.Description", idx, j + 1),
                            description.clone(),
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Common Types
// ---------------------------------------------------------------------------

/// A filter to apply when describing resources.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Filter {
    /// The name of the filter.
    pub name: String,
    /// The filter values.
    pub values: Vec<String>,
}

/// A tag to apply to a resource.
/// Note: EC2 XML responses use lowercase `<key>` and `<value>` tags.
#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct Tag {
    /// The key of the tag.
    pub key: String,
    /// The value of the tag.
    pub value: String,
}

/// Tag specification for creating resources with tags.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct TagSpecification {
    /// The type of resource to tag.
    pub resource_type: String,
    /// The tags to apply.
    pub tags: Vec<Tag>,
}

// ---------------------------------------------------------------------------
// VPC Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe VPCs.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeVpcsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing VPCs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVpcsResponse {
    #[serde(rename = "vpcSet")]
    pub vpc_set: Option<VpcSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VpcSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Vpc>,
}

/// Represents a VPC.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vpc {
    pub vpc_id: Option<String>,
    pub state: Option<String>,
    pub cidr_block: Option<String>,
    pub owner_id: Option<String>,
    pub instance_tenancy: Option<String>,
    pub is_default: Option<bool>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Tag>,
}

/// Request to describe a VPC attribute.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DescribeVpcAttributeRequest {
    pub vpc_id: String,
    /// The attribute to describe: "enableDnsSupport" or "enableDnsHostnames"
    pub attribute: String,
}

/// Response from describing a VPC attribute.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVpcAttributeResponse {
    pub vpc_id: Option<String>,
    #[serde(rename = "enableDnsSupport")]
    pub enable_dns_support: Option<AttributeBooleanValue>,
    #[serde(rename = "enableDnsHostnames")]
    pub enable_dns_hostnames: Option<AttributeBooleanValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeBooleanValue {
    pub value: Option<bool>,
}

/// Request to create a VPC.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateVpcRequest {
    pub cidr_block: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_tenancy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amazon_provided_ipv6_cidr_block: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a VPC.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVpcResponse {
    pub vpc: Option<Vpc>,
}

/// Request to modify VPC attributes.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct ModifyVpcAttributeRequest {
    pub vpc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_dns_support: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_dns_hostnames: Option<bool>,
}

// ---------------------------------------------------------------------------
// Subnet Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe subnets.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeSubnetsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing subnets.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeSubnetsResponse {
    #[serde(rename = "subnetSet")]
    pub subnet_set: Option<SubnetSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubnetSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Subnet>,
}

/// Represents a subnet.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subnet {
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub state: Option<String>,
    pub cidr_block: Option<String>,
    pub availability_zone: Option<String>,
    pub availability_zone_id: Option<String>,
    pub available_ip_address_count: Option<i32>,
    pub default_for_az: Option<bool>,
    pub map_public_ip_on_launch: Option<bool>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Request to create a subnet.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateSubnetRequest {
    pub vpc_id: String,
    pub cidr_block: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a subnet.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubnetResponse {
    pub subnet: Option<Subnet>,
}

// ---------------------------------------------------------------------------
// Internet Gateway Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create an internet gateway.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct CreateInternetGatewayRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating an internet gateway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInternetGatewayResponse {
    #[serde(rename = "internetGateway")]
    pub internet_gateway: Option<InternetGateway>,
}

/// Represents an internet gateway.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGateway {
    pub internet_gateway_id: Option<String>,
    #[serde(rename = "attachmentSet")]
    pub attachment_set: Option<InternetGatewayAttachmentSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGatewayAttachmentSet {
    #[serde(rename = "item", default)]
    pub items: Vec<InternetGatewayAttachment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGatewayAttachment {
    pub vpc_id: Option<String>,
    pub state: Option<String>,
}

/// Request to attach an internet gateway to a VPC.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AttachInternetGatewayRequest {
    pub internet_gateway_id: String,
    pub vpc_id: String,
}

/// Request to detach an internet gateway from a VPC.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DetachInternetGatewayRequest {
    pub internet_gateway_id: String,
    pub vpc_id: String,
}

/// Request to describe internet gateways.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeInternetGatewaysRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internet_gateway_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing internet gateways.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeInternetGatewaysResponse {
    #[serde(rename = "internetGatewaySet")]
    pub internet_gateway_set: Option<InternetGatewaySet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternetGatewaySet {
    #[serde(rename = "item", default)]
    pub items: Vec<InternetGateway>,
}

// ---------------------------------------------------------------------------
// NAT Gateway Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a NAT gateway.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateNatGatewayRequest {
    /// The subnet in which to create the NAT gateway.
    pub subnet_id: String,
    /// [Public NAT only] The allocation ID of the Elastic IP address for the gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_id: Option<String>,
    /// The connectivity type: "public" (default) or "private".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connectivity_type: Option<String>,
    /// The private IPv4 address to assign to the NAT gateway.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_ip_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a NAT gateway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNatGatewayResponse {
    #[serde(rename = "natGateway")]
    pub nat_gateway: Option<NatGateway>,
}

/// Represents a NAT gateway.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGateway {
    pub nat_gateway_id: Option<String>,
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub state: Option<String>,
    pub connectivity_type: Option<String>,
    #[serde(rename = "natGatewayAddressSet")]
    pub nat_gateway_address_set: Option<NatGatewayAddressSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewayAddressSet {
    #[serde(rename = "item", default)]
    pub items: Vec<NatGatewayAddress>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewayAddress {
    pub allocation_id: Option<String>,
    pub network_interface_id: Option<String>,
    pub private_ip: Option<String>,
    pub public_ip: Option<String>,
}

/// Response from deleting a NAT gateway.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteNatGatewayResponse {
    pub nat_gateway_id: Option<String>,
}

/// Request to describe NAT gateways.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeNatGatewaysRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_gateway_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing NAT gateways.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeNatGatewaysResponse {
    #[serde(rename = "natGatewaySet")]
    pub nat_gateway_set: Option<NatGatewaySet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NatGatewaySet {
    #[serde(rename = "item", default)]
    pub items: Vec<NatGateway>,
}

// ---------------------------------------------------------------------------
// Elastic IP Request/Response Types
// ---------------------------------------------------------------------------

/// Request to allocate an Elastic IP address.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct AllocateAddressRequest {
    /// The domain: "vpc" (default) or "standard".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from allocating an Elastic IP address.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocateAddressResponse {
    pub public_ip: Option<String>,
    pub allocation_id: Option<String>,
    pub domain: Option<String>,
}

// ---------------------------------------------------------------------------
// Route Table Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe route tables.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeRouteTablesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_table_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing route tables.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeRouteTablesResponse {
    #[serde(rename = "routeTableSet")]
    pub route_table_set: Option<RouteTableSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTableSet {
    #[serde(rename = "item", default)]
    pub items: Vec<RouteTable>,
}

/// Represents a route table.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTable {
    pub route_table_id: Option<String>,
    pub vpc_id: Option<String>,
    #[serde(rename = "routeSet")]
    pub route_set: Option<RouteSet>,
    #[serde(rename = "associationSet")]
    pub association_set: Option<RouteTableAssociationSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Route>,
}

/// Represents a route in a route table.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub destination_cidr_block: Option<String>,
    pub gateway_id: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub instance_id: Option<String>,
    pub network_interface_id: Option<String>,
    pub vpc_peering_connection_id: Option<String>,
    pub transit_gateway_id: Option<String>,
    pub state: Option<String>,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTableAssociationSet {
    #[serde(rename = "item", default)]
    pub items: Vec<RouteTableAssociation>,
}

/// Represents an association between a route table and a subnet.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTableAssociation {
    pub route_table_association_id: Option<String>,
    pub route_table_id: Option<String>,
    pub subnet_id: Option<String>,
    pub main: Option<bool>,
}

/// Request to create a route table.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateRouteTableRequest {
    pub vpc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a route table.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRouteTableResponse {
    #[serde(rename = "routeTable")]
    pub route_table: Option<RouteTable>,
}

/// Request to create a route.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateRouteRequest {
    pub route_table_id: String,
    pub destination_cidr_block: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_gateway_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interface_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_peering_connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transit_gateway_id: Option<String>,
}

/// Request to delete a route.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DeleteRouteRequest {
    pub route_table_id: String,
    pub destination_cidr_block: String,
}

/// Request to associate a route table with a subnet.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AssociateRouteTableRequest {
    pub route_table_id: String,
    pub subnet_id: String,
}

/// Response from associating a route table.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociateRouteTableResponse {
    pub association_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Security Group Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe security groups.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeSecurityGroupsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing security groups.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeSecurityGroupsResponse {
    #[serde(rename = "securityGroupInfo")]
    pub security_group_info: Option<SecurityGroupSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityGroupSet {
    #[serde(rename = "item", default)]
    pub items: Vec<SecurityGroup>,
}

/// Represents a security group.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityGroup {
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub vpc_id: Option<String>,
    pub owner_id: Option<String>,
    pub group_description: Option<String>,
    #[serde(rename = "ipPermissions")]
    pub ip_permissions: Option<IpPermissionSet>,
    #[serde(rename = "ipPermissionsEgress")]
    pub ip_permissions_egress: Option<IpPermissionSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpPermissionSet {
    #[serde(rename = "item", default)]
    pub items: Vec<IpPermissionResponse>,
}

/// IP permission in a response (from DescribeSecurityGroups).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpPermissionResponse {
    pub ip_protocol: Option<String>,
    pub from_port: Option<i32>,
    pub to_port: Option<i32>,
    #[serde(rename = "ipRanges")]
    pub ip_ranges: Option<IpRangeSet>,
    #[serde(rename = "ipv6Ranges")]
    pub ipv6_ranges: Option<Ipv6RangeSet>,
    #[serde(rename = "groups")]
    pub groups: Option<UserIdGroupPairSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpRangeSet {
    #[serde(rename = "item", default)]
    pub items: Vec<IpRangeResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpRangeResponse {
    pub cidr_ip: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ipv6RangeSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Ipv6RangeResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ipv6RangeResponse {
    pub cidr_ipv6: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIdGroupPairSet {
    #[serde(rename = "item", default)]
    pub items: Vec<UserIdGroupPairResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserIdGroupPairResponse {
    pub group_id: Option<String>,
    pub user_id: Option<String>,
    pub description: Option<String>,
}

/// Request to create a security group.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateSecurityGroupRequest {
    pub group_name: String,
    pub description: String,
    pub vpc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a security group.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSecurityGroupResponse {
    pub group_id: Option<String>,
}

/// IP permission for requests (authorize/revoke).
#[derive(Debug, Clone, Serialize, Builder)]
pub struct IpPermission {
    /// The IP protocol: tcp, udp, icmp, or -1 for all.
    pub ip_protocol: String,
    /// The start of port range (or ICMP type).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_port: Option<i32>,
    /// The end of port range (or ICMP code).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_port: Option<i32>,
    /// The IPv4 CIDR ranges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_ranges: Option<Vec<IpRange>>,
    /// The IPv6 CIDR ranges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6_ranges: Option<Vec<Ipv6Range>>,
    /// The security group and AWS account ID pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id_group_pairs: Option<Vec<UserIdGroupPair>>,
}

/// IPv4 CIDR range for security group rules.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct IpRange {
    pub cidr_ip: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// IPv6 CIDR range for security group rules.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct Ipv6Range {
    pub cidr_ipv6: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Security group and AWS account ID pair for security group rules.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct UserIdGroupPair {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Request to authorize security group ingress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AuthorizeSecurityGroupIngressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to authorize security group egress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AuthorizeSecurityGroupEgressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to revoke security group ingress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RevokeSecurityGroupIngressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to revoke security group egress.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct RevokeSecurityGroupEgressRequest {
    pub group_id: String,
    pub ip_permissions: Vec<IpPermission>,
}

// ---------------------------------------------------------------------------
// Availability Zone Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe availability zones.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeAvailabilityZonesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_availability_zones: Option<bool>,
}

/// Response from describing availability zones.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeAvailabilityZonesResponse {
    #[serde(rename = "availabilityZoneInfo")]
    pub availability_zone_info: Option<AvailabilityZoneSet>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityZoneSet {
    #[serde(rename = "item", default)]
    pub items: Vec<AvailabilityZone>,
}

/// Represents an availability zone.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityZone {
    pub zone_name: Option<String>,
    pub zone_id: Option<String>,
    pub zone_state: Option<String>,
    pub region_name: Option<String>,
    pub zone_type: Option<String>,
    pub opt_in_status: Option<String>,
}

// ---------------------------------------------------------------------------
// AMI Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe images (AMIs).
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeImagesRequest {
    /// The image IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_ids: Option<Vec<String>>,
    /// The owners (self, amazon, aws-marketplace, or account ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    /// Users that have explicit launch permissions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_users: Option<Vec<String>>,
    /// Filters for the images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    /// Whether to include deprecated AMIs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_deprecated: Option<bool>,
    /// Maximum results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    /// Token for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing images.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeImagesResponse {
    #[serde(rename = "imagesSet")]
    pub images_set: Option<ImageSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Image>,
}

/// Represents an AMI.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub image_id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub image_location: Option<String>,
    pub state: Option<String>,
    pub owner_id: Option<String>,
    pub creation_date: Option<String>,
    pub architecture: Option<String>,
    pub platform: Option<String>,
    pub platform_details: Option<String>,
    pub image_type: Option<String>,
    pub root_device_type: Option<String>,
    pub root_device_name: Option<String>,
    pub virtualization_type: Option<String>,
    pub hypervisor: Option<String>,
    pub is_public: Option<bool>,
    pub deprecation_time: Option<String>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
    #[serde(rename = "blockDeviceMapping")]
    pub block_device_mapping: Option<BlockDeviceMappingSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDeviceMappingSet {
    #[serde(rename = "item", default)]
    pub items: Vec<BlockDeviceMapping>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDeviceMapping {
    pub device_name: Option<String>,
    pub ebs: Option<EbsBlockDevice>,
    pub virtual_name: Option<String>,
    pub no_device: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EbsBlockDevice {
    pub snapshot_id: Option<String>,
    pub volume_size: Option<i32>,
    pub volume_type: Option<String>,
    pub delete_on_termination: Option<bool>,
    pub encrypted: Option<bool>,
    pub iops: Option<i32>,
    pub throughput: Option<i32>,
}

// ---------------------------------------------------------------------------
// Instance Request/Response Types
// ---------------------------------------------------------------------------

/// Response from terminating instances.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminateInstancesResponse {
    #[serde(rename = "instancesSet")]
    pub instances_set: Option<TerminatingInstanceSet>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminatingInstanceSet {
    #[serde(rename = "item", default)]
    pub items: Vec<TerminatingInstance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminatingInstance {
    pub instance_id: Option<String>,
    pub current_state: Option<InstanceState>,
    pub previous_state: Option<InstanceState>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceState {
    pub code: Option<i32>,
    pub name: Option<String>,
}

/// Request to describe instances.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeInstancesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing instances.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeInstancesResponse {
    #[serde(rename = "reservationSet")]
    pub reservation_set: Option<ReservationSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Reservation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reservation {
    pub reservation_id: Option<String>,
    pub owner_id: Option<String>,
    #[serde(rename = "instancesSet")]
    pub instances_set: Option<InstanceSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Instance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub instance_id: Option<String>,
    pub image_id: Option<String>,
    pub instance_type: Option<String>,
    pub key_name: Option<String>,
    pub launch_time: Option<String>,
    pub placement: Option<Placement>,
    pub private_dns_name: Option<String>,
    pub private_ip_address: Option<String>,
    pub public_dns_name: Option<String>,
    pub public_ip_address: Option<String>,
    pub state_reason: Option<StateReason>,
    pub instance_state: Option<InstanceState>,
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub architecture: Option<String>,
    pub root_device_type: Option<String>,
    pub root_device_name: Option<String>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub availability_zone: Option<String>,
    pub tenancy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateReason {
    pub code: Option<String>,
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// Volume Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a volume.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateVolumeRequest {
    /// The availability zone.
    pub availability_zone: String,
    /// The size of the volume in GiBs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,
    /// The snapshot ID to create the volume from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    /// The volume type: gp2, gp3, io1, io2, st1, sc1, standard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_type: Option<String>,
    /// IOPS for io1, io2, or gp3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<i32>,
    /// Throughput for gp3 in MiB/s.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<i32>,
    /// Whether to encrypt the volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    /// The KMS key ID for encryption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    /// Tags for the volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a volume.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVolumeResponse {
    pub volume_id: Option<String>,
    pub size: Option<i32>,
    pub availability_zone: Option<String>,
    pub state: Option<String>,
    pub volume_type: Option<String>,
    pub iops: Option<i32>,
    pub throughput: Option<i32>,
    pub encrypted: Option<bool>,
    pub create_time: Option<String>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Request to describe volumes.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeVolumesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing volumes.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVolumesResponse {
    #[serde(rename = "volumeSet")]
    pub volume_set: Option<VolumeSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Volume>,
}

/// Represents an EBS volume.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub volume_id: Option<String>,
    pub size: Option<i32>,
    pub availability_zone: Option<String>,
    pub state: Option<String>,
    pub volume_type: Option<String>,
    pub iops: Option<i32>,
    pub throughput: Option<i32>,
    pub encrypted: Option<bool>,
    pub snapshot_id: Option<String>,
    pub create_time: Option<String>,
    #[serde(rename = "attachmentSet")]
    pub attachment_set: Option<VolumeAttachmentSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeAttachmentSet {
    #[serde(rename = "item", default)]
    pub items: Vec<VolumeAttachment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeAttachment {
    pub volume_id: Option<String>,
    pub instance_id: Option<String>,
    pub device: Option<String>,
    pub state: Option<String>,
    pub attach_time: Option<String>,
    pub delete_on_termination: Option<bool>,
}

/// Request to attach a volume.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AttachVolumeRequest {
    /// The volume ID.
    pub volume_id: String,
    /// The instance ID.
    pub instance_id: String,
    /// The device name (e.g., /dev/sdf).
    pub device: String,
}

/// Response from attaching a volume.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachVolumeResponse {
    pub volume_id: Option<String>,
    pub instance_id: Option<String>,
    pub device: Option<String>,
    pub state: Option<String>,
    pub attach_time: Option<String>,
}

/// Request to detach a volume.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DetachVolumeRequest {
    /// The volume ID.
    pub volume_id: String,
    /// The instance ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// The device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Whether to force detach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

/// Response from detaching a volume.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachVolumeResponse {
    pub volume_id: Option<String>,
    pub instance_id: Option<String>,
    pub device: Option<String>,
    pub state: Option<String>,
    pub attach_time: Option<String>,
}

// ---------------------------------------------------------------------------
// Launch Template Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateLaunchTemplateRequest {
    /// The name for the launch template.
    pub launch_template_name: String,
    /// A description for the launch template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_description: Option<String>,
    /// The launch template data.
    pub launch_template_data: RequestLaunchTemplateData,
    /// Tags for the launch template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Launch template data for the request.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct RequestLaunchTemplateData {
    /// The ID of the AMI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
    /// The instance type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_type: Option<String>,
    /// The key name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    /// The user data (base64-encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<String>,
    /// The security group IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_group_ids: Option<Vec<String>>,
    /// The IAM instance profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iam_instance_profile: Option<LaunchTemplateIamInstanceProfileSpecification>,
    /// Block device mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_device_mappings: Option<Vec<LaunchTemplateBlockDeviceMapping>>,
    /// Network interfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interfaces: Option<Vec<LaunchTemplateNetworkInterface>>,
    /// Metadata options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_options: Option<LaunchTemplateInstanceMetadataOptions>,
}

/// IAM instance profile specification for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateIamInstanceProfileSpecification {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Block device mapping for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateBlockDeviceMapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebs: Option<LaunchTemplateEbsBlockDevice>,
}

/// EBS block device for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateEbsBlockDevice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_on_termination: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<i32>,
}

/// Network interface for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateNetworkInterface {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associate_public_ip_address: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
}

/// Instance metadata options for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateInstanceMetadataOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_tokens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_put_response_hop_limit: Option<i32>,
}

/// Response from creating a launch template.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLaunchTemplateResponse {
    pub launch_template: Option<LaunchTemplate>,
}

/// Request to create a new version of an existing launch template.
/// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_CreateLaunchTemplateVersion.html
#[derive(Debug, Clone, Builder, Default)]
pub struct CreateLaunchTemplateVersionRequest {
    pub launch_template_id: Option<String>,
    pub launch_template_name: Option<String>,
    pub source_version: Option<String>,
    pub version_description: Option<String>,
    pub launch_template_data: RequestLaunchTemplateData,
}

/// Response from creating a launch template version.
#[derive(Debug, Deserialize)]
pub struct CreateLaunchTemplateVersionResponse {
    #[serde(rename = "launchTemplateVersion")]
    pub launch_template_version: Option<LaunchTemplateVersion>,
}

/// A version of a launch template.
#[derive(Debug, Clone, Deserialize)]
pub struct LaunchTemplateVersion {
    #[serde(rename = "launchTemplateId")]
    pub launch_template_id: Option<String>,
    #[serde(rename = "versionNumber")]
    pub version_number: Option<i64>,
}

/// Request to delete a launch template.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DeleteLaunchTemplateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_name: Option<String>,
}

/// Response from deleting a launch template.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLaunchTemplateResponse {
    pub launch_template: Option<LaunchTemplate>,
}

/// Request to describe launch templates.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeLaunchTemplatesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing launch templates.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeLaunchTemplatesResponse {
    #[serde(rename = "launchTemplates")]
    pub launch_templates: Option<LaunchTemplateSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTemplateSet {
    #[serde(rename = "item", default)]
    pub items: Vec<LaunchTemplate>,
}

/// Represents a launch template.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTemplate {
    pub launch_template_id: Option<String>,
    pub launch_template_name: Option<String>,
    pub create_time: Option<String>,
    pub created_by: Option<String>,
    pub default_version_number: Option<i64>,
    pub latest_version_number: Option<i64>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Response from the GetConsoleOutput API.
/// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_GetConsoleOutput.html
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetConsoleOutputResponse {
    /// The ID of the instance.
    #[serde(rename = "instanceId")]
    pub instance_id: Option<String>,
    /// The console output, base64-encoded.
    pub output: Option<String>,
    /// The time at which the output was last updated.
    pub timestamp: Option<String>,
}

impl GetConsoleOutputResponse {
    /// Decodes the base64-encoded output to a UTF-8 string.
    pub fn decode_output(&self) -> Option<String> {
        self.output.as_ref().and_then(|b64| {
            STANDARD
                .decode(b64.trim())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
    }
}
