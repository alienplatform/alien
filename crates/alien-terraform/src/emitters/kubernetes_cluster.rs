use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    expr,
};
use alien_core::{
    import::EmitContext, Container, ErrorData, ExposeProtocol, Ingress, KubernetesCluster,
    KubernetesClusterOwnership, KubernetesClusterProvider, Network, ResourceLifecycle, Result,
    Stack, Worker,
};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKubernetesClusterEmitter;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpKubernetesClusterEmitter;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureKubernetesClusterEmitter;

const AKS_DEFAULT_VM_SIZE: &str = "Standard_D2s_v3";
const AKS_SINGLE_NODE_VM_SIZE: &str = "Standard_D4s_v3";
const AKS_D2S_V3_ALLOCATABLE_MCPU: u32 = 1_900;
const AKS_D4S_V3_ALLOCATABLE_MCPU: u32 = 3_900;
const AKS_BASE_SYSTEM_MCPU: u32 = 1_800;
const AKS_SINGLE_NODE_SYSTEM_MCPU: u32 = 1_500;
const AKS_MANAGER_AGENT_MCPU: u32 = 100;
const KUBERNETES_WORKER_MCPU: u32 = 100;

struct AksNodePoolDefaults {
    node_count: u32,
    vm_size: &'static str,
}

