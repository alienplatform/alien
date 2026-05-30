//! AWS KubernetesCluster — create-only EKS Auto Mode cluster.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        default_network, private_subnet_ids_expr, required_logical_id, resource_config,
        service_trust_policy, tag, tags, CONDITION_HAS_VPC_CIDR, PARAM_VPC_CIDR,
    },
    template::{CfExpression, CfResource},
};
use alien_core::{import::EmitContext, KubernetesCluster, Result};

const CONDITION_NETWORK_MODE_CREATE: &str = "NetworkModeCreate";
const CONDITION_NETWORK_MODE_USE_EXISTING: &str = "NetworkModeUseExisting";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKubernetesClusterEmitter;

impl CfEmitter for AwsKubernetesClusterEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        resource_config::<KubernetesCluster>(ctx, KubernetesCluster::RESOURCE_TYPE)?;
        let prefix = required_logical_id(ctx)?;
        Ok(eks_resources(ctx, prefix))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let cluster = resource_config::<KubernetesCluster>(ctx, KubernetesCluster::RESOURCE_TYPE)?;
        let prefix = required_logical_id(ctx)?;
        Ok(CfExpression::object([
            ("provider", CfExpression::from("eks")),
            ("ownership", CfExpression::from("managed")),
            ("namespace", CfExpression::from(cluster.namespace.clone())),
            ("clusterName", CfExpression::ref_(cluster_id(prefix))),
            ("clusterId", CfExpression::ref_(cluster_id(prefix))),
            ("cloudMetadataReady", CfExpression::from(true)),
        ]))
    }
}

fn eks_resources(ctx: &EmitContext<'_>, prefix: &str) -> Vec<CfResource> {
    let vpc_id = resource_id(prefix, "Vpc");
    let igw_id = resource_id(prefix, "InternetGateway");
    let gateway_attachment_id = resource_id(prefix, "VpcGatewayAttachment");
    let nat_eip_id = resource_id(prefix, "NatEip");
    let nat_gateway_id = resource_id(prefix, "NatGateway");
    let public_route_table_id = resource_id(prefix, "PublicRouteTable");
    let private_route_table_id = resource_id(prefix, "PrivateRouteTable");
    let cluster_role_id = resource_id(prefix, "ClusterRole");
    let node_role_id = resource_id(prefix, "NodeRole");
    let managed_node_role_id = resource_id(prefix, "ManagedNodeRole");
    let cluster_id = cluster_id(prefix);
    let node_group_id = resource_id(prefix, "NodeGroup");
    let oidc_provider_id = resource_id(prefix, "OidcProvider");
    let ebs_csi_role_id = resource_id(prefix, "EbsCsiRole");

    let mut resources = Vec::new();
    if default_network(ctx).is_none() {
        resources.extend([
            vpc(ctx, &vpc_id),
            internet_gateway(ctx, &igw_id),
            vpc_gateway_attachment(&gateway_attachment_id, &vpc_id, &igw_id),
            public_subnet(ctx, prefix, 1, &vpc_id),
            public_subnet(ctx, prefix, 2, &vpc_id),
            private_subnet(ctx, prefix, 1, &vpc_id),
            private_subnet(ctx, prefix, 2, &vpc_id),
            nat_eip(ctx, &nat_eip_id),
            nat_gateway(
                ctx,
                &nat_gateway_id,
                &nat_eip_id,
                &public_subnet_id(prefix, 1),
            ),
            route_table(ctx, &public_route_table_id, &vpc_id),
            route_table(ctx, &private_route_table_id, &vpc_id),
            default_route(
                &resource_id(prefix, "PublicDefaultRoute"),
                &public_route_table_id,
                "GatewayId",
                &igw_id,
            ),
            default_route(
                &resource_id(prefix, "PrivateDefaultRoute"),
                &private_route_table_id,
                "NatGatewayId",
                &nat_gateway_id,
            ),
            route_table_association(
                &resource_id(prefix, "PublicRouteTableAssociation1"),
                &public_subnet_id(prefix, 1),
                &public_route_table_id,
            ),
            route_table_association(
                &resource_id(prefix, "PublicRouteTableAssociation2"),
                &public_subnet_id(prefix, 2),
                &public_route_table_id,
            ),
            route_table_association(
                &resource_id(prefix, "PrivateRouteTableAssociation1"),
                &private_subnet_id(prefix, 1),
                &private_route_table_id,
            ),
            route_table_association(
                &resource_id(prefix, "PrivateRouteTableAssociation2"),
                &private_subnet_id(prefix, 2),
                &private_route_table_id,
            ),
        ]);
    }

    resources.extend([
        eks_cluster_role(
            ctx,
            &cluster_role_id,
            &[
                "arn:aws:iam::aws:policy/AmazonEKSClusterPolicy",
                "arn:aws:iam::aws:policy/AmazonEKSBlockStoragePolicy",
                "arn:aws:iam::aws:policy/AmazonEKSComputePolicy",
                "arn:aws:iam::aws:policy/AmazonEKSLoadBalancingPolicy",
                "arn:aws:iam::aws:policy/AmazonEKSNetworkingPolicy",
            ],
        ),
        iam_role(
            ctx,
            &node_role_id,
            "ec2.amazonaws.com",
            &[
                "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy",
                "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly",
                "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy",
                "arn:aws:iam::aws:policy/AmazonEKSWorkerNodeMinimalPolicy",
            ],
        ),
        iam_role(
            ctx,
            &managed_node_role_id,
            "ec2.amazonaws.com",
            &[
                "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy",
                "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly",
                "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy",
            ],
        ),
    ]);

    resources.push(eks_cluster(
        ctx,
        &cluster_id,
        &cluster_role_id,
        &node_role_id,
        prefix,
    ));
    resources.push(oidc_provider(&oidc_provider_id, &cluster_id));
    resources.push(eks_addon(
        &resource_id(prefix, "VpcCniAddon"),
        &cluster_id,
        "vpc-cni",
        None,
    ));
    resources.push(node_group(
        ctx,
        &node_group_id,
        &cluster_id,
        &managed_node_role_id,
        prefix,
    ));
    resources.push(ebs_csi_role(
        ctx,
        &ebs_csi_role_id,
        &oidc_provider_id,
        &cluster_id,
    ));
    resources.push(eks_addon(
        &resource_id(prefix, "EbsCsiAddon"),
        &cluster_id,
        "aws-ebs-csi-driver",
        Some(CfExpression::get_att(&ebs_csi_role_id, "Arn")),
    ));
    resources.push(eks_addon(
        &resource_id(prefix, "KubeProxyAddon"),
        &cluster_id,
        "kube-proxy",
        None,
    ));
    resources.push(eks_addon(
        &resource_id(prefix, "CoreDnsAddon"),
        &cluster_id,
        "coredns",
        None,
    ));

    resources
}

