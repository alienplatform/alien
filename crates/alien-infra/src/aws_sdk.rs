use std::collections::HashMap;

use alien_core::{AwsClientConfig, AwsCredentials, Platform};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use async_trait::async_trait;
use aws_config::{BehaviorVersion, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_acm::Client as AcmClient;
use aws_sdk_apigatewayv2::Client as ApiGatewayV2Client;
use aws_sdk_codebuild::{
    types::{
        ArtifactsType, CloudWatchLogsConfig, ComputeType as AwsCodeBuildComputeType,
        EnvironmentType, EnvironmentVariable as AwsCodeBuildEnvironmentVariable,
        ImagePullCredentialsType, LogsConfig, LogsConfigStatusType, ProjectArtifacts,
        ProjectEnvironment, ProjectSource, S3LogsConfig, SourceType, Tag as CodeBuildTag,
    },
    Client as CodeBuildClient,
};
use aws_sdk_dynamodb::{
    operation::{
        delete_table::DeleteTableError, describe_table::DescribeTableError,
        describe_time_to_live::DescribeTimeToLiveError,
    },
    types::{
        AttributeDefinition as AwsDynamoDbAttributeDefinition, BillingMode,
        KeySchemaElement as AwsDynamoDbKeySchemaElement, KeyType, ScalarAttributeType,
        Tag as DynamoDbTag, TimeToLiveSpecification,
    },
    Client as DynamoDbClient,
};
use aws_sdk_ec2::{
    types::{
        AttributeBooleanValue as AwsEc2AttributeBooleanValue,
        ConnectivityType as AwsEc2ConnectivityType, DomainType as AwsEc2DomainType,
        Filter as AwsEc2Filter, IpPermission as AwsEc2IpPermission, IpRange as AwsEc2IpRange,
        ResourceType as AwsEc2ResourceType, Tag as AwsEc2Tag,
        TagSpecification as AwsEc2TagSpecification,
    },
    Client as Ec2Client,
};
use aws_sdk_ecr::{
    types::{
        ImageScanningConfiguration as AwsEcrImageScanningConfiguration,
        ReplicationConfiguration as AwsEcrReplicationConfiguration,
        ReplicationDestination as AwsEcrReplicationDestination,
        ReplicationRule as AwsEcrReplicationRule, Repository as AwsEcrRepository,
        RepositoryFilter as AwsEcrRepositoryFilter,
    },
    Client as EcrClient,
};
use aws_sdk_eventbridge::Client as EventBridgeClient;
use aws_sdk_iam::{
    types::{
        AttachedPolicy as AwsIamAttachedPolicy, InstanceProfile as AwsIamInstanceProfile,
        Policy as AwsIamPolicy, PolicyVersion as AwsIamPolicyVersion, Role as AwsIamRole,
        Tag as AwsIamTag,
    },
    Client as IamClient,
};
use aws_sdk_lambda::{
    types::{Architecture as AwsLambdaArchitecture, PackageType},
    Client as LambdaClient,
};
use aws_sdk_s3::{
    error::ProvideErrorMetadata,
    operation::{
        create_bucket::CreateBucketError, delete_bucket::DeleteBucketError,
        delete_bucket_lifecycle::DeleteBucketLifecycleError,
        delete_bucket_policy::DeleteBucketPolicyError, get_bucket_acl::GetBucketAclError,
        get_bucket_encryption::GetBucketEncryptionError,
        get_bucket_lifecycle_configuration::GetBucketLifecycleConfigurationError,
        get_bucket_notification_configuration::GetBucketNotificationConfigurationError,
        get_bucket_policy::GetBucketPolicyError,
        get_public_access_block::GetPublicAccessBlockError,
        list_object_versions::ListObjectVersionsError, list_objects_v2::ListObjectsV2Error,
    },
    types::{
        BucketLifecycleConfiguration as AwsBucketLifecycleConfiguration, BucketLocationConstraint,
        BucketVersioningStatus, CreateBucketConfiguration, Delete, Event as AwsS3Event,
        ExpirationStatus, LambdaFunctionConfiguration as AwsLambdaFunctionConfiguration,
        LifecycleExpiration as AwsLifecycleExpiration, LifecycleRule as AwsLifecycleRule,
        LifecycleRuleFilter as AwsLifecycleRuleFilter,
        NotificationConfiguration as AwsNotificationConfiguration, ObjectIdentifier,
        PublicAccessBlockConfiguration as AwsPublicAccessBlockConfiguration, Tag as S3Tag, Tagging,
        VersioningConfiguration,
    },
    Client as S3Client,
};
use aws_sdk_sqs::{types::QueueAttributeName, Client as SqsClient};
use aws_sdk_ssm::{
    primitives::DateTimeFormat,
    types::{ParameterStringFilter, ParameterTier, ParameterType},
};
use aws_types::region::Region;
use bon::Builder;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::{ErrorData, Result};

pub use aws_sdk_acm::{
    operation::import_certificate::{
        ImportCertificateInput as ImportCertificateRequest,
        ImportCertificateOutput as ImportCertificateResponse,
    },
    primitives::Blob as AcmBlob,
    types::Tag as AcmTag,
};

pub type ReimportCertificateRequest = ImportCertificateRequest;

pub use aws_sdk_apigatewayv2::{
    operation::{
        create_api::{
            CreateApiInput as ApiGatewayV2CreateApiRequest,
            CreateApiOutput as ApiGatewayV2CreateApiResponse,
        },
        create_api_mapping::{
            CreateApiMappingInput as ApiGatewayV2CreateApiMappingRequest,
            CreateApiMappingOutput as ApiGatewayV2CreateApiMappingResponse,
        },
        create_domain_name::{
            CreateDomainNameInput as ApiGatewayV2CreateDomainNameRequest,
            CreateDomainNameOutput as ApiGatewayV2CreateDomainNameResponse,
        },
        create_integration::{
            CreateIntegrationInput as ApiGatewayV2CreateIntegrationRequest,
            CreateIntegrationOutput as ApiGatewayV2CreateIntegrationResponse,
        },
        create_route::{
            CreateRouteInput as ApiGatewayV2CreateRouteRequest,
            CreateRouteOutput as ApiGatewayV2CreateRouteResponse,
        },
        create_stage::{
            CreateStageInput as ApiGatewayV2CreateStageRequest,
            CreateStageOutput as ApiGatewayV2CreateStageResponse,
        },
        get_api::GetApiOutput as ApiGatewayV2GetApiResponse,
        get_api_mappings::GetApiMappingsOutput as ApiGatewayV2GetApiMappingsResponse,
        get_domain_name::GetDomainNameOutput as ApiGatewayV2GetDomainNameResponse,
    },
    types::{
        DomainNameConfiguration as ApiGatewayV2DomainNameConfiguration, EndpointType,
        IntegrationType, ProtocolType, SecurityPolicy,
    },
};

pub use aws_sdk_eventbridge::{
    operation::{
        put_rule::{PutRuleInput as PutRuleRequest, PutRuleOutput as PutRuleResponse},
        put_targets::PutTargetsInput as PutTargetsRequest,
    },
    types::{RuleState, Tag as EventBridgeTag, Target as EventBridgeTarget},
};

pub use aws_sdk_lambda::operation::{
    add_permission::{
        AddPermissionInput as AddPermissionRequest, AddPermissionOutput as AddPermissionResponse,
    },
    create_event_source_mapping::{
        CreateEventSourceMappingInput as CreateEventSourceMappingRequest,
        CreateEventSourceMappingOutput as CreateEventSourceMappingResponse,
    },
    delete_event_source_mapping::DeleteEventSourceMappingOutput as DeleteEventSourceMappingResponse,
    list_event_source_mappings::{
        ListEventSourceMappingsInput as ListEventSourceMappingsRequest,
        ListEventSourceMappingsOutput as ListEventSourceMappingsResponse,
    },
    update_function_code::UpdateFunctionCodeInput as UpdateFunctionCodeRequest,
    update_function_configuration::UpdateFunctionConfigurationInput as UpdateFunctionConfigurationRequest,
};
pub use aws_sdk_lambda::types::{Environment, FunctionCode, VpcConfig};

/// Parameter metadata returned from SSM for vault heartbeat sampling.
#[derive(Debug, Clone)]
pub struct SsmParameterMetadata {
    /// Parameter type, such as String, StringList, or SecureString.
    pub parameter_type: Option<String>,
    /// Parameter tier, such as Standard or Advanced.
    pub tier: Option<String>,
    /// Whether KMS key metadata is present.
    pub has_key_id: bool,
    /// Last modified timestamp.
    pub last_modified_at: Option<DateTime<Utc>>,
}

/// Result of describing parameters for a prefix.
#[derive(Debug, Clone)]
pub struct DescribeSsmParametersResponse {
    /// Sampled parameter metadata.
    pub parameters: Vec<SsmParameterMetadata>,
    /// Whether SSM returned a next token.
    pub has_more_parameters: bool,
}

/// DynamoDB key schema metadata used by infra heartbeat collection.
#[derive(Debug, Clone)]
pub struct DynamoDbKeySchemaElement {
    /// Attribute name.
    pub attribute_name: String,
    /// Key type, such as HASH or RANGE.
    pub key_type: String,
}

/// DynamoDB table metadata used by infra controllers.
#[derive(Debug, Clone)]
pub struct DynamoDbTableDescription {
    /// Table name.
    pub table_name: Option<String>,
    /// Table ARN.
    pub table_arn: Option<String>,
    /// Table lifecycle status.
    pub table_status: Option<String>,
    /// Billing mode, such as PAY_PER_REQUEST.
    pub billing_mode: Option<String>,
    /// Table primary key schema.
    pub key_schema: Vec<DynamoDbKeySchemaElement>,
    /// Number of global secondary indexes.
    pub global_secondary_index_count: Option<u32>,
    /// Number of local secondary indexes.
    pub local_secondary_index_count: Option<u32>,
    /// Approximate item count.
    pub item_count: Option<u64>,
    /// Approximate table size in bytes.
    pub table_size_bytes: Option<u64>,
    /// Whether DynamoDB Streams are enabled.
    pub stream_enabled: Option<bool>,
    /// Stream view type.
    pub stream_view_type: Option<String>,
    /// Whether deletion protection is enabled.
    pub deletion_protection_enabled: Option<bool>,
    /// Server-side encryption status.
    pub sse_status: Option<String>,
    /// Server-side encryption type.
    pub sse_type: Option<String>,
    /// Table class.
    pub table_class: Option<String>,
    /// Number of replicas.
    pub replica_count: Option<u32>,
    /// Whether restore is in progress.
    pub restore_in_progress: Option<bool>,
}

/// DynamoDB TTL metadata used by infra heartbeat collection.
#[derive(Debug, Clone)]
pub struct DynamoDbTtlDescription {
    /// TTL status.
    pub status: Option<String>,
    /// TTL attribute name.
    pub attribute_name: Option<String>,
}

/// CodeBuild project configuration used for create and update operations.
#[derive(Debug, Clone)]
pub struct CodeBuildProjectConfig {
    /// CodeBuild project name.
    pub name: String,
    /// Inline buildspec used by the project.
    pub buildspec: String,
    /// Environment type, such as LINUX_CONTAINER.
    pub environment_type: String,
    /// Build container image.
    pub image: String,
    /// Compute type, such as BUILD_GENERAL1_SMALL.
    pub compute_type: String,
    /// Image pull credentials type, such as SERVICE_ROLE.
    pub image_pull_credentials_type: String,
    /// Environment variables for the project.
    pub environment_variables: Vec<(String, String)>,
    /// IAM role ARN used by CodeBuild.
    pub service_role: String,
    /// Project description.
    pub description: String,
    /// Resource tags.
    pub tags: HashMap<String, String>,
}

/// CodeBuild project metadata used by infra controllers.
#[derive(Debug, Clone)]
pub struct CodeBuildProjectDescription {
    /// CodeBuild project name.
    pub name: String,
    /// CodeBuild project ARN.
    pub arn: Option<String>,
    /// Project description.
    pub description: Option<String>,
    /// Source type.
    pub source_type: Option<String>,
    /// Artifacts type.
    pub artifacts_type: Option<String>,
    /// Whether artifacts encryption is disabled.
    pub artifacts_encryption_disabled: Option<bool>,
    /// Environment type.
    pub environment_type: Option<String>,
    /// Environment image.
    pub environment_image: Option<String>,
    /// Compute type.
    pub compute_type: Option<String>,
    /// Image pull credentials type.
    pub image_pull_credentials_type: Option<String>,
    /// Whether privileged mode is enabled.
    pub privileged_mode: Option<bool>,
    /// Number of environment variables configured on the project.
    pub environment_variable_count: u32,
    /// Whether a service role is configured.
    pub service_role_present: bool,
    /// Whether an encryption key is configured.
    pub encryption_key_present: bool,
    /// CloudWatch logs status.
    pub cloud_watch_logs_status: Option<String>,
    /// S3 logs status.
    pub s3_logs_status: Option<String>,
    /// Build timeout in minutes.
    pub timeout_in_minutes: Option<i32>,
    /// Queued timeout in minutes.
    pub queued_timeout_in_minutes: Option<i32>,
    /// Created timestamp as epoch seconds.
    pub created: Option<f64>,
    /// Last modified timestamp as epoch seconds.
    pub last_modified: Option<f64>,
}

/// Lambda function creation request used by worker controllers.
#[derive(Debug, Clone, Builder)]
pub struct CreateFunctionRequest {
    /// Lambda function name.
    pub function_name: String,
    /// IAM execution role ARN.
    pub role: String,
    /// Lambda code configuration.
    pub code: FunctionCode,
    /// Deployment package type, such as Image.
    #[builder(default = "Image".to_string())]
    pub package_type: String,
    /// Function description.
    pub description: Option<String>,
    /// Timeout in seconds.
    pub timeout: Option<i32>,
    /// Memory size in MB.
    pub memory_size: Option<i32>,
    /// Whether to publish a version.
    pub publish: Option<bool>,
    /// Environment variables.
    pub environment: Option<Environment>,
    /// Supported architectures.
    pub architectures: Option<Vec<String>>,
    /// Function tags.
    pub tags: Option<HashMap<String, String>>,
    /// KMS key ARN.
    pub kms_key_arn: Option<String>,
    /// VPC configuration.
    pub vpc_config: Option<VpcConfig>,
}

/// Lambda function metadata used by worker controllers.
#[derive(Debug, Clone)]
pub struct FunctionConfiguration {
    /// Function name.
    pub function_name: Option<String>,
    /// Function ARN.
    pub function_arn: Option<String>,
    /// Function lifecycle state.
    pub state: Option<String>,
    /// Last update status.
    pub last_update_status: Option<String>,
    /// KMS key ARN.
    pub kms_key_arn: Option<String>,
}

/// EC2 filter used in describe requests.
#[derive(Debug, Clone, Builder)]
pub struct Filter {
    /// Filter name.
    pub name: String,
    /// Filter values.
    pub values: Vec<String>,
}

/// EC2 resource tag.
#[derive(Debug, Clone, Builder)]
pub struct Ec2Tag {
    /// Tag key.
    pub key: String,
    /// Tag value.
    pub value: String,
}

/// EC2 tag specification used for create calls.
#[derive(Debug, Clone, Builder)]
pub struct TagSpecification {
    /// EC2 resource type, such as vpc or subnet.
    pub resource_type: String,
    /// Tags to apply.
    pub tags: Vec<Ec2Tag>,
}

/// Request to describe VPCs.
#[derive(Debug, Clone, Builder, Default)]
pub struct DescribeVpcsRequest {
    /// Optional VPC IDs.
    pub vpc_ids: Option<Vec<String>>,
    /// Optional filters.
    pub filters: Option<Vec<Filter>>,
    /// Maximum results.
    pub max_results: Option<i32>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// Response from describing VPCs.
#[derive(Debug, Clone)]
pub struct DescribeVpcsResponse {
    /// VPC set.
    pub vpc_set: Option<VpcSet>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// EC2 VPC set.
#[derive(Debug, Clone)]
pub struct VpcSet {
    /// VPCs.
    pub items: Vec<Vpc>,
}

/// EC2 VPC metadata.
#[derive(Debug, Clone)]
pub struct Vpc {
    /// VPC ID.
    pub vpc_id: Option<String>,
    /// VPC state.
    pub state: Option<String>,
    /// Primary CIDR block.
    pub cidr_block: Option<String>,
}

/// Request to create a VPC.
#[derive(Debug, Clone, Builder)]
pub struct CreateVpcRequest {
    /// CIDR block.
    pub cidr_block: String,
    /// Resource tags.
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a VPC.
#[derive(Debug, Clone)]
pub struct CreateVpcResponse {
    /// Created VPC.
    pub vpc: Option<Vpc>,
}

/// Request to modify VPC attributes.
#[derive(Debug, Clone, Builder)]
pub struct ModifyVpcAttributeRequest {
    /// VPC ID.
    pub vpc_id: String,
    /// Enable DNS support.
    pub enable_dns_support: Option<bool>,
    /// Enable DNS hostnames.
    pub enable_dns_hostnames: Option<bool>,
}

/// Request to describe subnets.
#[derive(Debug, Clone, Builder, Default)]
pub struct DescribeSubnetsRequest {
    /// Optional subnet IDs.
    pub subnet_ids: Option<Vec<String>>,
    /// Optional filters.
    pub filters: Option<Vec<Filter>>,
    /// Maximum results.
    pub max_results: Option<i32>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// Response from describing subnets.
#[derive(Debug, Clone)]
pub struct DescribeSubnetsResponse {
    /// Subnet set.
    pub subnet_set: Option<SubnetSet>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// EC2 subnet set.
#[derive(Debug, Clone)]
pub struct SubnetSet {
    /// Subnets.
    pub items: Vec<Subnet>,
}

/// EC2 subnet metadata.
#[derive(Debug, Clone)]
pub struct Subnet {
    /// Subnet ID.
    pub subnet_id: Option<String>,
}

/// Request to create a subnet.
#[derive(Debug, Clone, Builder)]
pub struct CreateSubnetRequest {
    /// VPC ID.
    pub vpc_id: String,
    /// CIDR block.
    pub cidr_block: String,
    /// Availability zone name.
    pub availability_zone: Option<String>,
    /// Resource tags.
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a subnet.
#[derive(Debug, Clone)]
pub struct CreateSubnetResponse {
    /// Created subnet.
    pub subnet: Option<Subnet>,
}

/// Request to create an internet gateway.
#[derive(Debug, Clone, Builder, Default)]
pub struct CreateInternetGatewayRequest {
    /// Resource tags.
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating an internet gateway.
#[derive(Debug, Clone)]
pub struct CreateInternetGatewayResponse {
    /// Created gateway.
    pub internet_gateway: Option<InternetGateway>,
}

/// EC2 internet gateway metadata.
#[derive(Debug, Clone)]
pub struct InternetGateway {
    /// Internet gateway ID.
    pub internet_gateway_id: Option<String>,
}

/// Request to attach an internet gateway.
#[derive(Debug, Clone, Builder)]
pub struct AttachInternetGatewayRequest {
    /// Internet gateway ID.
    pub internet_gateway_id: String,
    /// VPC ID.
    pub vpc_id: String,
}

/// Request to detach an internet gateway.
#[derive(Debug, Clone, Builder)]
pub struct DetachInternetGatewayRequest {
    /// Internet gateway ID.
    pub internet_gateway_id: String,
    /// VPC ID.
    pub vpc_id: String,
}

/// Request to create a NAT gateway.
#[derive(Debug, Clone, Builder)]
pub struct CreateNatGatewayRequest {
    /// Subnet ID.
    pub subnet_id: String,
    /// Elastic IP allocation ID.
    pub allocation_id: Option<String>,
    /// Connectivity type.
    pub connectivity_type: Option<String>,
    /// Resource tags.
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a NAT gateway.
#[derive(Debug, Clone)]
pub struct CreateNatGatewayResponse {
    /// Created NAT gateway.
    pub nat_gateway: Option<NatGateway>,
}

/// EC2 NAT gateway metadata.
#[derive(Debug, Clone)]
pub struct NatGateway {
    /// NAT gateway ID.
    pub nat_gateway_id: Option<String>,
    /// NAT gateway state.
    pub state: Option<String>,
}

/// Response from deleting a NAT gateway.
#[derive(Debug, Clone)]
pub struct DeleteNatGatewayResponse {
    /// NAT gateway ID.
    pub nat_gateway_id: Option<String>,
}

/// Request to describe NAT gateways.
#[derive(Debug, Clone, Builder, Default)]
pub struct DescribeNatGatewaysRequest {
    /// NAT gateway IDs.
    pub nat_gateway_ids: Option<Vec<String>>,
    /// Optional filters.
    pub filters: Option<Vec<Filter>>,
    /// Maximum results.
    pub max_results: Option<i32>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// Response from describing NAT gateways.
#[derive(Debug, Clone)]
pub struct DescribeNatGatewaysResponse {
    /// NAT gateway set.
    pub nat_gateway_set: Option<NatGatewaySet>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// EC2 NAT gateway set.
#[derive(Debug, Clone)]
pub struct NatGatewaySet {
    /// NAT gateways.
    pub items: Vec<NatGateway>,
}

/// Request to allocate an Elastic IP.
#[derive(Debug, Clone, Builder, Default)]
pub struct AllocateAddressRequest {
    /// Address domain.
    pub domain: Option<String>,
    /// Resource tags.
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from allocating an Elastic IP.
#[derive(Debug, Clone)]
pub struct AllocateAddressResponse {
    /// Allocation ID.
    pub allocation_id: Option<String>,
}

/// Request to create a route table.
#[derive(Debug, Clone, Builder)]
pub struct CreateRouteTableRequest {
    /// VPC ID.
    pub vpc_id: String,
    /// Resource tags.
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a route table.
#[derive(Debug, Clone)]
pub struct CreateRouteTableResponse {
    /// Created route table.
    pub route_table: Option<RouteTable>,
}

/// EC2 route table metadata.
#[derive(Debug, Clone)]
pub struct RouteTable {
    /// Route table ID.
    pub route_table_id: Option<String>,
}

/// Request to create a route.
#[derive(Debug, Clone, Builder)]
pub struct CreateRouteRequest {
    /// Route table ID.
    pub route_table_id: String,
    /// Destination CIDR block.
    pub destination_cidr_block: String,
    /// Internet gateway ID.
    pub gateway_id: Option<String>,
    /// NAT gateway ID.
    pub nat_gateway_id: Option<String>,
}

/// Request to associate a route table.
#[derive(Debug, Clone, Builder)]
pub struct AssociateRouteTableRequest {
    /// Route table ID.
    pub route_table_id: String,
    /// Subnet ID.
    pub subnet_id: String,
}

/// Response from associating a route table.
#[derive(Debug, Clone)]
pub struct AssociateRouteTableResponse {
    /// Association ID.
    pub association_id: Option<String>,
}

/// Request to describe security groups.
#[derive(Debug, Clone, Builder, Default)]
pub struct DescribeSecurityGroupsRequest {
    /// Group IDs.
    pub group_ids: Option<Vec<String>>,
    /// Group names.
    pub group_names: Option<Vec<String>>,
    /// Optional filters.
    pub filters: Option<Vec<Filter>>,
    /// Maximum results.
    pub max_results: Option<i32>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// Response from describing security groups.
#[derive(Debug, Clone)]
pub struct DescribeSecurityGroupsResponse {
    /// Security group set.
    pub security_group_info: Option<SecurityGroupSet>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// EC2 security group set.
#[derive(Debug, Clone)]
pub struct SecurityGroupSet {
    /// Security groups.
    pub items: Vec<SecurityGroup>,
}

/// EC2 security group metadata.
#[derive(Debug, Clone)]
pub struct SecurityGroup {
    /// Group ID.
    pub group_id: Option<String>,
    /// Ingress permissions.
    pub ip_permissions: Option<IpPermissionSet>,
    /// Egress permissions.
    pub ip_permissions_egress: Option<IpPermissionSet>,
}

/// Request to describe network interfaces.
#[derive(Debug, Clone, Builder, Default)]
pub struct DescribeNetworkInterfacesRequest {
    /// Network interface IDs.
    pub network_interface_ids: Option<Vec<String>>,
    /// Optional filters.
    pub filters: Option<Vec<Filter>>,
    /// Maximum results.
    pub max_results: Option<i32>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// Response from describing network interfaces.
#[derive(Debug, Clone)]
pub struct DescribeNetworkInterfacesResponse {
    /// Network interface set.
    pub network_interface_set: Option<NetworkInterfaceSet>,
    /// Pagination token.
    pub next_token: Option<String>,
}

/// EC2 network interface set.
#[derive(Debug, Clone)]
pub struct NetworkInterfaceSet {
    /// Network interfaces.
    pub items: Vec<NetworkInterface>,
}

/// EC2 network interface metadata.
#[derive(Debug, Clone)]
pub struct NetworkInterface {
    /// Network interface ID.
    pub network_interface_id: Option<String>,
}

/// Security group permission set.
#[derive(Debug, Clone)]
pub struct IpPermissionSet {
    /// Permissions.
    pub items: Vec<IpPermissionResponse>,
}

/// Security group permission returned by EC2.
#[derive(Debug, Clone)]
pub struct IpPermissionResponse {
    /// IP protocol.
    pub ip_protocol: Option<String>,
    /// From port.
    pub from_port: Option<i32>,
    /// To port.
    pub to_port: Option<i32>,
    /// IPv4 ranges.
    pub ip_ranges: Option<IpRangeSet>,
    /// IPv6 ranges are not used by current infra code.
    pub ipv6_ranges: Option<()>,
    /// Source security groups are not used by current infra code.
    pub groups: Option<()>,
}

/// IPv4 range set.
#[derive(Debug, Clone)]
pub struct IpRangeSet {
    /// IPv4 ranges.
    pub items: Vec<IpRangeResponse>,
}

/// IPv4 range returned by EC2.
#[derive(Debug, Clone)]
pub struct IpRangeResponse {
    /// CIDR block.
    pub cidr_ip: Option<String>,
    /// Description.
    pub description: Option<String>,
}

/// Security group permission used in authorize requests.
#[derive(Debug, Clone)]
pub struct IpPermission {
    /// IP protocol.
    pub ip_protocol: String,
    /// From port.
    pub from_port: Option<i32>,
    /// To port.
    pub to_port: Option<i32>,
    /// IPv4 ranges.
    pub ip_ranges: Option<Vec<IpRange>>,
    /// IPv6 ranges are not used by current infra code.
    pub ipv6_ranges: Option<Vec<()>>,
    /// User/group pairs are not used by current infra code.
    pub user_id_group_pairs: Option<Vec<()>>,
}

/// IPv4 range used in authorize requests.
#[derive(Debug, Clone)]
pub struct IpRange {
    /// CIDR block.
    pub cidr_ip: String,
    /// Description.
    pub description: Option<String>,
}

/// Request to create a security group.
#[derive(Debug, Clone, Builder)]
pub struct CreateSecurityGroupRequest {
    /// Group name.
    pub group_name: String,
    /// Group description.
    pub description: String,
    /// VPC ID.
    pub vpc_id: String,
    /// Resource tags.
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a security group.
#[derive(Debug, Clone)]
pub struct CreateSecurityGroupResponse {
    /// Security group ID.
    pub group_id: Option<String>,
}

/// Request to authorize security group ingress.
#[derive(Debug, Clone, Builder)]
pub struct AuthorizeSecurityGroupIngressRequest {
    /// Security group ID.
    pub group_id: String,
    /// Permissions.
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to authorize security group egress.
#[derive(Debug, Clone, Builder)]
pub struct AuthorizeSecurityGroupEgressRequest {
    /// Security group ID.
    pub group_id: String,
    /// Permissions.
    pub ip_permissions: Vec<IpPermission>,
}

/// Request to describe availability zones.
#[derive(Debug, Clone, Builder, Default)]
pub struct DescribeAvailabilityZonesRequest {
    /// Zone names.
    pub zone_names: Option<Vec<String>>,
    /// Zone IDs.
    pub zone_ids: Option<Vec<String>>,
    /// Optional filters.
    pub filters: Option<Vec<Filter>>,
    /// Whether to include all zones.
    pub all_availability_zones: Option<bool>,
}

/// Response from describing availability zones.
#[derive(Debug, Clone)]
pub struct DescribeAvailabilityZonesResponse {
    /// Availability zone set.
    pub availability_zone_info: Option<AvailabilityZoneSet>,
}

/// EC2 availability zone set.
#[derive(Debug, Clone)]
pub struct AvailabilityZoneSet {
    /// Availability zones.
    pub items: Vec<AvailabilityZone>,
}

/// EC2 availability zone metadata.
#[derive(Debug, Clone)]
pub struct AvailabilityZone {
    /// Zone name.
    pub zone_name: Option<String>,
}

/// ECR repository metadata used for artifact registry heartbeats.
#[derive(Debug, Clone)]
pub struct EcrRepository {
    /// Repository ARN.
    pub repository_arn: String,
    /// Registry account ID.
    pub registry_id: String,
    /// Repository name.
    pub repository_name: String,
    /// Repository URI.
    pub repository_uri: String,
    /// Repository creation timestamp as epoch seconds.
    pub created_at: f64,
    /// Image tag mutability mode.
    pub image_tag_mutability: Option<String>,
    /// Image scanning configuration.
    pub image_scanning_configuration: Option<EcrImageScanningConfiguration>,
    /// Encryption configuration.
    pub encryption_configuration: Option<EcrEncryptionConfiguration>,
}

/// ECR image scanning configuration used for artifact registry heartbeats.
#[derive(Debug, Clone)]
pub struct EcrImageScanningConfiguration {
    /// Whether images are scanned when pushed.
    pub scan_on_push: Option<bool>,
}

/// ECR encryption configuration used for artifact registry heartbeats.
#[derive(Debug, Clone)]
pub struct EcrEncryptionConfiguration {
    /// Encryption type.
    pub encryption_type: String,
    /// KMS key ARN when customer-managed KMS encryption is configured.
    pub kms_key: Option<String>,
}

/// Request for listing ECR repositories.
#[derive(Debug, Clone, Default)]
pub struct DescribeEcrRepositoriesRequest {
    /// Registry account ID.
    pub registry_id: Option<String>,
    /// Optional explicit repository names.
    pub repository_names: Option<Vec<String>>,
    /// Pagination token.
    pub next_token: Option<String>,
    /// Maximum repositories to return.
    pub max_results: Option<i32>,
}

/// Response from listing ECR repositories.
#[derive(Debug, Clone)]
pub struct DescribeEcrRepositoriesResponse {
    /// Repository metadata.
    pub repositories: Vec<EcrRepository>,
    /// Pagination token when more repositories are available.
    pub next_token: Option<String>,
}

/// ECR registry replication configuration.
#[derive(Debug, Clone)]
pub struct EcrReplicationConfiguration {
    /// Replication rules.
    pub rules: Vec<EcrReplicationRule>,
}

/// ECR registry replication rule.
#[derive(Debug, Clone)]
pub struct EcrReplicationRule {
    /// Replication destinations.
    pub destinations: Vec<EcrReplicationDestination>,
    /// Optional repository filters.
    pub repository_filters: Vec<EcrRepositoryFilter>,
}

/// ECR registry replication destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcrReplicationDestination {
    /// Destination region.
    pub region: String,
    /// Destination registry account ID.
    pub registry_id: String,
}

/// ECR registry repository filter.
#[derive(Debug, Clone)]
pub struct EcrRepositoryFilter {
    /// Filter value.
    pub filter: String,
    /// Filter type.
    pub filter_type: String,
}

/// S3 bucket versioning status used by infra controllers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3VersioningStatus {
    /// Bucket versioning is enabled.
    Enabled,
    /// Bucket versioning is suspended.
    Suspended,
}

/// S3 public access block configuration used by infra controllers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct S3PublicAccessBlock {
    /// Whether public ACLs are blocked.
    pub block_public_acls: Option<bool>,
    /// Whether public ACLs are ignored.
    pub ignore_public_acls: Option<bool>,
    /// Whether public bucket policies are blocked.
    pub block_public_policy: Option<bool>,
    /// Whether public buckets are restricted.
    pub restrict_public_buckets: Option<bool>,
}

/// S3 lifecycle rule configuration used by infra controllers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3LifecycleRuleConfig {
    /// Rule ID.
    pub id: String,
    /// Optional key prefix filter.
    pub prefix: Option<String>,
    /// Expiration age in days.
    pub days: i32,
}

/// S3 bucket metadata used for storage heartbeats.
#[derive(Debug, Clone)]
pub struct S3BucketMetadata {
    /// Bucket region.
    pub region: String,
    /// Bucket versioning status.
    pub versioning_status: Option<S3VersioningStatus>,
    /// Number of lifecycle rules configured.
    pub lifecycle_rule_count: Option<u64>,
    /// Number of server-side encryption rules configured.
    pub encryption_rule_count: Option<u64>,
    /// Public access block configuration, when present.
    pub public_access_block: Option<S3PublicAccessBlock>,
    /// Whether a non-empty bucket policy is present.
    pub bucket_policy_present: Option<bool>,
    /// Whether bucket ACL metadata is present.
    pub bucket_acl_present: Option<bool>,
}

/// S3 bucket notification configuration used by worker storage triggers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationConfiguration {
    /// Lambda notification configurations for the bucket.
    pub lambda_function_configurations: Vec<LambdaFunctionConfiguration>,
}

/// Lambda target configuration for S3 bucket notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaFunctionConfiguration {
    /// Optional notification ID.
    pub id: Option<String>,
    /// Lambda function ARN.
    pub lambda_function_arn: String,
    /// S3 event names.
    pub events: Vec<String>,
    /// Optional key filter. Currently preserved as opaque absence/presence only.
    pub filter: Option<()>,
}

/// IAM role creation request used by infra controllers.
#[derive(Debug, Clone, Builder)]
pub struct CreateRoleRequest {
    /// IAM role name.
    pub role_name: String,
    /// Assume-role trust policy document.
    pub assume_role_policy_document: String,
    /// Optional IAM path.
    pub path: Option<String>,
    /// Optional role description.
    pub description: Option<String>,
    /// Optional max session duration in seconds.
    pub max_session_duration: Option<i32>,
    /// Tags to attach to the role.
    pub tags: Option<Vec<CreateRoleTag>>,
}

/// IAM tag used when creating roles and instance profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRoleTag {
    /// Tag key.
    pub key: String,
    /// Tag value.
    pub value: String,
}

/// Create an IAM OIDC provider.
#[derive(Debug, Clone, Builder)]
pub struct CreateOpenIdConnectProviderRequest {
    /// Provider URL.
    pub url: String,
    /// Client IDs trusted by the provider.
    #[builder(default)]
    pub client_id_list: Vec<String>,
    /// Certificate thumbprints.
    #[builder(default)]
    pub thumbprint_list: Vec<String>,
    /// Provider tags.
    #[builder(default)]
    pub tags: Vec<CreateRoleTag>,
}

/// IAM OIDC provider creation response.
#[derive(Debug, Clone)]
pub struct CreateOpenIdConnectProviderResponse {
    /// Operation result.
    pub create_open_id_connect_provider_result: CreateOpenIdConnectProviderResult,
}

/// IAM OIDC provider creation result.
#[derive(Debug, Clone)]
pub struct CreateOpenIdConnectProviderResult {
    /// Provider ARN.
    pub open_id_connect_provider_arn: String,
}

/// IAM role creation response.
#[derive(Debug, Clone)]
pub struct CreateRoleResponse {
    /// Operation result.
    pub create_role_result: CreateRoleResult,
}

/// IAM role creation result.
#[derive(Debug, Clone)]
pub struct CreateRoleResult {
    /// Created role.
    pub role: Role,
}

/// IAM role metadata used by controllers.
#[derive(Debug, Clone)]
pub struct Role {
    /// IAM path.
    pub path: String,
    /// Role name.
    pub role_name: String,
    /// Role ID.
    pub role_id: String,
    /// Role ARN.
    pub arn: String,
    /// Create timestamp.
    pub create_date: String,
    /// Assume-role trust policy document.
    pub assume_role_policy_document: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Max session duration in seconds.
    pub max_session_duration: Option<i32>,
    /// Permissions boundary metadata.
    pub permissions_boundary: Option<AttachedPermissionsBoundary>,
    /// Tags.
    pub tags: Option<Tags>,
    /// Last-used metadata.
    pub role_last_used: Option<RoleLastUsed>,
}

/// IAM permissions boundary metadata.
#[derive(Debug, Clone)]
pub struct AttachedPermissionsBoundary {
    /// Boundary type.
    pub permissions_boundary_type: Option<String>,
    /// Boundary ARN.
    pub permissions_boundary_arn: Option<String>,
}

/// IAM tag collection wrapper matching existing controller test DTOs.
#[derive(Debug, Clone)]
pub struct Tags {
    /// Tag members.
    pub member: Vec<Tag>,
}

/// IAM tag metadata.
#[derive(Debug, Clone)]
pub struct Tag {
    /// Tag key.
    pub key: String,
    /// Tag value.
    pub value: String,
}

/// IAM role last-used metadata.
#[derive(Debug, Clone)]
pub struct RoleLastUsed {
    /// Last-used timestamp.
    pub last_used_date: Option<String>,
    /// Last-used region.
    pub region: Option<String>,
}

/// IAM get-role response.
#[derive(Debug, Clone)]
pub struct GetRoleResponse {
    /// Operation result.
    pub get_role_result: GetRoleResult,
}

/// IAM get-role result.
#[derive(Debug, Clone)]
pub struct GetRoleResult {
    /// Role metadata.
    pub role: Role,
}

/// IAM inline role policy response.
#[derive(Debug, Clone)]
pub struct GetRolePolicyResponse {
    /// Operation result.
    pub get_role_policy_result: GetRolePolicyResult,
}

/// IAM inline role policy result.
#[derive(Debug, Clone)]
pub struct GetRolePolicyResult {
    /// Role name.
    pub role_name: String,
    /// Policy name.
    pub policy_name: String,
    /// Policy document.
    pub policy_document: String,
}

/// IAM managed policy creation response.
#[derive(Debug, Clone)]
pub struct CreatePolicyResponse {
    /// Operation result.
    pub create_policy_result: CreatePolicyResult,
}

/// IAM managed policy creation result.
#[derive(Debug, Clone)]
pub struct CreatePolicyResult {
    /// Created policy.
    pub policy: Policy,
}

/// IAM managed policy metadata.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Policy name.
    pub policy_name: Option<String>,
    /// Policy ID.
    pub policy_id: Option<String>,
    /// Policy ARN.
    pub arn: String,
    /// IAM path.
    pub path: Option<String>,
    /// Default version ID.
    pub default_version_id: Option<String>,
    /// Attachment count.
    pub attachment_count: Option<i32>,
    /// Whether the policy is attachable.
    pub is_attachable: Option<bool>,
    /// Create timestamp.
    pub create_date: Option<String>,
    /// Update timestamp.
    pub update_date: Option<String>,
}

/// IAM policy version creation response.
#[derive(Debug, Clone)]
pub struct CreatePolicyVersionResponse {
    /// Operation result.
    pub create_policy_version_result: CreatePolicyVersionResult,
}

/// IAM policy version creation result.
#[derive(Debug, Clone)]
pub struct CreatePolicyVersionResult {
    /// Created policy version.
    pub policy_version: PolicyVersion,
}

/// IAM list policy versions response.
#[derive(Debug, Clone)]
pub struct ListPolicyVersionsResponse {
    /// Operation result.
    pub list_policy_versions_result: ListPolicyVersionsResult,
}

/// IAM list policy versions result.
#[derive(Debug, Clone)]
pub struct ListPolicyVersionsResult {
    /// Versions wrapper.
    pub versions: Option<PolicyVersions>,
    /// Whether the result is truncated.
    pub is_truncated: Option<bool>,
    /// Pagination marker.
    pub marker: Option<String>,
}

/// IAM policy version collection wrapper.
#[derive(Debug, Clone)]
pub struct PolicyVersions {
    /// Version members.
    pub member: Vec<PolicyVersion>,
}

/// IAM policy version metadata.
#[derive(Debug, Clone)]
pub struct PolicyVersion {
    /// Policy document.
    pub document: Option<String>,
    /// Version ID.
    pub version_id: String,
    /// Whether this is the default version.
    pub is_default_version: bool,
    /// Create timestamp.
    pub create_date: Option<String>,
}

/// IAM list attached role policies response.
#[derive(Debug, Clone)]
pub struct ListAttachedRolePoliciesResponse {
    /// Operation result.
    pub list_attached_role_policies_result: ListAttachedRolePoliciesResult,
}

/// IAM list attached role policies result.
#[derive(Debug, Clone)]
pub struct ListAttachedRolePoliciesResult {
    /// Attached policies wrapper.
    pub attached_policies: Option<AttachedPolicies>,
    /// Whether the result is truncated.
    pub is_truncated: Option<bool>,
    /// Pagination marker.
    pub marker: Option<String>,
}

/// IAM attached policy collection wrapper.
#[derive(Debug, Clone)]
pub struct AttachedPolicies {
    /// Attached policy members.
    pub member: Vec<AttachedPolicy>,
}

/// IAM attached policy metadata.
#[derive(Debug, Clone)]
pub struct AttachedPolicy {
    /// Policy name.
    pub policy_name: String,
    /// Policy ARN.
    pub policy_arn: String,
}

/// IAM list role policies response.
#[derive(Debug, Clone)]
pub struct ListRolePoliciesResponse {
    /// Operation result.
    pub list_role_policies_result: ListRolePoliciesResult,
}

/// IAM list role policies result.
#[derive(Debug, Clone)]
pub struct ListRolePoliciesResult {
    /// Policy names wrapper.
    pub policy_names: Option<PolicyNames>,
    /// Whether the result is truncated.
    pub is_truncated: Option<bool>,
    /// Pagination marker.
    pub marker: Option<String>,
}

/// IAM inline policy name collection wrapper.
#[derive(Debug, Clone)]
pub struct PolicyNames {
    /// Policy name members.
    pub member: Vec<String>,
}

/// IAM instance profile creation request.
#[derive(Debug, Clone, Builder)]
pub struct CreateInstanceProfileRequest {
    /// Instance profile name.
    pub instance_profile_name: String,
    /// Optional IAM path.
    pub path: Option<String>,
    /// Tags.
    pub tags: Option<Vec<CreateRoleTag>>,
}

/// IAM create instance profile response.
#[derive(Debug, Clone)]
pub struct CreateInstanceProfileResponse {
    /// Operation result.
    pub create_instance_profile_result: CreateInstanceProfileResult,
}

/// IAM create instance profile result.
#[derive(Debug, Clone)]
pub struct CreateInstanceProfileResult {
    /// Instance profile.
    pub instance_profile: InstanceProfile,
}

/// IAM get instance profile response.
#[derive(Debug, Clone)]
pub struct GetInstanceProfileResponse {
    /// Operation result.
    pub get_instance_profile_result: GetInstanceProfileResult,
}

/// IAM get instance profile result.
#[derive(Debug, Clone)]
pub struct GetInstanceProfileResult {
    /// Instance profile.
    pub instance_profile: InstanceProfile,
}

/// IAM list instance profiles request.
#[derive(Debug, Clone, Builder, Default)]
pub struct ListInstanceProfilesRequest {
    /// Path prefix.
    pub path_prefix: Option<String>,
    /// Pagination marker.
    pub marker: Option<String>,
    /// Maximum number of items.
    pub max_items: Option<i32>,
}

/// IAM list instance profiles response.
#[derive(Debug, Clone)]
pub struct ListInstanceProfilesResponse {
    /// Operation result.
    pub list_instance_profiles_result: ListInstanceProfilesResult,
}

/// IAM list instance profiles result.
#[derive(Debug, Clone)]
pub struct ListInstanceProfilesResult {
    /// Instance profiles wrapper.
    pub instance_profiles: Option<InstanceProfiles>,
    /// Whether the result is truncated.
    pub is_truncated: Option<bool>,
    /// Pagination marker.
    pub marker: Option<String>,
}

/// IAM instance profile collection wrapper.
#[derive(Debug, Clone)]
pub struct InstanceProfiles {
    /// Instance profile members.
    pub member: Vec<InstanceProfile>,
}

/// IAM instance profile metadata.
#[derive(Debug, Clone)]
pub struct InstanceProfile {
    /// IAM path.
    pub path: String,
    /// Instance profile name.
    pub instance_profile_name: String,
    /// Instance profile ID.
    pub instance_profile_id: String,
    /// Instance profile ARN.
    pub arn: String,
    /// Create timestamp.
    pub create_date: String,
    /// Roles.
    pub roles: Option<InstanceProfileRoles>,
    /// Tags.
    pub tags: Option<Tags>,
}

/// IAM instance profile roles wrapper.
#[derive(Debug, Clone)]
pub struct InstanceProfileRoles {
    /// Role members.
    pub member: Vec<Role>,
}

/// Trust policy principal.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum TrustPolicyPrincipal {
    /// AWS service principal.
    Service {
        /// Service principal value.
        #[serde(rename = "Service")]
        service: TrustPolicyPrincipalValue,
    },
    /// AWS principal.
    Aws {
        /// AWS principal value.
        #[serde(rename = "AWS")]
        aws: TrustPolicyPrincipalValue,
    },
}

/// Trust policy principal value.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum TrustPolicyPrincipalValue {
    /// Single principal.
    Single(String),
    /// Multiple principals.
    Multiple(Vec<String>),
}

/// Trust policy statement.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TrustPolicyStatement {
    /// Statement effect.
    pub effect: String,
    /// Principal.
    pub principal: TrustPolicyPrincipal,
    /// Action.
    pub action: String,
}

/// Trust policy document.
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TrustPolicyDocument {
    /// Policy version.
    pub version: String,
    /// Statements.
    pub statement: Vec<TrustPolicyStatement>,
}

/// Minimal IAM operations required by infra controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait IamApi: Send + Sync {
    /// Create an IAM role.
    async fn create_role(&self, request: CreateRoleRequest) -> Result<CreateRoleResponse>;
    /// Get an IAM role.
    async fn get_role(&self, role_name: &str) -> Result<GetRoleResponse>;
    /// Delete an IAM role.
    async fn delete_role(&self, role_name: &str) -> Result<()>;
    /// Put an inline role policy.
    async fn put_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<()>;
    /// Get an inline role policy.
    async fn get_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
    ) -> Result<GetRolePolicyResponse>;
    /// Delete an inline role policy.
    async fn delete_role_policy(&self, role_name: &str, policy_name: &str) -> Result<()>;
    /// Update a role trust policy.
    async fn update_assume_role_policy(&self, role_name: &str, policy_document: &str)
        -> Result<()>;
    /// Create an IAM OIDC provider.
    async fn create_open_id_connect_provider(
        &self,
        request: CreateOpenIdConnectProviderRequest,
    ) -> Result<CreateOpenIdConnectProviderResponse>;
    /// Delete an IAM OIDC provider.
    async fn delete_open_id_connect_provider(&self, arn: &str) -> Result<()>;
    /// List managed policies attached to a role.
    async fn list_attached_role_policies(
        &self,
        role_name: &str,
    ) -> Result<ListAttachedRolePoliciesResponse>;
    /// Create a managed policy.
    async fn create_policy(
        &self,
        policy_name: &str,
        policy_document: &str,
        path: Option<String>,
    ) -> Result<CreatePolicyResponse>;
    /// Delete a managed policy.
    async fn delete_policy(&self, policy_arn: &str) -> Result<()>;
    /// Create a managed policy version.
    async fn create_policy_version(
        &self,
        policy_arn: &str,
        policy_document: &str,
        set_as_default: bool,
    ) -> Result<CreatePolicyVersionResponse>;
    /// Delete a managed policy version.
    async fn delete_policy_version(&self, policy_arn: &str, version_id: &str) -> Result<()>;
    /// List managed policy versions.
    async fn list_policy_versions(&self, policy_arn: &str) -> Result<ListPolicyVersionsResponse>;
    /// Attach a managed policy to a role.
    async fn attach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()>;
    /// Detach a managed policy from a role.
    async fn detach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()>;
    /// List inline role policies.
    async fn list_role_policies(&self, role_name: &str) -> Result<ListRolePoliciesResponse>;
    /// Create an instance profile.
    async fn create_instance_profile(
        &self,
        request: CreateInstanceProfileRequest,
    ) -> Result<CreateInstanceProfileResponse>;
    /// Get an instance profile.
    async fn get_instance_profile(
        &self,
        instance_profile_name: &str,
    ) -> Result<GetInstanceProfileResponse>;
    /// Delete an instance profile.
    async fn delete_instance_profile(&self, instance_profile_name: &str) -> Result<()>;
    /// Add a role to an instance profile.
    async fn add_role_to_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()>;
    /// Remove a role from an instance profile.
    async fn remove_role_from_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()>;
    /// List instance profiles.
    async fn list_instance_profiles(
        &self,
        request: ListInstanceProfilesRequest,
    ) -> Result<ListInstanceProfilesResponse>;
}

/// Minimal ACM operations required by infra controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait AcmApi: Send + Sync {
    /// Import a new certificate into ACM.
    async fn import_certificate(
        &self,
        request: ImportCertificateRequest,
    ) -> Result<ImportCertificateResponse>;

    /// Reimport certificate material for an existing ACM certificate.
    async fn reimport_certificate(&self, request: ReimportCertificateRequest) -> Result<()>;

    /// Delete an ACM certificate by ARN.
    async fn delete_certificate(&self, certificate_arn: &str) -> Result<()>;
}

/// Minimal Lambda operations required by worker controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait LambdaApi: Send + Sync {
    /// Create a Lambda function.
    async fn create_function(
        &self,
        request: CreateFunctionRequest,
    ) -> Result<FunctionConfiguration>;
    /// Add an invocation permission statement.
    async fn add_permission(&self, request: AddPermissionRequest) -> Result<AddPermissionResponse>;
    /// Update Lambda function code.
    async fn update_function_code(
        &self,
        request: UpdateFunctionCodeRequest,
    ) -> Result<FunctionConfiguration>;
    /// Update Lambda function configuration.
    async fn update_function_configuration(
        &self,
        request: UpdateFunctionConfigurationRequest,
    ) -> Result<FunctionConfiguration>;
    /// Get Lambda function configuration.
    async fn get_function_configuration(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<FunctionConfiguration>;
    /// Delete a Lambda function.
    async fn delete_function(&self, function_name: &str, qualifier: Option<String>) -> Result<()>;
    /// Create an event-source mapping.
    async fn create_event_source_mapping(
        &self,
        request: CreateEventSourceMappingRequest,
    ) -> Result<CreateEventSourceMappingResponse>;
    /// Delete an event-source mapping.
    async fn delete_event_source_mapping(
        &self,
        uuid: &str,
    ) -> Result<DeleteEventSourceMappingResponse>;
    /// List event-source mappings.
    async fn list_event_source_mappings(
        &self,
        request: ListEventSourceMappingsRequest,
    ) -> Result<ListEventSourceMappingsResponse>;
    /// Put reserved concurrency.
    async fn put_function_concurrency(
        &self,
        function_name: &str,
        reserved_concurrent_executions: u32,
    ) -> Result<()>;
    /// Delete reserved concurrency.
    async fn delete_function_concurrency(&self, function_name: &str) -> Result<()>;
}

/// Minimal API Gateway V2 operations required by worker controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait ApiGatewayV2Api: Send + Sync {
    /// Create an API Gateway HTTP API.
    async fn create_api(
        &self,
        request: ApiGatewayV2CreateApiRequest,
    ) -> Result<ApiGatewayV2CreateApiResponse>;
    /// Get an API Gateway API.
    async fn get_api(&self, api_id: &str) -> Result<ApiGatewayV2GetApiResponse>;
    /// Delete an API Gateway API.
    async fn delete_api(&self, api_id: &str) -> Result<()>;
    /// Create an API integration.
    async fn create_integration(
        &self,
        request: ApiGatewayV2CreateIntegrationRequest,
    ) -> Result<ApiGatewayV2CreateIntegrationResponse>;
    /// Create an API route.
    async fn create_route(
        &self,
        request: ApiGatewayV2CreateRouteRequest,
    ) -> Result<ApiGatewayV2CreateRouteResponse>;
    /// Create an API stage.
    async fn create_stage(
        &self,
        request: ApiGatewayV2CreateStageRequest,
    ) -> Result<ApiGatewayV2CreateStageResponse>;
    /// Create a custom domain name.
    async fn create_domain_name(
        &self,
        request: ApiGatewayV2CreateDomainNameRequest,
    ) -> Result<ApiGatewayV2CreateDomainNameResponse>;
    /// Get a custom domain name.
    async fn get_domain_name(&self, domain_name: &str)
        -> Result<ApiGatewayV2GetDomainNameResponse>;
    /// Delete a custom domain name.
    async fn delete_domain_name(&self, domain_name: &str) -> Result<()>;
    /// Create a custom domain API mapping.
    async fn create_api_mapping(
        &self,
        request: ApiGatewayV2CreateApiMappingRequest,
    ) -> Result<ApiGatewayV2CreateApiMappingResponse>;
    /// List custom domain API mappings.
    async fn get_api_mappings(
        &self,
        domain_name: &str,
    ) -> Result<ApiGatewayV2GetApiMappingsResponse>;
    /// Delete a custom domain API mapping.
    async fn delete_api_mapping(&self, domain_name: &str, api_mapping_id: &str) -> Result<()>;
}

/// Minimal EventBridge operations required by worker schedule triggers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait EventBridgeApi: Send + Sync {
    /// Create or update a rule.
    async fn put_rule(&self, request: PutRuleRequest) -> Result<PutRuleResponse>;
    /// Add or update targets on a rule.
    async fn put_targets(&self, request: PutTargetsRequest) -> Result<()>;
    /// Remove targets from a rule.
    async fn remove_targets(&self, rule_name: &str, target_ids: Vec<String>) -> Result<()>;
    /// Delete a rule.
    async fn delete_rule(&self, rule_name: &str) -> Result<()>;
}

/// Minimal SSM operations required by infra controllers.
#[async_trait]
pub trait SsmApi: Send + Sync {
    /// Describe parameter metadata whose name begins with the provided prefix.
    async fn describe_parameters_by_prefix(
        &self,
        prefix: &str,
        max_results: i32,
    ) -> Result<DescribeSsmParametersResponse>;
}

/// Minimal SQS operations required by infra controllers.
#[async_trait]
pub trait SqsApi: Send + Sync {
    /// Create a queue and return its queue URL.
    async fn create_queue(&self, queue_name: &str, tags: HashMap<String, String>)
        -> Result<String>;

    /// Return selected queue attributes keyed by AWS SQS attribute name.
    async fn get_queue_attributes(
        &self,
        queue_url: &str,
        attribute_names: Vec<String>,
    ) -> Result<HashMap<String, String>>;

    /// Set queue attributes keyed by AWS SQS attribute name.
    async fn set_queue_attributes(
        &self,
        queue_url: &str,
        attributes: HashMap<String, String>,
    ) -> Result<()>;

    /// Delete a queue by URL.
    async fn delete_queue(&self, queue_url: &str) -> Result<()>;
}

/// Minimal DynamoDB operations required by infra controllers.
#[async_trait]
pub trait DynamoDbApi: Send + Sync {
    /// Create the KV table shape used by Alien.
    async fn create_kv_table(&self, table_name: &str, tags: HashMap<String, String>) -> Result<()>;

    /// Describe a table, returning None when the table is not found.
    async fn describe_table(&self, table_name: &str) -> Result<Option<DynamoDbTableDescription>>;

    /// Enable DynamoDB TTL on the named attribute.
    async fn enable_ttl(&self, table_name: &str, attribute_name: &str) -> Result<()>;

    /// Describe TTL metadata for a table.
    async fn describe_ttl(&self, table_name: &str) -> Result<Option<DynamoDbTtlDescription>>;

    /// Delete a table. Returns false when it was already absent.
    async fn delete_table(&self, table_name: &str) -> Result<bool>;
}

/// Minimal CodeBuild operations required by infra controllers.
#[async_trait]
pub trait CodeBuildApi: Send + Sync {
    /// Create a CodeBuild project.
    async fn create_project(
        &self,
        config: CodeBuildProjectConfig,
    ) -> Result<CodeBuildProjectDescription>;

    /// Update a CodeBuild project.
    async fn update_project(
        &self,
        config: CodeBuildProjectConfig,
    ) -> Result<CodeBuildProjectDescription>;

    /// Get a project by name.
    async fn get_project(&self, project_name: &str) -> Result<Option<CodeBuildProjectDescription>>;

    /// Delete a project by name.
    async fn delete_project(&self, project_name: &str) -> Result<()>;
}

/// Minimal ECR operations required by infra artifact registry controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait EcrApi: Send + Sync {
    /// Describe ECR repositories.
    async fn describe_repositories(
        &self,
        request: DescribeEcrRepositoriesRequest,
    ) -> Result<DescribeEcrRepositoriesResponse>;

    /// Describe registry-level replication settings.
    async fn describe_registry(&self) -> Result<EcrReplicationConfiguration>;

    /// Put registry-level replication settings.
    async fn put_replication_configuration(
        &self,
        replication_configuration: EcrReplicationConfiguration,
    ) -> Result<EcrReplicationConfiguration>;
}

/// Minimal EC2 operations required by infra network and worker controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait Ec2Api: Send + Sync {
    /// Describe VPCs.
    async fn describe_vpcs(&self, request: DescribeVpcsRequest) -> Result<DescribeVpcsResponse>;
    /// Create a VPC.
    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse>;
    /// Delete a VPC.
    async fn delete_vpc(&self, vpc_id: &str) -> Result<()>;
    /// Modify a VPC attribute.
    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()>;
    /// Describe subnets.
    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse>;
    /// Create a subnet.
    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse>;
    /// Delete a subnet.
    async fn delete_subnet(&self, subnet_id: &str) -> Result<()>;
    /// Create an internet gateway.
    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse>;
    /// Delete an internet gateway.
    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()>;
    /// Attach an internet gateway.
    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()>;
    /// Detach an internet gateway.
    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()>;
    /// Create a NAT gateway.
    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse>;
    /// Delete a NAT gateway.
    async fn delete_nat_gateway(&self, nat_gateway_id: &str) -> Result<DeleteNatGatewayResponse>;
    /// Describe NAT gateways.
    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse>;
    /// Allocate an Elastic IP.
    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse>;
    /// Release an Elastic IP.
    async fn release_address(&self, allocation_id: &str) -> Result<()>;
    /// Create a route table.
    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse>;
    /// Delete a route table.
    async fn delete_route_table(&self, route_table_id: &str) -> Result<()>;
    /// Create a route.
    async fn create_route(&self, request: CreateRouteRequest) -> Result<()>;
    /// Associate a route table.
    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse>;
    /// Disassociate a route table.
    async fn disassociate_route_table(&self, association_id: &str) -> Result<()>;
    /// Describe security groups.
    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse>;
    /// Describe network interfaces.
    async fn describe_network_interfaces(
        &self,
        request: DescribeNetworkInterfacesRequest,
    ) -> Result<DescribeNetworkInterfacesResponse>;
    /// Create a security group.
    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse>;
    /// Delete a security group.
    async fn delete_security_group(&self, group_id: &str) -> Result<()>;
    /// Authorize ingress rules.
    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()>;
    /// Authorize egress rules.
    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()>;
    /// Describe availability zones.
    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse>;
}

