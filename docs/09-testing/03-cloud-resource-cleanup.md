# Cloud Test Resource Cleanup

This guide explains how to inspect and clean up cloud resources left behind by Alien's cloud tests.

It is intentionally split into:

1. **Terraform-owned baseline infrastructure** — keep this unless you are intentionally rebuilding the shared test environment
2. **Per-run test leftovers** — these are the resources that failed test teardown usually leaves behind

## First principle

Do not start by deleting everything named `alien-test`.

Do not manually delete anything managed by `infra/standalone` during routine cleanup.

There are three different classes of resources in these accounts:

- **Shared baseline infra** created by `infra/standalone`
- **Standalone manager E2E leftovers** created by `crates/alien-test`
- **Other cloud/integration test leftovers** created by crate-level cloud tests

The safe workflow is always:

1. List resources
2. Protect everything owned by `infra/standalone`
3. Separate baseline from leftovers
4. Delete only the leftovers
5. Re-list to confirm the account returned to the expected baseline

## Hard rule: protect `infra/standalone`

Before deleting anything, check Terraform state:

```bash
cd infra/standalone
terraform state list
```

Treat every resource in that state as protected baseline infrastructure.

Routine test cleanup must not delete Terraform-managed resources from `infra/standalone`. If a baseline resource drifted or must be rebuilt, do that intentionally through Terraform, not as ad hoc test cleanup.

## Credentials for inspection

The `.env.test` credentials (e.g. `alien-test-manager` IAM user) have limited permissions and **cannot list most resource types**. For cleanup inspection you need admin access:

- **AWS:** use the `alien-test-mgmt` SSO profile (requires `aws sso login --profile alien-test-mgmt`)
- **GCP:** the `.env.test` service account key works (it has project-level read access)
- **Azure:** the `.env.test` service principal works (it has subscription-level read access)

All inspection commands below use admin credentials where needed.

## Important current behavior

The current `alien-test` standalone harness does **not** actually deploy into the configured target accounts.

- `TestManager::inject_credential_env_vars()` injects only management credentials
- `setup_target()` is still a stub
- `setup_target()` is not called by the E2E flow

Today that means:

- **AWS leftovers from `alien-test` land in the management account**
- **GCP leftovers from `alien-test` land in the management project**
- **Azure leftovers from `alien-test` land in the management subscription**

The target accounts still exist in `.env.test`, but with the current code they are mostly useful as future wiring and for non-`alien-test` workflows.

## Shared baseline infrastructure

These resources are expected and should **never be deleted** during cleanup.

Some are Terraform-managed (by `infra/standalone`), others are manually provisioned bootstrap resources. Deleting any of them breaks test infrastructure, CI, and may require key regeneration.

### AWS management account

- S3 bucket from `.env.test`: `ALIEN_TEST_AWS_S3_BUCKET`
- ECR repo from `.env.test`: `ALIEN_TEST_AWS_ECR_REPOSITORY`
- IAM role: `alien-test-lambda-execution`
- IAM role: `alien-test-ecr-push`
- IAM role: `alien-test-ecr-pull`
- IAM role: `alien-test-management`
- IAM user: `alien-test-manager`

### AWS target account

- IAM user: `alien-test-target`

### GCP management project

- GCS bucket from `.env.test`: `ALIEN_TEST_GCP_GCS_BUCKET`
- Artifact Registry repo from `.env.test`: `alien-test`
- Artifact Registry repo: `test-alien-artifact-registry` (shared by Rust function E2E)
- Firestore database: `(default)`
- Service account: `alien-test-manager@alien-test-mgmt.iam.gserviceaccount.com` (Terraform-managed, key in `.env.test`)
- Service account: `alien-test-management@alien-test-mgmt.iam.gserviceaccount.com` (Terraform-managed)
- Service account: `alien-test-ar-pull@alien-test-mgmt.iam.gserviceaccount.com` (Terraform-managed)
- Service account: `alien-test-ar-push@alien-test-mgmt.iam.gserviceaccount.com` (Terraform-managed)
- Service account: `alien-terraform-bootstrap@alien-test-mgmt.iam.gserviceaccount.com` (manually provisioned)
- Service account: `450988722957-compute@developer.gserviceaccount.com` (GCP default)