impl TfEmitter for AwsKubernetesClusterEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default()
            .with_local(
                format!("{label}_cluster_name"),
                expr::raw(format!(
                    "var.kubernetes_cluster_mode == \"create\" ? aws_eks_cluster.{label}[0].name : var.eks_cluster_name"
                )),
            )
            .with_local(
                "kubernetes_kube_context".to_string(),
                expr::raw(format!("local.{label}_cluster_name")),
            )
            .with_local(
                "kubernetes_kubeconfig".to_string(),
                expr::raw(format!(
                    r#"var.kubernetes_cluster_mode == "create" ? yamlencode({{
  apiVersion = "v1"
  kind       = "Config"
  clusters = [{{
    name = aws_eks_cluster.{label}[0].name
    cluster = {{
      server                       = aws_eks_cluster.{label}[0].endpoint
      "certificate-authority-data" = aws_eks_cluster.{label}[0].certificate_authority[0].data
    }}
  }}]
  contexts = [{{
    name = aws_eks_cluster.{label}[0].name
    context = {{
      cluster = aws_eks_cluster.{label}[0].name
      user    = aws_eks_cluster.{label}[0].name
    }}
  }}]
  "current-context" = aws_eks_cluster.{label}[0].name
  users = [{{
    name = aws_eks_cluster.{label}[0].name
    user = {{
      exec = {{
        apiVersion = "client.authentication.k8s.io/v1beta1"
        command    = "aws"
        args       = ["eks", "get-token", "--cluster-name", aws_eks_cluster.{label}[0].name, "--region", var.aws_region]
      }}
    }}
  }}]
}}) : """#
                )),
            )
            .with_local(
                "kubernetes_exposure".to_string(),
                expr::raw(format!(
                    r#"{{
  mode = "generated"
  route = {{
    routeApi         = "ingress"
    controller       = "eks.amazonaws.com/alb"
    ingressClassName = "alb"
    labels           = {{}}
    annotations      = {{}}
    provider = {{
      provider   = "awsAlb"
      scheme     = "internet-facing"
      targetType = "ip"
      subnetIds  = {public_subnet_ids}
    }}
  }}
  certificate = {{
    mode = "none"
  }}
}}"#,
                    public_subnet_ids = public_subnet_ids_expr(label),
                ),
            )
            )
            .with_data(data_block(
                "aws_availability_zones",
                &format!("{label}_available"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? 1 : 0"),
                    ),
                    attr("state", Expression::String("available".to_string())),
                ],
            ))
            .with_data(data_block(
                "aws_eks_cluster",
                &format!("{label}_existing"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"existing\" ? 1 : 0"),
                    ),
                    attr("name", expr::raw("var.eks_cluster_name")),
                ],
            ));

        add_eks_workload_identity_data(&mut fragment, label);
        fragment.resource_blocks.extend([
            resource_block(
                "aws_vpc",
                label,
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? 1 : 0"),
                    ),
                    attr(
                        "cidr_block",
                        expr::raw("var.vpc_cidr == \"\" ? \"10.251.0.0/16\" : var.vpc_cidr"),
                    ),
                    attr("enable_dns_hostnames", Expression::Bool(true)),
                    attr("enable_dns_support", Expression::Bool(true)),
                    attr("tags", name_tags(format!("${{local.resource_prefix}}-{label}"))),
                ],
            ),
            resource_block(
                "aws_internet_gateway",
                label,
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? 1 : 0"),
                    ),
                    attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
                    attr("tags", name_tags(format!("${{local.resource_prefix}}-{label}"))),
                ],
            ),
            resource_block(
                "aws_subnet",
                &format!("{label}_public"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? var.availability_zones : 0"),
                    ),
                    attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
                    attr(
                        "cidr_block",
                        expr::raw(format!(
                            "cidrsubnet(aws_vpc.{label}[0].cidr_block, 8, count.index)"
                        )),
                    ),
                    attr(
                        "availability_zone",
                        expr::raw(format!(
                            "data.aws_availability_zones.{label}_available[0].names[count.index]"
                        )),
                    ),
                    attr("map_public_ip_on_launch", Expression::Bool(true)),
                    attr("tags", eks_subnet_tags(label, "public", "elb")),
                ],
            ),
            resource_block(
                "aws_subnet",
                &format!("{label}_private"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? var.availability_zones : 0"),
                    ),
                    attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
                    attr(
                        "cidr_block",
                        expr::raw(format!(
                            "cidrsubnet(aws_vpc.{label}[0].cidr_block, 8, count.index + 10)"
                        )),
                    ),
                    attr(
                        "availability_zone",
                        expr::raw(format!(
                            "data.aws_availability_zones.{label}_available[0].names[count.index]"
                        )),
                    ),
                    attr("tags", eks_subnet_tags(label, "private", "internal-elb")),
                ],
            ),
            resource_block(
                "aws_eip",
                &format!("{label}_nat"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? 1 : 0"),
                    ),
                    attr("domain", Expression::String("vpc".to_string())),
                    attr("tags", name_tags(format!("${{local.resource_prefix}}-{label}-nat"))),
                ],
            ),
            resource_block(
                "aws_nat_gateway",
                label,
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? 1 : 0"),
                    ),
                    attr(
                        "allocation_id",
                        expr::raw(format!("aws_eip.{label}_nat[0].id")),
                    ),
                    attr(
                        "subnet_id",
                        expr::raw(format!("aws_subnet.{label}_public[0].id")),
                    ),
                    attr("tags", name_tags(format!("${{local.resource_prefix}}-{label}"))),
                ],
            ),
            resource_block(
                "aws_route_table",
                &format!("{label}_public"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? 1 : 0"),
                    ),
                    attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
                    nested(block(
                        "route",
                        [
                            attr("cidr_block", Expression::String("0.0.0.0/0".to_string())),
                            attr(
                                "gateway_id",
                                expr::raw(format!("aws_internet_gateway.{label}[0].id")),
                            ),
                        ],
                    )),
                    attr("tags", name_tags(format!("${{local.resource_prefix}}-{label}-public"))),
                ],
            ),
            resource_block(
                "aws_route_table",
                &format!("{label}_private"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? 1 : 0"),
                    ),
                    attr("vpc_id", expr::raw(format!("aws_vpc.{label}[0].id"))),
                    nested(block(
                        "route",
                        [
                            attr("cidr_block", Expression::String("0.0.0.0/0".to_string())),
                            attr(
                                "nat_gateway_id",
                                expr::raw(format!("aws_nat_gateway.{label}[0].id")),
                            ),
                        ],
                    )),
                    attr("tags", name_tags(format!("${{local.resource_prefix}}-{label}-private"))),
                ],
            ),
            resource_block(
                "aws_route_table_association",
                &format!("{label}_public"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? var.availability_zones : 0"),
                    ),
                    attr(
                        "subnet_id",
                        expr::raw(format!("aws_subnet.{label}_public[count.index].id")),
                    ),
                    attr(
                        "route_table_id",
                        expr::raw(format!("aws_route_table.{label}_public[0].id")),
                    ),
                ],
            ),
            resource_block(
                "aws_route_table_association",
                &format!("{label}_private"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" && var.network_mode == \"create-new\" ? var.availability_zones : 0"),
                    ),
                    attr(
                        "subnet_id",
                        expr::raw(format!("aws_subnet.{label}_private[count.index].id")),
                    ),
                    attr(
                        "route_table_id",
                        expr::raw(format!("aws_route_table.{label}_private[0].id")),
                    ),
                ],
            ),
            resource_block(
                "aws_iam_role",
                &format!("{label}_cluster"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("name", expr::template(format!("${{local.resource_prefix}}-{label}-eks"))),
                    attr(
                        "assume_role_policy",
                        expr::jsonencode(expr::object([
                            ("Version", Expression::String("2012-10-17".to_string())),
                            (
                                "Statement",
                                Expression::Array(vec![expr::object([
                                    ("Effect", Expression::String("Allow".to_string())),
                                    (
                                        "Principal",
                                        expr::object([(
                                            "Service",
                                            Expression::String("eks.amazonaws.com".to_string()),
                                        )]),
                                    ),
                                    (
                                        "Action",
                                        Expression::Array(vec![
                                            Expression::String("sts:AssumeRole".to_string()),
                                            Expression::String("sts:TagSession".to_string()),
                                        ]),
                                    ),
                                ])]),
                            ),
                        ])),
                    ),
                ],
            ),
            resource_block(
                "aws_iam_role_policy_attachment",
                &format!("{label}_cluster"),
                [
                    attr(
                        "for_each",
                        expr::raw(
                            "var.kubernetes_cluster_mode == \"create\" ? toset([\"arn:aws:iam::aws:policy/AmazonEKSClusterPolicy\", \"arn:aws:iam::aws:policy/AmazonEKSBlockStoragePolicy\", \"arn:aws:iam::aws:policy/AmazonEKSComputePolicy\", \"arn:aws:iam::aws:policy/AmazonEKSLoadBalancingPolicy\", \"arn:aws:iam::aws:policy/AmazonEKSNetworkingPolicy\"]) : toset([])",
                        ),
                    ),
                    attr("role", expr::raw(format!("aws_iam_role.{label}_cluster[0].name"))),
                    attr("policy_arn", expr::raw("each.value")),
                ],
            ),
            resource_block(
                "aws_iam_role",
                &format!("{label}_node"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("name", expr::template(format!("${{local.resource_prefix}}-{label}-node"))),
                    attr(
                        "assume_role_policy",
                        expr::jsonencode(expr::object([
                            ("Version", Expression::String("2012-10-17".to_string())),
                            (
                                "Statement",
                                Expression::Array(vec![expr::object([
                                    ("Effect", Expression::String("Allow".to_string())),
                                    (
                                        "Principal",
                                        expr::object([(
                                            "Service",
                                            Expression::String("ec2.amazonaws.com".to_string()),
                                        )]),
                                    ),
                                    ("Action", Expression::String("sts:AssumeRole".to_string())),
                                ])]),
                            ),
                        ])),
                    ),
                ],
            ),
            resource_block(
                "aws_iam_role_policy_attachment",
                &format!("{label}_node"),
                [
                    attr(
                        "for_each",
                        expr::raw(
                            "var.kubernetes_cluster_mode == \"create\" ? toset([\"arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy\", \"arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly\", \"arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy\", \"arn:aws:iam::aws:policy/AmazonEKSWorkerNodeMinimalPolicy\"]) : toset([])",
                        ),
                    ),
                    attr("role", expr::raw(format!("aws_iam_role.{label}_node[0].name"))),
                    attr("policy_arn", expr::raw("each.value")),
                ],
            ),
            resource_block(
                "aws_iam_role",
                &format!("{label}_managed_node"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr(
                        "name",
                        expr::template(format!("${{local.resource_prefix}}-{label}-mng-node")),
                    ),
                    attr(
                        "assume_role_policy",
                        expr::jsonencode(expr::object([
                            ("Version", Expression::String("2012-10-17".to_string())),
                            (
                                "Statement",
                                Expression::Array(vec![expr::object([
                                    ("Effect", Expression::String("Allow".to_string())),
                                    (
                                        "Principal",
                                        expr::object([(
                                            "Service",
                                            Expression::String("ec2.amazonaws.com".to_string()),
                                        )]),
                                    ),
                                    ("Action", Expression::String("sts:AssumeRole".to_string())),
                                ])]),
                            ),
                        ])),
                    ),
                ],
            ),
            resource_block(
                "aws_iam_role_policy_attachment",
                &format!("{label}_managed_node"),
                [
                    attr(
                        "for_each",
                        expr::raw(
                            "var.kubernetes_cluster_mode == \"create\" ? toset([\"arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy\", \"arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly\", \"arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy\"]) : toset([])",
                        ),
                    ),
                    attr(
                        "role",
                        expr::raw(format!("aws_iam_role.{label}_managed_node[0].name")),
                    ),
                    attr("policy_arn", expr::raw("each.value")),
                ],
            ),
            resource_block(
                "aws_eks_cluster",
                label,
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("name", expr::template("${local.resource_prefix}-k8s")),
                    attr("role_arn", expr::raw(format!("aws_iam_role.{label}_cluster[0].arn"))),
                    attr("bootstrap_self_managed_addons", Expression::Bool(false)),
                    nested(block(
                        "vpc_config",
                        [
                            attr(
                                "subnet_ids",
                                expr::raw(private_subnet_ids_expr(label)),
                            ),
                            attr("endpoint_public_access", Expression::Bool(true)),
                            attr("endpoint_private_access", Expression::Bool(true)),
                        ],
                    )),
                    nested(block(
                        "access_config",
                        [
                            attr("authentication_mode", Expression::String("API_AND_CONFIG_MAP".to_string())),
                            attr("bootstrap_cluster_creator_admin_permissions", Expression::Bool(true)),
                        ],
                    )),
                    nested(block(
                        "compute_config",
                        [
                            attr("enabled", Expression::Bool(true)),
                            attr(
                                "node_pools",
                                Expression::Array(vec![Expression::String(
                                    "system".to_string(),
                                )]),
                            ),
                            attr("node_role_arn", expr::raw(format!("aws_iam_role.{label}_node[0].arn"))),
                        ],
                    )),
                    nested(block(
                        "kubernetes_network_config",
                        [nested(block(
                            "elastic_load_balancing",
                            [attr("enabled", Expression::Bool(true))],
                        ))],
                    )),
                    nested(block(
                        "storage_config",
                        [nested(block("block_storage", [attr("enabled", Expression::Bool(true))]))],
                    )),
                    attr(
                        "depends_on",
                        expr::raw(format!(
                            "[aws_iam_role_policy_attachment.{label}_cluster, aws_iam_role_policy_attachment.{label}_node]"
                        )),
                    ),
                ],
            ),
            resource_block(
                "aws_eks_addon",
                &format!("{label}_vpc_cni"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("cluster_name", expr::raw(format!("aws_eks_cluster.{label}[0].name"))),
                    attr("addon_name", Expression::String("vpc-cni".to_string())),
                ],
            ),
            resource_block(
                "aws_eks_node_group",
                label,
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("cluster_name", expr::raw(format!("aws_eks_cluster.{label}[0].name"))),
                    attr("node_group_name", expr::template(format!("${{local.resource_prefix}}-{label}"))),
                    attr("node_role_arn", expr::raw(format!("aws_iam_role.{label}_managed_node[0].arn"))),
                    attr("subnet_ids", expr::raw(private_subnet_ids_expr(label))),
                    attr("ami_type", Expression::String("AL2023_ARM_64_STANDARD".to_string())),
                    attr("capacity_type", Expression::String("ON_DEMAND".to_string())),
                    attr("disk_size", Expression::Number(hcl::Number::from(20))),
                    attr(
                        "instance_types",
                        Expression::Array(vec![Expression::String("t4g.medium".to_string())]),
                    ),
                    nested(block(
                        "scaling_config",
                        [
                            attr("desired_size", Expression::Number(hcl::Number::from(2))),
                            attr("max_size", Expression::Number(hcl::Number::from(3))),
                            attr("min_size", Expression::Number(hcl::Number::from(2))),
                        ],
                    )),
                    nested(block(
                        "update_config",
                        [attr("max_unavailable", Expression::Number(hcl::Number::from(1)))],
                    )),
                    attr(
                        "depends_on",
                        expr::raw(format!(
                            "[aws_eks_addon.{label}_vpc_cni, aws_iam_role_policy_attachment.{label}_managed_node]"
                        )),
                    ),
                ],
            ),
            resource_block(
                "aws_iam_role",
                &format!("{label}_ebs_csi"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("name", expr::template(format!("${{local.resource_prefix}}-{label}-ebs-csi"))),
                    attr(
                        "assume_role_policy",
                        expr::raw(r#"jsonencode({
  Version = "2012-10-17"
  Statement = [{
    Effect = "Allow"
    Principal = {
      Federated = local.eks_oidc_provider_arn
    }
    Action = "sts:AssumeRoleWithWebIdentity"
    Condition = {
      StringEquals = {
        "${local.eks_oidc_issuer_host_path}:aud" = "sts.amazonaws.com"
        "${local.eks_oidc_issuer_host_path}:sub" = "system:serviceaccount:kube-system:ebs-csi-controller-sa"
      }
    }
  }]
})"#),
                    ),
                ],
            ),
            resource_block(
                "aws_iam_role_policy_attachment",
                &format!("{label}_ebs_csi"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("role", expr::raw(format!("aws_iam_role.{label}_ebs_csi[0].name"))),
                    attr(
                        "policy_arn",
                        Expression::String(
                            "arn:aws:iam::aws:policy/service-role/AmazonEBSCSIDriverPolicy"
                                .to_string(),
                        ),
                    ),
                ],
            ),
            resource_block(
                "aws_eks_addon",
                &format!("{label}_ebs_csi"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("cluster_name", expr::raw(format!("aws_eks_cluster.{label}[0].name"))),
                    attr("addon_name", Expression::String("aws-ebs-csi-driver".to_string())),
                    attr(
                        "service_account_role_arn",
                        expr::raw(format!("aws_iam_role.{label}_ebs_csi[0].arn")),
                    ),
                    attr(
                        "depends_on",
                        expr::raw(format!(
                            "[aws_eks_node_group.{label}, aws_iam_role_policy_attachment.{label}_ebs_csi]"
                        )),
                    ),
                ],
            ),
            resource_block(
                "aws_eks_addon",
                &format!("{label}_kube_proxy"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("cluster_name", expr::raw(format!("aws_eks_cluster.{label}[0].name"))),
                    attr("addon_name", Expression::String("kube-proxy".to_string())),
                    attr(
                        "depends_on",
                        expr::raw(format!("[aws_eks_node_group.{label}]")),
                    ),
                ],
            ),
            resource_block(
                "aws_eks_addon",
                &format!("{label}_coredns"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("cluster_name", expr::raw(format!("aws_eks_cluster.{label}[0].name"))),
                    attr("addon_name", Expression::String("coredns".to_string())),
                    attr(
                        "depends_on",
                        expr::raw(format!("[aws_eks_node_group.{label}]")),
                    ),
                ],
            ),
        ]);
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        kubernetes_import_ref(
            ctx,
            KubernetesClusterProvider::Eks,
            "local.",
            "cluster_name",
        )
    }
}

fn add_eks_workload_identity_data(fragment: &mut TfFragment, label: &str) {
    fragment.data_blocks.push(data_block(
        "aws_eks_cluster",
        "target",
        [
            attr("name", expr::raw(format!("local.{label}_cluster_name"))),
            attr(
                "depends_on",
                expr::raw(format!("[aws_eks_cluster.{label}]")),
            ),
        ],
    ));
    fragment.data_blocks.push(data_block(
        "aws_eks_cluster_auth",
        "target",
        [attr(
            "name",
            expr::raw(format!("local.{label}_cluster_name")),
        )],
    ));
    fragment.data_blocks.push(data_block(
        "tls_certificate",
        "eks_oidc",
        [attr(
            "url",
            expr::raw("data.aws_eks_cluster.target.identity[0].oidc[0].issuer"),
        )],
    ));
    fragment.data_blocks.push(data_block(
        "aws_iam_openid_connect_provider",
        "eks_existing",
        [
            attr(
                "count",
                expr::raw("var.kubernetes_cluster_mode == \"existing\" ? 1 : 0"),
            ),
            attr(
                "url",
                expr::raw("data.aws_eks_cluster.target.identity[0].oidc[0].issuer"),
            ),
        ],
    ));
    fragment.locals.insert(
        "eks_oidc_issuer_host_path".to_string(),
        expr::raw(
            "trimprefix(data.aws_eks_cluster.target.identity[0].oidc[0].issuer, \"https://\")",
        ),
    );
    fragment.locals.insert(
        "eks_oidc_provider_arn".to_string(),
        expr::raw(
            "var.kubernetes_cluster_mode == \"create\" ? aws_iam_openid_connect_provider.eks[0].arn : data.aws_iam_openid_connect_provider.eks_existing[0].arn",
        ),
    );
    fragment.resource_blocks.push(resource_block(
        "aws_iam_openid_connect_provider",
        "eks",
        [
            attr(
                "count",
                expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
            ),
            attr(
                "url",
                expr::raw("data.aws_eks_cluster.target.identity[0].oidc[0].issuer"),
            ),
            attr(
                "client_id_list",
                Expression::Array(vec![Expression::String("sts.amazonaws.com".to_string())]),
            ),
            attr(
                "thumbprint_list",
                Expression::Array(vec![expr::raw(
                    "data.tls_certificate.eks_oidc.certificates[0].sha1_fingerprint",
                )]),
            ),
            attr(
                "tags",
                expr::object([
                    ("Name", expr::template("${local.resource_prefix}-eks-oidc")),
                    ("alien-resource-prefix", expr::raw("local.resource_prefix")),
                ]),
            ),
        ],
    ));
}

fn generated_kubernetes_exposure_count_expr(cluster_mode_condition: &str) -> Expression {
    expr::raw(format!(
        "({cluster_mode_condition}) && try(jsondecode(var.stack_settings_json).kubernetes.exposure.mode, \"generated\") == \"generated\" ? 1 : 0"
    ))
}

fn stack_has_public_https_endpoint(stack: &Stack) -> bool {
    stack.resources().any(|(_, entry)| {
        entry
            .config
            .downcast_ref::<Worker>()
            .map(|worker| worker.ingress == Ingress::Public)
            .unwrap_or(false)
            || entry
                .config
                .downcast_ref::<Container>()
                .map(|container| {
                    container
                        .ports
                        .iter()
                        .any(|port| matches!(port.expose, Some(ExposeProtocol::Http)))
                })
                .unwrap_or(false)
    })
}

fn aks_default_node_pool(stack: &Stack) -> AksNodePoolDefaults {
    let workload_mcpu = stack
        .resources()
        .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Live)
        .map(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Container>()
                .and_then(|container| cpu_to_millicores(&container.cpu.min))
                .or_else(|| {
                    entry
                        .config
                        .downcast_ref::<Worker>()
                        .map(|_| KUBERNETES_WORKER_MCPU)
                })
                .unwrap_or(0)
        })
        .sum::<u32>();

    let required_mcpu = workload_mcpu + AKS_BASE_SYSTEM_MCPU + AKS_MANAGER_AGENT_MCPU;
    if required_mcpu <= AKS_D2S_V3_ALLOCATABLE_MCPU * 2 {
        return AksNodePoolDefaults {
            node_count: 2,
            vm_size: AKS_DEFAULT_VM_SIZE,
        };
    }

    let single_node_required_mcpu =
        workload_mcpu + AKS_SINGLE_NODE_SYSTEM_MCPU + AKS_MANAGER_AGENT_MCPU;
    if single_node_required_mcpu <= AKS_D4S_V3_ALLOCATABLE_MCPU {
        return AksNodePoolDefaults {
            node_count: 1,
            vm_size: AKS_SINGLE_NODE_VM_SIZE,
        };
    }

    let node_count =
        (required_mcpu + AKS_D2S_V3_ALLOCATABLE_MCPU - 1) / AKS_D2S_V3_ALLOCATABLE_MCPU;
    AksNodePoolDefaults {
        node_count: node_count.max(2),
        vm_size: AKS_DEFAULT_VM_SIZE,
    }
}

fn cpu_to_millicores(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(millicores) = value.strip_suffix('m') {
        return millicores.trim().parse::<u32>().ok();
    }

    let (whole, fractional) = value.split_once('.').unwrap_or((value, ""));
    let whole_mcpu = whole.trim().parse::<u32>().ok()?.checked_mul(1_000)?;
    if fractional.is_empty() {
        return Some(whole_mcpu);
    }

    let mut fractional_digits = fractional
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if fractional_digits.is_empty() {
        return None;
    }
    let round_up = fractional_digits.len() > 3
        && fractional_digits
            .as_bytes()
            .get(3..)
            .is_some_and(|extra| extra.iter().any(|digit| *digit != b'0'));
    fractional_digits.truncate(3);
    while fractional_digits.len() < 3 {
        fractional_digits.push('0');
    }
    let fractional_mcpu = fractional_digits.parse::<u32>().ok()? + u32::from(round_up);
    whole_mcpu.checked_add(fractional_mcpu)
}

impl TfEmitter for GcpKubernetesClusterEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let label = required_label(ctx)?;
        let mut cluster_body = vec![
            attr(
                "count",
                expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
            ),
            attr("name", expr::template("${local.resource_prefix}-k8s")),
            attr("location", expr::raw("var.gcp_region")),
            attr("deletion_protection", Expression::Bool(false)),
            attr("enable_autopilot", Expression::Bool(true)),
        ];
        if let Some(network_label) = default_network_label(ctx) {
            cluster_body.push(attr(
                "network",
                expr::raw(gcp_network_self_link_expr(network_label)),
            ));
            cluster_body.push(attr(
                "subnetwork",
                expr::raw(gcp_subnetwork_self_link_expr(network_label)),
            ));
        }
        cluster_body.extend([
            nested(block("ip_allocation_policy", [])),
            nested(block(
                "workload_identity_config",
                [attr(
                    "workload_pool",
                    expr::template("${var.gcp_project}.svc.id.goog"),
                )],
            )),
            nested(block(
                "gateway_api_config",
                [attr(
                    "channel",
                    Expression::String("CHANNEL_STANDARD".to_string()),
                )],
            )),
            nested(block(
                "master_auth",
                [nested(block(
                    "client_certificate_config",
                    [attr("issue_client_certificate", Expression::Bool(true))],
                ))],
            )),
        ]);
        let fragment = TfFragment::default()
            .with_local(
                format!("{label}_cluster_name"),
                expr::raw(format!(
                    "var.kubernetes_cluster_mode == \"create\" ? google_container_cluster.{label}[0].name : var.gke_cluster_name"
                )),
            )
            .with_local(
                "kubernetes_kube_context".to_string(),
                expr::raw(format!("local.{label}_cluster_name")),
            )
            .with_local(
                "kubernetes_kubeconfig".to_string(),
                expr::raw(format!(
                    r#"var.kubernetes_cluster_mode == "create" ? yamlencode({{
  apiVersion = "v1"
  kind       = "Config"
  clusters = [{{
    name = google_container_cluster.{label}[0].name
    cluster = {{
      server                       = "https://${{google_container_cluster.{label}[0].endpoint}}"
      "certificate-authority-data" = google_container_cluster.{label}[0].master_auth[0].cluster_ca_certificate
    }}
  }}]
  contexts = [{{
    name = google_container_cluster.{label}[0].name
    context = {{
      cluster = google_container_cluster.{label}[0].name
      user    = google_container_cluster.{label}[0].name
    }}
  }}]
  "current-context" = google_container_cluster.{label}[0].name
  users = [{{
    name = google_container_cluster.{label}[0].name
    user = {{
      exec = {{
        apiVersion         = "client.authentication.k8s.io/v1beta1"
        command            = "gke-gcloud-auth-plugin"
        provideClusterInfo = true
      }}
    }}
  }}]
}}) : """#
                )),
            )
            .with_local(
                "kubernetes_exposure".to_string(),
                expr::raw(
                    r#"{
  mode = "generated"
  route = {
    routeApi         = "gateway"
    controller       = "networking.gke.io/gateway"
    gatewayClassName = "gke-l7-global-external-managed"
    listenerPort     = 80
    labels           = {}
    annotations      = {}
    provider = {
      provider          = "gkeGateway"
      staticAddressName = null
    }
  }
  certificate = {
    mode = "none"
  }
}"#,
                ),
            )
            .with_data(data_block(
                "google_client_config",
                "current",
                [],
            ))
            .with_data(data_block(
                "google_container_cluster",
                "target",
                [
                    attr("name", expr::raw(format!("local.{label}_cluster_name"))),
                    attr(
                        "location",
                        expr::raw(
                            "var.kubernetes_cluster_mode == \"create\" ? var.gcp_region : (var.gke_cluster_location == \"\" ? var.gcp_region : var.gke_cluster_location)",
                        ),
                    ),
                    attr(
                        "depends_on",
                        expr::raw(format!("[google_container_cluster.{label}]")),
                    ),
                ],
            ))
            .with_data(data_block(
                "google_container_cluster",
                &format!("{label}_existing"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"existing\" ? 1 : 0"),
                    ),
                    attr("name", expr::raw("var.gke_cluster_name")),
                    attr(
                        "location",
                        expr::raw("var.gke_cluster_location == \"\" ? var.gcp_region : var.gke_cluster_location"),
                    ),
                ],
            ))
            .with_resource(resource_block(
                "google_container_cluster",
                label,
                cluster_body,
            ));
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        kubernetes_import_ref(
            ctx,
            KubernetesClusterProvider::Gke,
            "local.",
            "cluster_name",
        )
    }
}