/// Minimal S3 operations required by infra controllers.
#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait S3Api: Send + Sync {
    /// Create a bucket. Already-owned buckets are treated as success.
    async fn create_bucket(&self, bucket_name: &str) -> Result<()>;

    /// Put ABAC tags on a bucket.
    async fn put_bucket_abac_tags(
        &self,
        bucket_name: &str,
        tags: &HashMap<String, String>,
    ) -> Result<()>;

    /// Configure bucket versioning.
    async fn put_bucket_versioning(
        &self,
        bucket_name: &str,
        status: S3VersioningStatus,
    ) -> Result<()>;

    /// Configure public access blocking.
    async fn put_public_access_block(
        &self,
        bucket_name: &str,
        config: S3PublicAccessBlock,
    ) -> Result<()>;

    /// Put a bucket policy document.
    async fn put_bucket_policy(&self, bucket_name: &str, policy: &str) -> Result<()>;

    /// Delete the bucket policy. Missing policies are treated as success.
    async fn delete_bucket_policy(&self, bucket_name: &str) -> Result<()>;

    /// Put lifecycle rules.
    async fn put_bucket_lifecycle_configuration(
        &self,
        bucket_name: &str,
        rules: Vec<S3LifecycleRuleConfig>,
    ) -> Result<()>;

    /// Delete lifecycle configuration. Missing lifecycle configuration is treated as success.
    async fn delete_bucket_lifecycle(&self, bucket_name: &str) -> Result<()>;

    /// Collect bucket metadata for heartbeat emission.
    async fn get_bucket_metadata(&self, bucket_name: &str) -> Result<S3BucketMetadata>;

    /// Empty a bucket, including versions and delete markers.
    async fn empty_bucket(&self, bucket_name: &str) -> Result<()>;

    /// Delete a bucket. Returns false when the bucket is already absent.
    async fn delete_bucket(&self, bucket_name: &str) -> Result<bool>;

    /// Get bucket notification configuration.
    async fn get_bucket_notification_configuration(
        &self,
        bucket_name: &str,
    ) -> Result<NotificationConfiguration>;

    /// Put bucket notification configuration.
    async fn put_bucket_notification_configuration(
        &self,
        bucket_name: &str,
        config: &NotificationConfiguration,
    ) -> Result<()>;
}

