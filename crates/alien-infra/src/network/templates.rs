//! CloudFormation templates and importers for Network resources.

use async_trait::async_trait;

use crate::error::{ErrorData, Result};
use alien_core::{Network, NetworkSettings, Resource};
use alien_error::{AlienError, Context};

/// CloudFormation importer for AWS Network resources.
///
/// This importer handles both:
/// - Create mode: Imports VPC, subnets, IGW, NAT, and security group from CloudFormation
/// - BYO-VPC mode: Uses the settings directly since infrastructure already exists
#[derive(Debug, Clone, Default)]
pub struct AwsNetworkCloudFormationImporter;

#[async_trait]
impl crate::cloudformation::traits::CloudFormationResourceImporter
    for AwsNetworkCloudFormationImporter
{
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &crate::cloudformation::traits::CloudFormationImportContext,
    ) -> Result<Box<dyn crate::core::ResourceController>> {
        use alien_aws_clients::ec2::{DescribeVpcsRequest, Ec2Api, Ec2Client};
        use tracing::info;

        let network = resource.downcast_ref::<Network>().ok_or_else(|| {
            AlienError::new(ErrorData::ControllerResourceTypeMismatch {
                expected: Network::RESOURCE_TYPE,
                actual: resource.resource_type(),
                resource_id: resource.id().to_string(),
            })
        })?;

        match &network.settings {
            NetworkSettings::UseDefault => {
                // UseDefault on AWS uses the default VPC — import as BYO-like state
                Ok(Box::new(crate::network::AwsNetworkController {
                    state: crate::network::AwsNetworkState::Ready,
                    vpc_id: None,
                    cidr_block: None,
                    internet_gateway_id: None,
                    nat_gateway_id: None,
                    eip_allocation_id: None,
                    public_subnet_ids: vec![],
                    private_subnet_ids: vec![],
                    public_route_table_id: None,
                    private_route_table_id: None,
                    route_table_association_ids: vec![],
                    security_group_id: None,
                    availability_zones: vec![],
                    is_byo_vpc: true,
                    _internal_stay_count: None,
                }))
            }

            NetworkSettings::ByoVpcAws {
                vpc_id,
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
            } => {
                info!(
                    vpc_id = %vpc_id,
                    network_id = %network.id,
                    "Importing BYO-VPC Network from CloudFormation"
                );

                // For BYO-VPC, we simply use the settings directly
                // The VPC already exists and is managed externally
                Ok(Box::new(crate::network::AwsNetworkController {
                    state: crate::network::AwsNetworkState::Ready,
                    vpc_id: Some(vpc_id.clone()),
                    cidr_block: None,
                    internet_gateway_id: None,
                    nat_gateway_id: None,
                    eip_allocation_id: None,
                    public_subnet_ids: public_subnet_ids.clone(),
                    private_subnet_ids: private_subnet_ids.clone(),
                    public_route_table_id: None,
                    private_route_table_id: None,
                    route_table_association_ids: vec![],
                    security_group_id: security_group_ids.first().cloned(),
                    availability_zones: (0..private_subnet_ids.len())
                        .map(|i| format!("az-{}", i))
                        .collect(),
                    is_byo_vpc: true,
                    _internal_stay_count: None,
                }))
            }

            NetworkSettings::Create { .. } => {
                info!(
                    network_id = %network.id,
                    "Importing created Network from CloudFormation"
                );

                // For created networks, we need to import the VPC and related resources
                // from CloudFormation physical IDs

                // Look for the VPC in CloudFormation resources
                // The logical ID pattern is typically: {resource_prefix}Vpc or DefaultNetworkVpc
                let vpc_logical_id_patterns = [
                    format!("{}Vpc", heck::AsPascalCase(&network.id)),
                    "DefaultNetworkVpc".to_string(),
                    "Vpc".to_string(),
                ];

                let mut vpc_physical_id = None;
                for pattern in &vpc_logical_id_patterns {
                    if let Some(physical_id) = context.cfn_resources.get(pattern) {
                        vpc_physical_id = Some(physical_id.clone());
                        break;
                    }
                }

                let vpc_id = vpc_physical_id.ok_or_else(|| {
                    AlienError::new(ErrorData::CloudFormationResourceMissing {
                        logical_id: "VPC".to_string(),
                        stack_name: context.stack_name.clone(),
                        resource_id: Some(network.id().to_string()),
                    })
                })?;

                // Query VPC details to get CIDR block
                let ec2_client = Ec2Client::new(reqwest::Client::new(), context.aws_config.clone());

                let vpc_response = ec2_client
                    .describe_vpcs(
                        DescribeVpcsRequest::builder()
                            .vpc_ids(vec![vpc_id.clone()])
                            .build(),
                    )
                    .await
                    .context(ErrorData::InfrastructureImportFailed {
                        message: format!("Failed to describe VPC '{}' during import", vpc_id),
                        import_source: Some("CloudFormation".to_string()),
                        resource_id: Some(network.id().to_string()),
                    })?;

                let cidr_block = vpc_response
                    .vpc_set
                    .and_then(|set| set.items.into_iter().next())
                    .and_then(|vpc| vpc.cidr_block);

                // Find subnets by looking for CloudFormation resources with subnet patterns
                let mut public_subnet_ids = Vec::new();
                let mut private_subnet_ids = Vec::new();

                for (logical_id, physical_id) in &context.cfn_resources {
                    if logical_id.contains("PublicSubnet")
                        || logical_id.contains("Public") && logical_id.contains("Subnet")
                    {
                        public_subnet_ids.push(physical_id.clone());
                    } else if logical_id.contains("PrivateSubnet")
                        || logical_id.contains("Private") && logical_id.contains("Subnet")
                    {
                        private_subnet_ids.push(physical_id.clone());
                    }
                }

                // Find Internet Gateway
                let igw_id = context
                    .cfn_resources
                    .iter()
                    .find(|(k, _)| k.contains("InternetGateway") || k.as_str() == "Igw")
                    .map(|(_, v)| v.clone());

                // Find NAT Gateway
                let nat_gateway_id = context
                    .cfn_resources
                    .iter()
                    .find(|(k, _)| k.contains("NatGateway") || k.as_str() == "Nat")
                    .map(|(_, v)| v.clone());

                // Find EIP allocation
                let eip_allocation_id = context
                    .cfn_resources
                    .iter()
                    .find(|(k, _)| k.contains("ElasticIp") || k.contains("Eip"))
                    .map(|(_, v)| v.clone());

                // Find Route Tables
                let public_route_table_id = context
                    .cfn_resources
                    .iter()
                    .find(|(k, _)| k.contains("PublicRouteTable"))
                    .map(|(_, v)| v.clone());

                let private_route_table_id = context
                    .cfn_resources
                    .iter()
                    .find(|(k, _)| k.contains("PrivateRouteTable"))
                    .map(|(_, v)| v.clone());

                // Find Security Group
                let security_group_id = context
                    .cfn_resources
                    .iter()
                    .find(|(k, _)| {
                        k.contains("SecurityGroup")
                            && (k.contains("Network") || k.contains("Default"))
                    })
                    .map(|(_, v)| v.clone());

                // Determine availability zones from subnet count
                let az_count = std::cmp::max(public_subnet_ids.len(), private_subnet_ids.len());
                let availability_zones: Vec<String> =
                    (0..az_count).map(|i| format!("az-{}", i)).collect();

                info!(
                    vpc_id = %vpc_id,
                    public_subnets = ?public_subnet_ids,
                    private_subnets = ?private_subnet_ids,
                    igw_id = ?igw_id,
                    nat_id = ?nat_gateway_id,
                    sg_id = ?security_group_id,
                    "Imported Network state from CloudFormation"
                );

                Ok(Box::new(crate::network::AwsNetworkController {
                    state: crate::network::AwsNetworkState::Ready,
                    vpc_id: Some(vpc_id),
                    cidr_block,
                    internet_gateway_id: igw_id,
                    nat_gateway_id,
                    eip_allocation_id,
                    public_subnet_ids,
                    private_subnet_ids,
                    public_route_table_id,
                    private_route_table_id,
                    route_table_association_ids: vec![], // Not tracked during import
                    security_group_id,
                    availability_zones,
                    is_byo_vpc: false,
                    _internal_stay_count: None,
                }))
            }

            _ => Err(AlienError::new(ErrorData::InfrastructureImportFailed {
                message: "Cannot import non-AWS network settings on AWS platform".to_string(),
                import_source: Some("CloudFormation".to_string()),
                resource_id: Some(network.id().to_string()),
            })),
        }
    }
}
