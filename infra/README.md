# Infrastructure

Two kinds of Terraform and Helm packages live here:

- **Manager deployments** run alien-manager itself: [`helm/alien-manager/`](helm/alien-manager/) on Kubernetes and [`aws-ecs-manager/`](aws-ecs-manager/) on Amazon ECS. You can also run the image with `docker run` (below).
- **Cloud modules** (`aws/`, `gcp/`, `azure/`) create the cloud resources a manager needs to deploy into those clouds: artifact registries, commands backends and IAM roles. Point the manager at them via `alien-manager.toml`.

```
infra/
├── helm/alien-manager/  Helm chart for the manager
├── aws-ecs-manager/     The manager on ECS Fargate (ALB, EFS, ECR, Secrets Manager)
├── aws/                 ECR, DynamoDB + S3, IAM roles
├── gcp/                 Artifact Registry, Firestore + GCS, service accounts
├── azure/               ACR, Table + Blob Storage, managed identities
└── test/                E2E test resources (dual-account per cloud)
```

## Running the manager

| Where | How |
|-------|-----|
| Any machine with Docker | `docker run` (below) |
| Kubernetes | `helm install alien-manager oci://ghcr.io/alienplatform/charts/alien-manager` or [`helm/alien-manager/`](helm/alien-manager/) |
| Amazon ECS | [`aws-ecs-manager/`](aws-ecs-manager/) |

The manager keeps its state (database, keys, and release images unless an external registry is configured) in `STATE_DIR`, `/data` in the image. Mount a volume there. On first start it prints an admin API key; set `ALIEN_ADMIN_TOKEN` to supply your own from a secret store instead.

## Cloud modules

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
| AWS   | OIDC provider and one CI role per account (management + target), S3 bucket, DynamoDB table, ECR repository, Lambda execution role, ECR push/pull roles, Lambda test image (linux/arm64) |
| GCP   | Service accounts (management + target), workload identity pool and GitHub provider, GCS bucket, Artifact Registry, Cloud Run test image (linux/amd64) |
| Azure | Resource group, Storage account + blob container, ACR, Container Apps environment, RBAC assignments, federated credentials on the two test app registrations, http-server image (linux/amd64) |

### Two-account model

Every cloud provider uses two separate accounts:

- **Management account** — where alien-manager runs. Owns the test infrastructure: artifact registries, object storage buckets, IAM roles, and service accounts.
- **Target account** — where workloads are deployed (simulates the customer's cloud). Has its own CI role, service account or app registration with admin access in that account only.

These can be the same account for simplicity, or separate for more realistic testing.

### Running Terraform

Terraform state is stored in Terraform Cloud (`alienplatform/alien-test-infra` workspace, execution mode: Local).

`infra/test` is applied by a person, never by CI. It holds the trust that lets
GitHub Actions sign in to the test accounts (an OIDC provider and role per AWS
account, a workload identity pool, federated credentials on the Azure app
registrations), so the identities CI signs in with cannot change it; a deny
policy and an IAM deny statement enforce that. Sign in with SSO first, then apply:

```bash
aws sso login --profile alien-test-mgmt
aws sso login --profile alien-test-target
gcloud auth application-default login
az login
cd infra/test
terraform init
terraform apply
```

No provider takes a static credential: AWS uses the `alien-test-mgmt` and
`alien-test-target` profiles, Google and Azure use your own sign-in. The person
applying needs admin rights in the management and target accounts and projects.
Set the project, subscription and tenant variables (see `variables.tf`) in your
shell or a local `*.auto.tfvars` file.

Terraform keeps no long-lived keys: there are no IAM users, access keys, service
account keys or client secrets in the state. After an apply, check that the Azure
test app registrations list no password or certificate credentials:

```bash
for app in "$AZURE_MANAGEMENT_CLIENT_ID" "$AZURE_TARGET_CLIENT_ID"; do
  az ad app credential list --id "$app" --query '[].keyId' -o tsv
done   # expect no output
```

### Moving to a new GCP target project

`infra/test` does not create GCP projects. To move tests to a fresh target
project, create the project manually and link billing, then set
`google_target_project_id` (or the matching `_1` / `_3` variable) and apply with
your own sign-in. Remove the old target-project resources from state first. Run
`./scripts/gen-env-test.sh` afterwards to regenerate `.env.test`.

### Generating .env.test and alien-manager.test.toml

After applying, generate the test configuration files from live Terraform outputs:

```bash
./scripts/gen-env-test.sh
```

This writes two files at the repo root:

- **`.env.test`** — configuration only (account ids, role ARNs, project ids, service account emails, resource names) for all three clouds. It holds no credentials; the script fails if anything credential-like ends up in it. Tests get credentials from the process environment (see "Run cloud tests locally" below).
- **`alien-manager.test.toml`** — a manager config using the AWS test resources. This validates that the TOML format matches real infrastructure.

### Run cloud tests locally

1. Sign in: `aws sso login --profile alien-test-mgmt`, `aws sso login --profile alien-test-target`,
   `gcloud auth application-default login`, `az login`.
2. `./scripts/gen-env-test.sh` writes `.env.test` (config only).
3. `eval "$(./scripts/cloud-test-credentials.sh)"` puts short-lived AWS credentials for the two
   accounts into your shell (`AWS_MANAGEMENT_*`, `AWS_TARGET_*`, with session tokens). Nothing secret
   goes in `.env.test`. Google tests impersonate the test service accounts named in `.env.test` through
   your Application Default Credentials; Azure tests use your `az login` tokens.
4. Run a crate's tests, for example `cargo test -p alien-aws-clients --test aws_s3_client_tests`.
   Tests that need a third-party key (OpenAI, Anthropic, Databricks, Axiom, ngrok) skip with a message unless you export it.
   A few Azure tests start providers that need the CI federated token file and only run in CI.

In CI, `cloud-tests.yml` exports short-lived credentials for the same names from GitHub OIDC.

### Validating the config chain

To verify the full chain (Terraform -> gen-env-test.sh -> alien-manager.toml -> working manager):

```bash
./scripts/validate-test-config.sh
```

This sources `.env.test`, starts alien-manager with `alien-manager.test.toml`, checks that `/health` returns 200, and shuts down. It catches config drift between the TOML format and the test infrastructure early.
