# alien-manager on Amazon ECS

Runs alien-manager on ECS Fargate behind an Application Load Balancer.

What it creates:

- a Fargate service (one task, ARM64) running the manager image
- an encrypted EFS file system for the manager's state (database, keys)
- an ECR repository for release images (`create_artifact_repository`)
- a Secrets Manager secret holding the admin API key
- an internet-facing or internal Application Load Balancer, HTTPS when you pass a certificate
- a CloudWatch log group, security groups and IAM roles

The manager runs as a single task: its state lives on one file system, and deployments of the
service stop the old task before starting the new one.

## Usage

```hcl
module "alien_manager" {
  source = "github.com/alienplatform/alien//infra/aws-ecs-manager"

  vpc_id                   = "vpc-0123456789abcdef0"
  load_balancer_subnet_ids = ["subnet-public-a", "subnet-public-b"]
  task_subnet_ids          = ["subnet-private-a", "subnet-private-b"]

  domain_name     = "manager.example.com"
  certificate_arn = "arn:aws:acm:us-east-1:123456789012:certificate/..."

  # Optional: extra alien-manager.toml sections.
  config = <<-EOT
    [telemetry]
    otlp-endpoint = "https://otlp.example.com"
  EOT
}
```

Then point `domain_name` at the `load_balancer_dns_name` output and connect the CLI:

```bash
terraform output -raw login_command | sh
alien whoami
```

Private task subnets need a NAT gateway or VPC endpoints (ECR, Secrets Manager, CloudWatch Logs,
EFS) so the task can pull its image and reach AWS. For a quick trial in a default VPC, pass the
public subnets as `task_subnet_ids` and set `assign_public_ip = true`.

## HTTPS

Without `certificate_arn` the manager is served over plain HTTP on the load balancer's DNS
name. That is enough to try the CLI (`alien release` pushes over HTTP when the manager URL is
`http://`), but Kubernetes nodes pull images over HTTPS, so customer installs need a
certificate. The load balancer's idle timeout is one hour so that tunnel and log connections
stay open.

## Inputs

| Name | Description | Default |
|------|-------------|---------|
| `name` | Name prefix for every resource | `alien-manager` |
| `vpc_id` | VPC to run the manager in | required |
| `load_balancer_subnet_ids` | Subnets for the load balancer | required |
| `task_subnet_ids` | Subnets for the task and its file system | required |
| `assign_public_ip` | Give the task a public IP | `false` |
| `internal` | Internal load balancer instead of internet-facing | `false` |
| `certificate_arn` | ACM certificate for HTTPS | `null` |
| `domain_name` | Public host name of the manager | load balancer DNS name |
| `allowed_cidr_blocks` | Networks that may reach the manager | `["0.0.0.0/0"]` |
| `image` | Manager image | `ghcr.io/alienplatform/alien-manager:<version>` |
| `cpu` / `memory` | Task size | `1024` / `2048` |
| `config` | Extra `alien-manager.toml` content | `""` |
| `environment` | Extra environment variables | `{}` |
| `create_artifact_repository` | Store release images in ECR | `true` |
| `log_retention_days` | CloudWatch log retention | `30` |
| `tags` | Tags for every resource | `{}` |

## Outputs

| Name | Description |
|------|-------------|
| `url` | Manager URL |
| `login_command` | Connects the CLI, reading the admin key from Secrets Manager |
| `admin_token_secret_arn` | Secret holding the admin API key |
| `artifact_repository_url` | ECR repository for release images |
| `load_balancer_dns_name` | Point `domain_name` here |

## How the manager is configured

The module passes `alien-manager.toml` through the `ALIEN_MANAGER_CONFIG` environment variable
and the admin key through `ALIEN_ADMIN_TOKEN` (from Secrets Manager). The task role grants push
and pull on the release repository; the manager reads it through the ECS task credentials.