fn vpc(ctx: &EmitContext<'_>, id: &str) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EC2::VPC".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource.properties.insert(
        "CidrBlock".to_string(),
        CfExpression::if_(
            CONDITION_HAS_VPC_CIDR,
            CfExpression::ref_(PARAM_VPC_CIDR),
            CfExpression::from("10.251.0.0/16"),
        ),
    );
    resource
        .properties
        .insert("EnableDnsSupport".to_string(), CfExpression::from(true));
    resource
        .properties
        .insert("EnableDnsHostnames".to_string(), CfExpression::from(true));
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource
}

fn internet_gateway(ctx: &EmitContext<'_>, id: &str) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EC2::InternetGateway".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource
}

fn vpc_gateway_attachment(id: &str, vpc_id: &str, igw_id: &str) -> CfResource {
    let mut resource =
        CfResource::new(id.to_string(), "AWS::EC2::VPCGatewayAttachment".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource
        .properties
        .insert("VpcId".to_string(), CfExpression::ref_(vpc_id));
    resource
        .properties
        .insert("InternetGatewayId".to_string(), CfExpression::ref_(igw_id));
    resource
}

fn public_subnet(ctx: &EmitContext<'_>, prefix: &str, index: usize, vpc_id: &str) -> CfResource {
    subnet(
        ctx,
        &public_subnet_id(prefix, index),
        vpc_id,
        index - 1,
        index - 1,
        Some("kubernetes.io/role/elb"),
    )
}

fn private_subnet(ctx: &EmitContext<'_>, prefix: &str, index: usize, vpc_id: &str) -> CfResource {
    subnet(
        ctx,
        &private_subnet_id(prefix, index),
        vpc_id,
        index - 1,
        index + 9,
        Some("kubernetes.io/role/internal-elb"),
    )
}

fn subnet(
    ctx: &EmitContext<'_>,
    id: &str,
    vpc_id: &str,
    az_index: usize,
    cidr_index: usize,
    role_tag: Option<&str>,
) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EC2::Subnet".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource
        .properties
        .insert("VpcId".to_string(), CfExpression::ref_(vpc_id));
    resource
        .properties
        .insert("CidrBlock".to_string(), cidr_block(vpc_id, cidr_index));
    resource
        .properties
        .insert("AvailabilityZone".to_string(), availability_zone(az_index));
    if id.contains("Public") {
        resource
            .properties
            .insert("MapPublicIpOnLaunch".to_string(), CfExpression::from(true));
    }
    let mut tag_values = match tags(ctx) {
        CfExpression::List(items) => items,
        other => vec![other],
    };
    tag_values.push(CfExpression::object([
        (
            "Key",
            CfExpression::sub("kubernetes.io/cluster/${AWS::StackName}-k8s"),
        ),
        ("Value", CfExpression::from("shared")),
    ]));
    if let Some(role_tag) = role_tag {
        tag_values.push(tag(role_tag, "1"));
    }
    resource
        .properties
        .insert("Tags".to_string(), CfExpression::list(tag_values));
    resource
}

