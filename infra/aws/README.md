# AWS Module

Provisions the AWS resources that alien-manager needs for push-mode deployments. Does **not** deploy compute — run the manager wherever you like and point it at these resources.

## Resources Created

- **Artifact Registry** (optional): ECR repository with lifecycle policy and image scanning
- **Commands Store** (optional): DynamoDB table (pk/sk + TTL) and S3 bucket
- **Impersonation** (optional): IAM role for cross-account access
- **IAM Policies**: Per-feature policies you can attach to your manager's principal

## Usage

```hcl
module "alien_infra" {
  source = "github.com/aliendotdev/alien//infra/aws"

  name          = "my-project"
  principal_arn = "arn:aws:iam::123456789:role/my-manager-role"

  enable_artifact_registry = true
  enable_commands_store    = true
  enable_impersonation     = true

  tags = {
    Environment = "production"
  }
}
```

Use `config_values` to populate your `alien-manager.toml`:

```hcl
output "toml_sections" {
  value = module.alien_infra.config_values
}
```

## Variables

| Name | Description | Type | Default | Required |
|------|-------------|------|---------|----------|
| `name` | Name prefix for all resources | `string` | — | yes |
| `principal_arn` | ARN of the IAM role/user running the manager | `string` | — | yes |
| `enable_artifact_registry` | Create ECR repository | `bool` | `true` | no |
| `enable_commands_store` | Create DynamoDB + S3 for commands | `bool` | `false` | no |
| `enable_impersonation` | Create IAM role for impersonation | `bool` | `false` | no |
| `impersonation_trusted_accounts` | AWS account IDs for impersonation trust | `list(string)` | `[]` | no |
| `tags` | Tags for all resources | `map(string)` | `{}` | no |

## Outputs

| Name | Description |
|------|-------------|
| `config_values` | Structured values for `alien-manager.toml` sections |
| `ecr_repository_url` | ECR repository URL (if enabled) |
| `dynamodb_table_name` | DynamoDB table name (if enabled) |
| `s3_bucket_name` | S3 bucket name (if enabled) |
| `impersonation_role_arn` | Impersonation IAM role ARN (if enabled) |
| `ecr_policy_arn` | IAM policy for ECR access (if enabled) |
| `commands_policy_arn` | IAM policy for DynamoDB + S3 access (if enabled) |
| `assume_impersonation_policy_arn` | IAM policy for AssumeRole on impersonation role (if enabled) |
