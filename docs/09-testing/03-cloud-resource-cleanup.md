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

These resources are expected, Terraform-managed, and should usually remain in place.

### AWS management account

- S3 bucket from `.env.test`: `ALIEN_TEST_AWS_S3_BUCKET`
- ECR repo from `.env.test`: `ALIEN_TEST_AWS_ECR_REPOSITORY`
- IAM role: `alien-test-lambda-execution`
- IAM role: `alien-test-ecr-push`
- IAM role: `alien-test-ecr-pull`
- IAM user: `alien-test-manager`

### AWS target account

- IAM user: `alien-test-target`

### GCP management project

- GCS bucket from `.env.test`: `ALIEN_TEST_GCP_GCS_BUCKET`
- Artifact Registry repo from `.env.test`: `alien-test`
- Service account: `alien-test-manager@...`

### GCP target project

- Service account: `alien-test-target@...`

### Azure management subscription

- Resource group from `.env.test`: `ALIEN_TEST_AZURE_RESOURCE_GROUP`
- Storage account from `.env.test`: `ALIEN_TEST_AZURE_STORAGE_ACCOUNT`
- Blob container from `.env.test`: `ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER`
- Container Apps environment from `.env.test`: `ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME`
- ACR from `.env.test`: `ALIEN_TEST_AZURE_REGISTRY_NAME`

If a resource matches the Terraform baseline above, it is not a cleanup candidate.

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

- DynamoDB: `alien-test-kv-*`
- IAM role: `alien-test-role-*`
- IAM role: `alien-test-build-role-*`
- SSM parameters: `alien-test-vault-*`
- Extra historical S3 buckets: `alien-test-*`

### GCP

Standalone E2E leftovers usually look like:

- Cloud Run service: `{prefix}-alien-rs-fn`, `{prefix}-alien-ts-fn`
- GCS bucket: `{prefix}-alien-storage`
- Pub/Sub topic: `{prefix}-alien-queue`
- Pub/Sub subscription: `{prefix}-alien-queue-sub`
- Service account: `{prefix}-execution-sa@...`
- Service account: `{prefix}-build-execution-sa@...`
- Secret Manager secret: `{prefix}-secrets-ALIEN_COMMANDS_TOKEN`

Rust function E2E also creates an Artifact Registry binding repo:

- Artifact Registry repo: `test-alien-artifact-registry`

### Azure

Azure is easiest to reason about because each deployment usually gets its own resource group:

- Resource group: `{prefix}-default-resource-group`
- Container Apps environment: `{prefix}-default-container-env`
- Container App: `{prefix}-alien-rs-fn`, `{prefix}-alien-ts-fn`
- Managed identity: `{prefix}-execution-sa`
- Managed identity: `{prefix}-build-execution-sa`
- Key Vault: `{prefix}-alien-vault`
- Service Bus namespace: `{prefix}-default-service-bus-namespace`
- Storage account: derived from `{prefix}-default-storage-account`, truncated to Azure limits

Rust function E2E may also create an ACR for the Artifact Registry binding inside the same per-run resource group.

## Inspection workflow

Start from the repo root:

```bash
set -a && source .env.test && set +a
```

### AWS

Management account:

```bash
env \
  AWS_ACCESS_KEY_ID="$AWS_MANAGEMENT_ACCESS_KEY_ID" \
  AWS_SECRET_ACCESS_KEY="$AWS_MANAGEMENT_SECRET_ACCESS_KEY" \
  AWS_DEFAULT_REGION="$AWS_MANAGEMENT_REGION" \
  aws sts get-caller-identity
```

List likely leftovers:

```bash
env AWS_ACCESS_KEY_ID="$AWS_MANAGEMENT_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$AWS_MANAGEMENT_SECRET_ACCESS_KEY" AWS_DEFAULT_REGION="$AWS_MANAGEMENT_REGION" \
  aws lambda list-functions | jq '.Functions[] | select(.FunctionName | test("alien-(rs|ts)-fn$")) | .FunctionName'

env AWS_ACCESS_KEY_ID="$AWS_MANAGEMENT_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$AWS_MANAGEMENT_SECRET_ACCESS_KEY" AWS_DEFAULT_REGION="$AWS_MANAGEMENT_REGION" \
  aws dynamodb list-tables | jq '.TableNames[] | select(test("alien-kv$") or startswith("alien-test-kv-"))'

env AWS_ACCESS_KEY_ID="$AWS_MANAGEMENT_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$AWS_MANAGEMENT_SECRET_ACCESS_KEY" AWS_DEFAULT_REGION="$AWS_MANAGEMENT_REGION" \
  aws sqs list-queues | jq -r '.QueueUrls[]? | split("/")[-1] | select(test("alien-queue$"))'

env AWS_ACCESS_KEY_ID="$AWS_MANAGEMENT_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$AWS_MANAGEMENT_SECRET_ACCESS_KEY" AWS_DEFAULT_REGION="$AWS_MANAGEMENT_REGION" \
  aws iam list-roles | jq -r '.Roles[] | .RoleName | select(test("execution-sa$") or test("build-execution-sa$") or test("test-alien-artifact-registry-(pull|push)$") or startswith("alien-test-role-") or startswith("alien-test-build-role-"))'

env AWS_ACCESS_KEY_ID="$AWS_MANAGEMENT_ACCESS_KEY_ID" AWS_SECRET_ACCESS_KEY="$AWS_MANAGEMENT_SECRET_ACCESS_KEY" AWS_DEFAULT_REGION="$AWS_MANAGEMENT_REGION" \
  aws ssm describe-parameters | jq -r '.Parameters[] | .Name | select(startswith("alien-test-vault-"))'
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
CLOUDSDK_CONFIG="$CFG" gcloud run services list --project="$GOOGLE_MANAGEMENT_PROJECT_ID" --platform=managed --format='value(metadata.name)'

CLOUDSDK_CONFIG="$CFG" gcloud storage buckets list --project="$GOOGLE_MANAGEMENT_PROJECT_ID" --format='value(name)'

CLOUDSDK_CONFIG="$CFG" gcloud pubsub topics list --project="$GOOGLE_MANAGEMENT_PROJECT_ID" --format='value(name)'

CLOUDSDK_CONFIG="$CFG" gcloud pubsub subscriptions list --project="$GOOGLE_MANAGEMENT_PROJECT_ID" --format='value(name)'

CLOUDSDK_CONFIG="$CFG" gcloud iam service-accounts list --project="$GOOGLE_MANAGEMENT_PROJECT_ID" --format='value(email)'

CLOUDSDK_CONFIG="$CFG" gcloud secrets list --project="$GOOGLE_MANAGEMENT_PROJECT_ID" --format='value(name)'
```