### GCP target project

- Service account: `alien-test-target@alien-test-target.iam.gserviceaccount.com` (Terraform-managed, key in `.env.test`)
- Service account: `alien-terraform-bootstrap@alien-test-target.iam.gserviceaccount.com` (manually provisioned)
- Service account: `888843052873-compute@developer.gserviceaccount.com` (GCP default)
- Service account: `iam-condition-test@alien-test-target.iam.gserviceaccount.com`
- Service account: `test-custom-sa@alien-test-target.iam.gserviceaccount.com`
- Service account: `horizon-cloud-tests@alien-test-target.iam.gserviceaccount.com`

### Azure management subscription

- Resource group from `.env.test`: `ALIEN_TEST_AZURE_RESOURCE_GROUP`
- Storage account from `.env.test`: `ALIEN_TEST_AZURE_STORAGE_ACCOUNT`
- Blob container from `.env.test`: `ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER`
- Container Apps environment from `.env.test`: `ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME`
- ACR from `.env.test`: `ALIEN_TEST_AZURE_REGISTRY_NAME`

### Azure target subscription

- Resource group: `alien-test-target`
- Resource group: `horizon2-test-rg`

If a resource matches the baseline above, it is not a cleanup candidate.

## Leftover naming patterns

These patterns are the main signal that a resource came from a test run and not from Terraform baseline infra.

### AWS

Standalone E2E resources usually use a random stack prefix plus the logical resource id:

- Lambda: `{prefix}-alien-rs-fn`, `{prefix}-alien-ts-fn`
- DynamoDB: `{prefix}-alien-kv`
- SQS: `{prefix}-alien-queue`
- IAM role: `{prefix}-execution-sa`
- IAM role: `{prefix}-build-execution-sa`
- IAM role: `{prefix}-test-alien-artifact-registry-pull`
- IAM role: `{prefix}-test-alien-artifact-registry-push`

Other AWS cloud tests also leave resources with broader test prefixes:

- Lambda: `alien-test-function-*`
- DynamoDB: `alien-test-kv-*`
- IAM role: `alien-test-role-*`
- IAM role: `alien-test-build-role-*`
- SSM parameters: `alien-test-vault-*`
- Extra S3 buckets: `alien-test-*` (check against `.env.test` baseline bucket)

### GCP

Standalone E2E leftovers usually look like:

- Cloud Run service: `{prefix}-alien-rs-fn`, `{prefix}-alien-ts-fn`
- GCS bucket: `{prefix}-alien-storage`
- Pub/Sub topic: `{prefix}-alien-queue`
- Pub/Sub subscription: `{prefix}-alien-queue-sub`
- Service account: `{prefix}-execution-sa@...`
- Service account: `{prefix}-build-execution-sa@...`
- Secret Manager secret: `{prefix}-secrets-ALIEN_COMMANDS_TOKEN`

Other GCP cloud tests also leave resources with broader test prefixes:

- Cloud Run service: `alien-test-svc-*`
- Firestore database: `alien-test-kv-db-*`
- Artifact Registry repo: `alien-test-repo-*`
- Secret Manager secret: `alien-test-vault-*`
- Stale project IAM bindings: bindings referencing `deleted:serviceAccount:` entries (service accounts from old test runs that were deleted but whose IAM bindings remain)

### Azure

Standalone E2E leftovers come in two forms:

**Separate resource groups** (each deployment gets its own):

- Resource group: `{prefix}-default-resource-group`
- Container Apps environment: `{prefix}-default-container-env`
- Container App: `{prefix}-alien-rs-fn`, `{prefix}-alien-ts-fn`
- Managed identity: `{prefix}-execution-sa`
- Managed identity: `{prefix}-build-execution-sa`
- Key Vault: `{prefix}-alien-vault`
- Service Bus namespace: `{prefix}-default-service-bus-namespace`
- Storage account: derived from `{prefix}-default-storage-account`, truncated to Azure limits

