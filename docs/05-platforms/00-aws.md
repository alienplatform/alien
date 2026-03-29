# AWS Platform

Platform-specific details for working on AWS controllers.

## Cross-Account Access

AWS uses **AssumeRole** for cross-account access. The managing account has an IAM role; the customer account has a role with a trust policy allowing the managing role to assume it.

```json
{
  "Principal": { "AWS": "arn:aws:iam::111111111111:role/acme-manager" },
  "Action": "sts:AssumeRole"
}
```

## Resource Mapping

| Alien Resource | AWS Service |
|---|---|
| Function | Lambda |
| Container | ECS on EC2 (via Horizon) |
| Storage | S3 |
| KV | DynamoDB |
| Queue | SQS |
| Vault | Secrets Manager |
| Build | CodeBuild |
| ServiceAccount | IAM Role |

## Naming

Resources are prefixed with the stack name: `my-app-data-storage`, `my-app-processor`. S3 bucket names must be globally unique — the prefix handles this.

## Networking

- **Default VPC** exists on all AWS accounts with public subnets only (no private subnets, no NAT)
- **Create mode**: Alien provisions VPC + public/private subnets + NAT Gateway + Elastic IP per AZ
- **BYO VPC**: Customer provides VPC ID + subnet IDs
- Subnet selection determines egress: public subnets auto-assign public IPs, private subnets route through NAT

## Build Targets

Default: `linux-arm64` (Graviton — cheaper and faster for Lambda/ECS)

## Permissions

All permissions go into IAM role inline policies. Stack-level scope uses ARN wildcard patterns (`arn:aws:s3:::my-app-*`). Resource-level scope uses specific ARNs.

## Quirks

- Lambda cold starts: first invocation after idle takes 1-3 seconds. Alien uses arm64 (Graviton) by default for better cold start performance.
- S3 eventual consistency on bucket creation: bucket may not be immediately visible in all regions after `CreateBucket` returns. Controllers add a short delay.
- ECR image pulling: cross-account ECR access requires explicit resource policy on the repository.
- SQS: default visibility timeout is 30 seconds. Alien uses this as the queue lease duration.
- DynamoDB: on-demand billing by default. No capacity planning needed.