#[async_trait]
impl IamApi for IamClient {
    async fn create_role(&self, request: CreateRoleRequest) -> Result<CreateRoleResponse> {
        let role_name = request.role_name.clone();
        let tags = iam_tags(request.tags.unwrap_or_default(), &role_name)?;
        let response = iam_result(
            self.create_role()
                .role_name(&role_name)
                .assume_role_policy_document(request.assume_role_policy_document)
                .set_path(request.path)
                .set_description(request.description)
                .set_max_session_duration(request.max_session_duration)
                .set_tags(nonempty_vec(tags))
                .send()
                .await,
            "CreateRole",
            "IAM Role",
            &role_name,
        )?;

        let role = response.role().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM CreateRole response for '{role_name}' did not include a role"
                ),
                resource_id: None,
            })
        })?;

        Ok(CreateRoleResponse {
            create_role_result: CreateRoleResult {
                role: iam_role(role),
            },
        })
    }

    async fn get_role(&self, role_name: &str) -> Result<GetRoleResponse> {
        let response = iam_result(
            self.get_role().role_name(role_name).send().await,
            "GetRole",
            "IAM Role",
            role_name,
        )?;

        let role = response.role().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("IAM GetRole response for '{role_name}' did not include a role"),
                resource_id: None,
            })
        })?;

        Ok(GetRoleResponse {
            get_role_result: GetRoleResult {
                role: iam_role(role),
            },
        })
    }

    async fn delete_role(&self, role_name: &str) -> Result<()> {
        iam_result(
            self.delete_role().role_name(role_name).send().await,
            "DeleteRole",
            "IAM Role",
            role_name,
        )?;
        Ok(())
    }

    async fn put_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
        policy_document: &str,
    ) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_name}");
        iam_result(
            self.put_role_policy()
                .role_name(role_name)
                .policy_name(policy_name)
                .policy_document(policy_document)
                .send()
                .await,
            "PutRolePolicy",
            "IAM RolePolicy",
            &resource_name,
        )?;
        Ok(())
    }

    async fn get_role_policy(
        &self,
        role_name: &str,
        policy_name: &str,
    ) -> Result<GetRolePolicyResponse> {
        let resource_name = format!("{role_name}/{policy_name}");
        let response = iam_result(
            self.get_role_policy()
                .role_name(role_name)
                .policy_name(policy_name)
                .send()
                .await,
            "GetRolePolicy",
            "IAM RolePolicy",
            &resource_name,
        )?;

        Ok(GetRolePolicyResponse {
            get_role_policy_result: GetRolePolicyResult {
                role_name: response.role_name().to_string(),
                policy_name: response.policy_name().to_string(),
                policy_document: response.policy_document().to_string(),
            },
        })
    }

    async fn delete_role_policy(&self, role_name: &str, policy_name: &str) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_name}");
        iam_result(
            self.delete_role_policy()
                .role_name(role_name)
                .policy_name(policy_name)
                .send()
                .await,
            "DeleteRolePolicy",
            "IAM RolePolicy",
            &resource_name,
        )?;
        Ok(())
    }

    async fn update_assume_role_policy(
        &self,
        role_name: &str,
        policy_document: &str,
    ) -> Result<()> {
        iam_result(
            self.update_assume_role_policy()
                .role_name(role_name)
                .policy_document(policy_document)
                .send()
                .await,
            "UpdateAssumeRolePolicy",
            "IAM Role",
            role_name,
        )?;
        Ok(())
    }

    async fn create_open_id_connect_provider(
        &self,
        request: CreateOpenIdConnectProviderRequest,
    ) -> Result<CreateOpenIdConnectProviderResponse> {
        let url = request.url.clone();
        let tags = iam_tags(request.tags, &url)?;
        let response = iam_result(
            self.create_open_id_connect_provider()
                .url(&url)
                .set_client_id_list(nonempty_vec(request.client_id_list))
                .set_thumbprint_list(nonempty_vec(request.thumbprint_list))
                .set_tags(nonempty_vec(tags))
                .send()
                .await,
            "CreateOpenIDConnectProvider",
            "IAM OpenIDConnectProvider",
            &url,
        )?;

        let arn = response.open_id_connect_provider_arn().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM CreateOpenIDConnectProvider response for '{url}' did not include an ARN"
                ),
                resource_id: None,
            })
        })?;

        Ok(CreateOpenIdConnectProviderResponse {
            create_open_id_connect_provider_result: CreateOpenIdConnectProviderResult {
                open_id_connect_provider_arn: arn.to_string(),
            },
        })
    }

    async fn delete_open_id_connect_provider(&self, arn: &str) -> Result<()> {
        iam_result(
            self.delete_open_id_connect_provider()
                .open_id_connect_provider_arn(arn)
                .send()
                .await,
            "DeleteOpenIDConnectProvider",
            "IAM OpenIDConnectProvider",
            arn,
        )?;
        Ok(())
    }

    async fn list_attached_role_policies(
        &self,
        role_name: &str,
    ) -> Result<ListAttachedRolePoliciesResponse> {
        let response = iam_result(
            self.list_attached_role_policies()
                .role_name(role_name)
                .send()
                .await,
            "ListAttachedRolePolicies",
            "IAM Role",
            role_name,
        )?;

        Ok(ListAttachedRolePoliciesResponse {
            list_attached_role_policies_result: ListAttachedRolePoliciesResult {
                attached_policies: Some(AttachedPolicies {
                    member: response
                        .attached_policies()
                        .iter()
                        .map(iam_attached_policy)
                        .collect(),
                }),
                is_truncated: Some(response.is_truncated()),
                marker: response.marker().map(ToString::to_string),
            },
        })
    }

    async fn create_policy(
        &self,
        policy_name: &str,
        policy_document: &str,
        path: Option<String>,
    ) -> Result<CreatePolicyResponse> {
        let response = iam_result(
            self.create_policy()
                .policy_name(policy_name)
                .policy_document(policy_document)
                .set_path(path)
                .send()
                .await,
            "CreatePolicy",
            "IAM Policy",
            policy_name,
        )?;

        let policy = response.policy().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM CreatePolicy response for '{policy_name}' did not include a policy"
                ),
                resource_id: None,
            })
        })?;

        Ok(CreatePolicyResponse {
            create_policy_result: CreatePolicyResult {
                policy: iam_policy(policy)?,
            },
        })
    }

    async fn delete_policy(&self, policy_arn: &str) -> Result<()> {
        iam_result(
            self.delete_policy().policy_arn(policy_arn).send().await,
            "DeletePolicy",
            "IAM Policy",
            policy_arn,
        )?;
        Ok(())
    }

    async fn create_policy_version(
        &self,
        policy_arn: &str,
        policy_document: &str,
        set_as_default: bool,
    ) -> Result<CreatePolicyVersionResponse> {
        let response = iam_result(
            self.create_policy_version()
                .policy_arn(policy_arn)
                .policy_document(policy_document)
                .set_as_default(set_as_default)
                .send()
                .await,
            "CreatePolicyVersion",
            "IAM Policy",
            policy_arn,
        )?;

        let version = response.policy_version().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM CreatePolicyVersion response for '{policy_arn}' did not include a version"
                ),
                resource_id: None,
            })
        })?;

        Ok(CreatePolicyVersionResponse {
            create_policy_version_result: CreatePolicyVersionResult {
                policy_version: iam_policy_version(version),
            },
        })
    }

    async fn delete_policy_version(&self, policy_arn: &str, version_id: &str) -> Result<()> {
        let resource_name = format!("{policy_arn}/{version_id}");
        iam_result(
            self.delete_policy_version()
                .policy_arn(policy_arn)
                .version_id(version_id)
                .send()
                .await,
            "DeletePolicyVersion",
            "IAM PolicyVersion",
            &resource_name,
        )?;
        Ok(())
    }

    async fn list_policy_versions(&self, policy_arn: &str) -> Result<ListPolicyVersionsResponse> {
        let response = iam_result(
            self.list_policy_versions()
                .policy_arn(policy_arn)
                .send()
                .await,
            "ListPolicyVersions",
            "IAM Policy",
            policy_arn,
        )?;

        Ok(ListPolicyVersionsResponse {
            list_policy_versions_result: ListPolicyVersionsResult {
                versions: Some(PolicyVersions {
                    member: response.versions().iter().map(iam_policy_version).collect(),
                }),
                is_truncated: Some(response.is_truncated()),
                marker: response.marker().map(ToString::to_string),
            },
        })
    }

    async fn attach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_arn}");
        iam_result(
            self.attach_role_policy()
                .role_name(role_name)
                .policy_arn(policy_arn)
                .send()
                .await,
            "AttachRolePolicy",
            "IAM RolePolicyAttachment",
            &resource_name,
        )?;
        Ok(())
    }

    async fn detach_role_policy(&self, role_name: &str, policy_arn: &str) -> Result<()> {
        let resource_name = format!("{role_name}/{policy_arn}");
        iam_result(
            self.detach_role_policy()
                .role_name(role_name)
                .policy_arn(policy_arn)
                .send()
                .await,
            "DetachRolePolicy",
            "IAM RolePolicyAttachment",
            &resource_name,
        )?;
        Ok(())
    }

    async fn list_role_policies(&self, role_name: &str) -> Result<ListRolePoliciesResponse> {
        let response = iam_result(
            self.list_role_policies().role_name(role_name).send().await,
            "ListRolePolicies",
            "IAM Role",
            role_name,
        )?;

        Ok(ListRolePoliciesResponse {
            list_role_policies_result: ListRolePoliciesResult {
                policy_names: Some(PolicyNames {
                    member: response.policy_names().to_vec(),
                }),
                is_truncated: Some(response.is_truncated()),
                marker: response.marker().map(ToString::to_string),
            },
        })
    }

    async fn create_instance_profile(
        &self,
        request: CreateInstanceProfileRequest,
    ) -> Result<CreateInstanceProfileResponse> {
        let instance_profile_name = request.instance_profile_name.clone();
        let tags = iam_tags(request.tags.unwrap_or_default(), &instance_profile_name)?;
        let response = iam_result(
            self.create_instance_profile()
                .instance_profile_name(&instance_profile_name)
                .set_path(request.path)
                .set_tags(nonempty_vec(tags))
                .send()
                .await,
            "CreateInstanceProfile",
            "IAM InstanceProfile",
            &instance_profile_name,
        )?;

        let instance_profile = response.instance_profile().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("IAM CreateInstanceProfile response for '{instance_profile_name}' did not include a profile"),
                resource_id: None,
            })
        })?;

        Ok(CreateInstanceProfileResponse {
            create_instance_profile_result: CreateInstanceProfileResult {
                instance_profile: iam_instance_profile(instance_profile),
            },
        })
    }

    async fn get_instance_profile(
        &self,
        instance_profile_name: &str,
    ) -> Result<GetInstanceProfileResponse> {
        let response = iam_result(
            self.get_instance_profile()
                .instance_profile_name(instance_profile_name)
                .send()
                .await,
            "GetInstanceProfile",
            "IAM InstanceProfile",
            instance_profile_name,
        )?;

        let instance_profile = response.instance_profile().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("IAM GetInstanceProfile response for '{instance_profile_name}' did not include a profile"),
                resource_id: None,
            })
        })?;

        Ok(GetInstanceProfileResponse {
            get_instance_profile_result: GetInstanceProfileResult {
                instance_profile: iam_instance_profile(instance_profile),
            },
        })
    }

    async fn delete_instance_profile(&self, instance_profile_name: &str) -> Result<()> {
        iam_result(
            self.delete_instance_profile()
                .instance_profile_name(instance_profile_name)
                .send()
                .await,
            "DeleteInstanceProfile",
            "IAM InstanceProfile",
            instance_profile_name,
        )?;
        Ok(())
    }

    async fn add_role_to_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()> {
        let resource_name = format!("{instance_profile_name}/{role_name}");
        iam_result(
            self.add_role_to_instance_profile()
                .instance_profile_name(instance_profile_name)
                .role_name(role_name)
                .send()
                .await,
            "AddRoleToInstanceProfile",
            "IAM InstanceProfileRole",
            &resource_name,
        )?;
        Ok(())
    }

    async fn remove_role_from_instance_profile(
        &self,
        instance_profile_name: &str,
        role_name: &str,
    ) -> Result<()> {
        let resource_name = format!("{instance_profile_name}/{role_name}");
        iam_result(
            self.remove_role_from_instance_profile()
                .instance_profile_name(instance_profile_name)
                .role_name(role_name)
                .send()
                .await,
            "RemoveRoleFromInstanceProfile",
            "IAM InstanceProfileRole",
            &resource_name,
        )?;
        Ok(())
    }

    async fn list_instance_profiles(
        &self,
        request: ListInstanceProfilesRequest,
    ) -> Result<ListInstanceProfilesResponse> {
        let resource_name = request
            .path_prefix
            .clone()
            .unwrap_or_else(|| "*".to_string());
        let response = iam_result(
            self.list_instance_profiles()
                .set_path_prefix(request.path_prefix)
                .set_marker(request.marker)
                .set_max_items(request.max_items)
                .send()
                .await,
            "ListInstanceProfiles",
            "IAM InstanceProfile",
            &resource_name,
        )?;

        Ok(ListInstanceProfilesResponse {
            list_instance_profiles_result: ListInstanceProfilesResult {
                instance_profiles: Some(InstanceProfiles {
                    member: response
                        .instance_profiles()
                        .iter()
                        .map(iam_instance_profile)
                        .collect(),
                }),
                is_truncated: Some(response.is_truncated()),
                marker: response.marker().map(ToString::to_string),
            },
        })
    }
}