fn nat_eip(ctx: &EmitContext<'_>, id: &str) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EC2::EIP".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource
        .properties
        .insert("Domain".to_string(), CfExpression::from("vpc"));
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource
}

fn nat_gateway(ctx: &EmitContext<'_>, id: &str, eip_id: &str, subnet_id: &str) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EC2::NatGateway".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource.properties.insert(
        "AllocationId".to_string(),
        CfExpression::get_att(eip_id, "AllocationId"),
    );
    resource
        .properties
        .insert("SubnetId".to_string(), CfExpression::ref_(subnet_id));
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource
}

fn route_table(ctx: &EmitContext<'_>, id: &str, vpc_id: &str) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EC2::RouteTable".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource
        .properties
        .insert("VpcId".to_string(), CfExpression::ref_(vpc_id));
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource
}

fn default_route(id: &str, route_table_id: &str, target_key: &str, target_id: &str) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EC2::Route".to_string());
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource.properties.insert(
        "RouteTableId".to_string(),
        CfExpression::ref_(route_table_id),
    );
    resource.properties.insert(
        "DestinationCidrBlock".to_string(),
        CfExpression::from("0.0.0.0/0"),
    );
    resource
        .properties
        .insert(target_key.to_string(), CfExpression::ref_(target_id));
    resource
}

fn route_table_association(id: &str, subnet_id: &str, route_table_id: &str) -> CfResource {
    let mut resource = CfResource::new(
        id.to_string(),
        "AWS::EC2::SubnetRouteTableAssociation".to_string(),
    );
    resource.condition = Some(CONDITION_NETWORK_MODE_CREATE.to_string());
    resource
        .properties
        .insert("SubnetId".to_string(), CfExpression::ref_(subnet_id));
    resource.properties.insert(
        "RouteTableId".to_string(),
        CfExpression::ref_(route_table_id),
    );
    resource
}

fn iam_role(ctx: &EmitContext<'_>, id: &str, service: &str, policy_arns: &[&str]) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::IAM::Role".to_string());
    resource.properties.insert(
        "RoleName".to_string(),
        CfExpression::sub(format!("${{AWS::StackName}}-{id}")),
    );
    resource.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        service_trust_policy([service]),
    );
    if !policy_arns.is_empty() {
        resource.properties.insert(
            "ManagedPolicyArns".to_string(),
            CfExpression::list(policy_arns.iter().copied().map(CfExpression::from)),
        );
    }
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource
}

fn eks_cluster_role(ctx: &EmitContext<'_>, id: &str, policy_arns: &[&str]) -> CfResource {
    let mut resource = iam_role(ctx, id, "eks.amazonaws.com", policy_arns);
    resource.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        eks_cluster_trust_policy(),
    );
    resource
}

fn eks_cluster_trust_policy() -> CfExpression {
    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        (
            "Statement",
            CfExpression::list([CfExpression::object([
                ("Effect", CfExpression::from("Allow")),
                (
                    "Principal",
                    CfExpression::object([("Service", CfExpression::from("eks.amazonaws.com"))]),
                ),
                (
                    "Action",
                    CfExpression::list([
                        CfExpression::from("sts:AssumeRole"),
                        CfExpression::from("sts:TagSession"),
                    ]),
                ),
            ])]),
        ),
    ])
}

