terraform {
  required_providers {
    aws = {
      source                = "hashicorp/aws"
      version               = "~> 5.0"
      configuration_aliases = [aws.management, aws.target]
    }
    random = { source = "hashicorp/random", version = "~> 3.0" }
  }
}

resource "random_id" "suffix" {
  byte_length = 4
}

# ── Management: IAM user ──────────────────────────────────────────────────────
# Dedicated test account — AdministratorAccess is safe here.

resource "aws_iam_user" "manager" {
  provider = aws.management
  name     = "alien-test-manager"
}

resource "aws_iam_access_key" "manager" {
  provider = aws.management
  user     = aws_iam_user.manager.name
}

resource "aws_iam_user_policy_attachment" "manager_admin" {
  provider   = aws.management
  user       = aws_iam_user.manager.name
  policy_arn = "arn:aws:iam::aws:policy/AdministratorAccess"
}

# ── Management: IAM role for SA impersonation ────────────────────────────────
# The management IAM user assumes this role via STS AssumeRole to get short-lived
# credentials. This mirrors the platform flow where the ServiceAccount resource
# creates an IAM role that the manager impersonates.

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

resource "aws_iam_role_policy_attachment" "management_admin" {
  provider   = aws.management
  role       = aws_iam_role.management.name
  policy_arn = "arn:aws:iam::aws:policy/AdministratorAccess"
}

# ── Management: S3 bucket ─────────────────────────────────────────────────────

resource "aws_s3_bucket" "test" {
  provider      = aws.management
  bucket        = "alien-test-${random_id.suffix.hex}"
  force_destroy = true
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
# Rather than placing the ECR repo in the target region, we keep it in the
# management region and configure private image replication to the target region.
# This mirrors the production flow where the manager's artifact registry
# controller configures replication via the ECR API.

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
# Required by the artifact registry tests — the ECR provider assumes these roles
# to create/manage repositories and generate pull credentials.

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
      Principal = { AWS = "arn:aws:iam::${data.aws_caller_identity.management.account_id}:root" }
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
    Statement = [{
      Effect = "Allow"
      Action = [
        "ecr:CreateRepository",
        "ecr:DeleteRepository",
        "ecr:DescribeRepositories",
        "ecr:GetRepositoryPolicy",
        "ecr:SetRepositoryPolicy",
        "ecr:DeleteRepositoryPolicy",
        "ecr:PutImage",
        "ecr:InitiateLayerUpload",
        "ecr:UploadLayerPart",
        "ecr:CompleteLayerUpload",
        "ecr:BatchCheckLayerAvailability",
        "ecr:GetAuthorizationToken",
        "ecr:TagResource",
        "ecr:UntagResource",
      ]
      Resource = "*"
    }]
  })
}

resource "aws_iam_role" "ecr_pull" {
  provider = aws.management
  name     = "alien-test-ecr-pull"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { AWS = "arn:aws:iam::${data.aws_caller_identity.management.account_id}:root" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "ecr_pull" {
  provider = aws.management
  name     = "ecr-pull"
  role     = aws_iam_role.ecr_pull.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "ecr:GetDownloadUrlForLayer",
        "ecr:BatchGetImage",
        "ecr:BatchCheckLayerAvailability",
        "ecr:DescribeRepositories",
        "ecr:GetRepositoryPolicy",
        "ecr:GetAuthorizationToken",
      ]
      Resource = "*"
    }]
  })
}

# ── Target: IAM user ──────────────────────────────────────────────────────────
# Dedicated test account — AdministratorAccess is safe here.

resource "aws_iam_user" "target" {
  provider = aws.target
  name     = "alien-test-target"
}

resource "aws_iam_access_key" "target" {
  provider = aws.target
  user     = aws_iam_user.target.name
}

resource "aws_iam_user_policy_attachment" "target_admin" {
  provider   = aws.target
  user       = aws_iam_user.target.name
  policy_arn = "arn:aws:iam::aws:policy/AdministratorAccess"
}

data "aws_caller_identity" "target" {
  provider = aws.target
}