impl TfEmitter for AzureKubernetesClusterEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let label = required_label(ctx)?;
        let node_pool = aks_default_node_pool(ctx.stack);
        let managed_alb_network_label = azure_managed_alb_network_label(ctx);
        let mut default_node_pool = vec![
            attr("name", Expression::String("default".to_string())),
            attr(
                "node_count",
                Expression::Number(hcl::Number::from(node_pool.node_count)),
            ),
            attr("vm_size", Expression::String(node_pool.vm_size.to_string())),
        ];
        if let Some(network_label) = default_network_label(ctx) {
            default_node_pool.push(attr(
                "vnet_subnet_id",
                expr::raw(azure_private_subnet_id_expr(network_label)),
            ));
        }
        let mut fragment = TfFragment::default()
            .with_local(
                format!("{label}_cluster_name"),
                expr::raw(format!(
                    "var.kubernetes_cluster_mode == \"create\" ? azurerm_kubernetes_cluster.{label}[0].name : var.aks_cluster_name"
                )),
            )
            .with_local(
                "kubernetes_kube_context".to_string(),
                expr::raw(format!(
                    r#"var.kubernetes_cluster_mode == "create" ? nonsensitive(try(yamldecode(azurerm_kubernetes_cluster.{label}[0].kube_config_raw)["current-context"], local.{label}_cluster_name)) : local.{label}_cluster_name"#
                )),
            )
            .with_local(
                "kubernetes_kubeconfig".to_string(),
                expr::raw(format!(
                    r#"var.kubernetes_cluster_mode == "create" ? azurerm_kubernetes_cluster.{label}[0].kube_config_raw : """#
                )),
            )
            .with_local(
                "kubernetes_exposure".to_string(),
                expr::raw(
                    r#"{
  mode = "generated"
  route = {
    routeApi         = "gateway"
    controller       = "alb.networking.azure.io/alb-controller"
    gatewayClassName = "azure-alb-external"
    listenerPort     = 80
    labels           = {}
    annotations      = {
      "alb.networking.azure.io/alb-namespace" = var.kubernetes_namespace
      "alb.networking.azure.io/alb-name"      = "${local.resource_prefix}-alb"
    }
    provider = {
      provider     = "azureApplicationGatewayForContainers"
      frontend     = "public"
      albNamespace = var.kubernetes_namespace
      albName      = "${local.resource_prefix}-alb"
    }
  }
  certificate = {
    mode = "none"
  }
}"#,
                ),
            )
            .with_data(data_block(
                "azurerm_client_config",
                &format!("{label}_current"),
                [],
            ))
            .with_data(data_block(
                "azurerm_kubernetes_cluster",
                "target",
                [
                    attr("name", expr::raw(format!("local.{label}_cluster_name"))),
                    attr(
                        "resource_group_name",
                        expr::raw(
                            "var.kubernetes_cluster_mode == \"create\" ? var.azure_resource_group_name : var.aks_cluster_resource_group_name",
                        ),
                    ),
                    attr(
                        "depends_on",
                        expr::raw(format!("[azurerm_kubernetes_cluster.{label}]")),
                    ),
                ],
            ))
            .with_data(data_block(
                "azurerm_kubernetes_cluster",
                &format!("{label}_existing"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"existing\" ? 1 : 0"),
                    ),
                    attr("name", expr::raw("var.aks_cluster_name")),
                    attr(
                        "resource_group_name",
                        expr::raw("var.aks_cluster_resource_group_name"),
                    ),
                ],
            ))
            .with_resource(resource_block(
                "azurerm_kubernetes_cluster",
                label,
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("name", expr::template("${local.resource_prefix}-k8s")),
                    attr("location", expr::raw("var.azure_location")),
                    attr("resource_group_name", expr::raw("var.azure_resource_group_name")),
                    attr("dns_prefix", expr::template("${local.resource_prefix}-k8s")),
                    attr("oidc_issuer_enabled", Expression::Bool(true)),
                    attr("workload_identity_enabled", Expression::Bool(true)),
                    nested(block(
                        "default_node_pool",
                        default_node_pool,
                    )),
                    nested(block("identity", [attr("type", Expression::String("SystemAssigned".to_string()))])),
                    nested(block(
                        "azure_active_directory_role_based_access_control",
                        [
                            attr("azure_rbac_enabled", Expression::Bool(true)),
                            attr("tenant_id", expr::raw("var.azure_tenant_id")),
                        ],
                    )),
                    nested(block(
                        "network_profile",
                        [
                            attr("network_plugin", Expression::String("azure".to_string())),
                            attr("load_balancer_sku", Expression::String("standard".to_string())),
                        ],
                    )),
                    attr("sku_tier", Expression::String("Standard".to_string())),
                ],
            ));
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_current_client_kubernetes_rbac_cluster_admin"),
            [
                attr(
                    "count",
                    expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                ),
                attr(
                    "name",
                    expr::raw(format!(
                        "uuidv5(\"dns\", \"${{local.resource_prefix}}-aks-rbac-cluster-admin-${{data.azurerm_client_config.{label}_current.object_id}}\")"
                    )),
                ),
                attr("scope", expr::raw(format!("azurerm_kubernetes_cluster.{label}[0].id"))),
                attr(
                    "role_definition_name",
                    Expression::String("Azure Kubernetes Service RBAC Cluster Admin".to_string()),
                ),
                attr(
                    "principal_id",
                    expr::raw(format!("data.azurerm_client_config.{label}_current.object_id")),
                ),
            ],
        ));
        let has_public_https_endpoint = stack_has_public_https_endpoint(ctx.stack);
        if has_public_https_endpoint {
            fragment.resource_blocks.push(resource_block(
                "azapi_resource_action",
                &format!("{label}_service_networking_provider_registration"),
                [
                    attr(
                        "count",
                        generated_kubernetes_exposure_count_expr(
                            "var.kubernetes_cluster_mode == \"create\" || var.kubernetes_cluster_mode == \"existing\"",
                        ),
                    ),
                    attr(
                        "type",
                        Expression::String("Microsoft.Resources/providers@2021-04-01".to_string()),
                    ),
                    attr(
                        "resource_id",
                        expr::raw(
                            "\"/subscriptions/${var.azure_subscription_id}/providers/Microsoft.ServiceNetworking\"",
                        ),
                    ),
                    attr("action", Expression::String("register".to_string())),
                    attr("method", Expression::String("POST".to_string())),
                ],
            ));
            fragment.resource_blocks.push(resource_block(
                "azapi_update_resource",
                &format!("{label}_alb_controller"),
                [
                    attr(
                        "count",
                        generated_kubernetes_exposure_count_expr(
                            "var.kubernetes_cluster_mode == \"create\" || var.kubernetes_cluster_mode == \"existing\"",
                        ),
                    ),
                    attr(
                        "type",
                        Expression::String(
                            "Microsoft.ContainerService/managedClusters@2025-09-02-preview"
                                .to_string(),
                        ),
                    ),
                    attr(
                        "resource_id",
                        expr::raw(format!(
                            "var.kubernetes_cluster_mode == \"create\" ? azurerm_kubernetes_cluster.{label}[0].id : data.azurerm_kubernetes_cluster.{label}_existing[0].id"
                        )),
                    ),
                    attr(
                        "body",
                        expr::raw(
                            r#"{
  properties = {
    oidcIssuerProfile = {
      enabled = true
    }
    securityProfile = {
      workloadIdentity = {
        enabled = true
      }
    }
    ingressProfile = {
      applicationLoadBalancer = {
        enabled = true
      }
      gatewayAPI = {
        installation = "Standard"
      }
    }
  }
}"#,
                        ),
                    ),
                    attr(
                        "depends_on",
                        expr::raw(format!(
                            "[azurerm_kubernetes_cluster.{label}, azapi_resource_action.{label}_service_networking_provider_registration]"
                        )),
                    ),
                ],
            ));
            if let Some(network_label) = managed_alb_network_label {
                fragment.data_blocks.push(data_block(
                    "azurerm_user_assigned_identity",
                    &format!("{label}_alb_controller"),
                    [
                        attr(
                            "count",
                            generated_kubernetes_exposure_count_expr(
                                "var.kubernetes_cluster_mode == \"create\"",
                            ),
                        ),
                        attr(
                            "name",
                            expr::raw(format!(
                                "\"applicationloadbalancer-${{local.{label}_cluster_name}}\""
                            )),
                        ),
                        attr(
                            "resource_group_name",
                            expr::raw("data.azurerm_kubernetes_cluster.target.node_resource_group"),
                        ),
                        attr(
                            "depends_on",
                            expr::raw(format!("[azapi_update_resource.{label}_alb_controller]")),
                        ),
                    ],
                ));
                fragment.resource_blocks.push(resource_block(
                    "azurerm_role_assignment",
                    &format!("{label}_alb_controller_association_subnet_network_contributor"),
                    [
                        attr(
                            "count",
                            generated_kubernetes_exposure_count_expr(
                                "var.kubernetes_cluster_mode == \"create\"",
                            ),
                        ),
                        attr(
                            "scope",
                            expr::raw(azure_alb_association_subnet_id_expr(network_label)),
                        ),
                        attr(
                            "role_definition_name",
                            Expression::String("Network Contributor".to_string()),
                        ),
                        attr(
                            "principal_id",
                            expr::raw(format!(
                                "data.azurerm_user_assigned_identity.{label}_alb_controller[0].principal_id"
                            )),
                        ),
                        attr("skip_service_principal_aad_check", Expression::Bool(true)),
                    ],
                ));
            }
        }
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        kubernetes_import_ref(
            ctx,
            KubernetesClusterProvider::Aks,
            "local.",
            "cluster_name",
        )
    }
}

