# Infrastructure

Terraform modules for provisioning the cloud resources that alien-manager needs. These modules create artifact registries, commands backends, IAM roles, and related resources — **they do not deploy the manager itself**. You run the manager wherever you like (Docker, Kubernetes, a VM, `alien serve`) and point it at these resources via `alien-manager.toml`.

```
infra/
├── aws/       ECR, DynamoDB + S3, IAM roles
├── gcp/       Artifact Registry, Firestore + GCS, service accounts
├── azure/     ACR, Table + Blob Storage, managed identities
└── test/      E2E test resources (dual-account per cloud)
```

## Modules

Each module creates the supporting resources for one cloud provider. All features are optional and controlled via `enable_*` variables.

| Module | Artifact Registry | Commands Store | Impersonation |
|--------|-------------------|----------------|---------------|
| [`aws/`](aws/) | ECR | DynamoDB + S3 | IAM role |
| [`gcp/`](gcp/) | Artifact Registry | Firestore + GCS | Service account |
| [`azure/`](azure/) | ACR | Table + Blob Storage | Managed identity |

Each module outputs structured `config_values` that map directly to `alien-manager.toml` sections. See each module's README for usage examples.

## Production Deployment

Provision resources for each cloud platform you want to support, then run a single manager that targets all of them.

**1. Create cloud resources** — run the modules for each platform you need:

```bash
cd infra/aws   && terraform init && terraform apply
cd infra/gcp   && terraform init && terraform apply
cd infra/azure && terraform init && terraform apply
```

**2. Generate and configure `alien-manager.toml`** — each module outputs `config_values` for its platform's sections:

```bash
alien serve --init
```

**3. Run the manager** — a single process handles all platforms:

```bash
docker run -d \
  -p 8080:8080 \
  -v alien-data:/data \
  -v ./alien-manager.toml:/app/alien-manager.toml \
  -e BASE_URL=https://manager.example.com \
  ghcr.io/alienplatform/alien-manager
```

See the [Self-Hosting Guide](https://alien.dev/docs/self-hosting) for the full configuration reference and production checklist.

## test/

The `test/` stack provisions the cloud resources needed for integration and E2E tests. A single `terraform apply` creates everything across AWS, GCP, and Azure — including building and pushing test Docker images.

### Relationship to the modules

The modules (`aws/`, `gcp/`, `azure/`) provision supporting resources. The test module (`test/`) creates similar resources plus dual-account infrastructure (management + target) that tests need. Tests run the manager as a local process via `TestManager`.

### What it provisions

| Cloud | Resources |
|-------|-----------|
| AWS   | IAM users (management + target), S3 bucket, DynamoDB table, ECR repository, Lambda execution role, ECR push/pull roles, Lambda test image (linux/arm64) |
| GCP   | Service accounts (management + target), GCS bucket, Artifact Registry, Cloud Run test image (linux/amd64) |
| Azure | Resource group, Storage account + blob container, ACR, Container Apps environment, RBAC assignments, http-server image (linux/amd64) |

### Two-account model

Every cloud provider uses two separate accounts:

- **Management account** — where alien-manager runs. Owns the test infrastructure: artifact registries, object storage buckets, IAM roles, and service accounts.
- **Target account** — where workloads are deployed (simulates the customer's cloud). Has its own IAM user or service principal with admin access.

These can be the same account for simplicity, or separate for more realistic testing.

### Running Terraform

Terraform state is stored in Terraform Cloud (`alienplatform/alien-test-infra` workspace, execution mode: Local).

```bash
cd infra/test
terraform init
terraform apply
```

In CI, `cloud-tests.yml` and `e2e-cloud.yml` both run `terraform apply` automatically. Variables are injected from GitHub secrets.

### Generating .env.test and alien-manager.test.toml

After applying, generate the test configuration files from live Terraform outputs:

```bash
./scripts/gen-env-test.sh
```

This writes two files at the repo root:

- **`.env.test`** — credentials and resource identifiers for all three clouds. Tests load this file to get their configuration.
- **`alien-manager.test.toml`** — a manager config using the AWS test resources. This validates that the TOML format matches real infrastructure.

For local development, download the CI-generated `.env.test` from 1Password instead:

```bash
op document get alien-test-env --vault <your-vault-name> --output .env.test
```

### Validating the config chain

To verify the full chain (Terraform -> gen-env-test.sh -> alien-manager.toml -> working manager):

```bash
./scripts/validate-test-config.sh
```

This sources `.env.test`, starts alien-manager with `alien-manager.test.toml`, checks that `/health` returns 200, and shuts down. It catches config drift between the TOML format and the test infrastructure early.
