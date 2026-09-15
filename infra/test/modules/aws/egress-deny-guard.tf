locals {
  egress_deny_guard_stack_name      = "egress-deny-guard"
  egress_deny_guard_artifact_prefix = "egress-deny-guard/"

  egress_deny_guard_stack_arn     = "arn:${data.aws_partition.target.partition}:cloudformation:${var.target_region}:${data.aws_caller_identity.target.account_id}:stack/${local.egress_deny_guard_stack_name}/*"
  egress_deny_guard_role_arn      = "arn:${data.aws_partition.target.partition}:iam::${data.aws_caller_identity.target.account_id}:role/${local.egress_deny_guard_stack_name}-*"
  egress_deny_guard_policy_arn    = "arn:${data.aws_partition.target.partition}:iam::${data.aws_caller_identity.target.account_id}:policy/${local.egress_deny_guard_stack_name}-*"
  egress_deny_guard_image_arn     = "arn:${data.aws_partition.target.partition}:lambda:${var.target_region}:${data.aws_caller_identity.target.account_id}:microvm-image:${local.egress_deny_guard_stack_name}-*"
  egress_deny_guard_connector_arn = "arn:${data.aws_partition.target.partition}:lambda:${var.target_region}:${data.aws_caller_identity.target.account_id}:network-connector:*"
}

data "aws_partition" "target" {
  provider = aws.target
}

data "aws_iam_openid_connect_provider" "github_actions" {
  provider = aws.target
  url      = "https://token.actions.githubusercontent.com"
}

resource "aws_s3_bucket" "egress_deny_guard" {
  provider = aws.target
  bucket   = "alien-egress-deny-guard-${random_id.suffix.hex}"
}

resource "aws_s3_bucket_public_access_block" "egress_deny_guard" {
  provider = aws.target
  bucket   = aws_s3_bucket.egress_deny_guard.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_lifecycle_configuration" "egress_deny_guard" {
  provider = aws.target
  bucket   = aws_s3_bucket.egress_deny_guard.id

  rule {
    id     = "expire-guard-artifacts"
    status = "Enabled"

    filter {
      prefix = local.egress_deny_guard_artifact_prefix
    }

    expiration {
      days = 1
    }

    abort_incomplete_multipart_upload {
      days_after_initiation = 1
    }
  }
}

resource "aws_iam_role" "egress_deny_guard" {
  provider             = aws.target
  name                 = "alien-egress-deny-guard"
  max_session_duration = 5400

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = {
        Federated = data.aws_iam_openid_connect_provider.github_actions.arn
      }
      Action = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = {
          "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com"
          "token.actions.githubusercontent.com:sub" = "repo:alienplatform/alien:environment:egress-deny-guard"
        }
      }
    }]
  })
}

