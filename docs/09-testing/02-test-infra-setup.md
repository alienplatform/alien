# Test Infrastructure Setup

All cloud resources needed for integration and E2E tests are provisioned by a single Terraform stack in `tests/infra/`. One `terraform apply` creates everything across AWS, GCP, and Azure — including building and pushing test Docker images.

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

Terraform state is stored in Terraform Cloud (`alienplatform/alien-test-infra` workspace, **execution mode: Local**). `terraform apply` runs locally or in CI — this is required because the Docker provider builds and pushes images from the machine running apply.

## CI

Every cloud test run starts with `terraform apply`. If nothing changed, it completes in ~2-3 minutes (state refresh only). If resources drift or infra changes, they're automatically fixed or updated.

After apply, `./scripts/gen-env-test.sh` generates `.env.test` from the live Terraform outputs. Tests receive credentials this way — there is no manual credential distribution.

As a side effect, `.env.test` is also uploaded to 1Password so engineers can download it for local runs (see below).

Two workflows use this infrastructure:

- **`cloud-tests.yml`** — Rust crate integration tests (`alien-aws-clients`, `alien-gcp-clients`, `alien-azure-clients`, `alien-bindings`). Triggers on PR/push changes to those crates. Wall clock: ~15 min steady state.
- **`e2e-cloud.yml`** — Full app deployment E2E tests. Triggers on `workflow_dispatch`, merge queue, or the `run-cloud-e2e` PR label. Wall clock: ~28 min.

## Local development

```bash
op document get alien-test-env --vault Engineering --output .env.test
```

This downloads the `.env.test` that CI last generated. Run tests normally after this:

```bash
cargo test -p alien-aws-clients --tests
cargo test -p alien-gcp-clients --tests --features gcp
cargo test -p alien-azure-clients --tests
cargo test -p alien-bindings --tests
```

For E2E tests, `source` the file so the env vars are visible to the test process:

```bash
set -a && source .env.test && set +a
pnpm -C tests/e2e test tests/rust-function/aws.test.ts
```

## First-time setup (Alien engineers)

The CI secrets are already configured in GitHub (`alienplatform/alien` repository secrets). For local runs, download `.env.test` from 1Password as described above.

If you need to re-provision the infrastructure from scratch (e.g., after account rotation or first-time setup):

### 1. Add required GitHub secrets

Add these to the `alienplatform/alien` repository secrets (Settings → Secrets → Actions):

```
TF_API_TOKEN                     — Terraform Cloud API token
AXIOM_API_TOKEN                  — Axiom ingest token
OP_SERVICE_ACCOUNT_TOKEN         — 1Password service account (Engineering vault, write)

TEST_AWS_MGMT_ACCESS_KEY_ID      TEST_AWS_TARGET_ACCESS_KEY_ID
TEST_AWS_MGMT_SECRET_ACCESS_KEY  TEST_AWS_TARGET_SECRET_ACCESS_KEY

TEST_GCP_MGMT_SA_KEY             TEST_GCP_TARGET_SA_KEY
TEST_GCP_MGMT_PROJECT_ID (var)   TEST_GCP_TARGET_PROJECT_ID (var)

TEST_AZURE_MGMT_SUBSCRIPTION_ID  TEST_AZURE_TARGET_SUBSCRIPTION_ID
TEST_AZURE_MGMT_TENANT_ID        TEST_AZURE_TARGET_TENANT_ID
TEST_AZURE_MGMT_CLIENT_ID        TEST_AZURE_TARGET_CLIENT_ID
TEST_AZURE_MGMT_CLIENT_SECRET    TEST_AZURE_TARGET_CLIENT_SECRET
```

### 2. Trigger the workflow

```bash
gh workflow run cloud-tests.yml
```

This runs `terraform apply` (first run ~10-20 min due to Docker image builds), generates `.env.test`, and uploads it to 1Password.

## Updating infrastructure

After any change to `tests/infra/`: push to a branch touching those paths and the `cloud-tests.yml` workflow will automatically pick it up, apply changes, and regenerate `.env.test`.