#[async_trait]
impl AcmApi for AcmClient {
    async fn import_certificate(
        &self,
        request: ImportCertificateRequest,
    ) -> Result<ImportCertificateResponse> {
        let response = acm_result(
            self.import_certificate()
                .set_certificate_arn(request.certificate_arn)
                .set_certificate(request.certificate)
                .set_private_key(request.private_key)
                .set_certificate_chain(request.certificate_chain)
                .set_tags(request.tags)
                .send()
                .await,
            "ImportCertificate",
            "Certificate",
            "new",
        )?;

        if response.certificate_arn().is_none() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: "ACM ImportCertificate response did not include certificateArn"
                    .to_string(),
                resource_id: None,
            }));
        }

        Ok(response)
    }

    async fn reimport_certificate(&self, request: ReimportCertificateRequest) -> Result<()> {
        let certificate_arn = request.certificate_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "ACM reimport request did not include certificateArn".to_string(),
                resource_id: None,
            })
        })?;

        acm_result(
            self.import_certificate()
                .set_certificate_arn(request.certificate_arn)
                .set_certificate(request.certificate)
                .set_private_key(request.private_key)
                .set_certificate_chain(request.certificate_chain)
                .set_tags(request.tags)
                .send()
                .await,
            "ImportCertificate",
            "Certificate",
            &certificate_arn,
        )?;
        Ok(())
    }

    async fn delete_certificate(&self, certificate_arn: &str) -> Result<()> {
        acm_result(
            self.delete_certificate()
                .certificate_arn(certificate_arn)
                .send()
                .await,
            "DeleteCertificate",
            "Certificate",
            certificate_arn,
        )?;
        Ok(())
    }
}

#[async_trait]
impl LambdaApi for LambdaClient {
    async fn create_function(
        &self,
        request: CreateFunctionRequest,
    ) -> Result<FunctionConfiguration> {
        let output = lambda_result(
            self.create_function()
                .function_name(request.function_name.clone())
                .role(request.role)
                .code(request.code)
                .package_type(PackageType::from(request.package_type.as_str()))
                .set_description(request.description)
                .set_timeout(request.timeout)
                .set_memory_size(request.memory_size)
                .set_publish(request.publish)
                .set_environment(request.environment)
                .set_architectures(request.architectures.map(lambda_architectures_to_aws))
                .set_tags(request.tags)
                .set_kms_key_arn(request.kms_key_arn)
                .set_vpc_config(request.vpc_config)
                .send()
                .await,
            "CreateFunction",
            "LambdaFunction",
            &request.function_name,
        )?;

        Ok(function_configuration_from_create_output(output))
    }

    async fn add_permission(&self, request: AddPermissionRequest) -> Result<AddPermissionResponse> {
        let function_name = request.function_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "AddPermission request did not include functionName".to_string(),
                resource_id: None,
            })
        })?;
        let output = lambda_result(
            self.add_permission()
                .set_function_name(request.function_name)
                .set_statement_id(request.statement_id)
                .set_action(request.action)
                .set_principal(request.principal)
                .set_source_arn(request.source_arn)
                .set_source_account(request.source_account)
                .set_event_source_token(request.event_source_token)
                .set_qualifier(request.qualifier)
                .set_revision_id(request.revision_id)
                .set_principal_org_id(request.principal_org_id)
                .set_function_url_auth_type(request.function_url_auth_type)
                .set_invoked_via_function_url(request.invoked_via_function_url)
                .send()
                .await,
            "AddPermission",
            "LambdaFunction",
            &function_name,
        )?;

        Ok(output)
    }

    async fn update_function_code(
        &self,
        request: UpdateFunctionCodeRequest,
    ) -> Result<FunctionConfiguration> {
        let function_name = request.function_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "UpdateFunctionCode request did not include functionName".to_string(),
                resource_id: None,
            })
        })?;
        let output = lambda_result(
            self.update_function_code()
                .set_function_name(request.function_name)
                .set_zip_file(request.zip_file)
                .set_s3_bucket(request.s3_bucket)
                .set_s3_key(request.s3_key)
                .set_s3_object_version(request.s3_object_version)
                .set_image_uri(request.image_uri)
                .set_publish(request.publish)
                .set_dry_run(request.dry_run)
                .set_revision_id(request.revision_id)
                .set_architectures(request.architectures)
                .set_source_kms_key_arn(request.source_kms_key_arn)
                .set_publish_to(request.publish_to)
                .send()
                .await,
            "UpdateFunctionCode",
            "LambdaFunction",
            &function_name,
        )?;

        Ok(function_configuration_from_update_code_output(output))
    }

    async fn update_function_configuration(
        &self,
        request: UpdateFunctionConfigurationRequest,
    ) -> Result<FunctionConfiguration> {
        let function_name = request.function_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "UpdateFunctionConfiguration request did not include functionName"
                    .to_string(),
                resource_id: None,
            })
        })?;
        let output = lambda_result(
            self.update_function_configuration()
                .set_function_name(request.function_name)
                .set_role(request.role)
                .set_handler(request.handler)
                .set_description(request.description)
                .set_timeout(request.timeout)
                .set_memory_size(request.memory_size)
                .set_vpc_config(request.vpc_config)
                .set_environment(request.environment)
                .set_runtime(request.runtime)
                .set_dead_letter_config(request.dead_letter_config)
                .set_kms_key_arn(request.kms_key_arn)
                .set_tracing_config(request.tracing_config)
                .set_revision_id(request.revision_id)
                .set_layers(request.layers)
                .set_file_system_configs(request.file_system_configs)
                .set_image_config(request.image_config)
                .set_ephemeral_storage(request.ephemeral_storage)
                .set_snap_start(request.snap_start)
                .set_logging_config(request.logging_config)
                .set_capacity_provider_config(request.capacity_provider_config)
                .set_durable_config(request.durable_config)
                .send()
                .await,
            "UpdateFunctionConfiguration",
            "LambdaFunction",
            &function_name,
        )?;

        Ok(function_configuration_from_update_config_output(output))
    }

    async fn get_function_configuration(
        &self,
        function_name: &str,
        qualifier: Option<String>,
    ) -> Result<FunctionConfiguration> {
        let output = lambda_result(
            self.get_function_configuration()
                .function_name(function_name)
                .set_qualifier(qualifier)
                .send()
                .await,
            "GetFunctionConfiguration",
            "LambdaFunction",
            function_name,
        )?;

        Ok(function_configuration_from_get_output(output))
    }

    async fn delete_function(&self, function_name: &str, qualifier: Option<String>) -> Result<()> {
        lambda_result(
            self.delete_function()
                .function_name(function_name)
                .set_qualifier(qualifier)
                .send()
                .await,
            "DeleteFunction",
            "LambdaFunction",
            function_name,
        )?;
        Ok(())
    }

    async fn create_event_source_mapping(
        &self,
        request: CreateEventSourceMappingRequest,
    ) -> Result<CreateEventSourceMappingResponse> {
        let resource_name = request
            .event_source_arn
            .as_deref()
            .or(request.function_name.as_deref())
            .unwrap_or("unknown")
            .to_string();
        let output = lambda_result(
            self.create_event_source_mapping()
                .set_event_source_arn(request.event_source_arn)
                .set_function_name(request.function_name)
                .set_enabled(request.enabled)
                .set_batch_size(request.batch_size)
                .set_filter_criteria(request.filter_criteria)
                .set_maximum_batching_window_in_seconds(request.maximum_batching_window_in_seconds)
                .set_parallelization_factor(request.parallelization_factor)
                .set_starting_position(request.starting_position)
                .set_starting_position_timestamp(request.starting_position_timestamp)
                .set_destination_config(request.destination_config)
                .set_maximum_record_age_in_seconds(request.maximum_record_age_in_seconds)
                .set_bisect_batch_on_function_error(request.bisect_batch_on_function_error)
                .set_maximum_retry_attempts(request.maximum_retry_attempts)
                .set_tags(request.tags)
                .set_tumbling_window_in_seconds(request.tumbling_window_in_seconds)
                .set_topics(request.topics)
                .set_queues(request.queues)
                .set_source_access_configurations(request.source_access_configurations)
                .set_self_managed_event_source(request.self_managed_event_source)
                .set_function_response_types(request.function_response_types)
                .set_amazon_managed_kafka_event_source_config(
                    request.amazon_managed_kafka_event_source_config,
                )
                .set_self_managed_kafka_event_source_config(
                    request.self_managed_kafka_event_source_config,
                )
                .set_scaling_config(request.scaling_config)
                .set_document_db_event_source_config(request.document_db_event_source_config)
                .set_kms_key_arn(request.kms_key_arn)
                .set_metrics_config(request.metrics_config)
                .set_logging_config(request.logging_config)
                .set_provisioned_poller_config(request.provisioned_poller_config)
                .send()
                .await,
            "CreateEventSourceMapping",
            "EventSourceMapping",
            &resource_name,
        )?;

        Ok(output)
    }

    async fn delete_event_source_mapping(
        &self,
        uuid: &str,
    ) -> Result<DeleteEventSourceMappingResponse> {
        let output = lambda_result(
            self.delete_event_source_mapping().uuid(uuid).send().await,
            "DeleteEventSourceMapping",
            "EventSourceMapping",
            uuid,
        )?;

        Ok(output)
    }

    async fn list_event_source_mappings(
        &self,
        request: ListEventSourceMappingsRequest,
    ) -> Result<ListEventSourceMappingsResponse> {
        let resource_name = request
            .event_source_arn
            .as_deref()
            .or(request.function_name.as_deref())
            .unwrap_or("all")
            .to_string();
        let output = lambda_result(
            self.list_event_source_mappings()
                .set_event_source_arn(request.event_source_arn)
                .set_function_name(request.function_name)
                .set_marker(request.marker)
                .set_max_items(request.max_items)
                .send()
                .await,
            "ListEventSourceMappings",
            "EventSourceMapping",
            &resource_name,
        )?;

        Ok(output)
    }

    async fn put_function_concurrency(
        &self,
        function_name: &str,
        reserved_concurrent_executions: u32,
    ) -> Result<()> {
        let reserved_concurrent_executions =
            i32::try_from(reserved_concurrent_executions).map_err(|_| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Lambda reserved concurrency '{reserved_concurrent_executions}' exceeds i32 range"
                    ),
                    resource_id: Some(function_name.to_string()),
                })
            })?;

        lambda_result(
            self.put_function_concurrency()
                .function_name(function_name)
                .reserved_concurrent_executions(reserved_concurrent_executions)
                .send()
                .await,
            "PutFunctionConcurrency",
            "LambdaFunction",
            function_name,
        )?;
        Ok(())
    }

    async fn delete_function_concurrency(&self, function_name: &str) -> Result<()> {
        lambda_result(
            self.delete_function_concurrency()
                .function_name(function_name)
                .send()
                .await,
            "DeleteFunctionConcurrency",
            "LambdaFunction",
            function_name,
        )?;
        Ok(())
    }
}

