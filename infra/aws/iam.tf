# -----------------------------------------------------------------------------
# IAM policies for the manager principal.
# Attach these to whatever IAM role/user runs your manager.
# -----------------------------------------------------------------------------

# -- ECR access ---------------------------------------------------------------

data "aws_iam_policy_document" "ecr" {
  count = var.enable_artifact_registry ? 1 : 0

  statement {
    actions = [
      "ecr:GetDownloadUrlForLayer",
      "ecr:BatchGetImage",
      "ecr:BatchCheckLayerAvailability",
      "ecr:PutImage",
      "ecr:InitiateLayerUpload",
      "ecr:UploadLayerPart",
      "ecr:CompleteLayerUpload",
      "ecr:DescribeRepositories",
      "ecr:ListImages",
    ]
    resources = [aws_ecr_repository.artifacts[0].arn]
  }

  statement {
    actions   = ["ecr:GetAuthorizationToken"]
    resources = ["*"]
  }
}

resource "aws_iam_policy" "ecr" {
  count = var.enable_artifact_registry ? 1 : 0

  name   = "${var.name}-manager-ecr"
  policy = data.aws_iam_policy_document.ecr[0].json
  tags   = local.common_tags
}

# -- Commands store access (DynamoDB + S3) ------------------------------------

data "aws_iam_policy_document" "commands" {
  count = var.enable_commands_store ? 1 : 0

  statement {
    actions = [
      "dynamodb:GetItem",
      "dynamodb:PutItem",
      "dynamodb:UpdateItem",
      "dynamodb:DeleteItem",
      "dynamodb:Query",
      "dynamodb:Scan",
    ]
    resources = [
      aws_dynamodb_table.commands[0].arn,
      "${aws_dynamodb_table.commands[0].arn}/index/*",
    ]
  }

  statement {
    actions = [
      "s3:GetObject",
      "s3:PutObject",
      "s3:DeleteObject",
      "s3:ListBucket",
    ]
    resources = [
      aws_s3_bucket.commands[0].arn,
      "${aws_s3_bucket.commands[0].arn}/*",
    ]
  }
}

resource "aws_iam_policy" "commands" {
  count = var.enable_commands_store ? 1 : 0

  name   = "${var.name}-manager-commands"
  policy = data.aws_iam_policy_document.commands[0].json
  tags   = local.common_tags
}

# -- Impersonation (STS AssumeRole) ------------------------------------------

data "aws_iam_policy_document" "assume_impersonation" {
  count = var.enable_impersonation ? 1 : 0

  statement {
    actions   = ["sts:AssumeRole"]
    resources = [aws_iam_role.impersonation[0].arn]
  }
}

resource "aws_iam_policy" "assume_impersonation" {
  count = var.enable_impersonation ? 1 : 0

  name   = "${var.name}-manager-assume-impersonation"
  policy = data.aws_iam_policy_document.assume_impersonation[0].json
  tags   = local.common_tags
}
