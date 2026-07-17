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
  principal_arn = aws_iam_user.target.arn
  type          = "STANDARD"
}

resource "aws_eks_access_policy_association" "e2e_target_admin" {
  provider      = aws.target
  cluster_name  = aws_eks_cluster.e2e.name
  principal_arn = aws_iam_user.target.arn
  policy_arn    = "arn:aws:eks::aws:cluster-access-policy/AmazonEKSClusterAdminPolicy"

  access_scope {
    type = "cluster"
  }

  depends_on = [aws_eks_access_entry.e2e_target]
}

# ── Management: IAM user ──────────────────────────────────────────────────────
# Scoped to the services the manager actually provisions, not AdministratorAccess.

resource "aws_iam_user" "manager" {
  provider = aws.management
  name     = "alien-test-manager"
}

resource "aws_iam_access_key" "manager" {
  provider = aws.management
  user     = aws_iam_user.manager.name
}

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

resource "aws_iam_user_policy_attachment" "manager" {
  provider   = aws.management
  user       = aws_iam_user.manager.name
  policy_arn = aws_iam_policy.manager.arn
}

# ── Management: IAM role for SA impersonation ────────────────────────────────
# The management IAM user assumes this role via STS AssumeRole to get short-lived
# credentials. Matches the production model: scoped to STS + resource management.