#[async_trait]
impl ApiGatewayV2Api for ApiGatewayV2Client {
    async fn create_api(
        &self,
        request: ApiGatewayV2CreateApiRequest,
    ) -> Result<ApiGatewayV2CreateApiResponse> {
        let resource_name = request
            .name
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = api_gateway_v2_result(
            self.create_api()
                .set_api_key_selection_expression(request.api_key_selection_expression)
                .set_cors_configuration(request.cors_configuration)
                .set_credentials_arn(request.credentials_arn)
                .set_description(request.description)
                .set_disable_schema_validation(request.disable_schema_validation)
                .set_disable_execute_api_endpoint(request.disable_execute_api_endpoint)
                .set_ip_address_type(request.ip_address_type)
                .set_name(request.name)
                .set_protocol_type(request.protocol_type)
                .set_route_key(request.route_key)
                .set_route_selection_expression(request.route_selection_expression)
                .set_tags(request.tags)
                .set_target(request.target)
                .set_version(request.version)
                .send()
                .await,
            "CreateApi",
            "ApiGatewayApi",
            &resource_name,
        )?;

        Ok(output)
    }

    async fn get_api(&self, api_id: &str) -> Result<ApiGatewayV2GetApiResponse> {
        let output = api_gateway_v2_result(
            self.get_api().api_id(api_id).send().await,
            "GetApi",
            "ApiGatewayApi",
            api_id,
        )?;

        Ok(output)
    }

    async fn delete_api(&self, api_id: &str) -> Result<()> {
        api_gateway_v2_result(
            self.delete_api().api_id(api_id).send().await,
            "DeleteApi",
            "ApiGatewayApi",
            api_id,
        )?;
        Ok(())
    }

    async fn create_integration(
        &self,
        request: ApiGatewayV2CreateIntegrationRequest,
    ) -> Result<ApiGatewayV2CreateIntegrationResponse> {
        let api_id = request
            .api_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = api_gateway_v2_result(
            self.create_integration()
                .set_api_id(request.api_id)
                .set_connection_id(request.connection_id)
                .set_connection_type(request.connection_type)
                .set_content_handling_strategy(request.content_handling_strategy)
                .set_credentials_arn(request.credentials_arn)
                .set_description(request.description)
                .set_integration_method(request.integration_method)
                .set_integration_subtype(request.integration_subtype)
                .set_integration_type(request.integration_type)
                .set_integration_uri(request.integration_uri)
                .set_passthrough_behavior(request.passthrough_behavior)
                .set_payload_format_version(request.payload_format_version)
                .set_request_parameters(request.request_parameters)
                .set_request_templates(request.request_templates)
                .set_response_parameters(request.response_parameters)
                .set_template_selection_expression(request.template_selection_expression)
                .set_timeout_in_millis(request.timeout_in_millis)
                .set_tls_config(request.tls_config)
                .send()
                .await,
            "CreateIntegration",
            "ApiGatewayIntegration",
            &api_id,
        )?;

        Ok(output)
    }

    async fn create_route(
        &self,
        request: ApiGatewayV2CreateRouteRequest,
    ) -> Result<ApiGatewayV2CreateRouteResponse> {
        let api_id = request
            .api_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = api_gateway_v2_result(
            self.create_route()
                .set_api_id(request.api_id)
                .set_api_key_required(request.api_key_required)
                .set_authorization_scopes(request.authorization_scopes)
                .set_authorization_type(request.authorization_type)
                .set_authorizer_id(request.authorizer_id)
                .set_model_selection_expression(request.model_selection_expression)
                .set_operation_name(request.operation_name)
                .set_request_models(request.request_models)
                .set_request_parameters(request.request_parameters)
                .set_route_key(request.route_key)
                .set_route_response_selection_expression(
                    request.route_response_selection_expression,
                )
                .set_target(request.target)
                .send()
                .await,
            "CreateRoute",
            "ApiGatewayRoute",
            &api_id,
        )?;

        Ok(output)
    }

    async fn create_stage(
        &self,
        request: ApiGatewayV2CreateStageRequest,
    ) -> Result<ApiGatewayV2CreateStageResponse> {
        let api_id = request
            .api_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = api_gateway_v2_result(
            self.create_stage()
                .set_access_log_settings(request.access_log_settings)
                .set_api_id(request.api_id)
                .set_auto_deploy(request.auto_deploy)
                .set_client_certificate_id(request.client_certificate_id)
                .set_default_route_settings(request.default_route_settings)
                .set_deployment_id(request.deployment_id)
                .set_description(request.description)
                .set_route_settings(request.route_settings)
                .set_stage_name(request.stage_name)
                .set_stage_variables(request.stage_variables)
                .set_tags(request.tags)
                .send()
                .await,
            "CreateStage",
            "ApiGatewayStage",
            &api_id,
        )?;

        Ok(output)
    }

    async fn create_domain_name(
        &self,
        request: ApiGatewayV2CreateDomainNameRequest,
    ) -> Result<ApiGatewayV2CreateDomainNameResponse> {
        let domain_name = request
            .domain_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = api_gateway_v2_result(
            self.create_domain_name()
                .set_domain_name(request.domain_name)
                .set_domain_name_configurations(request.domain_name_configurations)
                .set_mutual_tls_authentication(request.mutual_tls_authentication)
                .set_routing_mode(request.routing_mode)
                .set_tags(request.tags)
                .send()
                .await,
            "CreateDomainName",
            "ApiGatewayDomainName",
            &domain_name,
        )?;

        Ok(output)
    }

    async fn get_domain_name(
        &self,
        domain_name: &str,
    ) -> Result<ApiGatewayV2GetDomainNameResponse> {
        let output = api_gateway_v2_result(
            self.get_domain_name().domain_name(domain_name).send().await,
            "GetDomainName",
            "ApiGatewayDomainName",
            domain_name,
        )?;

        Ok(output)
    }

    async fn delete_domain_name(&self, domain_name: &str) -> Result<()> {
        api_gateway_v2_result(
            self.delete_domain_name()
                .domain_name(domain_name)
                .send()
                .await,
            "DeleteDomainName",
            "ApiGatewayDomainName",
            domain_name,
        )?;
        Ok(())
    }

    async fn create_api_mapping(
        &self,
        request: ApiGatewayV2CreateApiMappingRequest,
    ) -> Result<ApiGatewayV2CreateApiMappingResponse> {
        let domain_name = request
            .domain_name
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = api_gateway_v2_result(
            self.create_api_mapping()
                .set_api_id(request.api_id)
                .set_api_mapping_key(request.api_mapping_key)
                .set_domain_name(request.domain_name)
                .set_stage(request.stage)
                .send()
                .await,
            "CreateApiMapping",
            "ApiGatewayApiMapping",
            &domain_name,
        )?;

        Ok(output)
    }

    async fn get_api_mappings(
        &self,
        domain_name: &str,
    ) -> Result<ApiGatewayV2GetApiMappingsResponse> {
        let output = api_gateway_v2_result(
            self.get_api_mappings()
                .domain_name(domain_name)
                .send()
                .await,
            "GetApiMappings",
            "ApiGatewayApiMapping",
            domain_name,
        )?;

        Ok(output)
    }

    async fn delete_api_mapping(&self, domain_name: &str, api_mapping_id: &str) -> Result<()> {
        api_gateway_v2_result(
            self.delete_api_mapping()
                .domain_name(domain_name)
                .api_mapping_id(api_mapping_id)
                .send()
                .await,
            "DeleteApiMapping",
            "ApiGatewayApiMapping",
            domain_name,
        )?;
        Ok(())
    }
}

#[async_trait]
impl EventBridgeApi for EventBridgeClient {
    async fn put_rule(&self, request: PutRuleRequest) -> Result<PutRuleResponse> {
        let rule_name = request
            .name
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = eventbridge_result(
            self.put_rule()
                .set_name(request.name)
                .set_schedule_expression(request.schedule_expression)
                .set_event_pattern(request.event_pattern)
                .set_state(request.state)
                .set_description(request.description)
                .set_role_arn(request.role_arn)
                .set_tags(request.tags)
                .set_event_bus_name(request.event_bus_name)
                .send()
                .await,
            "PutRule",
            "EventBridgeRule",
            &rule_name,
        )?;

        Ok(output)
    }

    async fn put_targets(&self, request: PutTargetsRequest) -> Result<()> {
        let rule_name = request
            .rule
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let output = eventbridge_result(
            self.put_targets()
                .set_rule(request.rule)
                .set_event_bus_name(request.event_bus_name)
                .set_targets(request.targets)
                .send()
                .await,
            "PutTargets",
            "EventBridgeRule",
            &rule_name,
        )?;
        ensure_no_eventbridge_target_failures(
            output.failed_entry_count,
            format!("{:?}", output.failed_entries),
            "PutTargets",
            &rule_name,
        )
    }

    async fn remove_targets(&self, rule_name: &str, target_ids: Vec<String>) -> Result<()> {
        let output = eventbridge_result(
            self.remove_targets()
                .rule(rule_name)
                .set_ids(Some(target_ids))
                .send()
                .await,
            "RemoveTargets",
            "EventBridgeRule",
            rule_name,
        )?;
        ensure_no_eventbridge_target_failures(
            output.failed_entry_count,
            format!("{:?}", output.failed_entries),
            "RemoveTargets",
            rule_name,
        )
    }

    async fn delete_rule(&self, rule_name: &str) -> Result<()> {
        eventbridge_result(
            self.delete_rule().name(rule_name).send().await,
            "DeleteRule",
            "EventBridgeRule",
            rule_name,
        )?;
        Ok(())
    }
}

#[async_trait]
impl SsmApi for aws_sdk_ssm::Client {
    async fn describe_parameters_by_prefix(
        &self,
        prefix: &str,
        max_results: i32,
    ) -> Result<DescribeSsmParametersResponse> {
        let name_prefix_filter = ParameterStringFilter::builder()
            .key("Name")
            .option("BeginsWith")
            .values(prefix)
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to build SSM Parameter Store prefix filter for '{}'",
                    prefix
                ),
                resource_id: None,
            })?;

        let response = self
            .describe_parameters()
            .parameter_filters(name_prefix_filter)
            .max_results(max_results)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to describe SSM Parameter Store metadata for prefix '{}'",
                    prefix
                ),
                resource_id: None,
            })?;

        let parameters = response
            .parameters()
            .iter()
            .map(|parameter| SsmParameterMetadata {
                parameter_type: parameter.r#type().map(parameter_type_name),
                tier: parameter.tier().map(parameter_tier_name),
                has_key_id: parameter.key_id().is_some(),
                last_modified_at: parameter
                    .last_modified_date()
                    .and_then(|modified_at| smithy_datetime_to_chrono_utc(modified_at).ok()),
            })
            .collect();

        Ok(DescribeSsmParametersResponse {
            parameters,
            has_more_parameters: response.next_token().is_some(),
        })
    }
}

#[async_trait]
impl DynamoDbApi for DynamoDbClient {
    async fn create_kv_table(&self, table_name: &str, tags: HashMap<String, String>) -> Result<()> {
        let key_schema = vec![
            AwsDynamoDbKeySchemaElement::builder()
                .attribute_name("pk")
                .key_type(KeyType::Hash)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build DynamoDB partition key schema".to_string(),
                    resource_id: None,
                })?,
            AwsDynamoDbKeySchemaElement::builder()
                .attribute_name("sk")
                .key_type(KeyType::Range)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build DynamoDB sort key schema".to_string(),
                    resource_id: None,
                })?,
        ];

        let attribute_definitions = vec![
            AwsDynamoDbAttributeDefinition::builder()
                .attribute_name("pk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build DynamoDB partition key attribute definition"
                        .to_string(),
                    resource_id: None,
                })?,
            AwsDynamoDbAttributeDefinition::builder()
                .attribute_name("sk")
                .attribute_type(ScalarAttributeType::S)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build DynamoDB sort key attribute definition".to_string(),
                    resource_id: None,
                })?,
        ];

        let tags = tags
            .into_iter()
            .map(|(key, value)| {
                DynamoDbTag::builder()
                    .key(key)
                    .value(value)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to build DynamoDB tag for table '{table_name}'"),
                        resource_id: None,
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        self.create_table()
            .table_name(table_name)
            .set_key_schema(Some(key_schema))
            .set_attribute_definitions(Some(attribute_definitions))
            .billing_mode(BillingMode::PayPerRequest)
            .set_tags(Some(tags))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("DynamoDB CreateTable API failed for table '{table_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn describe_table(&self, table_name: &str) -> Result<Option<DynamoDbTableDescription>> {
        match self.describe_table().table_name(table_name).send().await {
            Ok(output) => Ok(output.table().map(dynamodb_table_description)),
            Err(err) if is_dynamodb_table_not_found(&err) => Ok(None),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("DynamoDB DescribeTable API failed for table '{table_name}'"),
                    resource_id: None,
                })),
        }
    }

    async fn enable_ttl(&self, table_name: &str, attribute_name: &str) -> Result<()> {
        let ttl_spec = TimeToLiveSpecification::builder()
            .attribute_name(attribute_name)
            .enabled(true)
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to build DynamoDB TTL specification for table '{table_name}'"
                ),
                resource_id: None,
            })?;

        self.update_time_to_live()
            .table_name(table_name)
            .time_to_live_specification(ttl_spec)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("DynamoDB UpdateTimeToLive API failed for table '{table_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn describe_ttl(&self, table_name: &str) -> Result<Option<DynamoDbTtlDescription>> {
        match self
            .describe_time_to_live()
            .table_name(table_name)
            .send()
            .await
        {
            Ok(output) => Ok(output
                .time_to_live_description()
                .map(dynamodb_ttl_description)),
            Err(err) if is_dynamodb_ttl_table_not_found(&err) => Ok(None),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "DynamoDB DescribeTimeToLive API failed for table '{table_name}'"
                    ),
                    resource_id: None,
                })),
        }
    }

    async fn delete_table(&self, table_name: &str) -> Result<bool> {
        match self.delete_table().table_name(table_name).send().await {
            Ok(_) => Ok(true),
            Err(err) if is_dynamodb_delete_table_not_found(&err) => Ok(false),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("DynamoDB DeleteTable API failed for table '{table_name}'"),
                    resource_id: None,
                })),
        }
    }
}

#[async_trait]
impl CodeBuildApi for CodeBuildClient {
    async fn create_project(
        &self,
        config: CodeBuildProjectConfig,
    ) -> Result<CodeBuildProjectDescription> {
        let project_name = config.name.clone();
        let request_parts = build_codebuild_project_request(config)?;

        let response = self
            .create_project()
            .name(&project_name)
            .source(request_parts.source)
            .artifacts(request_parts.artifacts)
            .environment(request_parts.environment)
            .logs_config(request_parts.logs_config)
            .service_role(request_parts.service_role)
            .description(request_parts.description)
            .set_tags(Some(request_parts.tags))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("CodeBuild CreateProject API failed for project '{project_name}'"),
                resource_id: None,
            })?;

        response
            .project()
            .map(codebuild_project_description)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "CodeBuild CreateProject response for '{project_name}' did not include a project"
                    ),
                    resource_id: None,
                })
            })
    }

    async fn update_project(
        &self,
        config: CodeBuildProjectConfig,
    ) -> Result<CodeBuildProjectDescription> {
        let project_name = config.name.clone();
        let request_parts = build_codebuild_project_request(config)?;

        let response = self
            .update_project()
            .name(&project_name)
            .source(request_parts.source)
            .artifacts(request_parts.artifacts)
            .environment(request_parts.environment)
            .logs_config(request_parts.logs_config)
            .service_role(request_parts.service_role)
            .description(request_parts.description)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("CodeBuild UpdateProject API failed for project '{project_name}'"),
                resource_id: None,
            })?;

        response
            .project()
            .map(codebuild_project_description)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "CodeBuild UpdateProject response for '{project_name}' did not include a project"
                    ),
                    resource_id: None,
                })
            })
    }

    async fn get_project(&self, project_name: &str) -> Result<Option<CodeBuildProjectDescription>> {
        let response = self
            .batch_get_projects()
            .names(project_name)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "CodeBuild BatchGetProjects API failed for project '{project_name}'"
                ),
                resource_id: None,
            })?;

        Ok(response
            .projects()
            .first()
            .map(codebuild_project_description))
    }

    async fn delete_project(&self, project_name: &str) -> Result<()> {
        self.delete_project()
            .name(project_name)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("CodeBuild DeleteProject API failed for project '{project_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }
}

#[async_trait]
impl EcrApi for EcrClient {
    async fn describe_repositories(
        &self,
        request: DescribeEcrRepositoriesRequest,
    ) -> Result<DescribeEcrRepositoriesResponse> {
        let response = self
            .describe_repositories()
            .set_registry_id(request.registry_id)
            .set_repository_names(request.repository_names)
            .set_next_token(request.next_token)
            .set_max_results(request.max_results)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "ECR DescribeRepositories API failed".to_string(),
                resource_id: None,
            })?;

        let repositories = response
            .repositories()
            .iter()
            .map(ecr_repository)
            .collect::<Result<Vec<_>>>()?;

        Ok(DescribeEcrRepositoriesResponse {
            repositories,
            next_token: response.next_token().map(ToString::to_string),
        })
    }

    async fn describe_registry(&self) -> Result<EcrReplicationConfiguration> {
        let response = self
            .describe_registry()
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "ECR DescribeRegistry API failed".to_string(),
                resource_id: None,
            })?;

        Ok(response
            .replication_configuration()
            .map(ecr_replication_configuration)
            .unwrap_or_else(|| EcrReplicationConfiguration { rules: vec![] }))
    }

    async fn put_replication_configuration(
        &self,
        replication_configuration: EcrReplicationConfiguration,
    ) -> Result<EcrReplicationConfiguration> {
        let request_config = aws_ecr_replication_configuration(replication_configuration)?;
        let response = self
            .put_replication_configuration()
            .replication_configuration(request_config)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "ECR PutReplicationConfiguration API failed".to_string(),
                resource_id: None,
            })?;

        response
            .replication_configuration()
            .map(ecr_replication_configuration)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "ECR PutReplicationConfiguration response did not include replication configuration".to_string(),
                    resource_id: None,
                })
            })
    }
}

#[async_trait]
impl Ec2Api for Ec2Client {
    async fn describe_vpcs(&self, request: DescribeVpcsRequest) -> Result<DescribeVpcsResponse> {
        let response = ec2_result(
            self.describe_vpcs()
                .set_vpc_ids(request.vpc_ids)
                .set_filters(aws_ec2_filters(request.filters))
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .send()
                .await,
            "DescribeVpcs",
            "VPC",
            "*",
        )?;

        Ok(DescribeVpcsResponse {
            vpc_set: Some(VpcSet {
                items: response.vpcs().iter().map(ec2_vpc).collect(),
            }),
            next_token: response.next_token().map(ToString::to_string),
        })
    }

    async fn create_vpc(&self, request: CreateVpcRequest) -> Result<CreateVpcResponse> {
        let cidr_block = request.cidr_block.clone();
        let response = ec2_result(
            self.create_vpc()
                .cidr_block(cidr_block.clone())
                .set_tag_specifications(aws_ec2_tag_specifications(request.tag_specifications))
                .send()
                .await,
            "CreateVpc",
            "VPC",
            &cidr_block,
        )?;

        Ok(CreateVpcResponse {
            vpc: response.vpc().map(ec2_vpc),
        })
    }

    async fn delete_vpc(&self, vpc_id: &str) -> Result<()> {
        ec2_result(
            self.delete_vpc().vpc_id(vpc_id).send().await,
            "DeleteVpc",
            "VPC",
            vpc_id,
        )?;
        Ok(())
    }

    async fn modify_vpc_attribute(&self, request: ModifyVpcAttributeRequest) -> Result<()> {
        let vpc_id = request.vpc_id.clone();
        let mut operation = self.modify_vpc_attribute().vpc_id(&vpc_id);
        if let Some(enable_dns_support) = request.enable_dns_support {
            operation = operation.enable_dns_support(
                AwsEc2AttributeBooleanValue::builder()
                    .value(enable_dns_support)
                    .build(),
            );
        }
        if let Some(enable_dns_hostnames) = request.enable_dns_hostnames {
            operation = operation.enable_dns_hostnames(
                AwsEc2AttributeBooleanValue::builder()
                    .value(enable_dns_hostnames)
                    .build(),
            );
        }

        ec2_result(operation.send().await, "ModifyVpcAttribute", "VPC", &vpc_id)?;
        Ok(())
    }

    async fn describe_subnets(
        &self,
        request: DescribeSubnetsRequest,
    ) -> Result<DescribeSubnetsResponse> {
        let response = ec2_result(
            self.describe_subnets()
                .set_subnet_ids(request.subnet_ids)
                .set_filters(aws_ec2_filters(request.filters))
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .send()
                .await,
            "DescribeSubnets",
            "Subnet",
            "*",
        )?;

        Ok(DescribeSubnetsResponse {
            subnet_set: Some(SubnetSet {
                items: response.subnets().iter().map(ec2_subnet).collect(),
            }),
            next_token: response.next_token().map(ToString::to_string),
        })
    }

