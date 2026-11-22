terraform {
  required_providers {
    aws = {
      source                = "hashicorp/aws"
      version               = "~> 5.0"
      configuration_aliases = [aws.management, aws.target]
    }
    random = { source = "hashicorp/random", version = "~> 3.0" }
    tls    = { source = "hashicorp/tls", version = "~> 4.0" }
  }
}

resource "random_id" "suffix" {
  byte_length = 4
}

locals {
  e2e_eks_cluster_name      = var.e2e_eks_cluster_name != "" ? var.e2e_eks_cluster_name : "alien-e2e-${random_id.suffix.hex}"
  e2e_eks_cluster_role_name = "alien-e2e-eks-cluster-${random_id.suffix.hex}"
  e2e_eks_node_role_name    = "alien-e2e-eks-node-${random_id.suffix.hex}"
  e2e_terraform_state_name  = "alien-e2e-terraform-state-${random_id.suffix.hex}"
}

data "aws_availability_zones" "target" {
  provider = aws.target
  state    = "available"
}

# ── Target: reusable E2E network ─────────────────────────────────────────────

resource "aws_vpc" "e2e" {
  provider             = aws.target
  cidr_block           = "10.251.0.0/16"
  enable_dns_hostnames = true
  enable_dns_support   = true

  tags = {
    Name = "alien-e2e-${random_id.suffix.hex}"
  }
}

resource "aws_internet_gateway" "e2e" {
  provider = aws.target
  vpc_id   = aws_vpc.e2e.id

  tags = {
    Name = "alien-e2e-${random_id.suffix.hex}"
  }
}

resource "aws_subnet" "e2e_public" {
  provider                = aws.target
  count                   = 2
  vpc_id                  = aws_vpc.e2e.id
  cidr_block              = cidrsubnet(aws_vpc.e2e.cidr_block, 8, count.index)
  availability_zone       = data.aws_availability_zones.target.names[count.index]
  map_public_ip_on_launch = true

  tags = {
    Name                                                  = "alien-e2e-public-${count.index + 1}-${random_id.suffix.hex}"
    "kubernetes.io/cluster/${local.e2e_eks_cluster_name}" = "shared"
    "kubernetes.io/role/elb"                              = "1"
  }
}

resource "aws_subnet" "e2e_private" {
  provider          = aws.target
  count             = 2
  vpc_id            = aws_vpc.e2e.id
  cidr_block        = cidrsubnet(aws_vpc.e2e.cidr_block, 8, count.index + 10)
  availability_zone = data.aws_availability_zones.target.names[count.index]

  tags = {
    Name                                                  = "alien-e2e-private-${count.index + 1}-${random_id.suffix.hex}"
    "kubernetes.io/cluster/${local.e2e_eks_cluster_name}" = "shared"
    "kubernetes.io/role/internal-elb"                     = "1"
  }
}

resource "aws_eip" "e2e_nat" {
  provider = aws.target
  domain   = "vpc"

  tags = {
    Name = "alien-e2e-nat-${random_id.suffix.hex}"
  }
}

resource "aws_nat_gateway" "e2e" {
  provider      = aws.target
  allocation_id = aws_eip.e2e_nat.id
  subnet_id     = aws_subnet.e2e_public[0].id

  tags = {
    Name = "alien-e2e-${random_id.suffix.hex}"
  }
}

resource "aws_route_table" "e2e_public" {
  provider = aws.target
  vpc_id   = aws_vpc.e2e.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.e2e.id
  }

  tags = {
    Name = "alien-e2e-public-${random_id.suffix.hex}"
  }
}

resource "aws_route_table_association" "e2e_public" {
  provider       = aws.target
  count          = length(aws_subnet.e2e_public)
  subnet_id      = aws_subnet.e2e_public[count.index].id
  route_table_id = aws_route_table.e2e_public.id
}

resource "aws_route_table" "e2e_private" {
  provider = aws.target
  vpc_id   = aws_vpc.e2e.id

  route {
    cidr_block     = "0.0.0.0/0"
    nat_gateway_id = aws_nat_gateway.e2e.id
  }

  tags = {
    Name = "alien-e2e-private-${random_id.suffix.hex}"
  }
}

