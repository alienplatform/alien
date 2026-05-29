use crate::{
    block::{attr, block, data_block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    expr,
};
use alien_core::{
    import::EmitContext, Container, ErrorData, ExposeProtocol, Ingress, KubernetesCluster,
    KubernetesClusterOwnership, KubernetesClusterProvider, Result, Stack, Worker,
};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsKubernetesClusterEmitter;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpKubernetesClusterEmitter;

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureKubernetesClusterEmitter;

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
      subnetIds  = var.kubernetes_cluster_mode == "create" ? aws_subnet.{label}_public[*].id : []
    }}
  }}
  certificate = {{
    mode   = "managedAcmImport"
    region = var.aws_region
    tags = {{
      "alien.dev/resource-prefix" = local.resource_prefix
      "alien.dev/managed-by"      = "alien"
    }}
  }}
}}"#
                )),
            )
            .with_data(data_block(
                "aws_availability_zones",
                &format!("{label}_available"),
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("cidr_block", Expression::String("10.251.0.0/16".to_string())),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 2 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 2 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 2 : 0"),
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
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 2 : 0"),
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
                                expr::raw(format!(
                                    "concat(aws_subnet.{label}_public[*].id, aws_subnet.{label}_private[*].id)"
                                )),
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
                                Expression::Array(vec![
                                    Expression::String("general-purpose".to_string()),
                                    Expression::String("system".to_string()),
                                ]),
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
                    attr("subnet_ids", expr::raw(format!("aws_subnet.{label}_private[*].id"))),
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
        add_eks_gp3_storage_class(&mut fragment, label);
        if stack_has_public_https_endpoint(ctx.stack) {
            add_eks_auto_mode_ingress_class(&mut fragment, label);
        }
        add_metrics_server(
            &mut fragment,
            label,
            expr::raw(format!("[aws_eks_cluster.{label}]")),
        );

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

fn add_eks_gp3_storage_class(fragment: &mut TfFragment, label: &str) {
    fragment.resource_blocks.push(resource_block(
        "kubernetes_manifest",
        &format!("{label}_gp3_storage_class"),
        [
            attr(
                "count",
                expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
            ),
            attr(
                "manifest",
                expr::raw(
                    r#"{
  apiVersion = "storage.k8s.io/v1"
  kind       = "StorageClass"
  metadata = {
    name = "gp3"
    annotations = {
      "storageclass.kubernetes.io/is-default-class" = "true"
    }
  }
  provisioner          = "ebs.csi.aws.com"
  parameters           = { type = "gp3", fsType = "ext4" }
  reclaimPolicy        = "Delete"
  volumeBindingMode    = "WaitForFirstConsumer"
  allowVolumeExpansion = true
}"#,
                ),
            ),
            attr(
                "depends_on",
                expr::raw(format!("[aws_eks_addon.{label}_ebs_csi]")),
            ),
        ],
    ));
}

fn add_eks_auto_mode_ingress_class(fragment: &mut TfFragment, label: &str) {
    for (name, manifest) in [
        (
            "alb_ingress_class_params",
            r#"{
  apiVersion = "eks.amazonaws.com/v1"
  kind       = "IngressClassParams"
  metadata = {
    name = "alb"
  }
  spec = {
    scheme     = "internet-facing"
    targetType = "ip"
  }
}"#,
        ),
        (
            "alb_ingress_class",
            r#"{
  apiVersion = "networking.k8s.io/v1"
  kind       = "IngressClass"
  metadata = {
    name = "alb"
  }
  spec = {
    controller = "eks.amazonaws.com/alb"
    parameters = {
      apiGroup = "eks.amazonaws.com"
      kind     = "IngressClassParams"
      name     = "alb"
    }
  }
}"#,
        ),
    ] {
        fragment.resource_blocks.push(resource_block(
            "kubernetes_manifest",
            &format!("{label}_{name}"),
            [
                attr(
                    "count",
                    generated_kubernetes_exposure_count_expr(
                        "var.kubernetes_cluster_mode == \"create\"",
                    ),
                ),
                attr("manifest", expr::raw(manifest)),
                attr(
                    "depends_on",
                    expr::raw(format!("[aws_eks_cluster.{label}]")),
                ),
            ],
        ));
    }
}

