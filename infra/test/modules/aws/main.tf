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
        Sid      = "AllServices"
        Effect   = "Allow"
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
          "lambda:UntagResource",
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
          "iam:UntagRole",
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
          "sqs:UntagQueue",
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
          "dynamodb:UntagResource",
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
          "ecr:TagResource", "ecr:UntagResource",
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