fn eks_cluster(
    ctx: &EmitContext<'_>,
    id: &str,
    cluster_role_id: &str,
    node_role_id: &str,
    prefix: &str,
) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EKS::Cluster".to_string());
    resource.properties.insert(
        "Name".to_string(),
        CfExpression::sub("${AWS::StackName}-k8s"),
    );
    resource.properties.insert(
        "RoleArn".to_string(),
        CfExpression::get_att(cluster_role_id, "Arn"),
    );
    resource.properties.insert(
        "BootstrapSelfManagedAddons".to_string(),
        CfExpression::from(false),
    );
    resource.properties.insert(
        "ResourcesVpcConfig".to_string(),
        CfExpression::object([
            ("SubnetIds", eks_private_subnet_ids(ctx, prefix)),
            ("EndpointPublicAccess", CfExpression::from(true)),
            ("EndpointPrivateAccess", CfExpression::from(true)),
        ]),
    );
    resource.properties.insert(
        "AccessConfig".to_string(),
        CfExpression::object([
            (
                "AuthenticationMode",
                CfExpression::from("API_AND_CONFIG_MAP"),
            ),
            (
                "BootstrapClusterCreatorAdminPermissions",
                CfExpression::from(true),
            ),
        ]),
    );
    resource.properties.insert(
        "ComputeConfig".to_string(),
        CfExpression::object([
            ("Enabled", CfExpression::from(true)),
            (
                "NodePools",
                CfExpression::list([CfExpression::from("system")]),
            ),
            ("NodeRoleArn", CfExpression::get_att(node_role_id, "Arn")),
        ]),
    );
    resource.properties.insert(
        "KubernetesNetworkConfig".to_string(),
        CfExpression::object([(
            "ElasticLoadBalancing",
            CfExpression::object([("Enabled", CfExpression::from(true))]),
        )]),
    );
    resource.properties.insert(
        "StorageConfig".to_string(),
        CfExpression::object([(
            "BlockStorage",
            CfExpression::object([("Enabled", CfExpression::from(true))]),
        )]),
    );
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource
        .depends_on
        .extend([cluster_role_id.to_string(), node_role_id.to_string()]);
    resource
}

fn node_group(
    ctx: &EmitContext<'_>,
    id: &str,
    cluster_id: &str,
    role_id: &str,
    prefix: &str,
) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EKS::Nodegroup".to_string());
    resource
        .properties
        .insert("ClusterName".to_string(), CfExpression::ref_(cluster_id));
    resource.properties.insert(
        "NodeRole".to_string(),
        CfExpression::get_att(role_id, "Arn"),
    );
    resource
        .properties
        .insert("Subnets".to_string(), eks_private_subnet_ids(ctx, prefix));
    resource.properties.insert(
        "AmiType".to_string(),
        CfExpression::from("AL2023_ARM_64_STANDARD"),
    );
    resource
        .properties
        .insert("CapacityType".to_string(), CfExpression::from("ON_DEMAND"));
    resource.properties.insert(
        "InstanceTypes".to_string(),
        CfExpression::list([CfExpression::from("t4g.medium")]),
    );
    resource.properties.insert(
        "ScalingConfig".to_string(),
        CfExpression::object([
            ("DesiredSize", CfExpression::from(2u8)),
            ("MaxSize", CfExpression::from(3u8)),
            ("MinSize", CfExpression::from(2u8)),
        ]),
    );
    resource
        .properties
        .insert("Tags".to_string(), eks_tags(ctx));
    resource.depends_on.push(cluster_id.to_string());
    resource
}

fn eks_tags(ctx: &EmitContext<'_>) -> CfExpression {
    CfExpression::object([
        ("managed-by", CfExpression::from("deployment")),
        ("deployment", CfExpression::ref_("AWS::StackName")),
        ("resource", CfExpression::from(ctx.resource_id)),
        (
            "resource-type",
            CfExpression::from(ctx.resource.config.resource_type().as_ref()),
        ),
    ])
}

fn eks_addon(
    id: &str,
    cluster_id: &str,
    addon_name: &str,
    service_account_role_arn: Option<CfExpression>,
) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::EKS::Addon".to_string());
    resource
        .properties
        .insert("ClusterName".to_string(), CfExpression::ref_(cluster_id));
    resource
        .properties
        .insert("AddonName".to_string(), CfExpression::from(addon_name));
    if let Some(role_arn) = service_account_role_arn {
        resource
            .properties
            .insert("ServiceAccountRoleArn".to_string(), role_arn);
    }
    resource.depends_on.push(cluster_id.to_string());
    resource
}

fn oidc_provider(id: &str, cluster_id: &str) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::IAM::OIDCProvider".to_string());
    resource.properties.insert(
        "Url".to_string(),
        CfExpression::get_att(cluster_id, "OpenIdConnectIssuerUrl"),
    );
    resource.properties.insert(
        "ClientIdList".to_string(),
        CfExpression::list([CfExpression::from("sts.amazonaws.com")]),
    );
    resource.depends_on.push(cluster_id.to_string());
    resource
}