Cleanup temp auth:

```bash
rm -f "$MGMT_KEY"
rm -rf "$CFG"
```

### Azure

Azure leftovers are best inspected by resource group:

```bash
AZ_CFG=$(mktemp -d)
AZURE_CONFIG_DIR="$AZ_CFG" az login \
  --service-principal \
  -u "$AZURE_MANAGEMENT_CLIENT_ID" \
  -p "$AZURE_MANAGEMENT_CLIENT_SECRET" \
  --tenant "$AZURE_MANAGEMENT_TENANT_ID"
AZURE_CONFIG_DIR="$AZ_CFG" az account set --subscription "$AZURE_MANAGEMENT_SUBSCRIPTION_ID"
```

List likely leftover resource groups:

```bash
AZURE_CONFIG_DIR="$AZ_CFG" az group list -o json | jq -r '.[].name | select(test("^[a-z][0-9a-f]{7}-default-resource-group$"))'
```

Inspect one resource group:

```bash
RG="<prefix>-default-resource-group"
AZURE_CONFIG_DIR="$AZ_CFG" az resource list --resource-group "$RG" -o table
```

Cleanup temp auth:

```bash
rm -rf "$AZ_CFG"
```

## Cleanup workflow

Only do this after you have reviewed the listing output.

### AWS cleanup

Delete higher-level compute resources first, then stateful resources, then IAM and vault leftovers:

1. Delete Lambda functions matching `*-alien-rs-fn` and `*-alien-ts-fn`
2. Delete SQS queues matching `*-alien-queue`
3. Delete DynamoDB tables matching `*-alien-kv` and `alien-test-kv-*`
4. Delete SSM parameters matching `alien-test-vault-*`
5. Delete IAM roles matching:
   - `*-execution-sa`
   - `*-build-execution-sa`
   - `*-test-alien-artifact-registry-pull`
   - `*-test-alien-artifact-registry-push`
   - `alien-test-role-*`
   - `alien-test-build-role-*`
6. Re-check S3 buckets and only remove extra `alien-test-*` buckets that are not the current baseline bucket from `.env.test`

Never delete:

- the current baseline S3 bucket from `.env.test`
- the baseline ECR repo from `.env.test`
- the baseline `alien-test-lambda-execution`, `alien-test-ecr-push`, or `alien-test-ecr-pull` roles

### GCP cleanup

Delete in this order:

1. Cloud Run services
2. Pub/Sub subscriptions
3. Pub/Sub topics
4. Secret Manager secrets
5. Leftover GCS buckets
6. Per-run service accounts
7. Optional: the shared `test-alien-artifact-registry` repo, but only if you know no current run needs it

Keep:

- The baseline bucket from `.env.test`
- The baseline `alien-test` Artifact Registry repo
- The default Firestore database unless you are intentionally rebuilding the whole test project

### Azure cleanup

For Azure, prefer deleting by leftover resource group:

1. Identify each `{prefix}-default-resource-group`
2. Inspect the resources in that group
3. Delete the whole resource group if it is clearly from a failed test run

That is safer and much simpler than deleting Container Apps, identities, storage accounts, Key Vaults, Service Bus namespaces, and ACRs one by one.

Keep:

- The baseline resource group from `.env.test`
- The baseline storage account, blob container, managed environment, and ACR in that resource group

## Reconciliation checklist

After cleanup, the expected steady state should look roughly like this:

- AWS management account contains only the baseline bucket, baseline ECR repo, and baseline IAM roles
- AWS target account contains only baseline principals
- GCP management project contains only the baseline bucket, baseline Artifact Registry repo, manager service account, and the default Firestore database
- GCP target project contains only whatever non-`alien-test` project baseline you intentionally maintain
- Azure management subscription contains only the baseline resource group and its baseline resources
- Azure target subscription should not contain standalone E2E deployments from the current harness

## When to prefer Terraform over manual cleanup

Use `terraform apply` or `terraform destroy` when:

- baseline infra itself drifted
- the resource you want to remove is Terraform-owned
- you are intentionally rebuilding the shared test environment

Use manual cleanup when:

- a test created per-run resources and failed before teardown
- Terraform does not know about the resource
- you want to keep the shared baseline intact