resource "aws_route_table_association" "e2e_private" {
  provider       = aws.target
  count          = length(aws_subnet.e2e_private)
  subnet_id      = aws_subnet.e2e_private[count.index].id
  route_table_id = aws_route_table.e2e_private.id
}

resource "aws_security_group" "e2e" {
  provider    = aws.target
  name        = "alien-e2e-${random_id.suffix.hex}"
  description = "Reusable Alien E2E security group"
  vpc_id      = aws_vpc.e2e.id

  ingress {
    description = "VPC internal"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = [aws_vpc.e2e.cidr_block]
  }

  egress {
    description = "All outbound"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name = "alien-e2e-${random_id.suffix.hex}"
  }
}

# ── Target: shared EKS cluster for Terraform -> Helm E2Es ────────────────────

resource "aws_iam_role" "e2e_eks_cluster" {
  provider = aws.target
  name     = local.e2e_eks_cluster_role_name

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "eks.amazonaws.com" }
      Action    = ["sts:AssumeRole", "sts:TagSession"]
    }]
  })
}

resource "aws_iam_role_policy_attachment" "e2e_eks_cluster" {
  provider   = aws.target
  role       = aws_iam_role.e2e_eks_cluster.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSClusterPolicy"
}

resource "aws_iam_role_policy_attachment" "e2e_eks_auto_mode_cluster" {
  provider = aws.target
  for_each = toset([
    "arn:aws:iam::aws:policy/AmazonEKSBlockStoragePolicy",
    "arn:aws:iam::aws:policy/AmazonEKSComputePolicy",
    "arn:aws:iam::aws:policy/AmazonEKSLoadBalancingPolicy",
    "arn:aws:iam::aws:policy/AmazonEKSNetworkingPolicy",
  ])

  role       = aws_iam_role.e2e_eks_cluster.name
  policy_arn = each.value
}

resource "aws_iam_role" "e2e_eks_node" {
  provider = aws.target
  name     = local.e2e_eks_node_role_name

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "e2e_eks_node" {
  provider = aws.target
  for_each = toset([
    "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy",
    "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly",
    "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy",
    "arn:aws:iam::aws:policy/AmazonEKSWorkerNodeMinimalPolicy",
  ])

  role       = aws_iam_role.e2e_eks_node.name
  policy_arn = each.value
}

resource "aws_iam_role" "e2e_eks_managed_node" {
  provider = aws.target
  name     = "alien-e2e-eks-mng-node-${random_id.suffix.hex}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "e2e_eks_managed_node" {
  provider = aws.target
  for_each = toset([
    "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy",
    "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryPullOnly",
    "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy",
  ])

  role       = aws_iam_role.e2e_eks_managed_node.name
  policy_arn = each.value
}

resource "aws_eks_cluster" "e2e" {
  provider                      = aws.target
  name                          = local.e2e_eks_cluster_name
  role_arn                      = "arn:aws:iam::${data.aws_caller_identity.target.account_id}:role/${local.e2e_eks_cluster_role_name}"
  version                       = var.e2e_eks_kubernetes_version
  bootstrap_self_managed_addons = false

  vpc_config {
    subnet_ids              = concat(aws_subnet.e2e_public[*].id, aws_subnet.e2e_private[*].id)
    endpoint_public_access  = true
    endpoint_private_access = true
  }

  access_config {
    authentication_mode                         = "API_AND_CONFIG_MAP"
    bootstrap_cluster_creator_admin_permissions = true
  }

  compute_config {
    enabled = true
    # Application images built for AWS/EKS E2E are linux/arm64. Keep Auto Mode
    # system capacity enabled, but run test workloads on the explicit ARM64
    # managed node group below instead of EKS's default mixed general-purpose pool.
    node_pools    = ["system"]
    node_role_arn = "arn:aws:iam::${data.aws_caller_identity.target.account_id}:role/${local.e2e_eks_node_role_name}"
  }

  kubernetes_network_config {
    elastic_load_balancing {
      enabled = true
    }
  }

  storage_config {
    block_storage {
      enabled = true
    }
  }

  depends_on = [
    aws_iam_role_policy_attachment.e2e_eks_cluster,
    aws_iam_role_policy_attachment.e2e_eks_auto_mode_cluster,
    aws_iam_role_policy_attachment.e2e_eks_node,
  ]
}

data "tls_certificate" "e2e_eks_oidc" {
  url = aws_eks_cluster.e2e.identity[0].oidc[0].issuer
}

resource "aws_iam_openid_connect_provider" "e2e_eks" {
  provider = aws.target

  url             = aws_eks_cluster.e2e.identity[0].oidc[0].issuer
  client_id_list  = ["sts.amazonaws.com"]
  thumbprint_list = [data.tls_certificate.e2e_eks_oidc.certificates[0].sha1_fingerprint]

  tags = {
    Name = "alien-e2e-eks-oidc-${random_id.suffix.hex}"
  }
}

resource "aws_iam_role" "e2e_eks_ebs_csi" {
  provider = aws.target
  name     = "alien-e2e-eks-ebs-csi-${random_id.suffix.hex}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Federated = aws_iam_openid_connect_provider.e2e_eks.arn
      }
      Action = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "${replace(aws_eks_cluster.e2e.identity[0].oidc[0].issuer, "https://", "")}:aud" = "sts.amazonaws.com"
          "${replace(aws_eks_cluster.e2e.identity[0].oidc[0].issuer, "https://", "")}:sub" = "system:serviceaccount:kube-system:ebs-csi-controller-sa"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "e2e_eks_ebs_csi" {
  provider   = aws.target
  role       = aws_iam_role.e2e_eks_ebs_csi.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonEBSCSIDriverPolicy"
}

