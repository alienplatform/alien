use super::api::Ec2Api;
use super::types::*;
use crate::aws::aws_request_utils::{AwsRequestBuilderExt, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::ContextError;
use async_trait::async_trait;
use form_urlencoded;
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashMap;

mod instances;
mod launch_templates;
mod network;

// ---------------------------------------------------------------------------
// EC2 Error Response Parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Ec2ErrorResponse {
    pub errors: Ec2ErrorsWrapper,
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
            "InvalidVolume.NotFound" | "InvalidVolumeID.NotFound" => {
                ErrorData::RemoteResourceNotFound {
                    resource_type: "Volume".into(),
                    resource_name: resource.into(),
                }
            }
            // Conflict / already exists errors
            "VpcLimitExceeded"
            | "SubnetLimitExceeded"
            | "SecurityGroupLimitExceeded"
            | "AddressLimitExceeded" => ErrorData::QuotaExceeded { message },
            "InvalidVpcState"
            | "InvalidSubnetID.DuplicateSubnet"
            | "InvalidGroup.Duplicate"
            | "InvalidPermission.Duplicate"
            | "InvalidLaunchTemplateName.AlreadyExistsException" => {
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
        Self::add_tag_specifications_with_prefix(form_data, "TagSpecification", tag_specs);
    }

    /// Add tag specification parameters under a specific EC2 query prefix.
    fn add_tag_specifications_with_prefix(
        form_data: &mut HashMap<String, String>,
        prefix: &str,
        tag_specs: &[TagSpecification],
    ) {
        for (i, spec) in tag_specs.iter().enumerate() {
            let idx = i + 1;
            form_data.insert(
                format!("{prefix}.{idx}.ResourceType"),
                spec.resource_type.clone(),
            );
            for (j, tag) in spec.tags.iter().enumerate() {
                form_data.insert(format!("{prefix}.{idx}.Tag.{}.Key", j + 1), tag.key.clone());
                form_data.insert(
                    format!("{prefix}.{idx}.Tag.{}.Value", j + 1),
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
        self.describe_vpcs_impl(request).await
    }

    async fn describe_vpc_attribute(
        &self,
        request: DescribeVpcAttributeRequest,
    ) -> Result<DescribeVpcAttributeResponse> {
        self.describe_vpc_attribute_impl(request).await
    }

    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse> {
        self.create_vpc_impl(request).await
    }

    async fn delete_vpc(&self, vpc_id: &str) -> Result<()> {
        self.delete_vpc_impl(vpc_id).await
    }

    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()> {
        self.modify_vpc_attribute_impl(request).await
    }

    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse> {
        self.describe_subnets_impl(request).await
    }

    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse> {
        self.create_subnet_impl(request).await
    }

    async fn delete_subnet(&self, subnet_id: &str) -> Result<()> {
        self.delete_subnet_impl(subnet_id).await
    }

    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse> {
        self.create_internet_gateway_impl(request).await
    }

    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()> {
        self.delete_internet_gateway_impl(internet_gateway_id).await
    }

    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()> {
        self.attach_internet_gateway_impl(request).await
    }

    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()> {
        self.detach_internet_gateway_impl(request).await
    }

    async fn describe_internet_gateways(
        &self,
        request: DescribeInternetGatewaysRequest,
    ) -> Result<DescribeInternetGatewaysResponse> {
        self.describe_internet_gateways_impl(request).await
    }

    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse> {
        self.create_nat_gateway_impl(request).await
    }

    async fn delete_nat_gateway(&self, nat_gateway_id: &str) -> Result<DeleteNatGatewayResponse> {
        self.delete_nat_gateway_impl(nat_gateway_id).await
    }

    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse> {
        self.describe_nat_gateways_impl(request).await
    }

    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse> {
        self.allocate_address_impl(request).await
    }

    async fn release_address(&self, allocation_id: &str) -> Result<()> {
        self.release_address_impl(allocation_id).await
    }

    async fn describe_route_tables(
        &self,
        request: DescribeRouteTablesRequest,
    ) -> Result<DescribeRouteTablesResponse> {
        self.describe_route_tables_impl(request).await
    }

    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse> {
        self.create_route_table_impl(request).await
    }

    async fn delete_route_table(&self, route_table_id: &str) -> Result<()> {
        self.delete_route_table_impl(route_table_id).await
    }

    async fn create_route(&self, request: CreateRouteRequest) -> Result<()> {
        self.create_route_impl(request).await
    }

    async fn delete_route(&self, request: DeleteRouteRequest) -> Result<()> {
        self.delete_route_impl(request).await
    }

    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse> {
        self.associate_route_table_impl(request).await
    }

    async fn disassociate_route_table(&self, association_id: &str) -> Result<()> {
        self.disassociate_route_table_impl(association_id).await
    }

    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse> {
        self.describe_security_groups_impl(request).await
    }

    async fn describe_network_interfaces(
        &self,
        request: DescribeNetworkInterfacesRequest,
    ) -> Result<DescribeNetworkInterfacesResponse> {
        self.describe_network_interfaces_impl(request).await
    }

    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse> {
        self.create_security_group_impl(request).await
    }

    async fn delete_security_group(&self, group_id: &str) -> Result<()> {
        self.delete_security_group_impl(group_id).await
    }

    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()> {
        self.authorize_security_group_ingress_impl(request).await
    }

    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()> {
        self.authorize_security_group_egress_impl(request).await
    }

    async fn revoke_security_group_ingress(
        &self,
        request: RevokeSecurityGroupIngressRequest,
    ) -> Result<()> {
        self.revoke_security_group_ingress_impl(request).await
    }

    async fn revoke_security_group_egress(
        &self,
        request: RevokeSecurityGroupEgressRequest,
    ) -> Result<()> {
        self.revoke_security_group_egress_impl(request).await
    }

    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse> {
        self.describe_availability_zones_impl(request).await
    }

    async fn describe_images(
        &self,
        request: DescribeImagesRequest,
    ) -> Result<DescribeImagesResponse> {
        self.describe_images_impl(request).await
    }

    async fn terminate_instances(
        &self,
        instance_ids: Vec<String>,
    ) -> Result<TerminateInstancesResponse> {
        self.terminate_instances_impl(instance_ids).await
    }

    async fn describe_instances(
        &self,
        request: DescribeInstancesRequest,
    ) -> Result<DescribeInstancesResponse> {
        self.describe_instances_impl(request).await
    }

    async fn create_volume(&self, request: CreateVolumeRequest) -> Result<CreateVolumeResponse> {
        self.create_volume_impl(request).await
    }

    async fn modify_volume(&self, request: ModifyVolumeRequest) -> Result<ModifyVolumeResponse> {
        self.modify_volume_impl(request).await
    }

    async fn describe_volumes_modifications(
        &self,
        request: DescribeVolumesModificationsRequest,
    ) -> Result<DescribeVolumesModificationsResponse> {
        self.describe_volumes_modifications_impl(request).await
    }

    async fn delete_volume(&self, volume_id: &str) -> Result<()> {
        self.delete_volume_impl(volume_id).await
    }

    async fn describe_volumes(
        &self,
        request: DescribeVolumesRequest,
    ) -> Result<DescribeVolumesResponse> {
        self.describe_volumes_impl(request).await
    }

    async fn attach_volume(&self, request: AttachVolumeRequest) -> Result<AttachVolumeResponse> {
        self.attach_volume_impl(request).await
    }

    async fn detach_volume(&self, request: DetachVolumeRequest) -> Result<DetachVolumeResponse> {
        self.detach_volume_impl(request).await
    }

    async fn create_launch_template(
        &self,
        request: CreateLaunchTemplateRequest,
    ) -> Result<CreateLaunchTemplateResponse> {
        self.create_launch_template_impl(request).await
    }

    async fn create_launch_template_version(
        &self,
        request: CreateLaunchTemplateVersionRequest,
    ) -> Result<CreateLaunchTemplateVersionResponse> {
        self.create_launch_template_version_impl(request).await
    }

    async fn delete_launch_template(
        &self,
        request: DeleteLaunchTemplateRequest,
    ) -> Result<DeleteLaunchTemplateResponse> {
        self.delete_launch_template_impl(request).await
    }

    async fn describe_launch_templates(
        &self,
        request: DescribeLaunchTemplatesRequest,
    ) -> Result<DescribeLaunchTemplatesResponse> {
        self.describe_launch_templates_impl(request).await
    }

    async fn get_console_output(&self, instance_id: String) -> Result<GetConsoleOutputResponse> {
        self.get_console_output_impl(instance_id).await
    }
}

impl Ec2Client {
    fn create_volume_form_data(request: &CreateVolumeRequest) -> HashMap<String, String> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "AvailabilityZone".to_string(),
            request.availability_zone.clone(),
        );

        if let Some(client_token) = &request.client_token {
            form_data.insert("ClientToken".to_string(), client_token.clone());
        }
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

        form_data
    }

    fn modify_volume_form_data(request: &ModifyVolumeRequest) -> HashMap<String, String> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "ModifyVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VolumeId".to_string(), request.volume_id.clone());
        form_data.insert("Size".to_string(), request.size.to_string());
        form_data
    }

    fn describe_volumes_modifications_form_data(
        request: &DescribeVolumesModificationsRequest,
    ) -> HashMap<String, String> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "DescribeVolumesModifications".to_string(),
        );
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

        form_data
    }

    /// Append `LaunchTemplateData.CpuOptions.*` form fields.
    ///
    /// Currently only emits `NestedVirtualization` when set. AWS validates
    /// support at launch time and rejects unsupported instance types with a
    /// clear error (nested virt is only available on 8th-gen Intel: c8i/m8i/r8i
    /// and their flex variants), so no client-side allowlist is needed.
    fn add_cpu_options(
        form_data: &mut HashMap<String, String>,
        cpu_options: Option<&LaunchTemplateCpuOptions>,
    ) {
        let Some(cpu_options) = cpu_options else {
            return;
        };
        if let Some(nested) = &cpu_options.nested_virtualization {
            form_data.insert(
                "LaunchTemplateData.CpuOptions.NestedVirtualization".to_string(),
                nested.clone(),
            );
        }
    }

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

#[cfg(test)]
mod tests;