Rust function E2E may also create an ACR for the Artifact Registry binding inside the same per-run resource group.

**Resources inside the baseline resource group** (cloud tests that reuse the shared environment):

- Container App: `alien-test-app-{8-char-hex}`
- Container App Job: `build-test-pre-{number}`
- Key Vault: `alientest{8-char-hex}`
- Managed Identity: `alien-test-app-{8-char-hex}-identity`
- Virtual Network: `alien-vnet-{8-char-hex}`
- Disk: `alien-disk-{8-char-hex}`

These accumulate inside the baseline RG and are easy to miss if you only look for separate leftover resource groups.

## Inspection workflow

Start from the repo root:

```bash
set -a && source .env.test && set +a
```

### AWS

The `.env.test` IAM user (`alien-test-manager`) lacks List permissions for most services. Use the admin SSO profile instead:

```bash
aws sso login --profile alien-test-mgmt

aws --profile alien-test-mgmt --region us-east-1 sts get-caller-identity
```

List likely leftovers:

```bash
P="--profile alien-test-mgmt --region us-east-1"

# Lambda (standalone E2E + cloud tests)
aws $P lambda list-functions --query 'Functions[].FunctionName' --output json \
  | jq -r '.[] | select(test("alien-(rs|ts)-fn$") or test("^alien-test-function-"))'

# DynamoDB
aws $P dynamodb list-tables --output json \
  | jq -r '.TableNames[] | select(test("alien-kv$") or startswith("alien-test-kv-"))'

# SQS
aws $P sqs list-queues --output json \
  | jq -r '.QueueUrls[]? | split("/")[-1] | select(test("alien-queue$"))'

# IAM roles
aws $P iam list-roles --output json \
  | jq -r '.Roles[] | .RoleName | select(
      test("execution-sa$") or test("build-execution-sa$")
      or test("test-alien-artifact-registry-(pull|push)$")
      or startswith("alien-test-role-") or startswith("alien-test-build-role-")
    )'

# SSM parameters
aws $P ssm describe-parameters --output json \
  | jq -r '.Parameters[] | .Name | select(
      startswith("alien-test-vault-") or test("secrets-ALIEN_COMMANDS_TOKEN$")
    )'

# S3 buckets (compare against ALIEN_TEST_AWS_S3_BUCKET to identify extras)
aws $P s3api list-buckets --output json \
  | jq -r '.Buckets[].Name | select(startswith("alien-test"))'
```

### GCP

Use a temporary Cloud SDK config so you do not pollute local auth state:

```bash
MGMT_KEY=$(mktemp)
printf '%s' "$GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY" > "$MGMT_KEY"
CFG=$(mktemp -d)
CLOUDSDK_CONFIG="$CFG" gcloud auth activate-service-account --key-file="$MGMT_KEY"
```

List likely leftovers:

```bash
G="CLOUDSDK_CONFIG=$CFG"
PROJECT="$GOOGLE_MANAGEMENT_PROJECT_ID"

# Cloud Run services
env $G gcloud run services list --project="$PROJECT" --platform=managed --format='value(metadata.name)'

# GCS buckets (compare against ALIEN_TEST_GCP_GCS_BUCKET)
env $G gcloud storage buckets list --project="$PROJECT" --format='value(name)'

# Pub/Sub topics and subscriptions
env $G gcloud pubsub topics list --project="$PROJECT" --format='value(name)'
env $G gcloud pubsub subscriptions list --project="$PROJECT" --format='value(name)'

# Service accounts
env $G gcloud iam service-accounts list --project="$PROJECT" --format='value(email)'

# Secret Manager secrets
env $G gcloud secrets list --project="$PROJECT" --format='value(name)'

# Firestore databases (look for alien-test-kv-db-* leftovers, keep (default))
env $G gcloud firestore databases list --project="$PROJECT" --format='value(name)'

# Artifact Registry repos (keep alien-test and test-alien-artifact-registry)
env $G gcloud artifacts repositories list --project="$PROJECT" --format='value(name)'

# Stale IAM bindings — deleted service accounts that still have role bindings
env $G gcloud projects get-iam-policy "$PROJECT" --format=json \
  | jq -r '[.bindings[].members[] | select(startswith("deleted:serviceAccount:"))] | unique | .[]'
```