resource "aws_iam_role" "management" {
  provider = aws.management
  name     = "alien-test-management"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { AWS = aws_iam_user.manager.arn }
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
      Principal = { AWS = aws_iam_user.manager.arn }
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
        Principal = { AWS = aws_iam_user.manager.arn }
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

# ── Target: IAM user ──────────────────────────────────────────────────────────
# Scoped to the services the deployment steps actually provision.

resource "aws_iam_user" "target" {
  provider = aws.target
  name     = "alien-test-target"
}

resource "aws_iam_access_key" "target" {
  provider = aws.target
  user     = aws_iam_user.target.name
}

# Target user gets AdministratorAccess — simulating a real customer admin.
# In the real flow, the admin has broad permissions in their own account.
# The E2E test will create a scoped-down role with auto-generated permissions
# (from alien-permissions) and impersonate it during push_initial_setup,
# validating that the auto-generated permissions are sufficient.
resource "aws_iam_user_policy_attachment" "target_admin" {
  provider   = aws.target
  user       = aws_iam_user.target.name
  policy_arn = "arn:aws:iam::aws:policy/AdministratorAccess"
}

# Legacy policy — superseded by AdministratorAccess (target_admin attachment above).
# Kept to avoid Terraform state churn. The E2E tests now create scoped roles
# with auto-generated permissions from alien-permissions instead.
resource "aws_iam_policy" "target" {
  provider = aws.target
  name     = "alien-target-policy"

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "Lambda"
        Effect = "Allow"
        Action = [
          "lambda:CreateFunction",
          "lambda:DeleteFunction",
          "lambda:GetFunction",
          "lambda:GetFunctionConfiguration",
          "lambda:UpdateFunctionCode",
          "lambda:UpdateFunctionConfiguration",
          "lambda:InvokeFunction",
          "lambda:AddPermission",
          "lambda:RemovePermission",
          "lambda:GetPolicy",
          "lambda:ListVersionsByFunction",
          "lambda:PublishVersion",
          "lambda:CreateAlias",
          "lambda:UpdateAlias",
          "lambda:DeleteAlias",
          "lambda:CreateEventSourceMapping",
          "lambda:DeleteEventSourceMapping",
          "lambda:GetEventSourceMapping",
          "lambda:ListEventSourceMappings",
          "lambda:UpdateEventSourceMapping",
          "lambda:TagResource",
          "lambda:ListTags",
        ]
        Resource = "*"
      },
      {
        Sid    = "IAM"
        Effect = "Allow"
        Action = [
          "iam:CreateRole",
          "iam:DeleteRole",
          "iam:GetRole",
          "iam:UpdateRole",
          "iam:PassRole",
          "iam:AttachRolePolicy",
          "iam:DetachRolePolicy",
          "iam:PutRolePolicy",
          "iam:DeleteRolePolicy",
          "iam:GetRolePolicy",
          "iam:ListRolePolicies",
          "iam:ListAttachedRolePolicies",
          "iam:TagRole",
        ]
        Resource = "*"
      },
      {
        Sid    = "SQS"
        Effect = "Allow"
        Action = [
          "sqs:CreateQueue",
          "sqs:DeleteQueue",
          "sqs:GetQueueAttributes",
          "sqs:GetQueueUrl",
          "sqs:SetQueueAttributes",
          "sqs:SendMessage",
          "sqs:ReceiveMessage",
          "sqs:DeleteMessage",
          "sqs:TagQueue",
        ]
        Resource = "*"
      },
      {
        Sid    = "DynamoDB"
        Effect = "Allow"
        Action = [
          "dynamodb:CreateTable",
          "dynamodb:DeleteTable",
          "dynamodb:DescribeTable",
          "dynamodb:UpdateTable",
          "dynamodb:GetItem",
          "dynamodb:PutItem",
          "dynamodb:DeleteItem",
          "dynamodb:Query",
          "dynamodb:BatchGetItem",
          "dynamodb:BatchWriteItem",
          "dynamodb:UpdateTimeToLive",
          "dynamodb:DescribeTimeToLive",
          "dynamodb:TagResource",
        ]
        Resource = "*"
      },
      {
        Sid    = "S3"
        Effect = "Allow"
        Action = [
          "s3:CreateBucket",
          "s3:DeleteBucket",
          "s3:GetBucketLocation",
          "s3:ListBucket",
          "s3:GetObject",
          "s3:PutObject",
          "s3:DeleteObject",
          "s3:PutBucketPolicy",
          "s3:GetBucketPolicy",
          "s3:PutBucketVersioning",
          "s3:GetBucketVersioning",
          "s3:PutPublicAccessBlock",
          "s3:GetPublicAccessBlock",
          "s3:PutBucketTagging",
          "s3:GetBucketTagging",
          "s3:PutBucketNotification",
          "s3:GetBucketNotification",
          "s3:PutLifecycleConfiguration",
          "s3:GetLifecycleConfiguration",
          "s3:DeleteLifecycleConfiguration",
          "s3:ListBucketVersions",
          "s3:DeleteObjectVersion",
        ]
        Resource = "*"
      },
      {
        Sid    = "SecretsManager"
        Effect = "Allow"
        Action = [
          "secretsmanager:CreateSecret",
          "secretsmanager:DeleteSecret",
          "secretsmanager:GetSecretValue",
          "secretsmanager:PutSecretValue",
          "secretsmanager:UpdateSecret",
          "secretsmanager:DescribeSecret",
          "secretsmanager:TagResource",
          "secretsmanager:UntagResource",
        ]
        Resource = "*"
      },
      {
        Sid    = "EventBridge"
        Effect = "Allow"
        Action = [
          "events:PutRule",
          "events:DeleteRule",
          "events:DescribeRule",
          "events:PutTargets",
          "events:RemoveTargets",
          "events:ListTargetsByRule",
          "events:TagResource",
          "events:UntagResource",
          "scheduler:CreateSchedule",
          "scheduler:DeleteSchedule",
          "scheduler:GetSchedule",
          "scheduler:UpdateSchedule",
        ]
        Resource = "*"
      },
      {
        Sid    = "CloudWatch"
        Effect = "Allow"
        Action = [
          "logs:CreateLogGroup",
          "logs:DeleteLogGroup",
          "logs:PutRetentionPolicy",
          "logs:TagResource",
        ]
        Resource = "*"
      },
      {
        Sid      = "ECR"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        # Lambda CreateFunction with a cross-account ECR image requires the
        # calling principal to have identity-based ECR permissions on the
        # source repo. Lambda invokes BatchGetImage on behalf of the caller
        # (verified via CloudTrail: invokedBy=lambda.amazonaws.com).
        Sid    = "ECRCrossAccountAccess"
        Effect = "Allow"
        Action = [
          "ecr:BatchGetImage",
          "ecr:GetDownloadUrlForLayer",
          "ecr:GetRepositoryPolicy",
          "ecr:SetRepositoryPolicy",
        ]
        Resource = "arn:aws:ecr:*:${data.aws_caller_identity.management.account_id}:repository/alien-e2e*"
      },
      {
        Sid    = "CodeBuild"
        Effect = "Allow"
        Action = [
          "codebuild:CreateProject",
          "codebuild:DeleteProject",
          "codebuild:UpdateProject",
          "codebuild:BatchGetProjects",
          "codebuild:StartBuild",
          "codebuild:BatchGetBuilds",
          "codebuild:StopBuild",
        ]
        Resource = "*"
      },
      {
        Sid    = "SNS"
        Effect = "Allow"
        Action = [
          "sns:CreateTopic",
          "sns:DeleteTopic",
          "sns:GetTopicAttributes",
          "sns:SetTopicAttributes",
          "sns:Subscribe",
          "sns:Unsubscribe",
          "sns:Publish",
          "sns:TagResource",
          "sns:UntagResource",
        ]
        Resource = "*"
      },
    ]
  })
}

# Legacy attachment — kept for Terraform state compatibility.
resource "aws_iam_user_policy_attachment" "target" {
  provider   = aws.target
  user       = aws_iam_user.target.name
  policy_arn = aws_iam_policy.target.arn
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
      Principal = { AWS = aws_iam_user.manager.arn }
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
        Principal = { AWS = aws_iam_user.manager.arn }
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