resource "aws_eip" "e2e_ingress" {
  provider = aws.target
  count    = length(aws_subnet.e2e_public)
  domain   = "vpc"

  tags = {
    Name = "alien-e2e-ingress-${count.index + 1}-${random_id.suffix.hex}"
  }
}

resource "aws_eks_addon" "e2e_vpc_cni" {
  provider     = aws.target
  cluster_name = aws_eks_cluster.e2e.name
  addon_name   = "vpc-cni"

  depends_on = [
    aws_eks_cluster.e2e,
  ]
}

resource "aws_eks_node_group" "e2e" {
  provider        = aws.target
  cluster_name    = aws_eks_cluster.e2e.name
  node_group_name = "alien-e2e-${random_id.suffix.hex}"
  node_role_arn   = aws_iam_role.e2e_eks_managed_node.arn
  subnet_ids      = aws_subnet.e2e_private[*].id

  ami_type       = "AL2023_ARM_64_STANDARD"
  capacity_type  = "ON_DEMAND"
  disk_size      = 20
  instance_types = ["t4g.medium"]

  scaling_config {
    # The two-node baseline is fully consumed by cluster add-ons at the
    # t4g.medium pod limit. Keep a third ARM64 workload node available because
    # this test cluster does not run an autoscaler.
    desired_size = 3
    max_size     = 3
    min_size     = 3
  }

  update_config {
    max_unavailable = 1
  }

  depends_on = [
    aws_eks_addon.e2e_vpc_cni,
    aws_iam_role_policy_attachment.e2e_eks_managed_node,
  ]
}

resource "aws_eks_addon" "e2e_kube_proxy" {
  provider     = aws.target
  cluster_name = aws_eks_cluster.e2e.name
  addon_name   = "kube-proxy"

  depends_on = [
    aws_eks_node_group.e2e,
  ]
}

resource "aws_eks_addon" "e2e_coredns" {
  provider     = aws.target
  cluster_name = aws_eks_cluster.e2e.name
  addon_name   = "coredns"

  depends_on = [
    aws_eks_node_group.e2e,
  ]
}

resource "aws_eks_addon" "e2e_ebs_csi" {
  provider                 = aws.target
  cluster_name             = aws_eks_cluster.e2e.name
  addon_name               = "aws-ebs-csi-driver"
  service_account_role_arn = aws_iam_role.e2e_eks_ebs_csi.arn

  depends_on = [
    aws_eks_node_group.e2e,
    aws_iam_role_policy_attachment.e2e_eks_ebs_csi,
  ]
}

