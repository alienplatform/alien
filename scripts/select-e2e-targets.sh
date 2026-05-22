#!/usr/bin/env bash
# Append canonical harness env vars for selected E2E target aliases.
#
# Usage:
#   scripts/select-e2e-targets.sh --env-file .env.test \
#     --aws aws-target-1 --gcp gcp-target-3 --azure azure-target-1
set -euo pipefail

env_file=".env.test"
aws_alias=""
gcp_alias=""
azure_alias=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --env-file)
      env_file="${2:?--env-file requires a path}"
      shift 2
      ;;
    --aws)
      aws_alias="${2:?--aws requires an alias}"
      shift 2
      ;;
    --gcp)
      gcp_alias="${2:?--gcp requires an alias}"
      shift 2
      ;;
    --azure)
      azure_alias="${2:?--azure requires an alias}"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [[ ! -f "$env_file" ]]; then
  echo "env file not found: $env_file" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "$env_file"
set +a

aws_alias="${aws_alias:-${ALIEN_TEST_DEFAULT_TARGET_AWS:-aws-target-1}}"
gcp_alias="${gcp_alias:-${ALIEN_TEST_DEFAULT_TARGET_GCP:-gcp-target-3}}"
azure_alias="${azure_alias:-${ALIEN_TEST_DEFAULT_TARGET_AZURE:-azure-target-1}}"

alias_env() {
  printf '%s' "$1" | tr '[:lower:]-' '[:upper:]_'
}

quote_env() {
  local value="$1"
  printf "'%s'" "${value//\'/\'\\\'\'}"
}

require_target_value() {
  local cloud="$1"
  local alias="$2"
  local key="$3"
  local alias_key
  alias_key="$(alias_env "$alias")"
  local var="ALIEN_TARGET_${cloud}_${alias_key}_${key}"
  local value="${!var-}"
  if [[ -z "$value" ]]; then
    echo "missing target option value: $var" >&2
    exit 1
  fi
  printf '%s' "$value"
}

append_key() {
  local key="$1"
  local value="$2"
  printf "%s=%s\n" "$key" "$(quote_env "$value")" >> "$env_file"
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
    echo "::add-mask::$value"
  fi
}

append_cloud_selection() {
  local cloud="$1"
  local alias="$2"
  shift 2

  for key in "$@"; do
    append_key "$key" "$(require_target_value "$cloud" "$alias" "$key")"
  done
}

AWS_KEYS=(
  AWS_TARGET_REGION
  AWS_TARGET_ACCESS_KEY_ID
  AWS_TARGET_SECRET_ACCESS_KEY
  AWS_TARGET_ACCOUNT_ID
  ALIEN_E2E_AWS_VPC_ID
  ALIEN_E2E_AWS_PUBLIC_SUBNET_IDS
  ALIEN_E2E_AWS_PRIVATE_SUBNET_IDS
  ALIEN_E2E_AWS_SECURITY_GROUP_IDS
  ALIEN_TEST_AWS_S3_BUCKET
  ALIEN_TEST_AWS_COMMAND_KV_TABLE
  ALIEN_TEST_AWS_LAMBDA_IMAGE
  ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN
  ALIEN_TEST_AWS_ECR_PUSH_ROLE_ARN
  ALIEN_TEST_AWS_ECR_PULL_ROLE_ARN
  ALIEN_TEST_AWS_ECR_REPOSITORY
  E2E_AWS_AR_PUSH_ROLE_ARN
  E2E_AWS_AR_PULL_ROLE_ARN
)

GCP_KEYS=(
  GOOGLE_TARGET_SERVICE_ACCOUNT_KEY
  GOOGLE_TARGET_PROJECT_ID
  GOOGLE_TARGET_REGION
  ALIEN_E2E_GCP_NETWORK_NAME
  ALIEN_E2E_GCP_SUBNET_NAME
  ALIEN_E2E_GCP_REGION
)

AZURE_KEYS=(
  AZURE_TARGET_SUBSCRIPTION_ID
  AZURE_TARGET_TENANT_ID
  AZURE_TARGET_CLIENT_ID
  AZURE_TARGET_CLIENT_SECRET
  AZURE_TARGET_REGION
  AZURE_TARGET_RESOURCE_GROUP
  AZURE_REGION
  ARM_SUBSCRIPTION_ID
  ARM_TENANT_ID
  ARM_CLIENT_ID
  ARM_CLIENT_SECRET
  ALIEN_TEST_AZURE_RESOURCE_GROUP
  ALIEN_TEST_AZURE_STORAGE_ACCOUNT
  ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER
  ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE
  ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME
  ALIEN_TEST_AZURE_REGISTRY_NAME
  ALIEN_TEST_AZURE_ACR_REPOSITORY
  E2E_AZURE_ACR_REPOSITORY
  AZURE_SHARED_CONTAINER_ENV_NAME
  AZURE_SHARED_CONTAINER_ENV_RESOURCE_ID
  AZURE_SHARED_CONTAINER_ENV_RESOURCE_GROUP
  AZURE_SHARED_CONTAINER_ENV_DEFAULT_DOMAIN
  AZURE_SHARED_CONTAINER_ENV_STATIC_IP
  AZURE_SHARED_CONTAINER_ENV_JOIN_ROLE_ID
  ALIEN_E2E_AZURE_VNET_RESOURCE_ID
  ALIEN_E2E_AZURE_PUBLIC_SUBNET_NAME
  ALIEN_E2E_AZURE_PRIVATE_SUBNET_NAME
)

remove_existing_selection() {
  local tmp keys
  tmp="$(mktemp)"
  keys="$(
    printf '%s\n' \
      ALIEN_SELECTED_AWS_TARGET \
      ALIEN_SELECTED_GCP_TARGET \
      ALIEN_SELECTED_AZURE_TARGET \
      "${AWS_KEYS[@]}" \
      "${GCP_KEYS[@]}" \
      "${AZURE_KEYS[@]}" \
      | paste -sd, -
  )"

  awk -v keys="$keys" '
    BEGIN {
      split(keys, names, ",")
      for (i in names) skip[names[i]] = 1
    }
    {
      line = $0
      sub(/^[[:space:]]*export[[:space:]]+/, "", line)
      if (line ~ /^[A-Za-z_][A-Za-z0-9_]*=/) {
        key = line
        sub(/=.*/, "", key)
        if (skip[key]) next
      }
      print
    }
  ' "$env_file" > "$tmp"
  mv "$tmp" "$env_file"
}

remove_existing_selection

{
  echo ""
  echo "# Selected E2E target aliases"
  echo "ALIEN_SELECTED_AWS_TARGET=$(quote_env "$aws_alias")"
  echo "ALIEN_SELECTED_GCP_TARGET=$(quote_env "$gcp_alias")"
  echo "ALIEN_SELECTED_AZURE_TARGET=$(quote_env "$azure_alias")"
} >> "$env_file"

append_cloud_selection AWS "$aws_alias" "${AWS_KEYS[@]}"
append_cloud_selection GCP "$gcp_alias" "${GCP_KEYS[@]}"
append_cloud_selection AZURE "$azure_alias" "${AZURE_KEYS[@]}"

echo "Selected E2E targets: AWS=$aws_alias GCP=$gcp_alias Azure=$azure_alias"
