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

resource "aws_iam_user" "manager" {
  provider = aws.management
  name     = "alien-test-manager"
}

resource "aws_iam_access_key" "manager" {
  provider = aws.management
  user     = aws_iam_user.manager.name
}

resource "aws_iam_user_policy_attachment" "manager_ecr" {
  provider   = aws.management
  user       = aws_iam_user.manager.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryFullAccess"
}

resource "aws_iam_user_policy_attachment" "manager_s3" {
  provider   = aws.management
  user       = aws_iam_user.manager.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonS3FullAccess"
}

resource "aws_iam_user_policy_attachment" "manager_lambda" {
  provider   = aws.management
  user       = aws_iam_user.manager.name
  policy_arn = "arn:aws:iam::aws:policy/AWSLambda_FullAccess"
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

# ── Target: IAM user ──────────────────────────────────────────────────────────

resource "aws_iam_user" "target" {
  provider = aws.target
  name     = "alien-test-target"
}

resource "aws_iam_access_key" "target" {
  provider = aws.target
  user     = aws_iam_user.target.name
}

resource "aws_iam_user_policy_attachment" "target_lambda" {
  provider   = aws.target
  user       = aws_iam_user.target.name
  policy_arn = "arn:aws:iam::aws:policy/AWSLambda_FullAccess"
}

data "aws_caller_identity" "target" {
  provider = aws.target
}