Cleanup temp auth:

```bash
rm -f "$MGMT_KEY"
rm -rf "$CFG"
```

### Azure

Azure leftovers appear in two places: as separate leftover resource groups, and as resources **inside** the baseline resource group.

```bash
AZ_CFG=$(mktemp -d)
AZURE_CONFIG_DIR="$AZ_CFG" az login \
  --service-principal \
  -u "$AZURE_MANAGEMENT_CLIENT_ID" \
  -p "$AZURE_MANAGEMENT_CLIENT_SECRET" \
  --tenant "$AZURE_MANAGEMENT_TENANT_ID"
AZURE_CONFIG_DIR="$AZ_CFG" az account set --subscription "$AZURE_MANAGEMENT_SUBSCRIPTION_ID"
```

List separate leftover resource groups (standalone E2E):

```bash
AZURE_CONFIG_DIR="$AZ_CFG" az group list -o json \
  | jq -r '.[].name | select(test("^[a-z][0-9a-f]{7}-default-resource-group$"))'
```

List leftovers **inside** the baseline resource group:

```bash
AZ="AZURE_CONFIG_DIR=$AZ_CFG"
RG="$ALIEN_TEST_AZURE_RESOURCE_GROUP"

# Container Apps (alien-test-app-{hex})
env $AZ az containerapp list -o json \
  | jq -r '.[] | select(.resourceGroup == "'"$RG"'") | .name' \
  | grep -E '^alien-test-app-[0-9a-f]{8}$'

# Container App Jobs (build-test-pre-{number})
env $AZ az resource list --resource-group "$RG" -o json \
  | jq -r '.[] | select(.type == "Microsoft.App/jobs") | .name'

# Key Vaults (alientest{hex})
env $AZ az resource list --resource-group "$RG" -o json \
  | jq -r '.[] | select(.type == "Microsoft.KeyVault/vaults") | .name'

# Managed Identities (alien-test-app-{hex}-identity)
env $AZ az resource list --resource-group "$RG" -o json \
  | jq -r '.[] | select(.type == "Microsoft.ManagedIdentity/userAssignedIdentities") | .name'

# Virtual Networks (alien-vnet-{hex})
env $AZ az resource list --resource-group "$RG" -o json \
  | jq -r '.[] | select(.type == "Microsoft.Network/virtualNetworks") | .name'

# Disks (alien-disk-{hex})
env $AZ az resource list --resource-group "$RG" -o json \
  | jq -r '.[] | select(.type == "Microsoft.Compute/disks") | .name'
```

Cleanup temp auth:

```bash
rm -rf "$AZ_CFG"
```

## Cleanup workflow

Only do this after you have reviewed the listing output.

**Critical safety rule:** Only delete resources whose names match a test-run prefix pattern (8-character hex like `f0953933-*` or a UUID suffix like `alien-test-kv-01fb7ffe3807`). If a resource name does not contain a random test-run prefix, it is almost certainly baseline infrastructure. When in doubt, do not delete it.

**How to identify test-run leftovers vs baseline:**

- Test-run resources always have a random prefix: `{8-char-hex}-alien-*`, `alien-test-kv-{12-char-hex}`, `alien-test-role-{uuid}`, `alien-test-vault-{uuid}`, `{prefix}-secrets-ALIEN_COMMANDS_TOKEN`
- Baseline resources have stable, human-readable names: `alien-test-manager`, `alien-test-ecr-push`, `alien-terraform-bootstrap`, etc.
- **Never use bulk `delete-all` or `list | delete` without filtering.** Always filter by the leftover naming patterns below.