    async fn create_subnet(&self, request: CreateSubnetRequest) -> Result<CreateSubnetResponse> {
        let cidr_block = request.cidr_block.clone();
        let response = ec2_result(
            self.create_subnet()
                .vpc_id(request.vpc_id)
                .cidr_block(&cidr_block)
                .set_availability_zone(request.availability_zone)
                .set_tag_specifications(aws_ec2_tag_specifications(request.tag_specifications))
                .send()
                .await,
            "CreateSubnet",
            "Subnet",
            &cidr_block,
        )?;

        Ok(CreateSubnetResponse {
            subnet: response.subnet().map(ec2_subnet),
        })
    }

    async fn delete_subnet(&self, subnet_id: &str) -> Result<()> {
        ec2_result(
            self.delete_subnet().subnet_id(subnet_id).send().await,
            "DeleteSubnet",
            "Subnet",
            subnet_id,
        )?;
        Ok(())
    }

    async fn create_internet_gateway(
        &self,
        request: CreateInternetGatewayRequest,
    ) -> Result<CreateInternetGatewayResponse> {
        let response = ec2_result(
            self.create_internet_gateway()
                .set_tag_specifications(aws_ec2_tag_specifications(request.tag_specifications))
                .send()
                .await,
            "CreateInternetGateway",
            "InternetGateway",
            "*",
        )?;

        Ok(CreateInternetGatewayResponse {
            internet_gateway: response.internet_gateway().map(|gateway| InternetGateway {
                internet_gateway_id: gateway.internet_gateway_id().map(ToString::to_string),
            }),
        })
    }

    async fn delete_internet_gateway(&self, internet_gateway_id: &str) -> Result<()> {
        ec2_result(
            self.delete_internet_gateway()
                .internet_gateway_id(internet_gateway_id)
                .send()
                .await,
            "DeleteInternetGateway",
            "InternetGateway",
            internet_gateway_id,
        )?;
        Ok(())
    }

    async fn attach_internet_gateway(&self, request: AttachInternetGatewayRequest) -> Result<()> {
        let internet_gateway_id = request.internet_gateway_id.clone();
        ec2_result(
            self.attach_internet_gateway()
                .internet_gateway_id(request.internet_gateway_id)
                .vpc_id(request.vpc_id)
                .send()
                .await,
            "AttachInternetGateway",
            "InternetGateway",
            &internet_gateway_id,
        )?;
        Ok(())
    }

    async fn detach_internet_gateway(&self, request: DetachInternetGatewayRequest) -> Result<()> {
        let internet_gateway_id = request.internet_gateway_id.clone();
        ec2_result(
            self.detach_internet_gateway()
                .internet_gateway_id(request.internet_gateway_id)
                .vpc_id(request.vpc_id)
                .send()
                .await,
            "DetachInternetGateway",
            "InternetGateway",
            &internet_gateway_id,
        )?;
        Ok(())
    }

    async fn create_nat_gateway(
        &self,
        request: CreateNatGatewayRequest,
    ) -> Result<CreateNatGatewayResponse> {
        let subnet_id = request.subnet_id.clone();
        let response = ec2_result(
            self.create_nat_gateway()
                .subnet_id(&subnet_id)
                .set_allocation_id(request.allocation_id)
                .set_connectivity_type(
                    request
                        .connectivity_type
                        .as_deref()
                        .map(AwsEc2ConnectivityType::from),
                )
                .set_tag_specifications(aws_ec2_tag_specifications(request.tag_specifications))
                .send()
                .await,
            "CreateNatGateway",
            "NatGateway",
            &subnet_id,
        )?;

        Ok(CreateNatGatewayResponse {
            nat_gateway: response.nat_gateway().map(ec2_nat_gateway),
        })
    }

    async fn delete_nat_gateway(&self, nat_gateway_id: &str) -> Result<DeleteNatGatewayResponse> {
        let response = ec2_result(
            self.delete_nat_gateway()
                .nat_gateway_id(nat_gateway_id)
                .send()
                .await,
            "DeleteNatGateway",
            "NatGateway",
            nat_gateway_id,
        )?;

        Ok(DeleteNatGatewayResponse {
            nat_gateway_id: response.nat_gateway_id().map(ToString::to_string),
        })
    }

    async fn describe_nat_gateways(
        &self,
        request: DescribeNatGatewaysRequest,
    ) -> Result<DescribeNatGatewaysResponse> {
        let response = ec2_result(
            self.describe_nat_gateways()
                .set_nat_gateway_ids(request.nat_gateway_ids)
                .set_filter(aws_ec2_filters(request.filters))
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .send()
                .await,
            "DescribeNatGateways",
            "NatGateway",
            "*",
        )?;

        Ok(DescribeNatGatewaysResponse {
            nat_gateway_set: Some(NatGatewaySet {
                items: response
                    .nat_gateways()
                    .iter()
                    .map(ec2_nat_gateway)
                    .collect(),
            }),
            next_token: response.next_token().map(ToString::to_string),
        })
    }

    async fn allocate_address(
        &self,
        request: AllocateAddressRequest,
    ) -> Result<AllocateAddressResponse> {
        let response = ec2_result(
            self.allocate_address()
                .set_domain(request.domain.as_deref().map(AwsEc2DomainType::from))
                .set_tag_specifications(aws_ec2_tag_specifications(request.tag_specifications))
                .send()
                .await,
            "AllocateAddress",
            "ElasticIP",
            "*",
        )?;

        Ok(AllocateAddressResponse {
            allocation_id: response.allocation_id().map(ToString::to_string),
        })
    }

    async fn release_address(&self, allocation_id: &str) -> Result<()> {
        ec2_result(
            self.release_address()
                .allocation_id(allocation_id)
                .send()
                .await,
            "ReleaseAddress",
            "ElasticIP",
            allocation_id,
        )?;
        Ok(())
    }

    async fn create_route_table(
        &self,
        request: CreateRouteTableRequest,
    ) -> Result<CreateRouteTableResponse> {
        let vpc_id = request.vpc_id.clone();
        let response = ec2_result(
            self.create_route_table()
                .vpc_id(&vpc_id)
                .set_tag_specifications(aws_ec2_tag_specifications(request.tag_specifications))
                .send()
                .await,
            "CreateRouteTable",
            "RouteTable",
            &vpc_id,
        )?;

        Ok(CreateRouteTableResponse {
            route_table: response.route_table().map(|route_table| RouteTable {
                route_table_id: route_table.route_table_id().map(ToString::to_string),
            }),
        })
    }

    async fn delete_route_table(&self, route_table_id: &str) -> Result<()> {
        ec2_result(
            self.delete_route_table()
                .route_table_id(route_table_id)
                .send()
                .await,
            "DeleteRouteTable",
            "RouteTable",
            route_table_id,
        )?;
        Ok(())
    }

    async fn create_route(&self, request: CreateRouteRequest) -> Result<()> {
        let route_table_id = request.route_table_id.clone();
        ec2_result(
            self.create_route()
                .route_table_id(&route_table_id)
                .destination_cidr_block(request.destination_cidr_block)
                .set_gateway_id(request.gateway_id)
                .set_nat_gateway_id(request.nat_gateway_id)
                .send()
                .await,
            "CreateRoute",
            "Route",
            &route_table_id,
        )?;
        Ok(())
    }

    async fn associate_route_table(
        &self,
        request: AssociateRouteTableRequest,
    ) -> Result<AssociateRouteTableResponse> {
        let route_table_id = request.route_table_id.clone();
        let response = ec2_result(
            self.associate_route_table()
                .route_table_id(&route_table_id)
                .subnet_id(request.subnet_id)
                .send()
                .await,
            "AssociateRouteTable",
            "RouteTableAssociation",
            &route_table_id,
        )?;

        Ok(AssociateRouteTableResponse {
            association_id: response.association_id().map(ToString::to_string),
        })
    }

    async fn disassociate_route_table(&self, association_id: &str) -> Result<()> {
        ec2_result(
            self.disassociate_route_table()
                .association_id(association_id)
                .send()
                .await,
            "DisassociateRouteTable",
            "RouteTableAssociation",
            association_id,
        )?;
        Ok(())
    }

    async fn describe_security_groups(
        &self,
        request: DescribeSecurityGroupsRequest,
    ) -> Result<DescribeSecurityGroupsResponse> {
        let response = ec2_result(
            self.describe_security_groups()
                .set_group_ids(request.group_ids)
                .set_group_names(request.group_names)
                .set_filters(aws_ec2_filters(request.filters))
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .send()
                .await,
            "DescribeSecurityGroups",
            "SecurityGroup",
            "*",
        )?;

        Ok(DescribeSecurityGroupsResponse {
            security_group_info: Some(SecurityGroupSet {
                items: response
                    .security_groups()
                    .iter()
                    .map(ec2_security_group)
                    .collect(),
            }),
            next_token: response.next_token().map(ToString::to_string),
        })
    }

    async fn describe_network_interfaces(
        &self,
        request: DescribeNetworkInterfacesRequest,
    ) -> Result<DescribeNetworkInterfacesResponse> {
        let response = ec2_result(
            self.describe_network_interfaces()
                .set_network_interface_ids(request.network_interface_ids)
                .set_filters(aws_ec2_filters(request.filters))
                .set_max_results(request.max_results)
                .set_next_token(request.next_token)
                .send()
                .await,
            "DescribeNetworkInterfaces",
            "NetworkInterface",
            "*",
        )?;

        Ok(DescribeNetworkInterfacesResponse {
            network_interface_set: Some(NetworkInterfaceSet {
                items: response
                    .network_interfaces()
                    .iter()
                    .map(|interface| NetworkInterface {
                        network_interface_id: interface
                            .network_interface_id()
                            .map(ToString::to_string),
                    })
                    .collect(),
            }),
            next_token: response.next_token().map(ToString::to_string),
        })
    }

    async fn create_security_group(
        &self,
        request: CreateSecurityGroupRequest,
    ) -> Result<CreateSecurityGroupResponse> {
        let group_name = request.group_name.clone();
        let response = ec2_result(
            self.create_security_group()
                .group_name(&group_name)
                .description(request.description)
                .vpc_id(request.vpc_id)
                .set_tag_specifications(aws_ec2_tag_specifications(request.tag_specifications))
                .send()
                .await,
            "CreateSecurityGroup",
            "SecurityGroup",
            &group_name,
        )?;

        Ok(CreateSecurityGroupResponse {
            group_id: response.group_id().map(ToString::to_string),
        })
    }

    async fn delete_security_group(&self, group_id: &str) -> Result<()> {
        ec2_result(
            self.delete_security_group().group_id(group_id).send().await,
            "DeleteSecurityGroup",
            "SecurityGroup",
            group_id,
        )?;
        Ok(())
    }

    async fn authorize_security_group_ingress(
        &self,
        request: AuthorizeSecurityGroupIngressRequest,
    ) -> Result<()> {
        let group_id = request.group_id.clone();
        ec2_result(
            self.authorize_security_group_ingress()
                .group_id(&group_id)
                .set_ip_permissions(Some(aws_ec2_ip_permissions(request.ip_permissions)))
                .send()
                .await,
            "AuthorizeSecurityGroupIngress",
            "SecurityGroupRule",
            &group_id,
        )?;
        Ok(())
    }

    async fn authorize_security_group_egress(
        &self,
        request: AuthorizeSecurityGroupEgressRequest,
    ) -> Result<()> {
        let group_id = request.group_id.clone();
        ec2_result(
            self.authorize_security_group_egress()
                .group_id(&group_id)
                .set_ip_permissions(Some(aws_ec2_ip_permissions(request.ip_permissions)))
                .send()
                .await,
            "AuthorizeSecurityGroupEgress",
            "SecurityGroupRule",
            &group_id,
        )?;
        Ok(())
    }

    async fn describe_availability_zones(
        &self,
        request: DescribeAvailabilityZonesRequest,
    ) -> Result<DescribeAvailabilityZonesResponse> {
        let response = ec2_result(
            self.describe_availability_zones()
                .set_zone_names(request.zone_names)
                .set_zone_ids(request.zone_ids)
                .set_filters(aws_ec2_filters(request.filters))
                .set_all_availability_zones(request.all_availability_zones)
                .send()
                .await,
            "DescribeAvailabilityZones",
            "AvailabilityZone",
            "*",
        )?;

        Ok(DescribeAvailabilityZonesResponse {
            availability_zone_info: Some(AvailabilityZoneSet {
                items: response
                    .availability_zones()
                    .iter()
                    .map(|zone| AvailabilityZone {
                        zone_name: zone.zone_name().map(ToString::to_string),
                    })
                    .collect(),
            }),
        })
    }
}

#[async_trait]
impl S3Api for S3Client {
    async fn create_bucket(&self, bucket_name: &str) -> Result<()> {
        let mut request = self.create_bucket().bucket(bucket_name);
        let region = self
            .config()
            .region()
            .map(|region| region.as_ref().to_string())
            .unwrap_or_else(|| "us-east-1".to_string());

        if region != "us-east-1" {
            let configuration = CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::from(region.as_str()))
                .build();
            request = request.create_bucket_configuration(configuration);
        }

        match request.send().await {
            Ok(_) => Ok(()),
            Err(err) if is_s3_create_bucket_already_owned(&err) => Ok(()),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 CreateBucket API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                })),
        }
    }

    async fn put_bucket_abac_tags(
        &self,
        bucket_name: &str,
        tags: &HashMap<String, String>,
    ) -> Result<()> {
        let tag_set = tags
            .iter()
            .map(|(key, value)| {
                S3Tag::builder()
                    .key(key)
                    .value(value)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to build S3 tag for bucket '{bucket_name}'"),
                        resource_id: None,
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let tagging = Tagging::builder()
            .set_tag_set(Some(tag_set))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to build S3 tagging for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        self.put_bucket_tagging()
            .bucket(bucket_name)
            .tagging(tagging)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutBucketTagging API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn put_bucket_versioning(
        &self,
        bucket_name: &str,
        status: S3VersioningStatus,
    ) -> Result<()> {
        let versioning_configuration = VersioningConfiguration::builder()
            .status(match status {
                S3VersioningStatus::Enabled => BucketVersioningStatus::Enabled,
                S3VersioningStatus::Suspended => BucketVersioningStatus::Suspended,
            })
            .build();

        self.put_bucket_versioning()
            .bucket(bucket_name)
            .versioning_configuration(versioning_configuration)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutBucketVersioning API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn put_public_access_block(
        &self,
        bucket_name: &str,
        config: S3PublicAccessBlock,
    ) -> Result<()> {
        let configuration = AwsPublicAccessBlockConfiguration::builder()
            .set_block_public_acls(config.block_public_acls)
            .set_ignore_public_acls(config.ignore_public_acls)
            .set_block_public_policy(config.block_public_policy)
            .set_restrict_public_buckets(config.restrict_public_buckets)
            .build();

        self.put_public_access_block()
            .bucket(bucket_name)
            .public_access_block_configuration(configuration)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutPublicAccessBlock API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn put_bucket_policy(&self, bucket_name: &str, policy: &str) -> Result<()> {
        self.put_bucket_policy()
            .bucket(bucket_name)
            .policy(policy)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("S3 PutBucketPolicy API failed for bucket '{bucket_name}'"),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn delete_bucket_policy(&self, bucket_name: &str) -> Result<()> {
        match self.delete_bucket_policy().bucket(bucket_name).send().await {
            Ok(_) => Ok(()),
            Err(err) if is_s3_delete_bucket_policy_not_found(&err) => Ok(()),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 DeleteBucketPolicy API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                })),
        }
    }

    async fn put_bucket_lifecycle_configuration(
        &self,
        bucket_name: &str,
        rules: Vec<S3LifecycleRuleConfig>,
    ) -> Result<()> {
        let rules = rules
            .into_iter()
            .map(|rule| {
                let expiration = AwsLifecycleExpiration::builder().days(rule.days).build();
                let filter = AwsLifecycleRuleFilter::builder()
                    .set_prefix(rule.prefix)
                    .build();

                AwsLifecycleRule::builder()
                    .id(rule.id)
                    .status(ExpirationStatus::Enabled)
                    .filter(filter)
                    .expiration(expiration)
                    .build()
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to build S3 lifecycle rule for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        let configuration = AwsBucketLifecycleConfiguration::builder()
            .set_rules(Some(rules))
            .build()
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to build S3 lifecycle configuration for bucket '{bucket_name}'"
                ),
                resource_id: None,
            })?;

        self.put_bucket_lifecycle_configuration()
            .bucket(bucket_name)
            .lifecycle_configuration(configuration)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "S3 PutBucketLifecycleConfiguration API failed for bucket '{bucket_name}'"
                ),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn delete_bucket_lifecycle(&self, bucket_name: &str) -> Result<()> {
        match self
            .delete_bucket_lifecycle()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(err) if is_s3_delete_bucket_lifecycle_not_found(&err) => Ok(()),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "S3 DeleteBucketLifecycle API failed for bucket '{bucket_name}'"
                    ),
                    resource_id: None,
                })),
        }
    }

    async fn get_bucket_metadata(&self, bucket_name: &str) -> Result<S3BucketMetadata> {
        let location = match self.get_bucket_location().bucket(bucket_name).send().await {
            Ok(output) => output,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketLocation API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let versioning = match self
            .get_bucket_versioning()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => output,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketVersioning API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let lifecycle_rule_count = match self
            .get_bucket_lifecycle_configuration()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => Some(output.rules().len() as u64),
            Err(err) if is_s3_get_lifecycle_not_found(&err) => None,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                        "S3 GetBucketLifecycleConfiguration API failed for bucket '{bucket_name}'"
                    ),
                        resource_id: None,
                    }));
            }
        };

        let encryption_rule_count = match self
            .get_bucket_encryption()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => output
                .server_side_encryption_configuration()
                .map(|configuration| configuration.rules().len() as u64),
            Err(err) if is_s3_get_encryption_not_found(&err) => None,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketEncryption API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let public_access_block = match self
            .get_public_access_block()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => output
                .public_access_block_configuration()
                .map(s3_public_access_block),
            Err(err) if is_s3_get_public_access_block_not_found(&err) => None,
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetPublicAccessBlock API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let bucket_policy_present = match self.get_bucket_policy().bucket(bucket_name).send().await
        {
            Ok(output) => Some(
                output
                    .policy()
                    .is_some_and(|policy| !policy.trim().is_empty()),
            ),
            Err(err) if is_s3_get_bucket_policy_not_found(&err) => Some(false),
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "S3 GetBucketPolicy API failed for bucket '{bucket_name}'"
                        ),
                        resource_id: None,
                    }));
            }
        };

        let bucket_acl_present = match self.get_bucket_acl().bucket(bucket_name).send().await {
            Ok(output) => Some(output.owner().is_some() || !output.grants().is_empty()),
            Err(err) if is_s3_get_bucket_acl_not_found(&err) => Some(false),
            Err(err) => {
                return Err(err
                    .into_alien_error()
                    .context(ErrorData::CloudPlatformError {
                        message: format!("S3 GetBucketAcl API failed for bucket '{bucket_name}'"),
                        resource_id: None,
                    }));
            }
        };

        Ok(S3BucketMetadata {
            region: s3_bucket_location_region(location.location_constraint().map(|c| c.as_str())),
            versioning_status: versioning.status().map(s3_versioning_status),
            lifecycle_rule_count,
            encryption_rule_count,
            public_access_block,
            bucket_policy_present,
            bucket_acl_present,
        })
    }

    async fn empty_bucket(&self, bucket_name: &str) -> Result<()> {
        let mut key_marker = None;
        let mut version_id_marker = None;

        loop {
            match self
                .list_object_versions()
                .bucket(bucket_name)
                .set_key_marker(key_marker.clone())
                .set_version_id_marker(version_id_marker.clone())
                .max_keys(1000)
                .send()
                .await
            {
                Ok(output) => {
                    let mut objects =
                        Vec::with_capacity(output.versions().len() + output.delete_markers().len());
                    for version in output.versions() {
                        if let (Some(key), Some(version_id)) = (version.key(), version.version_id())
                        {
                            objects.push(s3_object_identifier(key, Some(version_id))?);
                        }
                    }
                    for marker in output.delete_markers() {
                        if let (Some(key), Some(version_id)) = (marker.key(), marker.version_id()) {
                            objects.push(s3_object_identifier(key, Some(version_id))?);
                        }
                    }

                    if !objects.is_empty() {
                        delete_s3_objects(self, bucket_name, objects).await?;
                    }

                    if output.is_truncated().unwrap_or(false) {
                        key_marker = output.next_key_marker().map(ToString::to_string);
                        version_id_marker =
                            output.next_version_id_marker().map(ToString::to_string);
                        continue;
                    }

                    break;
                }
                Err(err) if is_s3_list_versions_bucket_not_found(&err) => return Ok(()),
                Err(err) if is_s3_list_versions_invalid_argument(&err) => break,
                Err(err) => {
                    return Err(err
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "S3 ListObjectVersions API failed for bucket '{bucket_name}'"
                            ),
                            resource_id: None,
                        }));
                }
            }
        }

        let mut continuation_token = None;
        loop {
            let output = match self
                .list_objects_v2()
                .bucket(bucket_name)
                .set_continuation_token(continuation_token.clone())
                .max_keys(1000)
                .send()
                .await
            {
                Ok(output) => output,
                Err(err) if is_s3_list_objects_bucket_not_found(&err) => return Ok(()),
                Err(err) => {
                    return Err(err
                        .into_alien_error()
                        .context(ErrorData::CloudPlatformError {
                            message: format!(
                                "S3 ListObjectsV2 API failed for bucket '{bucket_name}'"
                            ),
                            resource_id: None,
                        }));
                }
            };

            let objects = output
                .contents()
                .iter()
                .filter_map(|object| object.key())
                .map(|key| s3_object_identifier(key, None))
                .collect::<Result<Vec<_>>>()?;

            if !objects.is_empty() {
                delete_s3_objects(self, bucket_name, objects).await?;
            }

            if output.is_truncated().unwrap_or(false) {
                continuation_token = output.next_continuation_token().map(ToString::to_string);
            } else {
                break;
            }
        }

        Ok(())
    }

    async fn delete_bucket(&self, bucket_name: &str) -> Result<bool> {
        match self.delete_bucket().bucket(bucket_name).send().await {
            Ok(_) => Ok(true),
            Err(err) if is_s3_delete_bucket_not_found(&err) => Ok(false),
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("S3 DeleteBucket API failed for bucket '{bucket_name}'"),
                    resource_id: None,
                })),
        }
    }

    async fn get_bucket_notification_configuration(
        &self,
        bucket_name: &str,
    ) -> Result<NotificationConfiguration> {
        match self
            .get_bucket_notification_configuration()
            .bucket(bucket_name)
            .send()
            .await
        {
            Ok(output) => Ok(notification_configuration_from_aws(&output)),
            Err(err) if is_s3_get_notification_not_found(&err) => {
                Ok(NotificationConfiguration::default())
            }
            Err(err) => Err(err
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                    "S3 GetBucketNotificationConfiguration API failed for bucket '{bucket_name}'"
                ),
                    resource_id: None,
                })),
        }
    }

    async fn put_bucket_notification_configuration(
        &self,
        bucket_name: &str,
        config: &NotificationConfiguration,
    ) -> Result<()> {
        let configuration = notification_configuration_to_aws(config)?;
        self.put_bucket_notification_configuration()
            .bucket(bucket_name)
            .notification_configuration(configuration)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "S3 PutBucketNotificationConfiguration API failed for bucket '{bucket_name}'"
                ),
                resource_id: None,
            })?;

        Ok(())
    }
}