fn required_label<'a>(ctx: &EmitContext<'a>) -> Result<&'a str> {
    ctx.name_for(ctx.resource_id).ok_or_else(|| {
        AlienError::new(ErrorData::GenericError {
            message: format!("missing Terraform label for resource '{}'", ctx.resource_id),
        })
    })
}

fn kubernetes_import_ref(
    ctx: &EmitContext<'_>,
    provider: KubernetesClusterProvider,
    local_prefix: &str,
    _cluster_name_field: &str,
) -> Result<Expression> {
    let cluster = ctx
        .resource
        .config
        .downcast_ref::<KubernetesCluster>()
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "Terraform emitter expected {} resource '{}'",
                    KubernetesCluster::RESOURCE_TYPE,
                    ctx.resource_id
                ),
            })
        })?;
    let label = required_label(ctx)?;
    let ownership = match cluster.ownership {
        KubernetesClusterOwnership::Managed => "managed",
        KubernetesClusterOwnership::Existing => "existing",
        KubernetesClusterOwnership::External => "external",
    };
    let mut fields = vec![
        (
            "provider",
            Expression::String(provider_string(provider).to_string()),
        ),
        (
            "ownership",
            expr::raw(format!(
                "var.kubernetes_cluster_mode == \"create\" ? \"managed\" : \"{ownership}\""
            )),
        ),
        ("namespace", expr::raw("var.kubernetes_namespace")),
        (
            "clusterName",
            expr::raw(format!("{local_prefix}{label}_cluster_name")),
        ),
        (
            "clusterId",
            expr::raw(format!("{local_prefix}{label}_cluster_name")),
        ),
        ("cloudMetadataReady", Expression::Bool(true)),
    ];

    if provider == KubernetesClusterProvider::Aks {
        let azure_agc = azure_managed_alb_network_label(ctx)
            .map(|network_label| {
                expr::raw(format!(
                    r#"var.kubernetes_cluster_mode == "create" ? {{
  albName = "${{local.resource_prefix}}-alb"
  albNamespace = var.kubernetes_namespace
  associationSubnetId = {association_subnet_id}
}} : null"#,
                    association_subnet_id = azure_alb_association_subnet_id_expr(network_label),
                ))
            })
            .unwrap_or(Expression::Null);
        fields.push(("azureApplicationGatewayForContainers", azure_agc));
    }

    Ok(expr::object(fields))
}