fn generated_kubernetes_exposure_count_expr(cluster_mode_condition: &str) -> Expression {
    expr::raw(format!(
        "{cluster_mode_condition} && try(jsondecode(var.stack_settings_json).kubernetes.exposure.mode, \"generated\") == \"generated\" ? 1 : 0"
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

fn add_metrics_server(fragment: &mut TfFragment, label: &str, depends_on: Expression) {
    for (name, manifest) in [
        (
            "service_account",
            r#"{
  apiVersion = "v1"
  kind       = "ServiceAccount"
  metadata = {
    name      = "metrics-server"
    namespace = "kube-system"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
}"#,
        ),
        (
            "aggregated_metrics_reader",
            r#"{
  apiVersion = "rbac.authorization.k8s.io/v1"
  kind       = "ClusterRole"
  metadata = {
    name = "system:aggregated-metrics-reader"
    labels = {
      "rbac.authorization.k8s.io/aggregate-to-admin" = "true"
      "rbac.authorization.k8s.io/aggregate-to-edit"  = "true"
      "rbac.authorization.k8s.io/aggregate-to-view"  = "true"
    }
  }
  rules = [{
    apiGroups = ["metrics.k8s.io"]
    resources = ["pods", "nodes"]
    verbs     = ["get", "list", "watch"]
  }]
}"#,
        ),
        (
            "cluster_role",
            r#"{
  apiVersion = "rbac.authorization.k8s.io/v1"
  kind       = "ClusterRole"
  metadata = {
    name = "system:metrics-server"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
  rules = [
    {
      apiGroups = [""]
      resources = ["nodes/metrics"]
      verbs     = ["get"]
    },
    {
      apiGroups = [""]
      resources = ["pods", "nodes"]
      verbs     = ["get", "list", "watch"]
    }
  ]
}"#,
        ),
        (
            "auth_reader_role_binding",
            r#"{
  apiVersion = "rbac.authorization.k8s.io/v1"
  kind       = "RoleBinding"
  metadata = {
    name      = "metrics-server-auth-reader"
    namespace = "kube-system"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
  roleRef = {
    apiGroup = "rbac.authorization.k8s.io"
    kind     = "Role"
    name     = "extension-apiserver-authentication-reader"
  }
  subjects = [{
    kind      = "ServiceAccount"
    name      = "metrics-server"
    namespace = "kube-system"
  }]
}"#,
        ),
        (
            "auth_delegator_binding",
            r#"{
  apiVersion = "rbac.authorization.k8s.io/v1"
  kind       = "ClusterRoleBinding"
  metadata = {
    name = "metrics-server:system:auth-delegator"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
  roleRef = {
    apiGroup = "rbac.authorization.k8s.io"
    kind     = "ClusterRole"
    name     = "system:auth-delegator"
  }
  subjects = [{
    kind      = "ServiceAccount"
    name      = "metrics-server"
    namespace = "kube-system"
  }]
}"#,
        ),
        (
            "cluster_role_binding",
            r#"{
  apiVersion = "rbac.authorization.k8s.io/v1"
  kind       = "ClusterRoleBinding"
  metadata = {
    name = "system:metrics-server"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
  roleRef = {
    apiGroup = "rbac.authorization.k8s.io"
    kind     = "ClusterRole"
    name     = "system:metrics-server"
  }
  subjects = [{
    kind      = "ServiceAccount"
    name      = "metrics-server"
    namespace = "kube-system"
  }]
}"#,
        ),
        (
            "service",
            r#"{
  apiVersion = "v1"
  kind       = "Service"
  metadata = {
    name      = "metrics-server"
    namespace = "kube-system"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
  spec = {
    selector = {
      "k8s-app" = "metrics-server"
    }
    ports = [{
      name       = "https"
      port       = 443
      protocol   = "TCP"
      targetPort = "https"
    }]
  }
}"#,
        ),
        (
            "deployment",
            r#"{
  apiVersion = "apps/v1"
  kind       = "Deployment"
  metadata = {
    name      = "metrics-server"
    namespace = "kube-system"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
  spec = {
    selector = {
      matchLabels = {
        "k8s-app" = "metrics-server"
      }
    }
    strategy = {
      rollingUpdate = {
        maxUnavailable = 0
      }
    }
    template = {
      metadata = {
        labels = {
          "k8s-app" = "metrics-server"
        }
      }
      spec = {
        serviceAccountName = "metrics-server"
        priorityClassName  = "system-cluster-critical"
        containers = [{
          name  = "metrics-server"
          image = "registry.k8s.io/metrics-server/metrics-server:v0.8.1"
          args = [
            "--cert-dir=/tmp",
            "--secure-port=10250",
            "--kubelet-preferred-address-types=InternalIP,ExternalIP,Hostname",
            "--kubelet-use-node-status-port",
            "--metric-resolution=15s"
          ]
          ports = [{
            name          = "https"
            containerPort = 10250
            protocol      = "TCP"
          }]
          resources = {
            requests = {
              cpu    = "100m"
              memory = "200Mi"
            }
          }
          volumeMounts = [{
            name      = "tmp-dir"
            mountPath = "/tmp"
          }]
        }]
        volumes = [{
          name = "tmp-dir"
          emptyDir = {}
        }]
      }
    }
  }
}"#,
        ),
        (
            "api_service",
            r#"{
  apiVersion = "apiregistration.k8s.io/v1"
  kind       = "APIService"
  metadata = {
    name = "v1beta1.metrics.k8s.io"
    labels = {
      "k8s-app" = "metrics-server"
    }
  }
  spec = {
    group                    = "metrics.k8s.io"
    version                  = "v1beta1"
    groupPriorityMinimum     = 100
    versionPriority          = 100
    insecureSkipTLSVerify    = true
    service = {
      name      = "metrics-server"
      namespace = "kube-system"
    }
  }
}"#,
        ),
    ] {
        fragment.resource_blocks.push(resource_block(
            "kubernetes_manifest",
            &format!("{label}_metrics_server_{name}"),
            [
                attr(
                    "count",
                    expr::raw("var.kubernetes_cluster_mode == \"create\" && var.install_metrics_server ? 1 : 0"),
                ),
                attr("manifest", expr::raw(manifest)),
                attr("depends_on", depends_on.clone()),
            ],
        ));
    }
}

impl TfEmitter for GcpKubernetesClusterEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default()
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
      "client-certificate-data" = google_container_cluster.{label}[0].master_auth[0].client_certificate
      "client-key-data"         = google_container_cluster.{label}[0].master_auth[0].client_key
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
    listenerPort     = 443
    labels           = {}
    annotations      = {}
    provider = {
      provider          = "gkeGateway"
      staticAddressName = null
    }
  }
  certificate = {
    mode               = "managedTlsSecret"
    secretNameTemplate = "alien-{{ resourceId }}-tls"
  }
}"#,
                ),
            )
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
                [
                    attr(
                        "count",
                        expr::raw("var.kubernetes_cluster_mode == \"create\" ? 1 : 0"),
                    ),
                    attr("name", expr::template("${local.resource_prefix}-k8s")),
                    attr("location", expr::raw("var.gcp_region")),
                    attr("deletion_protection", Expression::Bool(false)),
                    attr("enable_autopilot", Expression::Bool(true)),
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
                        [attr("channel", Expression::String("CHANNEL_STANDARD".to_string()))],
                    )),
                    nested(block(
                        "master_auth",
                        [nested(block(
                            "client_certificate_config",
                            [attr("issue_client_certificate", Expression::Bool(true))],
                        ))],
                    )),
                ],
            ));
        add_metrics_server(
            &mut fragment,
            label,
            expr::raw(format!("[google_container_cluster.{label}]")),
        );
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
        let mut fragment = TfFragment::default()
            .with_local(
                format!("{label}_cluster_name"),
                expr::raw(format!(
                    "var.kubernetes_cluster_mode == \"create\" ? azurerm_kubernetes_cluster.{label}[0].name : var.aks_cluster_name"
                )),
            )
            .with_local(
                "kubernetes_kube_context".to_string(),
                expr::raw(format!("local.{label}_cluster_name")),
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
    listenerPort     = 443
    labels           = {}
    annotations      = {}
    provider = {
      provider = "azureApplicationGatewayForContainers"
      frontend = "public"
    }
  }
  certificate = {
    mode               = "managedTlsSecret"
    secretNameTemplate = "alien-{{ resourceId }}-tls"
  }
}"#,
                ),
            )
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
                        [
                            attr("name", Expression::String("default".to_string())),
                            attr("node_count", Expression::Number(hcl::Number::from(3))),
                            attr("vm_size", Expression::String("Standard_D2s_v3".to_string())),
                        ],
                    )),
                    nested(block("identity", [attr("type", Expression::String("SystemAssigned".to_string()))])),
                    nested(block(
                        "azure_active_directory_role_based_access_control",
                        [
                            attr("azure_rbac_enabled", Expression::Bool(true)),
                            attr("tenant_id", expr::raw("var.azure_managing_tenant_id")),
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
        let has_public_https_endpoint = stack_has_public_https_endpoint(ctx.stack);
        if has_public_https_endpoint {
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
                    attr("depends_on", expr::raw(format!("[azurerm_kubernetes_cluster.{label}]"))),
                ],
            ));
        }
        let metrics_server_depends_on = if has_public_https_endpoint {
            expr::raw(format!(
                "[azurerm_kubernetes_cluster.{label}, azapi_update_resource.{label}_alb_controller]"
            ))
        } else {
            expr::raw(format!("[azurerm_kubernetes_cluster.{label}]"))
        };
        add_metrics_server(&mut fragment, label, metrics_server_depends_on);
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
    Ok(expr::object([
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
    ]))
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
    expr::object([
        (
            "Name".to_string(),
            Expression::String(format!("${{local.resource_prefix}}-{label}-{kind}")),
        ),
        (
            "kubernetes.io/cluster/${local.resource_prefix}-k8s".to_string(),
            Expression::String("shared".to_string()),
        ),
        (
            format!("kubernetes.io/role/{role}"),
            Expression::String("1".to_string()),
        ),
    ])
}