resource "aws_eks_access_entry" "e2e_target" {
  provider      = aws.target
  cluster_name  = aws_eks_cluster.e2e.name
  principal_arn = aws_iam_role.target_ci.arn
  type          = "STANDARD"
}

resource "aws_eks_access_policy_association" "e2e_target_admin" {
  provider      = aws.target
  cluster_name  = aws_eks_cluster.e2e.name
  principal_arn = aws_iam_role.target_ci.arn
  policy_arn    = "arn:aws:eks::aws:cluster-access-policy/AmazonEKSClusterAdminPolicy"

  access_scope {
    type = "cluster"
  }

  depends_on = [aws_eks_access_entry.e2e_target]
}

# ── CI identities ─────────────────────────────────────────────────────────────
# Cloud tests sign in from GitHub Actions with a short-lived OIDC token; no IAM
# user or access key exists. Each account has one role that only a job running in
# the `cloud-tests` environment of this repository can assume. Terraform for this
# directory is applied by a person, never by these roles: the deny statements
# below keep a test job from changing the provider, the CI roles or their trust.

locals {
  github_oidc_url  = "token.actions.githubusercontent.com"
  github_oidc_sub  = "repo:alienplatform/alien:environment:cloud-tests"
  github_thumbs    = ["6938fd4d98bab03faadb97b34396831e3780aea1", "1c58a3a8518e8759bf075b76b750d4f2df264fcd"]
  ci_max_session_s = 14400
}

resource "aws_iam_openid_connect_provider" "github_management" {
  provider        = aws.management
  url             = "https://${local.github_oidc_url}"
  client_id_list  = ["sts.amazonaws.com"]
  thumbprint_list = local.github_thumbs
}

# The target account already has GitHub's provider, created and owned by another
# repository's Terraform. Read it instead of creating a second owner.
data "aws_iam_openid_connect_provider" "github_target" {
  provider = aws.target
  url      = "https://${local.github_oidc_url}"
}

resource "aws_iam_role" "manager_ci" {
  provider             = aws.management
  name                 = "alien-test-manager-ci"
  max_session_duration = local.ci_max_session_s

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = aws_iam_openid_connect_provider.github_management.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "${local.github_oidc_url}:aud" = "sts.amazonaws.com"
          "${local.github_oidc_url}:sub" = local.github_oidc_sub
        }
      }
    }]
  })
}

resource "aws_iam_role" "target_ci" {
  provider             = aws.target
  name                 = "alien-test-target-ci"
  max_session_duration = local.ci_max_session_s

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = data.aws_iam_openid_connect_provider.github_target.arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "${local.github_oidc_url}:aud" = "sts.amazonaws.com"
          "${local.github_oidc_url}:sub" = local.github_oidc_sub
        }
      }
    }]
  })
}

# Known limit: tests create roles, and IAM has no condition key for the trust
# policy of a role being created or updated, so a rule such as "the trust stays
# inside this account" cannot be expressed here. The role keeps the ability to
# create other roles; it cannot change the provider, the CI roles or create
# users and access keys.
# Explicit denies win over the broad allows below. They cover the OIDC
# provider, both CI roles, and long-lived credentials (users, access keys).
data "aws_iam_policy_document" "ci_guard_management" {
  statement {
    sid       = "NoCiIdentityChanges"
    effect    = "Deny"
    actions   = ["iam:*OpenIDConnectProvider*"]
    resources = [aws_iam_openid_connect_provider.github_management.arn]
  }
  statement {
    sid    = "NoCiRoleChanges"
    effect = "Deny"
    actions = [
      "iam:UpdateAssumeRolePolicy", "iam:UpdateRole", "iam:UpdateRoleDescription",
      "iam:DeleteRole", "iam:PutRolePolicy", "iam:DeleteRolePolicy",
      "iam:AttachRolePolicy", "iam:DetachRolePolicy",
      "iam:PutRolePermissionsBoundary", "iam:DeleteRolePermissionsBoundary",
    ]
    resources = [aws_iam_role.manager_ci.arn]
  }
  statement {
    sid    = "NoLongLivedCredentials"
    effect = "Deny"
    actions = [
      "iam:CreateUser", "iam:CreateAccessKey", "iam:CreateLoginProfile",
      "iam:UpdateLoginProfile", "iam:CreateOpenIDConnectProvider",
      "iam:CreateSAMLProvider", "iam:UpdateSAMLProvider",
    ]
    resources = ["*"]
  }
}

