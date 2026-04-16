# -----------------------------------------------------------------------------
# Structured config values for alien-manager.toml
# -----------------------------------------------------------------------------

output "config_values" {
  description = "Values for alien-manager.toml sections (artifact-registry, commands, impersonation)"
  value = {
    artifact_registry = var.enable_artifact_registry ? {
      service          = "ecr"
      repositoryPrefix = aws_ecr_repository.artifacts[0].name
    } : null

    commands = var.enable_commands_store ? {
      kv = {
        service   = "dynamodb"
        tableName = aws_dynamodb_table.commands[0].name
        region    = data.aws_region.current.id
      }
      storage = {
        service    = "s3"
        bucketName = aws_s3_bucket.commands[0].id
      }
    } : null

    impersonation = var.enable_impersonation ? {
      service  = "awsiam"
      roleName = aws_iam_role.impersonation[0].name
      roleArn  = aws_iam_role.impersonation[0].arn
    } : null
  }
}

# -----------------------------------------------------------------------------
# Individual resource outputs
# -----------------------------------------------------------------------------

output "ecr_repository_url" {
  description = "URL of the ECR repository (if artifact registry is enabled)"
  value       = var.enable_artifact_registry ? aws_ecr_repository.artifacts[0].repository_url : ""
}

output "dynamodb_table_name" {
  description = "Name of the DynamoDB commands table (if commands store is enabled)"
  value       = var.enable_commands_store ? aws_dynamodb_table.commands[0].name : ""
}

output "s3_bucket_name" {
  description = "Name of the S3 commands bucket (if commands store is enabled)"
  value       = var.enable_commands_store ? aws_s3_bucket.commands[0].id : ""
}

output "impersonation_role_arn" {
  description = "ARN of the impersonation IAM role (if impersonation is enabled)"
  value       = var.enable_impersonation ? aws_iam_role.impersonation[0].arn : ""
}

# -----------------------------------------------------------------------------
# IAM policy ARNs — attach these to whatever principal runs your manager
# -----------------------------------------------------------------------------

output "ecr_policy_arn" {
  description = "ARN of the IAM policy granting ECR access (if artifact registry is enabled)"
  value       = var.enable_artifact_registry ? aws_iam_policy.ecr[0].arn : ""
}

output "commands_policy_arn" {
  description = "ARN of the IAM policy granting DynamoDB + S3 access (if commands store is enabled)"
  value       = var.enable_commands_store ? aws_iam_policy.commands[0].arn : ""
}

output "assume_impersonation_policy_arn" {
  description = "ARN of the IAM policy granting sts:AssumeRole on the impersonation role (if impersonation is enabled)"
  value       = var.enable_impersonation ? aws_iam_policy.assume_impersonation[0].arn : ""
}
