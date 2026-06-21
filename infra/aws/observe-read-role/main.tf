terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

locals {
  common_tags = merge(var.tags, {
    "alien.dev/component" = "observe-read-role"
  })
}

data "aws_iam_policy_document" "assume" {
  statement {
    sid     = "AllowAlienManagerToObserve"
    actions = ["sts:AssumeRole"]

    principals {
      type        = "AWS"
      identifiers = [var.manager_managing_role_arn]
    }

    condition {
      test     = "StringEquals"
      variable = "sts:ExternalId"
      values   = [var.external_id]
    }
  }
}

data "aws_iam_policy_document" "read" {
  statement {
    sid = "Discovery"
    actions = [
      "tag:GetResources",
      "tag:GetTagKeys",
      "tag:GetTagValues",
    ]
    resources = ["*"]
  }

  statement {
    sid = "HealthMetrics"
    actions = [
      "cloudwatch:GetMetricData",
      "cloudwatch:ListMetrics",
    ]
    resources = ["*"]
  }

  statement {
    sid = "DescribeReadOnly"
    actions = [
      "ec2:Describe*",
      "rds:Describe*",
      "ecs:Describe*",
      "ecs:List*",
      "elasticloadbalancing:Describe*",
      "s3:ListAllMyBuckets",
      "s3:GetBucketLocation",
      "lambda:List*",
      "lambda:GetFunctionConfiguration",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_role" "this" {
  name               = var.name
  assume_role_policy = data.aws_iam_policy_document.assume.json
  tags               = local.common_tags
}

resource "aws_iam_policy" "read" {
  name   = "${var.name}-read"
  policy = data.aws_iam_policy_document.read.json
  tags   = local.common_tags
}

resource "aws_iam_role_policy_attachment" "read" {
  role       = aws_iam_role.this.name
  policy_arn = aws_iam_policy.read.arn
}