data "aws_iam_policy_document" "ci_guard_target" {
  statement {
    sid       = "NoCiIdentityChanges"
    effect    = "Deny"
    actions   = ["iam:*OpenIDConnectProvider*"]
    resources = [data.aws_iam_openid_connect_provider.github_target.arn]
  }
  statement {
    sid    = "NoCiRoleChanges"
    effect = "Deny"
    actions = [
      "iam:UpdateAssumeRolePolicy", "iam:UpdateRole", "iam:UpdateRoleDescription",
      "iam:DeleteRole", "iam:PutRolePolicy", "iam:DeleteRolePolicy",
      "iam:AttachRolePolicy", "iam:DetachRolePolicy",
      "iam:PutRolePermissionsBoundary", "iam:DeleteRolePermissionsBoundary",
    ]
    resources = [aws_iam_role.target_ci.arn]
  }
  statement {
    sid    = "NoLongLivedCredentials"
    effect = "Deny"
    actions = [
      "iam:CreateUser", "iam:CreateAccessKey", "iam:CreateLoginProfile",
      "iam:UpdateLoginProfile", "iam:CreateSAMLProvider", "iam:UpdateSAMLProvider",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_role_policy" "manager_ci_guard" {
  provider = aws.management
  name     = "ci-guard"
  role     = aws_iam_role.manager_ci.id
  policy   = data.aws_iam_policy_document.ci_guard_management.json
}

resource "aws_iam_role_policy" "target_ci_guard" {
  provider = aws.target
  name     = "ci-guard"
  role     = aws_iam_role.target_ci.id
  policy   = data.aws_iam_policy_document.ci_guard_target.json
}

# ── Management: CI role permissions ───────────────────────────────────────────
# Scoped to the services the manager actually provisions, not AdministratorAccess.

resource "aws_iam_policy" "manager" {
  provider = aws.management
  name     = "alien-manager-policy"

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "AssumeRoles"
        Effect = "Allow"
        Action = "sts:AssumeRole"
        Resource = [
          aws_iam_role.management.arn,
          aws_iam_role.ecr_push.arn,
          aws_iam_role.ecr_pull.arn,
          aws_iam_role.e2e_ar_push.arn,
          aws_iam_role.e2e_ar_pull.arn,
        ]
      },
      {
        Sid    = "NativeEncryptionSetup"
        Effect = "Allow"
        Action = [
          "kms:CreateKey", "kms:DescribeKey", "kms:GetKeyPolicy", "kms:PutKeyPolicy",
          "kms:EnableKey", "kms:DisableKey", "kms:EnableKeyRotation",
          "kms:GetKeyRotationStatus", "kms:TagResource", "kms:UntagResource",
          "kms:ListResourceTags", "kms:ScheduleKeyDeletion", "kms:CancelKeyDeletion",
          "kms:CreateCustomKeyStore", "kms:DescribeCustomKeyStores",
          "kms:ConnectCustomKeyStore", "kms:DisconnectCustomKeyStore",
          "kms:UpdateCustomKeyStore", "kms:DeleteCustomKeyStore",
          "kms:CreateGrant", "kms:ListGrants", "kms:RevokeGrant", "kms:RetireGrant",
          "kms:Encrypt", "kms:Decrypt", "kms:GenerateDataKey*", "kms:ReEncrypt*",
          "rds:CreateDBCluster", "rds:CreateDBInstance", "rds:CreateDBSubnetGroup",
          "rds:DescribeDBClusters", "rds:DescribeDBInstances", "rds:DescribeDBSubnetGroups",
          "rds:ModifyDBCluster", "rds:ModifyDBInstance", "rds:StartDBCluster",
          "rds:StopDBCluster", "rds:RebootDBInstance", "rds:DeleteDBCluster",
          "rds:DeleteDBInstance", "rds:DeleteDBSubnetGroup", "rds:AddTagsToResource",
          "rds:RemoveTagsFromResource", "rds:ListTagsForResource",
        ]
        Resource = "*"
      },
      {
        Sid    = "AllServices"
        Effect = "Allow"
        Action = [
          "ec2:*",
          "ecr:*",
          "lambda:*",
          "iam:*",
          "sqs:*",
          "dynamodb:*",
          "s3:*",
          "secretsmanager:*",
          "events:*",
          "scheduler:*",
          "logs:*",
          "acm:*",
          "cloudformation:*",
          "autoscaling:*",
          "apigateway:*",
          "codebuild:*",
          "elasticloadbalancing:*",
          "ssm:*",
          "sts:GetCallerIdentity",
          "sts:AssumeRoleWithWebIdentity",
        ]
        Resource = "*"
      },
    ]
  })
}