fn ebs_csi_role(
    ctx: &EmitContext<'_>,
    id: &str,
    oidc_provider_id: &str,
    cluster_id: &str,
) -> CfResource {
    let mut resource = CfResource::new(id.to_string(), "AWS::IAM::Role".to_string());
    resource.properties.insert(
        "RoleName".to_string(),
        CfExpression::sub(format!("${{AWS::StackName}}-{id}")),
    );
    resource.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        irsa_policy_document(
            oidc_provider_id,
            cluster_id,
            "kube-system",
            "ebs-csi-controller-sa",
        ),
    );
    resource.properties.insert(
        "ManagedPolicyArns".to_string(),
        CfExpression::list([CfExpression::from(
            "arn:aws:iam::aws:policy/service-role/AmazonEBSCSIDriverPolicy",
        )]),
    );
    resource.properties.insert("Tags".to_string(), tags(ctx));
    resource.depends_on.push(oidc_provider_id.to_string());
    resource
}

fn irsa_policy_document(
    oidc_provider_id: &str,
    cluster_id: &str,
    namespace: &str,
    service_account_name: &str,
) -> CfExpression {
    CfExpression::object([(
        "Fn::Sub",
        CfExpression::list([
            CfExpression::from(format!(
                r#"{{
  "Version": "2012-10-17",
  "Statement": [{{
    "Effect": "Allow",
    "Principal": {{"Federated": "${{OidcProviderArn}}"}},
    "Action": "sts:AssumeRoleWithWebIdentity",
    "Condition": {{
      "StringEquals": {{
        "${{OidcIssuerHostPath}}:aud": "sts.amazonaws.com",
        "${{OidcIssuerHostPath}}:sub": "system:serviceaccount:{namespace}:{service_account_name}"
      }}
    }}
  }}]
}}"#
            )),
            CfExpression::object([
                ("OidcProviderArn", CfExpression::ref_(oidc_provider_id)),
                ("OidcIssuerHostPath", oidc_issuer_host_path(cluster_id)),
            ]),
        ]),
    )])
}

fn oidc_issuer_host_path(cluster_id: &str) -> CfExpression {
    CfExpression::object([(
        "Fn::Select",
        CfExpression::list([
            CfExpression::from(1u8),
            CfExpression::object([(
                "Fn::Split",
                CfExpression::list([
                    CfExpression::from("https://"),
                    CfExpression::get_att(cluster_id, "OpenIdConnectIssuerUrl"),
                ]),
            )]),
        ]),
    )])
}

fn cidr_block(vpc_id: &str, index: usize) -> CfExpression {
    CfExpression::object([(
        "Fn::Select",
        CfExpression::list([
            CfExpression::Integer(index as i64),
            CfExpression::object([(
                "Fn::Cidr",
                CfExpression::list([
                    CfExpression::get_att(vpc_id, "CidrBlock"),
                    CfExpression::from(16u8),
                    CfExpression::from(8u8),
                ]),
            )]),
        ]),
    )])
}

fn availability_zone(index: usize) -> CfExpression {
    CfExpression::object([(
        "Fn::Select",
        CfExpression::list([
            CfExpression::Integer(index as i64),
            CfExpression::object([("Fn::GetAZs", CfExpression::ref_("AWS::Region"))]),
        ]),
    )])
}

fn resource_id(prefix: &str, suffix: &str) -> String {
    format!("{prefix}{suffix}")
}

fn cluster_id(prefix: &str) -> String {
    resource_id(prefix, "Cluster")
}

fn eks_private_subnet_ids(ctx: &EmitContext<'_>, prefix: &str) -> CfExpression {
    if default_network(ctx).is_some() {
        return private_subnet_ids_expr(ctx);
    }

    CfExpression::if_(
        CONDITION_NETWORK_MODE_CREATE,
        CfExpression::list([
            CfExpression::ref_(private_subnet_id(prefix, 1)),
            CfExpression::ref_(private_subnet_id(prefix, 2)),
        ]),
        CfExpression::if_(
            CONDITION_NETWORK_MODE_USE_EXISTING,
            CfExpression::ref_("PrivateSubnetIds"),
            CfExpression::list([]),
        ),
    )
}

fn public_subnet_id(prefix: &str, index: usize) -> String {
    resource_id(prefix, &format!("PublicSubnet{index}"))
}

fn private_subnet_id(prefix: &str, index: usize) -> String {
    resource_id(prefix, &format!("PrivateSubnet{index}"))
}
