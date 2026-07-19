#!/usr/bin/env bash
#
# Permission-gate apply-and-inspect e2e (GCP / Azure, real cloud) via Terraform.
#
# The AWS sibling (`alien-cloudformation/tests/e2e/permission-gate-aws.sh`)
# proves the gate on CloudFormation-baked IAM. This proves the same end-state on
# the Terraform emitter for the other two clouds: one Kv `store` whose
# `kv/data-write` grant on the runtime identity is gated on the boolean
# `kvEnabled` deployer input. Applying with the input off leaves the grant off
# the identity; on adds it. The gate renders as an input-conditioned `count` on
# the IAM resource, so "off" means the binding is never created.
#
# The module is rendered with no self-registration, so `terraform apply` needs
# no external registration step: it just materializes the Frozen resources and
# the gated binding, and we read the live cloud IAM to check the grant followed
# the input.
#
# Not run in CI (real cloud). Run it manually with target-account creds:
#
#   ./permission-gate-tf.sh gcp   [path-to-env-file]   # default: <repo-root>/.env.test
#   ./permission-gate-tf.sh azure [path-to-env-file]
#
# The env file must export, for GCP:  GOOGLE_TARGET_PROJECT_ID /
# GOOGLE_TARGET_REGION / GOOGLE_TARGET_SERVICE_ACCOUNT_KEY (inline JSON), for an
# account allowed to create service accounts, a Firestore database, and project
# IAM bindings. For Azure: AZURE_TARGET_SUBSCRIPTION_ID / _TENANT_ID /
# _CLIENT_ID / _CLIENT_SECRET / _REGION, for a principal allowed to create a
# resource group, a storage account, a user-assigned identity, and role
# assignments.
set -euo pipefail

CLOUD="${1:?usage: permission-gate-tf.sh <gcp|azure> [env-file]}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
ENV_FILE="${2:-$REPO_ROOT/.env.test}"

case "$CLOUD" in
  gcp|azure) ;;
  *) echo "unsupported cloud '$CLOUD' (expected gcp|azure)"; exit 2 ;;
esac

# shellcheck disable=SC1090
set -a; source "$ENV_FILE"; set +a

WORK="$(mktemp -d -t gated-tf-XXXX)"
MODULE="$WORK/module"
TF=(terraform -chdir="$MODULE")

# A per-run suffix keeps parallel/interrupted runs from colliding on cloud names.
RUN_ID="e2e${RANDOM}${RANDOM}"

# Populated per cloud below: the `-var` flags common to every apply/destroy.
COMMON_VARS=()

cleanup() {
  echo "--- cleanup: terraform destroy ---"
  "${TF[@]}" destroy -auto-approve -input=false -no-color \
    "${COMMON_VARS[@]}" -var "input_kv_enabled=false" >/dev/null 2>&1 || \
    echo "WARN: destroy reported an error; verify no leaked resources in the target account"
  verify_torn_down || echo "WARN: post-destroy verification found leftovers; check the target account"
  rm -rf "$WORK"
}

# ---- GCP ---------------------------------------------------------------------

gcp_setup() {
  export GOOGLE_CREDENTIALS="$GOOGLE_TARGET_SERVICE_ACCOUNT_KEY"
  COMMON_VARS=(
    -var "name=alien-gate-$RUN_ID"
    -var "token=e2e-unused"
    -var "gcp_project=$GOOGLE_TARGET_PROJECT_ID"
    -var "gcp_region=$GOOGLE_TARGET_REGION"
  )
  # Read live IAM with an isolated gcloud config so we never touch the user's
  # active account.
  export CLOUDSDK_CONFIG="$WORK/gcloud"
  printf '%s' "$GOOGLE_TARGET_SERVICE_ACCOUNT_KEY" > "$WORK/gcp-key.json"
  gcloud auth activate-service-account --key-file="$WORK/gcp-key.json" --quiet >/dev/null 2>&1
}

gcp_identity() {
  "${TF[@]}" show -json | jq -r \
    '.values.root_module.resources[]
     | select(.address=="google_service_account.execution_sa") | .values.email'
}

# The gated grant is roles/datastore.user on the runtime SA at the project level.
gcp_grant_present() {
  local sa; sa="$(gcp_identity)"
  [ -n "$sa" ] && [ "$sa" != "null" ] || return 2
  local role
  role="$(gcloud projects get-iam-policy "$GOOGLE_TARGET_PROJECT_ID" \
    --flatten='bindings[].members' \
    --filter="bindings.role=roles/datastore.user AND bindings.members:serviceAccount:$sa" \
    --format='value(bindings.role)' 2>/dev/null)"
  [ -n "$role" ]
}