resource "aws_iam_role_policy" "egress_deny_guard" {
  provider = aws.target
  name     = "egress-deny-guard"
  role     = aws_iam_role.egress_deny_guard.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "Identity"
        Effect   = "Allow"
        Action   = "sts:GetCallerIdentity"
        Resource = "*"
      },
      {
        Sid    = "ArtifactBucket"
        Effect = "Allow"
        Action = [
          "s3:GetBucketLocation",
          "s3:ListBucket",
        ]
        Resource = aws_s3_bucket.egress_deny_guard.arn
        Condition = {
          StringLike = {
            "s3:prefix" = [
              local.egress_deny_guard_artifact_prefix,
              "${local.egress_deny_guard_artifact_prefix}*",
            ]
          }
        }
      },
      {
        Sid    = "ArtifactObjects"
        Effect = "Allow"
        Action = [
          "s3:DeleteObject",
          "s3:PutObject",
        ]
        Resource = "${aws_s3_bucket.egress_deny_guard.arn}/${local.egress_deny_guard_artifact_prefix}*"
      },
      {
        Sid    = "GuardStack"
        Effect = "Allow"
        Action = [
          "cloudformation:CreateChangeSet",
          "cloudformation:CreateStack",
          "cloudformation:DeleteChangeSet",
          "cloudformation:DeleteStack",
          "cloudformation:DescribeChangeSet",
          "cloudformation:DescribeStackEvents",
          "cloudformation:DescribeStackResource",
          "cloudformation:DescribeStackResources",
          "cloudformation:DescribeStacks",
          "cloudformation:ExecuteChangeSet",
          "cloudformation:GetTemplate",
          "cloudformation:ListStackResources",
          "cloudformation:UpdateStack",
        ]
        Resource = [
          local.egress_deny_guard_stack_arn,
          "arn:${data.aws_partition.target.partition}:cloudformation:${var.target_region}:${data.aws_caller_identity.target.account_id}:changeSet/*/*",
          "arn:${data.aws_partition.target.partition}:cloudformation:${var.target_region}:aws:transform/LanguageExtensions",
        ]
      },
      {
        Sid    = "ValidateGuardTemplate"
        Effect = "Allow"
        Action = [
          "cloudformation:GetTemplateSummary",
          "cloudformation:ValidateTemplate",
        ]
        Resource = "*"
      },
      {
        Sid    = "GuardNetwork"
        Effect = "Allow"
        Action = [
          "ec2:AuthorizeSecurityGroupEgress",
          "ec2:CreateSecurityGroup",
          "ec2:CreateTags",
          "ec2:DeleteSecurityGroup",
          "ec2:DeleteTags",
          "ec2:DescribeNetworkAcls",
          "ec2:DescribeNetworkInterfaces",
          "ec2:DescribeSecurityGroups",
          "ec2:DescribeSubnets",
          "ec2:DescribeTags",
          "ec2:DescribeVpcAttribute",
          "ec2:DescribeVpcs",
          "ec2:RevokeSecurityGroupEgress",
        ]
        Resource = "*"
      },
      {
        Sid      = "ReadVpcQuota"
        Effect   = "Allow"
        Action   = "servicequotas:GetServiceQuota"
        Resource = "arn:${data.aws_partition.target.partition}:servicequotas:${var.target_region}:${data.aws_caller_identity.target.account_id}:vpc/L-E79EC296"
      },
      {
        Sid    = "GuardRoles"
        Effect = "Allow"
        Action = [
          "iam:AttachRolePolicy",
          "iam:CreateRole",
          "iam:DeleteRole",
          "iam:DeleteRolePolicy",
          "iam:DetachRolePolicy",
          "iam:GetRole",
          "iam:GetRolePolicy",
          "iam:ListAttachedRolePolicies",
          "iam:ListRolePolicies",
          "iam:PutRolePolicy",
          "iam:TagRole",
          "iam:UntagRole",
          "iam:UpdateAssumeRolePolicy",
        ]
        Resource = local.egress_deny_guard_role_arn
      },
      {
        Sid    = "GuardPolicies"
        Effect = "Allow"
        Action = [
          "iam:CreatePolicy",
          "iam:CreatePolicyVersion",
          "iam:DeletePolicy",
          "iam:DeletePolicyVersion",
          "iam:GetPolicy",
          "iam:GetPolicyVersion",
          "iam:ListEntitiesForPolicy",
          "iam:ListPolicyVersions",
          "iam:TagPolicy",
          "iam:UntagPolicy",
        ]
        Resource = local.egress_deny_guard_policy_arn
      },
      {
        Sid      = "PassGuardRoles"
        Effect   = "Allow"
        Action   = "iam:PassRole"
        Resource = local.egress_deny_guard_role_arn
      },
      {
        Sid      = "CreateNetworkConnectorServiceRole"
        Effect   = "Allow"
        Action   = "iam:CreateServiceLinkedRole"
        Resource = "arn:${data.aws_partition.target.partition}:iam::${data.aws_caller_identity.target.account_id}:role/aws-service-role/network-connectors.lambda.amazonaws.com/AWSServiceRoleForLambdaNetworkConnector"
        Condition = {
          StringEquals = {
            "iam:AWSServiceName" = "network-connectors.lambda.amazonaws.com"
          }
        }
      },
      {
        Sid    = "CreateGuardLambdaResources"
        Effect = "Allow"
        Action = [
          "lambda:CreateMicrovmImage",
          "lambda:CreateNetworkConnector",
        ]
        Resource = "*"
        Condition = {
          StringEquals = {
            "aws:RequestTag/deployment" = local.egress_deny_guard_stack_name
            "aws:RequestTag/managed-by" = "setup"
          }
        }
      },
      {
        Sid      = "TagGuardLambdaResourcesOnCreate"
        Effect   = "Allow"
        Action   = "lambda:TagResource"
        Resource = "*"
        Condition = {
          StringEquals = {
            "aws:RequestTag/deployment" = local.egress_deny_guard_stack_name
            "aws:RequestTag/managed-by" = "setup"
          }
          "ForAllValues:StringEquals" = {
            "aws:TagKeys" = [
              "deployment",
              "managed-by",
              "resource",
              "resource-type",
            ]
          }
        }
      },
      {
        Sid    = "GuardMicrovmImages"
        Effect = "Allow"
        Action = [
          "lambda:CreateMicrovmAuthToken",
          "lambda:DeleteMicrovmImage",
          "lambda:DeleteMicrovmImageVersion",
          "lambda:GetMicrovm",
          "lambda:GetMicrovmImage",
          "lambda:GetMicrovmImageBuild",
          "lambda:GetMicrovmImageVersion",
          "lambda:ListMicrovmImageBuilds",
          "lambda:ListMicrovmImageVersions",
          "lambda:ListTags",
          "lambda:RunMicrovm",
          "lambda:TagResource",
          "lambda:TerminateMicrovm",
          "lambda:UntagResource",
          "lambda:UpdateMicrovmImage",
          "lambda:UpdateMicrovmImageVersion",
        ]
        Resource = local.egress_deny_guard_image_arn
      },
      {
        Sid    = "GuardNetworkConnectors"
        Effect = "Allow"
        Action = [
          "lambda:DeleteNetworkConnector",
          "lambda:GetNetworkConnector",
          "lambda:ListTags",
          "lambda:TagResource",
          "lambda:UntagResource",
          "lambda:UpdateNetworkConnector",
        ]
        Resource = local.egress_deny_guard_connector_arn
        Condition = {
          StringEquals = {
            "aws:ResourceTag/deployment" = local.egress_deny_guard_stack_name
            "aws:ResourceTag/managed-by" = "setup"
          }
        }
      },
      {
        Sid    = "ListGuardLambdaResources"
        Effect = "Allow"
        Action = [
          "lambda:ListMicrovmImages",
          "lambda:ListMicrovms",
          "lambda:ListNetworkConnectors",
          "lambda:PassNetworkConnector",
        ]
        Resource = "*"
      },
    ]
  })
}