### AWS cleanup

Delete higher-level compute resources first, then stateful resources, then IAM and vault leftovers:

1. Delete Lambda functions matching `*-alien-rs-fn`, `*-alien-ts-fn`, and `alien-test-function-*`
2. Delete SQS queues matching `*-alien-queue`
3. Delete DynamoDB tables matching `*-alien-kv` and `alien-test-kv-*`
4. Delete SSM parameters matching `*-secrets-ALIEN_COMMANDS_TOKEN` and `alien-test-vault-*`
5. Delete IAM roles matching:
   - `{prefix}-execution-sa` (where prefix is a random test-run hex)
   - `{prefix}-build-execution-sa`
   - `{prefix}-management` (where prefix is a random test-run hex, NOT `alien-test-management`)
   - `{prefix}-test-alien-artifact-registry-pull`
   - `{prefix}-test-alien-artifact-registry-push`
   - `alien-test-role-*`
   - `alien-test-build-role-*`
6. Re-check S3 buckets and only remove extra `alien-test-*` buckets that are not the current baseline bucket from `.env.test`

Never delete:

- the current baseline S3 bucket from `.env.test`
- the baseline ECR repo from `.env.test`
- the baseline `alien-test-lambda-execution`, `alien-test-ecr-push`, `alien-test-ecr-pull`, or `alien-test-management` roles
- the baseline `alien-test-manager` IAM user
- any `AWSServiceRole*`, `AWSReservedSSO*`, or `OrganizationAccountAccessRole` roles
- `horizon-cloud-tests` or `horizon-test-role` roles (owned by the horizon repo)

### GCP cleanup

Delete in this order:

1. Cloud Run services with test-run prefixes (`{prefix}-alien-*-fn`, `alien-test-svc-*`)
2. Pub/Sub subscriptions with test-run prefixes
3. Pub/Sub topics with test-run prefixes
4. Secret Manager secrets matching `{prefix}-secrets-ALIEN_COMMANDS_TOKEN` and `alien-test-vault-*`
5. Leftover Firestore databases matching `alien-test-kv-db-*`
6. Leftover Artifact Registry repos matching `alien-test-repo-*`
7. Leftover GCS buckets with test-run prefixes
8. Per-run service accounts with test-run prefixes (`{prefix}-*@...`)
9. Scrub stale IAM bindings referencing `deleted:serviceAccount:` members — these are bindings for service accounts that were already deleted but whose policy entries remain

**Never delete these service accounts (management project):**

- `alien-test-manager@alien-test-mgmt.iam.gserviceaccount.com`
- `alien-test-management@alien-test-mgmt.iam.gserviceaccount.com`
- `alien-test-ar-pull@alien-test-mgmt.iam.gserviceaccount.com`
- `alien-test-ar-push@alien-test-mgmt.iam.gserviceaccount.com`
- `alien-terraform-bootstrap@alien-test-mgmt.iam.gserviceaccount.com`
- `450988722957-compute@developer.gserviceaccount.com`

**Never delete these service accounts (target project):**

- `alien-test-target@alien-test-target.iam.gserviceaccount.com`
- `alien-terraform-bootstrap@alien-test-target.iam.gserviceaccount.com`
- `888843052873-compute@developer.gserviceaccount.com`
- `iam-condition-test@alien-test-target.iam.gserviceaccount.com`
- `test-custom-sa@alien-test-target.iam.gserviceaccount.com`
- `horizon-cloud-tests@alien-test-target.iam.gserviceaccount.com`

Also keep:

- The baseline bucket from `.env.test`
- The baseline `alien-test` Artifact Registry repo
- The `test-alien-artifact-registry` repo (shared by Rust function E2E, only delete if you know no current run needs it)
- The default Firestore database

### Azure cleanup

Azure leftovers appear in two places.

**Separate leftover resource groups** (standalone E2E):

1. Identify each `{prefix}-default-resource-group` (prefix matches `^[a-z][0-9a-f]{7}$`)
2. Inspect the resources in that group
3. Delete the whole resource group if it is clearly from a failed test run