resource "aws_iam_role_policy_attachment" "manager_ci" {
  provider   = aws.management
  role       = aws_iam_role.manager_ci.name
  policy_arn = aws_iam_policy.manager.arn
}

# ── Management: IAM role for SA impersonation ────────────────────────────────
# The management CI role assumes this role via STS AssumeRole to get short-lived
# credentials. Matches the production model: scoped to STS + resource management.

resource "aws_iam_role" "management" {
  provider = aws.management
  name     = "alien-test-management"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { AWS = aws_iam_role.manager_ci.arn }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "management" {
  provider = aws.management
  name     = "alien-management-policy"
  role     = aws_iam_role.management.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "AssumeCustomerRoles"
        Effect   = "Allow"
        Action   = "sts:AssumeRole"
        Resource = "*"
      },
      {
        Sid      = "ECRAuth"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "DynamoDBAccess"
        Effect = "Allow"
        Action = [
          "dynamodb:GetItem",
          "dynamodb:PutItem",
          "dynamodb:DeleteItem",
          "dynamodb:Query",
          "dynamodb:BatchGetItem",
          "dynamodb:BatchWriteItem",
        ]
        Resource = aws_dynamodb_table.command_kv.arn
      },
      {
        Sid    = "S3CommandStorageAccess"
        Effect = "Allow"
        Action = [
          "s3:GetObject",
          "s3:PutObject",
          "s3:DeleteObject",
          "s3:ListBucket",
        ]
        Resource = [
          aws_s3_bucket.test.arn,
          "${aws_s3_bucket.test.arn}/*",
        ]
      },
    ]
  })
}

# ── Management: S3 bucket ─────────────────────────────────────────────────────

resource "aws_s3_bucket" "test" {
  provider      = aws.management
  bucket        = "alien-test-${random_id.suffix.hex}"
  force_destroy = true
}