fn provider_string(provider: KubernetesClusterProvider) -> &'static str {
    match provider {
        KubernetesClusterProvider::Eks => "eks",
        KubernetesClusterProvider::Gke => "gke",
        KubernetesClusterProvider::Aks => "aks",
        KubernetesClusterProvider::Generic => "generic",
    }
}

fn name_tags(name: impl Into<String>) -> Expression {
    expr::object([("Name", Expression::String(name.into()))])
}

fn eks_subnet_tags(label: &str, kind: &str, role: &str) -> Expression {
    expr::raw(format!(
        r#"{{
  Name = "${{local.resource_prefix}}-{label}-{kind}"
  "kubernetes.io/cluster/${{local.resource_prefix}}-k8s" = "shared"
  "kubernetes.io/role/{role}" = "1"
}}"#
    ))
}

fn public_subnet_ids_expr(label: &str) -> String {
    format!(
        "var.kubernetes_cluster_mode == \"create\" ? (var.network_mode == \"create-new\" ? aws_subnet.{label}_public[*].id : var.network_mode == \"use-existing\" ? var.public_subnet_ids : []) : []"
    )
}

fn private_subnet_ids_expr(label: &str) -> String {
    format!(
        "var.kubernetes_cluster_mode == \"create\" ? (var.network_mode == \"create-new\" ? aws_subnet.{label}_private[*].id : var.network_mode == \"use-existing\" ? var.private_subnet_ids : []) : []"
    )
}

