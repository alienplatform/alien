# Test Infrastructure Setup

All cloud resources needed for integration and E2E tests are provisioned by a single Terraform stack in `infra/test/`. One `terraform apply` creates everything across AWS, GCP, and Azure — including building and pushing test Docker images.

> **Note for external contributors:** this infrastructure runs against Alien's internal cloud accounts. If you're contributing from outside the team, you'll need your own cloud accounts and can point the Terraform variables at them.

## Overview

### Two-account model

Every cloud provider uses two accounts:

- **Management account** — where alien-manager runs; owns test infrastructure (registries, buckets, IAM roles)
- **Target account** — where workloads are deployed (the "customer cloud")

These can be the same account for simplicity, or separate for more realistic testing.

### What Terraform provisions

| Cloud | Resources |
|-------|-----------|
| AWS   | IAM users (manager + target), S3 bucket, ECR repository, Lambda execution role, Lambda test image (linux/arm64) |
| GCP   | Service accounts (manager + target), GCS bucket, Artifact Registry, Cloud Run test image (linux/amd64) |
| Azure | Resource group, Storage account + blob container, ACR, Container Apps environment, RBAC assignments, http-server image (linux/amd64) |
| Axiom | `alien-test` dataset |

## Prerequisites

- [Terraform CLI](https://developer.hashicorp.com/terraform/install) + `terraform login` (Terraform Cloud)
- [Docker](https://docs.docker.com/get-docker/) — test images are built locally during `terraform apply`
- Bootstrap credentials for each cloud (set once as TF Cloud workspace variables — see below)

> **TF Cloud execution mode:** `infra/test/` uses Terraform Cloud for state storage only. In the `alien-test-infra` workspace, set **Execution Mode → Local** (Settings → General). This keeps `terraform apply` running on your machine (or in CI), which is required for the Docker image builds.

## First-time setup (Alien engineers)

### 1. Set bootstrap credentials in Terraform Cloud

Log into [app.terraform.io](https://app.terraform.io), open the `alien-test-infra` workspace, and add these as **sensitive** workspace variables:

```
aws_management_access_key_id      aws_target_access_key_id
aws_management_secret_access_key  aws_target_secret_access_key
aws_management_region             aws_target_region

google_management_service_account_key  google_target_service_account_key
google_management_project_id           google_target_project_id
google_management_region               google_target_region

azure_management_subscription_id  azure_target_subscription_id
azure_management_tenant_id        azure_target_tenant_id
azure_management_client_id        azure_target_client_id
azure_management_client_secret    azure_target_client_secret
azure_management_region

axiom_api_token
```

### 2. Apply

```bash
cd infra/test
terraform init
terraform apply
```

### 3. Generate `.env.test` and upload to 1Password

```bash
./scripts/gen-env-test.sh

# First time:
op document create .env.test --title alien-test-env --vault Engineering
# Subsequent updates:
op document edit alien-test-env .env.test --vault Engineering
```

## Day-to-day development

```bash
op document get alien-test-env --vault Engineering --output .env.test
```

## Updating infrastructure

After any change to `infra/test/`:

```bash
cd infra/test && terraform apply
./scripts/gen-env-test.sh
op document edit alien-test-env .env.test --vault Engineering
```

## CI

CI loads `.env.test` from 1Password via a single GitHub secret (`OP_SERVICE_ACCOUNT_TOKEN`). Add it once to repository secrets (Settings → Secrets → Actions); the service account needs read access to the `Engineering` vault.

## Notes

- A future `infra/alien-manager/` will sit alongside `infra/test/` for alien-manager deployment Terraform.