**Resources inside the baseline resource group** (cloud tests):

These are the most common leftovers and are easy to miss. Delete in this order:

1. Container Apps matching `alien-test-app-{8-char-hex}`
2. Container App Jobs matching `build-test-pre-{number}`
3. Key Vaults matching `alientest{8-char-hex}` — note: Azure soft-deletes vaults, so you may also need to purge them
4. Managed Identities matching `alien-test-app-{8-char-hex}-identity`
5. Virtual Networks matching `alien-vnet-{8-char-hex}`
6. Disks matching `alien-disk-{8-char-hex}`

Never delete:

- The baseline resource group from `.env.test` (`ALIEN_TEST_AZURE_RESOURCE_GROUP`)
- The baseline storage account, blob container, managed environment, and ACR in that resource group
- `alien-dev`, `alien-staging`, `alien-prod` resource groups
- `NetworkWatcherRG`
- `alien-test-target` (target subscription baseline)
- `horizon2-test-rg` (owned by the horizon repo)

## Reconciliation checklist

After cleanup, the expected steady state should look like this:

- **AWS management account:** baseline bucket, baseline ECR repo, IAM roles (`alien-test-lambda-execution`, `alien-test-ecr-push`, `alien-test-ecr-pull`, `alien-test-management`), IAM user (`alien-test-manager`), no `alien-test-function-*` Lambdas, no `alien-test-kv-*` DynamoDB tables, no `alien-test-vault-*` SSM params, no extra `alien-test-*` S3 buckets, plus AWS service roles
- **AWS target account:** IAM user (`alien-test-target`), plus `horizon-cloud-tests`, `horizon-test-role`, and AWS service roles
- **GCP management project:** baseline bucket, `alien-test` and `test-alien-artifact-registry` Artifact Registry repos, service accounts (`alien-test-manager@`, `alien-test-management@`, `alien-test-ar-pull@`, `alien-test-ar-push@`, `alien-terraform-bootstrap@`, compute default), the `(default)` Firestore database only, no `alien-test-kv-db-*` databases, no `alien-test-repo-*` repos, no `alien-test-svc-*` Cloud Run services, no stale IAM bindings for deleted service accounts
- **GCP target project:** service accounts (`alien-test-target@`, `alien-terraform-bootstrap@`, compute default, `iam-condition-test@`, `test-custom-sa@`, `horizon-cloud-tests@`)
- **Azure management subscription:** baseline resource group containing only the storage account, ACR, and managed environment — no `alien-test-app-*` Container Apps, no `build-test-pre-*` Jobs, no `alientest*` Key Vaults, no leftover identities/vnets/disks. Plus `alien-dev`, `alien-staging`, `alien-prod`. No `*-default-resource-group` leftovers.
- **Azure target subscription:** `alien-test-target`, `horizon2-test-rg`, `NetworkWatcherRG` — no `*-default-resource-group` leftovers

## Programmatic cleanup via push_deletion

E2E tests can tear down deployments programmatically using `alien_deploy_cli::commands::push_deletion`. This drives `DeletePending → Deleting → Deleted` locally with target-environment credentials, running the same state machine as a normal deletion.

The `alien-test` harness uses this via `teardown_target()`:

```rust
alien_deploy_cli::commands::push_deletion(
    manager.client(),
    deployment_id,
    platform,
    target_config,
).await?;
```

This is the preferred cleanup approach for E2E tests because it drives the full deletion state machine rather than force-deleting the deployment record. Cloud resources are actually torn down, not orphaned.

For tests that panic or fail before teardown, the resources become the leftover patterns documented above and require manual cleanup.

## When to prefer Terraform over manual cleanup

Use `terraform apply` or `terraform destroy` when:

- baseline infra itself drifted
- the resource you want to remove is Terraform-owned
- you are intentionally rebuilding the shared test environment

Use manual cleanup when:

- a test created per-run resources and failed before teardown
- Terraform does not know about the resource
- you want to keep the shared baseline intact
