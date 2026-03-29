# Infrastructure

This directory contains Terraform configurations for provisioning cloud infrastructure used by alien-manager.

## standalone/

The `standalone/` stack provisions the cloud resources needed for integration and E2E tests. A single `terraform apply` creates everything across AWS, GCP, and Azure -- including building and pushing test Docker images.

### What it provisions

| Cloud | Resources |
|-------|-----------|
| AWS   | IAM users (management + target), S3 bucket, ECR repository, Lambda execution role, ECR push/pull roles, Lambda test image (linux/arm64) |
| GCP   | Service accounts (management + target), GCS bucket, Artifact Registry, Cloud Run test image (linux/amd64) |
| Azure | Resource group, Storage account + blob container, ACR, Container Apps environment, RBAC assignments, http-server image (linux/amd64) |

### Two-account model

Every cloud provider uses two separate accounts:

- **Management account** -- where alien-manager runs. Owns the test infrastructure: artifact registries, object storage buckets, IAM roles, and service accounts.
- **Target account** -- where workloads are deployed (simulates the customer's cloud). Has its own IAM user or service principal with admin access.

These can be the same account for simplicity, or separate for more realistic testing.

### Running Terraform

Terraform state is stored in Terraform Cloud (`alienplatform/alien-test-infra` workspace, execution mode: Local).

```bash
cd infra/standalone
terraform init
terraform apply
```

In CI, `cloud-tests.yml` and `e2e-cloud.yml` both run `terraform apply` automatically. Variables are injected from GitHub secrets.

### Generating .env.test

After applying, generate the `.env.test` file from live Terraform outputs:

```bash
./scripts/gen-env-test.sh
```

This writes `.env.test` at the repo root with credentials and resource identifiers for all three clouds. Tests load this file to get their configuration.

For local development, download the CI-generated `.env.test` from 1Password instead:

```bash
op document get alien-test-env --vault Engineering --output .env.test
```