struct CodeBuildProjectRequestParts {
    source: ProjectSource,
    artifacts: ProjectArtifacts,
    environment: ProjectEnvironment,
    logs_config: LogsConfig,
    service_role: String,
    description: String,
    tags: Vec<CodeBuildTag>,
}

fn build_codebuild_project_request(
    config: CodeBuildProjectConfig,
) -> Result<CodeBuildProjectRequestParts> {
    let source = ProjectSource::builder()
        .r#type(SourceType::NoSource)
        .buildspec(config.buildspec)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild source for project '{}'",
                config.name
            ),
            resource_id: None,
        })?;

    let artifacts = ProjectArtifacts::builder()
        .r#type(ArtifactsType::NoArtifacts)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild artifacts for project '{}'",
                config.name
            ),
            resource_id: None,
        })?;

    let environment_variables = config
        .environment_variables
        .into_iter()
        .map(|(name, value)| {
            AwsCodeBuildEnvironmentVariable::builder()
                .name(name)
                .value(value)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to build CodeBuild environment variable for project '{}'",
                        config.name
                    ),
                    resource_id: None,
                })
        })
        .collect::<Result<Vec<_>>>()?;

    let environment = ProjectEnvironment::builder()
        .r#type(EnvironmentType::from(config.environment_type.as_str()))
        .image(config.image)
        .compute_type(AwsCodeBuildComputeType::from(config.compute_type.as_str()))
        .image_pull_credentials_type(ImagePullCredentialsType::from(
            config.image_pull_credentials_type.as_str(),
        ))
        .set_environment_variables(Some(environment_variables))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild environment for project '{}'",
                config.name
            ),
            resource_id: None,
        })?;

    let cloud_watch_logs = CloudWatchLogsConfig::builder()
        .status(LogsConfigStatusType::Enabled)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild CloudWatch logs config for project '{}'",
                config.name
            ),
            resource_id: None,
        })?;

    let s3_logs = S3LogsConfig::builder()
        .status(LogsConfigStatusType::Disabled)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to build CodeBuild S3 logs config for project '{}'",
                config.name
            ),
            resource_id: None,
        })?;

    let logs_config = LogsConfig::builder()
        .cloud_watch_logs(cloud_watch_logs)
        .s3_logs(s3_logs)
        .build();

    let tags = config
        .tags
        .into_iter()
        .map(|(key, value)| CodeBuildTag::builder().key(key).value(value).build())
        .collect::<Vec<_>>();

    Ok(CodeBuildProjectRequestParts {
        source,
        artifacts,
        environment,
        logs_config,
        service_role: config.service_role,
        description: config.description,
        tags,
    })
}

fn codebuild_project_description(
    project: &aws_sdk_codebuild::types::Project,
) -> CodeBuildProjectDescription {
    let environment = project.environment();
    let artifacts = project.artifacts();
    let source = project.source();
    let logs_config = project.logs_config();
    let name = project.name().unwrap_or_default().to_string();

    CodeBuildProjectDescription {
        name,
        arn: project.arn().map(ToString::to_string),
        description: project.description().map(ToString::to_string),
        source_type: source.map(|source| source.r#type().as_str().to_string()),
        artifacts_type: artifacts.map(|artifacts| artifacts.r#type().as_str().to_string()),
        artifacts_encryption_disabled: artifacts
            .and_then(|artifacts| artifacts.encryption_disabled()),
        environment_type: environment.map(|environment| environment.r#type().as_str().to_string()),
        environment_image: environment.map(|environment| environment.image().to_string()),
        compute_type: environment
            .map(|environment| environment.compute_type().as_str().to_string()),
        image_pull_credentials_type: environment
            .and_then(|environment| environment.image_pull_credentials_type())
            .map(|credentials_type| credentials_type.as_str().to_string()),
        privileged_mode: environment.and_then(|environment| environment.privileged_mode()),
        environment_variable_count: environment
            .and_then(|environment| u32::try_from(environment.environment_variables().len()).ok())
            .unwrap_or(0),
        service_role_present: project.service_role().is_some(),
        encryption_key_present: project.encryption_key().is_some(),
        cloud_watch_logs_status: logs_config
            .and_then(|logs_config| logs_config.cloud_watch_logs())
            .map(|logs| logs.status().as_str().to_string()),
        s3_logs_status: logs_config
            .and_then(|logs_config| logs_config.s3_logs())
            .map(|logs| logs.status().as_str().to_string()),
        timeout_in_minutes: project.timeout_in_minutes(),
        queued_timeout_in_minutes: project.queued_timeout_in_minutes(),
        created: project.created().map(|created| created.as_secs_f64()),
        last_modified: project
            .last_modified()
            .map(|last_modified| last_modified.as_secs_f64()),
    }
}

#[async_trait]
impl SqsApi for SqsClient {
    async fn create_queue(
        &self,
        queue_name: &str,
        tags: HashMap<String, String>,
    ) -> Result<String> {
        let response = self
            .create_queue()
            .queue_name(queue_name)
            .set_tags(Some(tags))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("SQS CreateQueue API failed for queue '{}'", queue_name),
                resource_id: None,
            })?;

        response
            .queue_url()
            .map(ToString::to_string)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "SQS CreateQueue response for '{}' did not include a queue URL",
                        queue_name
                    ),
                    resource_id: None,
                })
            })
    }

    async fn get_queue_attributes(
        &self,
        queue_url: &str,
        attribute_names: Vec<String>,
    ) -> Result<HashMap<String, String>> {
        let mut request = self.get_queue_attributes().queue_url(queue_url);
        for attribute_name in attribute_names {
            request = request.attribute_names(QueueAttributeName::from(attribute_name.as_str()));
        }

        let response =
            request
                .send()
                .await
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "SQS GetQueueAttributes API failed for queue '{}'",
                        queue_url
                    ),
                    resource_id: None,
                })?;

        Ok(response
            .attributes()
            .map(|attributes| {
                attributes
                    .iter()
                    .map(|(name, value)| (name.as_str().to_string(), value.clone()))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn set_queue_attributes(
        &self,
        queue_url: &str,
        attributes: HashMap<String, String>,
    ) -> Result<()> {
        let attributes = attributes
            .into_iter()
            .map(|(name, value)| (QueueAttributeName::from(name.as_str()), value))
            .collect::<HashMap<_, _>>();

        self.set_queue_attributes()
            .queue_url(queue_url)
            .set_attributes(Some(attributes))
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "SQS SetQueueAttributes API failed for queue '{}'",
                    queue_url
                ),
                resource_id: None,
            })?;

        Ok(())
    }

    async fn delete_queue(&self, queue_url: &str) -> Result<()> {
        self.delete_queue()
            .queue_url(queue_url)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: format!("SQS DeleteQueue API failed for queue '{}'", queue_url),
                resource_id: None,
            })?;

        Ok(())
    }
}

fn is_dynamodb_table_not_found(
    error: &aws_sdk_dynamodb::error::SdkError<DescribeTableError>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(DescribeTableError::is_resource_not_found_exception)
}

fn is_dynamodb_ttl_table_not_found(
    error: &aws_sdk_dynamodb::error::SdkError<DescribeTimeToLiveError>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(DescribeTimeToLiveError::is_resource_not_found_exception)
}

fn is_dynamodb_delete_table_not_found(
    error: &aws_sdk_dynamodb::error::SdkError<DeleteTableError>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(DeleteTableError::is_resource_not_found_exception)
}

fn nonempty_vec<T>(values: Vec<T>) -> Option<Vec<T>> {
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn aws_ec2_filters(filters: Option<Vec<Filter>>) -> Option<Vec<AwsEc2Filter>> {
    nonempty_vec(
        filters?
            .into_iter()
            .map(|filter| {
                AwsEc2Filter::builder()
                    .name(filter.name)
                    .set_values(nonempty_vec(filter.values))
                    .build()
            })
            .collect(),
    )
}

fn aws_ec2_tags(tags: Vec<Ec2Tag>) -> Vec<AwsEc2Tag> {
    tags.into_iter()
        .map(|tag| AwsEc2Tag::builder().key(tag.key).value(tag.value).build())
        .collect()
}

fn aws_ec2_tag_specifications(
    tag_specifications: Option<Vec<TagSpecification>>,
) -> Option<Vec<AwsEc2TagSpecification>> {
    nonempty_vec(
        tag_specifications?
            .into_iter()
            .map(|specification| {
                AwsEc2TagSpecification::builder()
                    .resource_type(AwsEc2ResourceType::from(
                        specification.resource_type.as_str(),
                    ))
                    .set_tags(nonempty_vec(aws_ec2_tags(specification.tags)))
                    .build()
            })
            .collect(),
    )
}

fn aws_ec2_ip_permissions(permissions: Vec<IpPermission>) -> Vec<AwsEc2IpPermission> {
    permissions
        .into_iter()
        .map(|permission| {
            AwsEc2IpPermission::builder()
                .ip_protocol(permission.ip_protocol)
                .set_from_port(permission.from_port)
                .set_to_port(permission.to_port)
                .set_ip_ranges(permission.ip_ranges.map(|ranges| {
                    ranges
                        .into_iter()
                        .map(|range| {
                            AwsEc2IpRange::builder()
                                .cidr_ip(range.cidr_ip)
                                .set_description(range.description)
                                .build()
                        })
                        .collect()
                }))
                .build()
        })
        .collect()
}

fn ec2_vpc(vpc: &aws_sdk_ec2::types::Vpc) -> Vpc {
    Vpc {
        vpc_id: vpc.vpc_id().map(ToString::to_string),
        state: vpc.state().map(|state| state.as_str().to_string()),
        cidr_block: vpc.cidr_block().map(ToString::to_string),
    }
}

fn ec2_subnet(subnet: &aws_sdk_ec2::types::Subnet) -> Subnet {
    Subnet {
        subnet_id: subnet.subnet_id().map(ToString::to_string),
    }
}

fn ec2_nat_gateway(nat_gateway: &aws_sdk_ec2::types::NatGateway) -> NatGateway {
    NatGateway {
        nat_gateway_id: nat_gateway.nat_gateway_id().map(ToString::to_string),
        state: nat_gateway.state().map(|state| state.as_str().to_string()),
    }
}

fn ec2_security_group(group: &aws_sdk_ec2::types::SecurityGroup) -> SecurityGroup {
    SecurityGroup {
        group_id: group.group_id().map(ToString::to_string),
        ip_permissions: Some(IpPermissionSet {
            items: group
                .ip_permissions()
                .iter()
                .map(ec2_ip_permission_response)
                .collect(),
        }),
        ip_permissions_egress: Some(IpPermissionSet {
            items: group
                .ip_permissions_egress()
                .iter()
                .map(ec2_ip_permission_response)
                .collect(),
        }),
    }
}

fn ec2_ip_permission_response(permission: &AwsEc2IpPermission) -> IpPermissionResponse {
    IpPermissionResponse {
        ip_protocol: permission.ip_protocol().map(ToString::to_string),
        from_port: permission.from_port(),
        to_port: permission.to_port(),
        ip_ranges: Some(IpRangeSet {
            items: permission
                .ip_ranges()
                .iter()
                .map(|range| IpRangeResponse {
                    cidr_ip: range.cidr_ip().map(ToString::to_string),
                    description: range.description().map(ToString::to_string),
                })
                .collect(),
        }),
        ipv6_ranges: None,
        groups: None,
    }
}

fn ec2_result<T, E>(
    result: std::result::Result<T, aws_sdk_ec2::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                if let Some(error_data) = ec2_error_data(
                    service_error.code(),
                    service_error.message(),
                    operation,
                    resource_type,
                    resource_name,
                ) {
                    return Err(AlienError::new(error_data));
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "EC2 {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn ec2_error_data(
    code: Option<&str>,
    message: Option<&str>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Option<ErrorData> {
    let code = code?;
    let message = message.unwrap_or(code);

    let not_found_resource_type = match code {
        "InvalidVpcID.NotFound" | "InvalidVpc.NotFound" => Some("VPC"),
        "InvalidSubnetID.NotFound" | "InvalidSubnet.NotFound" => Some("Subnet"),
        "InvalidInternetGatewayID.NotFound" | "InvalidInternetGateway.NotFound" => {
            Some("InternetGateway")
        }
        "InvalidNatGatewayID.NotFound" | "NatGatewayNotFound" => Some("NatGateway"),
        "InvalidRouteTableID.NotFound" | "InvalidRouteTableId.NotFound" => Some("RouteTable"),
        "InvalidGroup.NotFound" | "InvalidSecurityGroupID.NotFound" => Some("SecurityGroup"),
        "InvalidAllocationID.NotFound" | "InvalidAddress.NotFound" => Some("ElasticIP"),
        "InvalidAssociationID.NotFound" => Some("RouteTableAssociation"),
        _ => None,
    };
    if let Some(resource_type) = not_found_resource_type {
        return Some(ErrorData::CloudResourceNotFound {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
        });
    }

    match code {
        "InvalidGroup.Duplicate"
        | "InvalidPermission.Duplicate"
        | "DependencyViolation"
        | "ResourceInUse"
        | "Gateway.NotAttached"
        | "RouteAlreadyExists" => Some(ErrorData::CloudResourceConflict {
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
            message: format!("{operation} reported {code}: {message}"),
        }),
        _ => None,
    }
}

fn ecr_repository(repository: &AwsEcrRepository) -> Result<EcrRepository> {
    let repository_name = repository.repository_name().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "ECR DescribeRepositories response included a repository without a name"
                .to_string(),
            resource_id: None,
        })
    })?;

    let required_field = |field_name: &str, value: Option<&str>| {
        value.map(ToString::to_string).ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "ECR DescribeRepositories response for '{repository_name}' did not include {field_name}"
                ),
                resource_id: Some(repository_name.to_string()),
            })
        })
    };

    Ok(EcrRepository {
        repository_arn: required_field("repository ARN", repository.repository_arn())?,
        registry_id: required_field("registry ID", repository.registry_id())?,
        repository_name: repository_name.to_string(),
        repository_uri: required_field("repository URI", repository.repository_uri())?,
        created_at: repository
            .created_at()
            .map(|created_at| created_at.secs() as f64)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "ECR DescribeRepositories response for '{repository_name}' did not include creation time"
                    ),
                    resource_id: Some(repository_name.to_string()),
                })
            })?,
        image_tag_mutability: repository
            .image_tag_mutability()
            .map(|mutability| mutability.as_str().to_string()),
        image_scanning_configuration: repository
            .image_scanning_configuration()
            .map(ecr_image_scanning_configuration),
        encryption_configuration: repository
            .encryption_configuration()
            .map(|config| EcrEncryptionConfiguration {
                encryption_type: config.encryption_type().as_str().to_string(),
                kms_key: config.kms_key().map(ToString::to_string),
            }),
    })
}

fn ecr_image_scanning_configuration(
    config: &AwsEcrImageScanningConfiguration,
) -> EcrImageScanningConfiguration {
    EcrImageScanningConfiguration {
        scan_on_push: Some(config.scan_on_push()),
    }
}

fn ecr_replication_configuration(
    config: &AwsEcrReplicationConfiguration,
) -> EcrReplicationConfiguration {
    EcrReplicationConfiguration {
        rules: config
            .rules()
            .iter()
            .map(|rule| EcrReplicationRule {
                destinations: rule
                    .destinations()
                    .iter()
                    .map(|destination| EcrReplicationDestination {
                        region: destination.region().to_string(),
                        registry_id: destination.registry_id().to_string(),
                    })
                    .collect(),
                repository_filters: rule
                    .repository_filters()
                    .iter()
                    .map(|filter| EcrRepositoryFilter {
                        filter: filter.filter().to_string(),
                        filter_type: filter.filter_type().as_str().to_string(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn aws_ecr_replication_configuration(
    config: EcrReplicationConfiguration,
) -> Result<AwsEcrReplicationConfiguration> {
    let rules = config
        .rules
        .into_iter()
        .map(aws_ecr_replication_rule)
        .collect::<Result<Vec<_>>>()?;

    AwsEcrReplicationConfiguration::builder()
        .set_rules(Some(rules))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to build ECR replication configuration".to_string(),
            resource_id: None,
        })
}

fn aws_ecr_replication_rule(rule: EcrReplicationRule) -> Result<AwsEcrReplicationRule> {
    let destinations = rule
        .destinations
        .into_iter()
        .map(|destination| {
            AwsEcrReplicationDestination::builder()
                .region(destination.region)
                .registry_id(destination.registry_id)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build ECR replication destination".to_string(),
                    resource_id: None,
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let repository_filters = rule
        .repository_filters
        .into_iter()
        .map(|filter| {
            AwsEcrRepositoryFilter::builder()
                .filter(filter.filter)
                .filter_type(aws_sdk_ecr::types::RepositoryFilterType::from(
                    filter.filter_type.as_str(),
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build ECR repository filter".to_string(),
                    resource_id: None,
                })
        })
        .collect::<Result<Vec<_>>>()?;

    AwsEcrReplicationRule::builder()
        .set_destinations(Some(destinations))
        .set_repository_filters(nonempty_vec(repository_filters))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to build ECR replication rule".to_string(),
            resource_id: None,
        })
}

fn iam_tags(tags: Vec<CreateRoleTag>, resource_name: &str) -> Result<Vec<AwsIamTag>> {
    tags.into_iter()
        .map(|tag| {
            AwsIamTag::builder()
                .key(tag.key)
                .value(tag.value)
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to build IAM tag for '{resource_name}'"),
                    resource_id: None,
                })
        })
        .collect()
}

fn iam_role(role: &AwsIamRole) -> Role {
    Role {
        path: role.path().to_string(),
        role_name: role.role_name().to_string(),
        role_id: role.role_id().to_string(),
        arn: role.arn().to_string(),
        create_date: smithy_datetime_debug(role.create_date()),
        assume_role_policy_document: role.assume_role_policy_document().map(ToString::to_string),
        description: role.description().map(ToString::to_string),
        max_session_duration: role.max_session_duration(),
        permissions_boundary: role.permissions_boundary().map(|boundary| {
            AttachedPermissionsBoundary {
                permissions_boundary_type: boundary
                    .permissions_boundary_type()
                    .map(|boundary_type| boundary_type.as_str().to_string()),
                permissions_boundary_arn: boundary
                    .permissions_boundary_arn()
                    .map(ToString::to_string),
            }
        }),
        tags: optional_iam_tags(role.tags()),
        role_last_used: role.role_last_used().map(|last_used| RoleLastUsed {
            last_used_date: last_used.last_used_date().map(smithy_datetime_debug),
            region: last_used.region().map(ToString::to_string),
        }),
    }
}

fn iam_policy(policy: &AwsIamPolicy) -> Result<Policy> {
    let arn = policy.arn().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "IAM policy metadata did not include an ARN".to_string(),
            resource_id: None,
        })
    })?;

    Ok(Policy {
        policy_name: policy.policy_name().map(ToString::to_string),
        policy_id: policy.policy_id().map(ToString::to_string),
        arn: arn.to_string(),
        path: policy.path().map(ToString::to_string),
        default_version_id: policy.default_version_id().map(ToString::to_string),
        attachment_count: policy.attachment_count(),
        is_attachable: Some(policy.is_attachable()),
        create_date: policy.create_date().map(smithy_datetime_debug),
        update_date: policy.update_date().map(smithy_datetime_debug),
    })
}

fn iam_policy_version(version: &AwsIamPolicyVersion) -> PolicyVersion {
    PolicyVersion {
        document: version.document().map(ToString::to_string),
        version_id: version.version_id().unwrap_or_default().to_string(),
        is_default_version: version.is_default_version(),
        create_date: version.create_date().map(smithy_datetime_debug),
    }
}