fn default_network_label<'a>(ctx: &EmitContext<'a>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(resource_id, entry)| {
        entry.config.downcast_ref::<Network>()?;
        ctx.name_for(resource_id)
    })
}

fn azure_managed_alb_network_label<'a>(ctx: &EmitContext<'a>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(resource_id, entry)| {
        let network = entry.config.downcast_ref::<Network>()?;
        matches!(
            network.settings,
            alien_core::NetworkSettings::UseDefault | alien_core::NetworkSettings::Create { .. }
        )
        .then(|| ctx.name_for(resource_id))
        .flatten()
    })
}

fn gcp_network_self_link_expr(label: &str) -> String {
    format!(
        "var.network_mode == \"create-new\" ? google_compute_network.{label}[0].self_link : var.network_mode == \"use-existing\" ? data.google_compute_network.{label}[0].self_link : null"
    )
}

fn gcp_subnetwork_self_link_expr(label: &str) -> String {
    format!(
        "var.network_mode == \"create-new\" ? google_compute_subnetwork.{label}_workload[0].self_link : var.network_mode == \"use-existing\" ? data.google_compute_subnetwork.{label}_existing_subnet[0].self_link : null"
    )
}

fn azure_private_subnet_id_expr(label: &str) -> String {
    format!("azurerm_subnet.{label}_private.id")
}

fn azure_alb_association_subnet_id_expr(label: &str) -> String {
    format!("azurerm_subnet.{label}_alb.id")
}