resource "aws_s3_bucket_versioning" "test" {
  provider = aws.management
  bucket   = aws_s3_bucket.test.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_public_access_block" "test" {
  provider                = aws.management
  bucket                  = aws_s3_bucket.test.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# ── Target: Terraform state for end-to-end test runs ─────────────────────────
# Test runs that deploy with Terraform keep their state and a copy of their
# initialized working directory here, so a run that dies before its own
# teardown can still be destroyed later. Objects can hold deployment tokens:
# the bucket is private and objects expire after 30 days.

resource "aws_s3_bucket" "e2e_terraform_state" {
  provider = aws.target
  bucket   = local.e2e_terraform_state_name

  # Reject teardown before it removes protections from retained test state.
  lifecycle {
    prevent_destroy = true
  }
}

resource "aws_s3_bucket_versioning" "e2e_terraform_state" {
  provider = aws.target
  bucket   = aws_s3_bucket.e2e_terraform_state.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "e2e_terraform_state" {
  provider = aws.target
  bucket   = aws_s3_bucket.e2e_terraform_state.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "e2e_terraform_state" {
  provider                = aws.target
  bucket                  = aws_s3_bucket.e2e_terraform_state.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_ownership_controls" "e2e_terraform_state" {
  provider = aws.target
  bucket   = aws_s3_bucket.e2e_terraform_state.id
  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}

resource "aws_s3_bucket_lifecycle_configuration" "e2e_terraform_state" {
  provider = aws.target
  bucket   = aws_s3_bucket.e2e_terraform_state.id

  rule {
    id     = "expire-test-run-state"
    status = "Enabled"
    filter {}

    expiration {
      days = 30
    }
    noncurrent_version_expiration {
      noncurrent_days = 7
    }
    abort_incomplete_multipart_upload {
      days_after_initiation = 1
    }
  }
}

# ── Management: DynamoDB table for command KV ─────────────────────────────────
# Matching production base infra — used by command storage for key-value state.

resource "aws_dynamodb_table" "command_kv" {
  provider     = aws.management
  name         = "alien-test-command-kv-${random_id.suffix.hex}"
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "pk"
  range_key    = "sk"

  attribute {
    name = "pk"
    type = "S"
  }

  attribute {
    name = "sk"
    type = "S"
  }

  ttl {
    attribute_name = "ttl"
    enabled        = true
  }
}

# ── Management: ECR repository ────────────────────────────────────────────────

resource "aws_ecr_repository" "lambda_test" {
  provider             = aws.management
  name                 = "alien-test-lambda"
  image_tag_mutability = "MUTABLE"
  force_delete         = true
}

# ── Management: ECR replication to target region ─────────────────────────────
# Lambda requires container images in the same region as the function.

resource "aws_ecr_replication_configuration" "cross_region" {
  provider = aws.management

  replication_configuration {
    rule {
      destination {
        region      = var.target_region
        registry_id = data.aws_caller_identity.management.account_id
      }
    }
  }
}

# ── Management: Lambda execution role ────────────────────────────────────────

resource "aws_iam_role" "lambda_execution" {
  provider = aws.management
  name     = "alien-test-lambda-execution"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "lambda.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lambda_basic" {
  provider   = aws.management
  role       = aws_iam_role.lambda_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_iam_role_policy_attachment" "lambda_sqs" {
  provider   = aws.management
  role       = aws_iam_role.lambda_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaSQSQueueExecutionRole"
}

# ── Management: ECR push/pull roles ────────────────────────────────────────────
# Scoped to alien-test-* repositories, matching production IAM model.

data "aws_caller_identity" "management" {
  provider = aws.management
}

resource "aws_iam_role" "ecr_push" {
  provider = aws.management
  name     = "alien-test-ecr-push"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { AWS = aws_iam_role.manager_ci.arn }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "ecr_push" {
  provider = aws.management
  name     = "ecr-push"
  role     = aws_iam_role.ecr_push.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ECRAuth"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "ECRPushPull"
        Effect = "Allow"
        Action = "ecr:*"
        Resource = [
          "arn:aws:ecr:*:${data.aws_caller_identity.management.account_id}:repository/alien-test-*",
          "arn:aws:ecr:*:${data.aws_caller_identity.management.account_id}:repository/test-alien-test-*",
        ]
      },
    ]
  })
}

resource "aws_iam_role" "ecr_pull" {
  provider = aws.management
  name     = "alien-test-ecr-pull"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect    = "Allow"
        Principal = { AWS = aws_iam_role.manager_ci.arn }
        Action    = "sts:AssumeRole"
      },
      {
        Effect = "Allow"
        Principal = {
          Service = [
            "lambda.amazonaws.com",
            "codebuild.amazonaws.com",
          ]
        }
        Action = "sts:AssumeRole"
      },
    ]
  })
}