fn iam_attached_policy(policy: &AwsIamAttachedPolicy) -> AttachedPolicy {
    AttachedPolicy {
        policy_name: policy.policy_name().unwrap_or_default().to_string(),
        policy_arn: policy.policy_arn().unwrap_or_default().to_string(),
    }
}

fn iam_instance_profile(profile: &AwsIamInstanceProfile) -> InstanceProfile {
    InstanceProfile {
        path: profile.path().to_string(),
        instance_profile_name: profile.instance_profile_name().to_string(),
        instance_profile_id: profile.instance_profile_id().to_string(),
        arn: profile.arn().to_string(),
        create_date: smithy_datetime_debug(profile.create_date()),
        roles: Some(InstanceProfileRoles {
            member: profile.roles().iter().map(iam_role).collect(),
        }),
        tags: optional_iam_tags(profile.tags()),
    }
}

fn optional_iam_tags(tags: &[AwsIamTag]) -> Option<Tags> {
    if tags.is_empty() {
        None
    } else {
        Some(Tags {
            member: tags
                .iter()
                .map(|tag| Tag {
                    key: tag.key().to_string(),
                    value: tag.value().to_string(),
                })
                .collect(),
        })
    }
}

fn smithy_datetime_debug(date_time: &aws_sdk_iam::primitives::DateTime) -> String {
    format!("{date_time:?}")
}

fn iam_result<T, E>(
    result: std::result::Result<T, aws_sdk_iam::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("NoSuchEntity") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("EntityAlreadyExists") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: format!("{operation} reported EntityAlreadyExists"),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "IAM {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn api_gateway_v2_result<T, E>(
    result: std::result::Result<T, aws_sdk_apigatewayv2::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("NotFoundException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("ConflictException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: service_error
                                .message()
                                .unwrap_or("API Gateway V2 conflict")
                                .to_string(),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                    "API Gateway V2 {operation} API failed for {resource_type} '{resource_name}'"
                ),
                    resource_id: None,
                }))
        }
    }
}

fn ensure_no_eventbridge_target_failures(
    failed_entry_count: i32,
    failed_entries: String,
    operation: &str,
    rule_name: &str,
) -> Result<()> {
    if failed_entry_count == 0 {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::CloudPlatformError {
        message: format!(
            "EventBridge {operation} reported {failed_entry_count} failed target entries for rule '{rule_name}': {failed_entries}"
        ),
        resource_id: None,
    }))
}

fn eventbridge_result<T, E>(
    result: std::result::Result<T, aws_sdk_eventbridge::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("ResourceNotFoundException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("ResourceAlreadyExistsException" | "ConcurrentModificationException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: service_error
                                .message()
                                .unwrap_or("EventBridge conflict")
                                .to_string(),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "EventBridge {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn acm_result<T, E>(
    result: std::result::Result<T, aws_sdk_acm::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                if service_error.code() == Some("ResourceNotFoundException") {
                    return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                        resource_type: resource_type.to_string(),
                        resource_name: resource_name.to_string(),
                    }));
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "ACM {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn lambda_architectures_to_aws(architectures: Vec<String>) -> Vec<AwsLambdaArchitecture> {
    architectures
        .iter()
        .map(|architecture| AwsLambdaArchitecture::from(architecture.as_str()))
        .collect()
}

fn function_configuration_from_create_output(
    output: aws_sdk_lambda::operation::create_function::CreateFunctionOutput,
) -> FunctionConfiguration {
    FunctionConfiguration {
        function_name: output.function_name,
        function_arn: output.function_arn,
        state: output.state.map(|state| state.as_str().to_string()),
        last_update_status: output
            .last_update_status
            .map(|status| status.as_str().to_string()),
        kms_key_arn: output.kms_key_arn,
    }
}

fn function_configuration_from_update_code_output(
    output: aws_sdk_lambda::operation::update_function_code::UpdateFunctionCodeOutput,
) -> FunctionConfiguration {
    FunctionConfiguration {
        function_name: output.function_name,
        function_arn: output.function_arn,
        state: output.state.map(|state| state.as_str().to_string()),
        last_update_status: output
            .last_update_status
            .map(|status| status.as_str().to_string()),
        kms_key_arn: output.kms_key_arn,
    }
}

fn function_configuration_from_update_config_output(
    output: aws_sdk_lambda::operation::update_function_configuration::UpdateFunctionConfigurationOutput,
) -> FunctionConfiguration {
    FunctionConfiguration {
        function_name: output.function_name,
        function_arn: output.function_arn,
        state: output.state.map(|state| state.as_str().to_string()),
        last_update_status: output
            .last_update_status
            .map(|status| status.as_str().to_string()),
        kms_key_arn: output.kms_key_arn,
    }
}

fn function_configuration_from_get_output(
    output: aws_sdk_lambda::operation::get_function_configuration::GetFunctionConfigurationOutput,
) -> FunctionConfiguration {
    FunctionConfiguration {
        function_name: output.function_name,
        function_arn: output.function_arn,
        state: output.state.map(|state| state.as_str().to_string()),
        last_update_status: output
            .last_update_status
            .map(|status| status.as_str().to_string()),
        kms_key_arn: output.kms_key_arn,
    }
}

fn lambda_result<T, E>(
    result: std::result::Result<T, aws_sdk_lambda::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("ResourceNotFoundException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("ResourceConflictException") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: format!("{operation} reported ResourceConflictException"),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Lambda {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

fn is_s3_create_bucket_already_owned(
    error: &aws_sdk_s3::error::SdkError<CreateBucketError>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(CreateBucketError::is_bucket_already_owned_by_you)
}

fn is_s3_delete_bucket_not_found(error: &aws_sdk_s3::error::SdkError<DeleteBucketError>) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_delete_bucket_policy_not_found(
    error: &aws_sdk_s3::error::SdkError<DeleteBucketPolicyError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchBucketPolicy"],
    )
}

fn is_s3_delete_bucket_lifecycle_not_found(
    error: &aws_sdk_s3::error::SdkError<DeleteBucketLifecycleError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchLifecycleConfiguration"],
    )
}

fn is_s3_get_lifecycle_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketLifecycleConfigurationError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchLifecycleConfiguration"],
    )
}

fn is_s3_get_encryption_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketEncryptionError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &[
            "NoSuchBucket",
            "ServerSideEncryptionConfigurationNotFoundError",
        ],
    )
}

fn is_s3_get_public_access_block_not_found(
    error: &aws_sdk_s3::error::SdkError<GetPublicAccessBlockError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchPublicAccessBlockConfiguration"],
    )
}

fn is_s3_get_bucket_policy_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketPolicyError>,
) -> bool {
    s3_error_code(
        error.as_service_error(),
        &["NoSuchBucket", "NoSuchBucketPolicy"],
    )
}

fn is_s3_get_bucket_acl_not_found(error: &aws_sdk_s3::error::SdkError<GetBucketAclError>) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_get_notification_not_found(
    error: &aws_sdk_s3::error::SdkError<GetBucketNotificationConfigurationError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_list_versions_bucket_not_found(
    error: &aws_sdk_s3::error::SdkError<ListObjectVersionsError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["NoSuchBucket"])
}

fn is_s3_list_versions_invalid_argument(
    error: &aws_sdk_s3::error::SdkError<ListObjectVersionsError>,
) -> bool {
    s3_error_code(error.as_service_error(), &["InvalidArgument"])
}

fn is_s3_list_objects_bucket_not_found(
    error: &aws_sdk_s3::error::SdkError<ListObjectsV2Error>,
) -> bool {
    error
        .as_service_error()
        .is_some_and(ListObjectsV2Error::is_no_such_bucket)
}

fn s3_error_code<E>(error: Option<&E>, codes: &[&str]) -> bool
where
    E: ProvideErrorMetadata,
{
    error
        .and_then(ProvideErrorMetadata::code)
        .is_some_and(|code| codes.contains(&code))
}

fn s3_public_access_block(
    configuration: &AwsPublicAccessBlockConfiguration,
) -> S3PublicAccessBlock {
    S3PublicAccessBlock {
        block_public_acls: configuration.block_public_acls(),
        ignore_public_acls: configuration.ignore_public_acls(),
        block_public_policy: configuration.block_public_policy(),
        restrict_public_buckets: configuration.restrict_public_buckets(),
    }
}

fn s3_versioning_status(status: &BucketVersioningStatus) -> S3VersioningStatus {
    match status {
        BucketVersioningStatus::Enabled => S3VersioningStatus::Enabled,
        BucketVersioningStatus::Suspended => S3VersioningStatus::Suspended,
        _ => S3VersioningStatus::Suspended,
    }
}

fn s3_bucket_location_region(location_constraint: Option<&str>) -> String {
    match location_constraint {
        None | Some("") => "us-east-1".to_string(),
        Some("EU") => "eu-west-1".to_string(),
        Some(region) => region.to_string(),
    }
}

fn s3_object_identifier(key: &str, version_id: Option<&str>) -> Result<ObjectIdentifier> {
    ObjectIdentifier::builder()
        .key(key)
        .set_version_id(version_id.map(ToString::to_string))
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build S3 object identifier for key '{key}'"),
            resource_id: None,
        })
}

async fn delete_s3_objects(
    client: &S3Client,
    bucket_name: &str,
    objects: Vec<ObjectIdentifier>,
) -> Result<()> {
    let delete = Delete::builder()
        .set_objects(Some(objects))
        .quiet(true)
        .build()
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to build S3 DeleteObjects request for '{bucket_name}'"),
            resource_id: None,
        })?;

    client
        .delete_objects()
        .bucket(bucket_name)
        .delete(delete)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("S3 DeleteObjects API failed for bucket '{bucket_name}'"),
            resource_id: None,
        })?;

    Ok(())
}

fn notification_configuration_from_aws(
    output: &aws_sdk_s3::operation::get_bucket_notification_configuration::GetBucketNotificationConfigurationOutput,
) -> NotificationConfiguration {
    NotificationConfiguration {
        lambda_function_configurations: output
            .lambda_function_configurations()
            .iter()
            .map(|configuration| LambdaFunctionConfiguration {
                id: configuration.id().map(ToString::to_string),
                lambda_function_arn: configuration.lambda_function_arn().to_string(),
                events: configuration
                    .events()
                    .iter()
                    .map(|event| event.as_str().to_string())
                    .collect(),
                filter: None,
            })
            .collect(),
    }
}

fn notification_configuration_to_aws(
    config: &NotificationConfiguration,
) -> Result<AwsNotificationConfiguration> {
    let lambda_function_configurations = config
        .lambda_function_configurations
        .iter()
        .map(|configuration| {
            AwsLambdaFunctionConfiguration::builder()
                .set_id(configuration.id.clone())
                .lambda_function_arn(&configuration.lambda_function_arn)
                .set_events(Some(
                    configuration
                        .events
                        .iter()
                        .map(|event| AwsS3Event::from(event.as_str()))
                        .collect(),
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to build S3 Lambda notification configuration".to_string(),
                    resource_id: None,
                })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(AwsNotificationConfiguration::builder()
        .set_lambda_function_configurations(Some(lambda_function_configurations))
        .build())
}

fn dynamodb_table_description(
    table: &aws_sdk_dynamodb::types::TableDescription,
) -> DynamoDbTableDescription {
    DynamoDbTableDescription {
        table_name: table.table_name().map(ToString::to_string),
        table_arn: table.table_arn().map(ToString::to_string),
        table_status: table
            .table_status()
            .map(|status| status.as_str().to_string()),
        billing_mode: table
            .billing_mode_summary()
            .and_then(|summary| summary.billing_mode())
            .map(|mode| mode.as_str().to_string()),
        key_schema: table
            .key_schema()
            .iter()
            .map(|key| DynamoDbKeySchemaElement {
                attribute_name: key.attribute_name().to_string(),
                key_type: key.key_type().as_str().to_string(),
            })
            .collect(),
        global_secondary_index_count: usize_to_u32(table.global_secondary_indexes().len()),
        local_secondary_index_count: usize_to_u32(table.local_secondary_indexes().len()),
        item_count: nonnegative_i64_to_u64(table.item_count()),
        table_size_bytes: nonnegative_i64_to_u64(table.table_size_bytes()),
        stream_enabled: table
            .stream_specification()
            .map(|stream| stream.stream_enabled()),
        stream_view_type: table
            .stream_specification()
            .and_then(|stream| stream.stream_view_type())
            .map(|stream_type| stream_type.as_str().to_string()),
        deletion_protection_enabled: table.deletion_protection_enabled(),
        sse_status: table
            .sse_description()
            .and_then(|sse| sse.status())
            .map(|status| status.as_str().to_string()),
        sse_type: table
            .sse_description()
            .and_then(|sse| sse.sse_type())
            .map(|sse_type| sse_type.as_str().to_string()),
        table_class: table
            .table_class_summary()
            .and_then(|summary| summary.table_class())
            .map(|table_class| table_class.as_str().to_string()),
        replica_count: usize_to_u32(table.replicas().len()),
        restore_in_progress: table
            .restore_summary()
            .map(|summary| summary.restore_in_progress()),
    }
}

fn dynamodb_ttl_description(
    ttl: &aws_sdk_dynamodb::types::TimeToLiveDescription,
) -> DynamoDbTtlDescription {
    DynamoDbTtlDescription {
        status: ttl
            .time_to_live_status()
            .map(|status| status.as_str().to_string()),
        attribute_name: ttl.attribute_name().map(ToString::to_string),
    }
}

fn nonnegative_i64_to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn usize_to_u32(value: usize) -> Option<u32> {
    u32::try_from(value).ok()
}

fn smithy_datetime_to_chrono_utc(
    date_time: &aws_sdk_ssm::primitives::DateTime,
) -> Result<DateTime<Utc>> {
    let formatted = date_time
        .fmt(DateTimeFormat::DateTime)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to format SSM Parameter Store timestamp".to_string(),
            resource_id: None,
        })?;

    chrono::DateTime::parse_from_rfc3339(&formatted)
        .map(|date_time| date_time.to_utc())
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to parse SSM Parameter Store timestamp '{formatted}'"),
            resource_id: None,
        })
}

fn parameter_type_name(parameter_type: &ParameterType) -> String {
    match parameter_type {
        ParameterType::String => "String",
        ParameterType::StringList => "StringList",
        ParameterType::SecureString => "SecureString",
        _ => parameter_type.as_str(),
    }
    .to_string()
}

fn parameter_tier_name(tier: &ParameterTier) -> String {
    match tier {
        ParameterTier::Standard => "Standard",
        ParameterTier::Advanced => "Advanced",
        ParameterTier::IntelligentTiering => "Intelligent-Tiering",
        _ => tier.as_str(),
    }
    .to_string()
}

/// Build an official AWS SDK config from Alien's public AWS client config.
pub async fn sdk_config_from_alien_config(config: &AwsClientConfig) -> Result<SdkConfig> {
    let region = Region::new(config.region.clone());
    let loader = aws_config::defaults(BehaviorVersion::latest()).region(region.clone());

    let loader = match &config.credentials {
        AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } => loader.credentials_provider(Credentials::new(
            access_key_id,
            secret_access_key,
            session_token.clone(),
            None,
            "AlienAccessKeys",
        )),
        AwsCredentials::SessionCredentials {
            access_key_id,
            secret_access_key,
            session_token,
            expires_at,
        } => {
            let expires_after = chrono::DateTime::parse_from_rfc3339(expires_at)
                .map(|expires_at| expires_at.to_utc().into())
                .into_alien_error()
                .context(ErrorData::ClientConfigInvalid {
                    platform: Platform::Aws,
                    message: format!("Invalid AWS credential expiration timestamp: {expires_at}"),
                })?;

            loader.credentials_provider(Credentials::new(
                access_key_id,
                secret_access_key,
                Some(session_token.clone()),
                Some(expires_after),
                "AlienSessionCredentials",
            ))
        }
        AwsCredentials::Profile { name } => loader.profile_name(name),
        AwsCredentials::WebIdentity { config } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let provider =
                aws_config::web_identity_token::WebIdentityTokenCredentialsProvider::builder()
                    .configure(&provider_config)
                    .static_configuration(aws_config::web_identity_token::StaticConfiguration {
                        web_identity_token_file: config.web_identity_token_file.clone().into(),
                        role_arn: config.role_arn.clone(),
                        session_name: config
                            .session_name
                            .clone()
                            .unwrap_or_else(|| "alien-web-identity".to_string()),
                    })
                    .build();
            loader.credentials_provider(provider)
        }
        AwsCredentials::Imds { endpoint } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let mut client_builder =
                aws_config::imds::Client::builder().configure(&provider_config);
            if let Some(endpoint) = endpoint {
                client_builder = client_builder.endpoint(endpoint).map_err(|err| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: format!("Invalid AWS IMDS endpoint override '{endpoint}': {err}"),
                    })
                })?;
            }
            let imds_client = client_builder.build();
            let provider = aws_config::imds::credentials::ImdsCredentialsProvider::builder()
                .configure(&provider_config)
                .imds_client(imds_client)
                .build();
            loader.credentials_provider(provider)
        }
    };

    Ok(loader.load().await)
}

/// Create an official AWS SDK CodeBuild client with Alien endpoint override support.
pub async fn codebuild_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<CodeBuildClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut codebuild_config = aws_sdk_codebuild::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("codebuild"))
    {
        codebuild_config = codebuild_config.endpoint_url(endpoint);
    }

    Ok(CodeBuildClient::from_conf(codebuild_config.build()))
}

/// Create an official AWS SDK ACM client with Alien endpoint override support.
pub async fn acm_client_from_alien_config(config: &AwsClientConfig) -> Result<AcmClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut acm_config = aws_sdk_acm::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("acm"))
    {
        acm_config = acm_config.endpoint_url(endpoint);
    }

    Ok(AcmClient::from_conf(acm_config.build()))
}

/// Create an official AWS SDK Lambda client with Alien endpoint override support.
pub async fn lambda_client_from_alien_config(config: &AwsClientConfig) -> Result<LambdaClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut lambda_config = aws_sdk_lambda::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("lambda"))
    {
        lambda_config = lambda_config.endpoint_url(endpoint);
    }

    Ok(LambdaClient::from_conf(lambda_config.build()))
}

/// Create an official AWS SDK API Gateway V2 client with Alien endpoint override support.
pub async fn apigatewayv2_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<ApiGatewayV2Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut apigatewayv2_config = aws_sdk_apigatewayv2::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("apigateway"))
    {
        apigatewayv2_config = apigatewayv2_config.endpoint_url(endpoint);
    }

    Ok(ApiGatewayV2Client::from_conf(apigatewayv2_config.build()))
}

/// Create an official AWS SDK EventBridge client with Alien endpoint override support.
pub async fn eventbridge_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<EventBridgeClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut eventbridge_config = aws_sdk_eventbridge::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("events"))
    {
        eventbridge_config = eventbridge_config.endpoint_url(endpoint);
    }

    Ok(EventBridgeClient::from_conf(eventbridge_config.build()))
}

/// Create an official AWS SDK EC2 client with Alien endpoint override support.
pub async fn ec2_client_from_alien_config(config: &AwsClientConfig) -> Result<Ec2Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ec2_config = aws_sdk_ec2::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ec2"))
    {
        ec2_config = ec2_config.endpoint_url(endpoint);
    }

    Ok(Ec2Client::from_conf(ec2_config.build()))
}

/// Create an official AWS SDK ECR client with Alien endpoint override support.
pub async fn ecr_client_from_alien_config(config: &AwsClientConfig) -> Result<EcrClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ecr_config = aws_sdk_ecr::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ecr"))
    {
        ecr_config = ecr_config.endpoint_url(endpoint);
    }

    Ok(EcrClient::from_conf(ecr_config.build()))
}

/// Create an official AWS SDK IAM client with Alien endpoint override support.
pub async fn iam_client_from_alien_config(config: &AwsClientConfig) -> Result<IamClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut iam_config = aws_sdk_iam::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("iam"))
    {
        iam_config = iam_config.endpoint_url(endpoint);
    }

    Ok(IamClient::from_conf(iam_config.build()))
}

/// Create an official AWS SDK SSM client with Alien endpoint override support.
pub async fn ssm_client_from_alien_config(config: &AwsClientConfig) -> Result<aws_sdk_ssm::Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ssm_config = aws_sdk_ssm::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ssm"))
    {
        ssm_config = ssm_config.endpoint_url(endpoint);
    }

    Ok(aws_sdk_ssm::Client::from_conf(ssm_config.build()))
}

/// Create an official AWS SDK DynamoDB client with Alien endpoint override support.
pub async fn dynamodb_client_from_alien_config(config: &AwsClientConfig) -> Result<DynamoDbClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut dynamodb_config = aws_sdk_dynamodb::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("dynamodb"))
    {
        dynamodb_config = dynamodb_config.endpoint_url(endpoint);
    }

    Ok(DynamoDbClient::from_conf(dynamodb_config.build()))
}

/// Create an official AWS SDK SQS client with Alien endpoint override support.
pub async fn sqs_client_from_alien_config(config: &AwsClientConfig) -> Result<SqsClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut sqs_config = aws_sdk_sqs::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("sqs"))
    {
        sqs_config = sqs_config.endpoint_url(endpoint);
    }

    Ok(SqsClient::from_conf(sqs_config.build()))
}

/// Create an official AWS SDK S3 client with Alien endpoint override support.
pub async fn s3_client_from_alien_config(config: &AwsClientConfig) -> Result<S3Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("s3"))
    {
        s3_config = s3_config.endpoint_url(endpoint);
    }

    Ok(S3Client::from_conf(s3_config.build()))
}