gcp_verify_torn_down() {
  local sa; sa="$(gcp_identity 2>/dev/null || true)"
  [ -z "$sa" ] && return 0
  ! gcloud iam service-accounts describe "$sa" --project "$GOOGLE_TARGET_PROJECT_ID" >/dev/null 2>&1
}

# ---- Azure -------------------------------------------------------------------

azure_setup() {
  export ARM_CLIENT_ID="$AZURE_TARGET_CLIENT_ID"
  export ARM_CLIENT_SECRET="$AZURE_TARGET_CLIENT_SECRET"
  export ARM_TENANT_ID="$AZURE_TARGET_TENANT_ID"
  export ARM_SUBSCRIPTION_ID="$AZURE_TARGET_SUBSCRIPTION_ID"
  # The module creates the resource group named by this var, so it must be a
  # fresh name (destroy deletes the whole group, cascading every child).
  COMMON_VARS=(
    -var "name=alien-gate-$RUN_ID"
    -var "token=e2e-unused"
    -var "azure_subscription_id=$AZURE_TARGET_SUBSCRIPTION_ID"
    -var "azure_location=$AZURE_TARGET_REGION"
    -var "azure_resource_group_name=alien-gate-$RUN_ID"
  )
  export AZURE_CONFIG_DIR="$WORK/azure-cli"
  az login --service-principal -u "$AZURE_TARGET_CLIENT_ID" \
    -p "$AZURE_TARGET_CLIENT_SECRET" --tenant "$AZURE_TARGET_TENANT_ID" --output none
  az account set --subscription "$AZURE_TARGET_SUBSCRIPTION_ID"
}

azure_identity() {
  "${TF[@]}" show -json | jq -r \
    '.values.root_module.resources[]
     | select(.address=="azurerm_user_assigned_identity.execution_sa") | .values.principal_id'
}

# The gated grant is the Storage Table Data Contributor role assignment on the
# runtime identity. In this minimal stack it is the identity's only assignment,
# so an assignee+role match is unambiguous and scope-independent.
azure_grant_present() {
  local pid; pid="$(azure_identity)"
  [ -n "$pid" ] && [ "$pid" != "null" ] || return 2
  local hit
  hit="$(az role assignment list --assignee "$pid" --all \
    --query "[?roleDefinitionName=='Storage Table Data Contributor'].roleDefinitionName" \
    -o tsv 2>/dev/null)"
  [ -n "$hit" ]
}

azure_verify_torn_down() {
  ! az group show --name "alien-gate-$RUN_ID" >/dev/null 2>&1
}

# ---- Dispatch ----------------------------------------------------------------

grant_present()   { "${CLOUD}_grant_present"; }
verify_torn_down(){ "${CLOUD}_verify_torn_down"; }

apply() {
  local mode="$1"
  echo "=== terraform apply (kvEnabled=$mode) ==="
  "${TF[@]}" apply -auto-approve -input=false -no-color \
    "${COMMON_VARS[@]}" -var "input_kv_enabled=$mode" >/dev/null
}

# Cloud IAM reads are eventually consistent; poll for the expected transition
# rather than reading once right after apply.
poll_present() {
  for _ in $(seq 1 24); do
    if grant_present; then return 0; fi
    sleep 5
  done
  return 1
}

echo "=== render the gated Terraform module ($CLOUD) ==="
(cd "$REPO_ROOT" && cargo run --quiet --example emit_gated_tf -p alien-terraform -- "$CLOUD" "$MODULE")

"${CLOUD}_setup"
trap cleanup EXIT

"${TF[@]}" init -input=false -no-color >/dev/null
echo "init OK"

apply false
if grant_present; then
  echo "FAIL: runtime identity carries the gated grant with the gate OFF (fail-open)"
  exit 1
fi
echo "PASS: gate OFF -> runtime identity lacks the gated grant"

apply true
if ! poll_present; then
  echo "FAIL: runtime identity lacks the gated grant with the gate ON"
  exit 1
fi
echo "PASS: gate ON -> runtime identity carries the gated grant"

echo "=== e2e PASSED ($CLOUD): the gated grant follows the deployer input on real IAM ==="