resource "aws_iam_role_policy" "ecr_pull" {
  provider = aws.management
  name     = "ecr-pull"
  role     = aws_iam_role.ecr_pull.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ECRAuth"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "ECRPull"
        Effect = "Allow"
        Action = [
          "ecr:GetDownloadUrlForLayer",
          "ecr:BatchGetImage",
          "ecr:BatchCheckLayerAvailability",
          "ecr:DescribeRepositories",
          "ecr:ListImages",
        ]
        Resource = [
          "arn:aws:ecr:*:${data.aws_caller_identity.management.account_id}:repository/alien-test-*",
          "arn:aws:ecr:*:${data.aws_caller_identity.management.account_id}:repository/test-alien-test-*",
        ]
      },
    ]
  })
}

# ── Target: CI role permissions ───────────────────────────────────────────────
# The target CI role gets AdministratorAccess in the target test account only,
# simulating a real customer admin. The tests create a scoped-down role with
# permissions generated from alien-permissions and impersonate it, validating
# that those permissions are sufficient. The ci-guard policy above still denies
# changes to the CI identity itself.
resource "aws_iam_role_policy_attachment" "target_ci_admin" {
  provider   = aws.target
  role       = aws_iam_role.target_ci.name
  policy_arn = "arn:aws:iam::aws:policy/AdministratorAccess"
}

data "aws_caller_identity" "target" {
  provider = aws.target
}

# ── E2E: Artifact Registry push/pull roles ─────────────────────────────────────
# These match the alien-infra AwsArtifactRegistryController pattern:
# prefix-scoped roles that the manager assumes to create/manage ECR repos.
# The binding creates repos dynamically as {prefix}-{repo_name}.
# Separate from alien-test-ecr-* roles which are for alien-bindings unit tests.

resource "aws_iam_role" "e2e_ar_push" {
  provider = aws.management
  name     = "alien-e2e-ar-push"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { AWS = aws_iam_role.manager_ci.arn }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "e2e_ar_push" {
  provider = aws.management
  name     = "ecr-push"
  role     = aws_iam_role.e2e_ar_push.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ECRAuth"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "ECRPushPull"
        Effect = "Allow"
        Action = [
          "ecr:CreateRepository", "ecr:DeleteRepository", "ecr:DescribeRepositories",
          "ecr:GetRepositoryPolicy", "ecr:SetRepositoryPolicy", "ecr:DeleteRepositoryPolicy",
          "ecr:PutImage", "ecr:InitiateLayerUpload", "ecr:UploadLayerPart",
          "ecr:CompleteLayerUpload", "ecr:BatchCheckLayerAvailability",
          "ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer", "ecr:ListImages",
          "ecr:TagResource",
          "ecr:PutLifecyclePolicy", "ecr:DeleteLifecyclePolicy",
          "ecr:PutImageScanningConfiguration", "ecr:PutImageTagMutability",
          "ecr:DescribeImages", "ecr:DescribeImageScanFindings",
          "ecr:ListTagsForResource", "ecr:DescribeRegistry",
          "ecr:PutReplicationConfiguration",
        ]
        Resource = "arn:aws:ecr:*:${data.aws_caller_identity.management.account_id}:repository/alien-e2e*"
      },
    ]
  })
}

resource "aws_iam_role" "e2e_ar_pull" {
  provider = aws.management
  name     = "alien-e2e-ar-pull"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect    = "Allow"
        Principal = { AWS = aws_iam_role.manager_ci.arn }
        Action    = "sts:AssumeRole"
      },
      {
        Effect = "Allow"
        Principal = {
          Service = ["lambda.amazonaws.com", "codebuild.amazonaws.com"]
        }
        Action = "sts:AssumeRole"
      },
    ]
  })
}

resource "aws_iam_role_policy" "e2e_ar_pull" {
  provider = aws.management
  name     = "ecr-pull"
  role     = aws_iam_role.e2e_ar_pull.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ECRAuth"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "ECRPull"
        Effect = "Allow"
        Action = [
          "ecr:GetDownloadUrlForLayer", "ecr:BatchGetImage",
          "ecr:BatchCheckLayerAvailability", "ecr:DescribeRepositories",
          "ecr:ListImages", "ecr:DescribeImages",
        ]
        Resource = "arn:aws:ecr:*:${data.aws_caller_identity.management.account_id}:repository/alien-e2e*"
      },
    ]
  })
}
