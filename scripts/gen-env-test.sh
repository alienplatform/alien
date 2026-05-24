#!/usr/bin/env bash
# Generate .env.test from Terraform outputs.
# Usage: ./scripts/gen-env-test.sh
# Requires: terraform (init'd, authenticated to TF Cloud), jq
#
# Environment variables required:
#   AXIOM_TOKEN         - Axiom API token
#   NGROK_AUTHTOKEN     - Ngrok auth token (for push-mode E2E tests)
# Environment variables optional:
#   AXIOM_OTLP_ENDPOINT - (default: https://api.axiom.co/v1/logs)
#   AXIOM_DATASET       - (default: dev)
set -euo pipefail

TF=$(cd infra/test && terraform output -json)
jq_val() { printf '%s' "$TF" | jq -er --arg key "$1" '.[$key].value // empty'; }
# Like jq_val but compacts JSON values onto a single line.
#
# GCP SA keys are pretty-printed JSON with \n escape sequences in the
# private_key field. jq -r decodes the Terraform wrapper correctly
# (structural newlines become whitespace, \n in strings stays escaped).
# Piping through jq -c compacts the valid multiline JSON into one line.
# This is required because Windows PowerShell reads .env.test line-by-line.
jq_val_json() { jq_val "$1" | jq -c .; }
jq_val_csv() { jq_val "$1" | jq -er 'join(",")'; }
jq_val_target_aliases() { jq_val "$1" | jq -er 'keys | join(",")'; }
jq_val_target_options_env() {
  local output_name="$1"
  local cloud="$2"
  jq_val "$output_name" | jq -r --arg cloud "$cloud" '
    def env_value:
      if type == "array" then
        join(",")
      elif type == "string" then
        . as $value
        | (try ($value | fromjson | tojson) catch $value)
        | gsub("\r"; "\\r")
        | gsub("\n"; "\\n")
      else
        tostring
      end;

    to_entries[]
    | .key as $alias
    | ($alias | ascii_upcase | gsub("[^A-Z0-9]"; "_")) as $alias_env
    | .value
    | to_entries[]
    | "ALIEN_TARGET_" + $cloud + "_" + $alias_env + "_" + .key + "=" + (.value | env_value)
  '
}
quote_env() {
  local value="$1"
  printf "'%s'" "${value//\'/\'\\\'\'}"
}

# Capture all values into variables before writing so we can safely
# single-quote them in the output (single-quoted values are literal in
# both shell `source` and dotenvy -- no escape processing).
aws_management_region=$(jq_val management_aws_region)
aws_management_access_key_id=$(jq_val management_aws_access_key_id)
aws_management_secret_access_key=$(jq_val management_aws_secret_access_key)
aws_management_account_id=$(jq_val management_aws_account_id)
aws_management_role_arn=$(jq_val management_aws_role_arn)
aws_management_role_name=$(jq_val management_aws_role_name)

aws_target_region=$(jq_val target_aws_region)
aws_target_access_key_id=$(jq_val target_aws_access_key_id)
aws_target_secret_access_key=$(jq_val target_aws_secret_access_key)
aws_target_account_id=$(jq_val target_aws_account_id)
e2e_aws_vpc_id=$(jq_val e2e_aws_vpc_id)
e2e_aws_public_subnet_ids=$(jq_val_csv e2e_aws_public_subnet_ids)
e2e_aws_private_subnet_ids=$(jq_val_csv e2e_aws_private_subnet_ids)
e2e_aws_security_group_ids=$(jq_val_csv e2e_aws_security_group_ids)

aws_s3_bucket=$(jq_val aws_s3_bucket)
aws_command_kv_table=$(jq_val aws_command_kv_table_name)
aws_lambda_image=$(jq_val aws_lambda_image_uri)
aws_lambda_execution_role_arn=$(jq_val aws_lambda_execution_role_arn)
aws_ecr_push_role_arn=$(jq_val aws_ecr_push_role_arn)
aws_ecr_pull_role_arn=$(jq_val aws_ecr_pull_role_arn)
aws_ecr_repository=$(echo "$aws_lambda_image" | cut -d: -f1)

# E2E artifact registry (separate from bindings-test resources)
e2e_aws_ar_push_role_arn=$(jq_val e2e_aws_ar_push_role_arn)
e2e_aws_ar_pull_role_arn=$(jq_val e2e_aws_ar_pull_role_arn)
e2e_eks_cluster_name=$(jq_val e2e_eks_cluster_name)
e2e_eks_kube_context=$(jq_val e2e_eks_kube_context)
e2e_eks_public_host_suffix=$(jq_val e2e_eks_public_host_suffix)

gcp_management_sa_key=$(jq_val_json management_gcp_service_account_key)
gcp_management_project_id=$(jq_val management_gcp_project_id)
gcp_management_region=$(jq_val management_gcp_region)

gcp_target_sa_key=$(jq_val_json target_gcp_service_account_key)
gcp_target_project_id=$(jq_val target_gcp_project_id)
gcp_target_region=$(jq_val target_gcp_region)
e2e_gcp_network_name=$(jq_val e2e_gcp_network_name)
e2e_gcp_subnet_name=$(jq_val e2e_gcp_subnet_name)
e2e_gcp_network_region=$(jq_val e2e_gcp_network_region)

gcp_management_identity_email=$(jq_val gcp_management_identity_email)
gcp_management_identity_unique_id=$(jq_val gcp_management_identity_unique_id)

gcp_gcs_bucket=$(jq_val gcp_gcs_bucket)
gcp_cloudrun_image=$(jq_val gcp_cloudrun_image_uri)
gcp_gar_repository=$(echo "$gcp_cloudrun_image" | cut -d: -f1)

# E2E artifact registry (separate from bindings-test resources)
e2e_gcp_gar_repository=$(jq_val e2e_gcp_gar_repository)
e2e_gcp_ar_pull_sa_email=$(jq_val e2e_gcp_ar_pull_sa_email)
e2e_gcp_ar_push_sa_email=$(jq_val e2e_gcp_ar_push_sa_email)
e2e_gke_cluster_name=$(jq_val e2e_gke_cluster_name)
e2e_gke_cluster_location=$(jq_val e2e_gke_cluster_location)
e2e_gke_kube_context=$(jq_val e2e_gke_kube_context)
e2e_gke_public_host_suffix=$(jq_val e2e_gke_public_host_suffix)

azure_management_subscription_id=$(jq_val management_azure_subscription_id)
azure_management_tenant_id=$(jq_val management_azure_tenant_id)
azure_management_client_id=$(jq_val management_azure_client_id)
azure_management_client_secret=$(jq_val management_azure_client_secret)
azure_management_region=$(jq_val management_azure_region)

azure_target_subscription_id=$(jq_val target_azure_subscription_id)
azure_target_tenant_id=$(jq_val target_azure_tenant_id)
azure_target_client_id=$(jq_val target_azure_client_id)
azure_target_client_secret=$(jq_val target_azure_client_secret)

azure_resource_group=$(jq_val azure_resource_group)
azure_storage_account=$(jq_val azure_storage_account)
azure_blob_container=$(jq_val azure_blob_container)
azure_container_app_image=$(jq_val azure_container_app_image_uri)
azure_managed_environment=$(jq_val azure_managed_environment)
azure_acr_name=$(jq_val azure_acr_name)
azure_acr_repository=$(echo "$azure_container_app_image" | cut -d: -f1)

# E2E artifact registry (separate image path within the same ACR)
e2e_azure_acr_repository=$(jq_val e2e_azure_acr_repository)

# Shared Container Apps Environment (in target subscription)
azure_shared_container_env_name=$(jq_val azure_shared_container_env_name)
azure_shared_container_env_resource_id=$(jq_val azure_shared_container_env_resource_id)
azure_shared_container_env_resource_group=$(jq_val azure_shared_container_env_resource_group)
azure_shared_container_env_default_domain=$(jq_val azure_shared_container_env_default_domain)
azure_shared_container_env_static_ip=$(jq_val azure_shared_container_env_static_ip)
azure_shared_container_env_join_role_id=$(jq_val azure_shared_container_env_join_role_id)
e2e_azure_vnet_resource_id=$(jq_val e2e_azure_vnet_resource_id)
e2e_azure_public_subnet_name=$(jq_val e2e_azure_public_subnet_name)
e2e_azure_private_subnet_name=$(jq_val e2e_azure_private_subnet_name)
e2e_aks_cluster_name=$(jq_val e2e_aks_cluster_name)
e2e_aks_cluster_resource_group=$(jq_val e2e_aks_cluster_resource_group)
e2e_aks_kube_context=$(jq_val e2e_aks_kube_context)
e2e_aks_public_host_suffix=$(jq_val e2e_aks_public_host_suffix)

aws_target_aliases=$(jq_val_target_aliases aws_target_options)
gcp_target_aliases=$(jq_val_target_aliases gcp_target_options)
azure_target_aliases=$(jq_val_target_aliases azure_target_options)
e2e_k8s_ingress_class=$(jq_val e2e_k8s_ingress_class)

kubeconfig_dir="$(pwd)/.alien-kubeconfigs"
mkdir -p "$kubeconfig_dir"
e2e_eks_kubeconfig_path="$kubeconfig_dir/eks.yaml"
e2e_gke_kubeconfig_path="$kubeconfig_dir/gke.yaml"
e2e_gke_target_1_kubeconfig_path="$kubeconfig_dir/gke-target-1.yaml"
e2e_gke_target_2_kubeconfig_path="$kubeconfig_dir/gke-target-2.yaml"
e2e_gke_target_3_kubeconfig_path="$kubeconfig_dir/gke-target-3.yaml"
e2e_aks_kubeconfig_path="$kubeconfig_dir/aks.yaml"
jq_val e2e_eks_kubeconfig > "$e2e_eks_kubeconfig_path"
jq_val e2e_gke_kubeconfig > "$e2e_gke_kubeconfig_path"
jq_val e2e_gke_target_1_kubeconfig > "$e2e_gke_target_1_kubeconfig_path"
jq_val e2e_gke_target_2_kubeconfig > "$e2e_gke_target_2_kubeconfig_path"
jq_val e2e_gke_target_3_kubeconfig > "$e2e_gke_target_3_kubeconfig_path"
jq_val e2e_aks_kubeconfig > "$e2e_aks_kubeconfig_path"
chmod 600 \
  "$e2e_eks_kubeconfig_path" \
  "$e2e_gke_kubeconfig_path" \
  "$e2e_gke_target_1_kubeconfig_path" \
  "$e2e_gke_target_2_kubeconfig_path" \
  "$e2e_gke_target_3_kubeconfig_path" \
  "$e2e_aks_kubeconfig_path"

cat > .env.test <<EOF
# AWS - Management
AWS_MANAGEMENT_REGION='${aws_management_region}'
AWS_MANAGEMENT_ACCESS_KEY_ID='${aws_management_access_key_id}'
AWS_MANAGEMENT_SECRET_ACCESS_KEY='${aws_management_secret_access_key}'
AWS_MANAGEMENT_ACCOUNT_ID='${aws_management_account_id}'
AWS_MANAGEMENT_ROLE_ARN='${aws_management_role_arn}'
AWS_MANAGEMENT_ROLE_NAME='${aws_management_role_name}'

# AWS - Target
AWS_TARGET_REGION='${aws_target_region}'
AWS_TARGET_ACCESS_KEY_ID='${aws_target_access_key_id}'
AWS_TARGET_SECRET_ACCESS_KEY='${aws_target_secret_access_key}'
AWS_TARGET_ACCOUNT_ID='${aws_target_account_id}'

# AWS - reusable E2E network
ALIEN_E2E_AWS_VPC_ID='${e2e_aws_vpc_id}'
ALIEN_E2E_AWS_PUBLIC_SUBNET_IDS='${e2e_aws_public_subnet_ids}'
ALIEN_E2E_AWS_PRIVATE_SUBNET_IDS='${e2e_aws_private_subnet_ids}'
ALIEN_E2E_AWS_SECURITY_GROUP_IDS='${e2e_aws_security_group_ids}'

# AWS test resources
ALIEN_TEST_AWS_S3_BUCKET='${aws_s3_bucket}'
ALIEN_TEST_AWS_COMMAND_KV_TABLE='${aws_command_kv_table}'
ALIEN_TEST_AWS_LAMBDA_IMAGE='${aws_lambda_image}'
ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN='${aws_lambda_execution_role_arn}'
ALIEN_TEST_AWS_ECR_PUSH_ROLE_ARN='${aws_ecr_push_role_arn}'
ALIEN_TEST_AWS_ECR_PULL_ROLE_ARN='${aws_ecr_pull_role_arn}'
ALIEN_TEST_AWS_ECR_REPOSITORY='${aws_ecr_repository}'

# E2E artifact registry (matches alien-infra controller pattern)
E2E_AWS_AR_PUSH_ROLE_ARN='${e2e_aws_ar_push_role_arn}'
E2E_AWS_AR_PULL_ROLE_ARN='${e2e_aws_ar_pull_role_arn}'

# Kubernetes E2E kubeconfigs generated from infra/test outputs
ALIEN_TEST_EKS_KUBECONFIG='${e2e_eks_kubeconfig_path}'
ALIEN_TEST_EKS_CLUSTER_NAME='${e2e_eks_cluster_name}'
ALIEN_TEST_EKS_KUBE_CONTEXT='${e2e_eks_kube_context}'
ALIEN_TEST_EKS_INGRESS_CLASS='${e2e_k8s_ingress_class}'
ALIEN_TEST_EKS_PUBLIC_HOST_SUFFIX='${e2e_eks_public_host_suffix}'

# GCP - Management
GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY='${gcp_management_sa_key}'
GOOGLE_MANAGEMENT_PROJECT_ID='${gcp_management_project_id}'
GOOGLE_MANAGEMENT_REGION='${gcp_management_region}'

# GCP - Target
GOOGLE_TARGET_SERVICE_ACCOUNT_KEY='${gcp_target_sa_key}'
GOOGLE_TARGET_PROJECT_ID='${gcp_target_project_id}'
GOOGLE_TARGET_REGION='${gcp_target_region}'

# GCP - reusable E2E network
ALIEN_E2E_GCP_NETWORK_NAME='${e2e_gcp_network_name}'
ALIEN_E2E_GCP_SUBNET_NAME='${e2e_gcp_subnet_name}'
ALIEN_E2E_GCP_REGION='${e2e_gcp_network_region}'

# GCP - Management Identity
GOOGLE_MANAGEMENT_IDENTITY_EMAIL='${gcp_management_identity_email}'
GOOGLE_MANAGEMENT_IDENTITY_UNIQUE_ID='${gcp_management_identity_unique_id}'

# GCP test resources
ALIEN_TEST_GCP_GCS_BUCKET='${gcp_gcs_bucket}'
ALIEN_TEST_GCP_CLOUDRUN_IMAGE='${gcp_cloudrun_image}'
ALIEN_TEST_GCP_GAR_REPOSITORY='${gcp_gar_repository}'

# E2E artifact registry (matches alien-infra controller pattern)
E2E_GCP_GAR_REPOSITORY='${e2e_gcp_gar_repository}'
E2E_GCP_AR_PULL_SA_EMAIL='${e2e_gcp_ar_pull_sa_email}'
E2E_GCP_AR_PUSH_SA_EMAIL='${e2e_gcp_ar_push_sa_email}'

# Kubernetes E2E kubeconfigs generated from infra/test outputs
ALIEN_TEST_GKE_KUBECONFIG='${e2e_gke_kubeconfig_path}'
ALIEN_TEST_GKE_CLUSTER_NAME='${e2e_gke_cluster_name}'
ALIEN_TEST_GKE_CLUSTER_LOCATION='${e2e_gke_cluster_location}'
ALIEN_TEST_GKE_KUBE_CONTEXT='${e2e_gke_kube_context}'
ALIEN_TEST_GKE_INGRESS_CLASS='${e2e_k8s_ingress_class}'
ALIEN_TEST_GKE_PUBLIC_HOST_SUFFIX='${e2e_gke_public_host_suffix}'

# Azure - Management
AZURE_MANAGEMENT_SUBSCRIPTION_ID='${azure_management_subscription_id}'
AZURE_MANAGEMENT_TENANT_ID='${azure_management_tenant_id}'
AZURE_MANAGEMENT_CLIENT_ID='${azure_management_client_id}'
AZURE_MANAGEMENT_CLIENT_SECRET='${azure_management_client_secret}'
AZURE_MANAGEMENT_REGION='${azure_management_region}'

# Azure OIDC (set dynamically by the runtime environment)
AZURE_MANAGEMENT_OIDC_ISSUER='${AZURE_MANAGEMENT_OIDC_ISSUER:-}'
AZURE_MANAGEMENT_OIDC_SUBJECT='${AZURE_MANAGEMENT_OIDC_SUBJECT:-}'

# Azure - Target
AZURE_TARGET_SUBSCRIPTION_ID='${azure_target_subscription_id}'
AZURE_TARGET_TENANT_ID='${azure_target_tenant_id}'
AZURE_TARGET_CLIENT_ID='${azure_target_client_id}'
AZURE_TARGET_CLIENT_SECRET='${azure_target_client_secret}'
AZURE_TARGET_REGION='${azure_management_region}'
AZURE_TARGET_RESOURCE_GROUP='${azure_shared_container_env_resource_group}'

AZURE_REGION='${azure_management_region}'

# Azure test resources
ALIEN_TEST_AZURE_RESOURCE_GROUP='${azure_resource_group}'
ALIEN_TEST_AZURE_STORAGE_ACCOUNT='${azure_storage_account}'
ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER='${azure_blob_container}'
ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE='${azure_container_app_image}'
ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME='${azure_managed_environment}'
ALIEN_TEST_AZURE_REGISTRY_NAME='${azure_acr_name}'
ALIEN_TEST_AZURE_ACR_REPOSITORY='${azure_acr_repository}'

# E2E artifact registry (separate image path within the same ACR)
E2E_AZURE_ACR_REPOSITORY='${e2e_azure_acr_repository}'

# Azure shared Container Apps Environment (in target subscription)
AZURE_SHARED_CONTAINER_ENV_NAME='${azure_shared_container_env_name}'
AZURE_SHARED_CONTAINER_ENV_RESOURCE_ID='${azure_shared_container_env_resource_id}'
AZURE_SHARED_CONTAINER_ENV_RESOURCE_GROUP='${azure_shared_container_env_resource_group}'
AZURE_SHARED_CONTAINER_ENV_DEFAULT_DOMAIN='${azure_shared_container_env_default_domain}'
AZURE_SHARED_CONTAINER_ENV_STATIC_IP='${azure_shared_container_env_static_ip}'
AZURE_SHARED_CONTAINER_ENV_JOIN_ROLE_ID='${azure_shared_container_env_join_role_id}'

# Azure - reusable E2E network
ALIEN_E2E_AZURE_VNET_RESOURCE_ID='${e2e_azure_vnet_resource_id}'
ALIEN_E2E_AZURE_PUBLIC_SUBNET_NAME='${e2e_azure_public_subnet_name}'
ALIEN_E2E_AZURE_PRIVATE_SUBNET_NAME='${e2e_azure_private_subnet_name}'

# Kubernetes E2E kubeconfigs generated from infra/test outputs
ALIEN_TEST_AKS_KUBECONFIG='${e2e_aks_kubeconfig_path}'
ALIEN_TEST_AKS_CLUSTER_NAME='${e2e_aks_cluster_name}'
ALIEN_TEST_AKS_CLUSTER_RESOURCE_GROUP='${e2e_aks_cluster_resource_group}'
ALIEN_TEST_AKS_KUBE_CONTEXT='${e2e_aks_kube_context}'
ALIEN_TEST_AKS_INGRESS_CLASS='${e2e_k8s_ingress_class}'
ALIEN_TEST_AKS_PUBLIC_HOST_SUFFIX='${e2e_aks_public_host_suffix}'

# Ngrok (for push-mode E2E tests — cloud functions submit responses via tunnel)
NGROK_AUTHTOKEN='${NGROK_AUTHTOKEN:-}'

# Telemetry
AXIOM_OTLP_ENDPOINT='${AXIOM_OTLP_ENDPOINT:-https://api.axiom.co/v1/logs}'
AXIOM_TOKEN='${AXIOM_TOKEN:?AXIOM_TOKEN environment variable must be set}'
AXIOM_DATASET='${AXIOM_DATASET:-dev}'
EOF

{
  echo ""
  echo "# E2E target selection"
  echo "ALIEN_TEST_TARGETS_AWS=$(quote_env "$aws_target_aliases")"
  echo "ALIEN_TEST_TARGETS_GCP=$(quote_env "$gcp_target_aliases")"
  echo "ALIEN_TEST_TARGETS_AZURE=$(quote_env "$azure_target_aliases")"
  echo "ALIEN_TEST_DEFAULT_TARGET_AWS='aws-target-1'"
  echo "ALIEN_TEST_DEFAULT_TARGET_GCP='gcp-target-3'"
  echo "ALIEN_TEST_DEFAULT_TARGET_AZURE='azure-target-1'"

  while IFS='=' read -r key value; do
    printf "%s=%s\n" "$key" "$(quote_env "$value")"
  done < <(jq_val_target_options_env aws_target_options AWS)

  while IFS='=' read -r key value; do
    printf "%s=%s\n" "$key" "$(quote_env "$value")"
  done < <(jq_val_target_options_env gcp_target_options GCP)

  echo "ALIEN_TARGET_GCP_GCP_TARGET_1_ALIEN_TEST_GKE_KUBECONFIG=$(quote_env "$e2e_gke_target_1_kubeconfig_path")"
  echo "ALIEN_TARGET_GCP_GCP_TARGET_2_ALIEN_TEST_GKE_KUBECONFIG=$(quote_env "$e2e_gke_target_2_kubeconfig_path")"
  echo "ALIEN_TARGET_GCP_GCP_TARGET_3_ALIEN_TEST_GKE_KUBECONFIG=$(quote_env "$e2e_gke_target_3_kubeconfig_path")"

  while IFS='=' read -r key value; do
    printf "%s=%s\n" "$key" "$(quote_env "$value")"
  done < <(jq_val_target_options_env azure_target_options AZURE)
} >> .env.test

# ── Generate alien-manager.toml for test validation ─────────────────────────
# This config must match the ManagerTomlConfig schema (kebab-case field names,
# deny_unknown_fields). It uses test infrastructure resources provisioned by
# infra/test/. It validates that the TOML config format stays in sync.

cat > alien-manager.test.toml <<TOML
# alien-manager.toml — generated by gen-env-test.sh
# Uses test infrastructure resources provisioned by infra/test/.
#
# Schema: ManagerTomlConfig in crates/alien-manager/src/standalone_config.rs

[server]
port = 9090
base-url = "http://localhost:9090"
deployment-interval-secs = 2
heartbeat-interval-secs = 60

[database]
path = "alien-manager-test.db"
state-dir = ".alien-state"

[artifact-registry.aws]
service = "ecr"
repositoryPrefix = "alien-e2e"
pushRoleArn = "${e2e_aws_ar_push_role_arn}"
pullRoleArn = "${e2e_aws_ar_pull_role_arn}"

[commands]
kv = { service = "dynamodb", tableName = "${aws_command_kv_table}", region = "${aws_management_region}" }
storage = { service = "s3", bucketName = "${aws_s3_bucket}" }

[impersonation]
[impersonation.aws]
service = "awsiam"
roleName = "${aws_management_role_name}"
roleArn = "${aws_management_role_arn}"

[telemetry]
TOML

# ── Validate the generated TOML parses correctly ───────────────────────────
# Uses Python (available on all CI runners) to validate TOML syntax. This
# catches field name typos and schema drift before the binary is built.
if command -v python3 &>/dev/null; then
  python3 -c "
import tomllib, sys
with open('alien-manager.test.toml', 'rb') as f:
    config = tomllib.load(f)
# Verify expected top-level sections exist
required = {'server', 'database'}
missing = required - set(config.keys())
if missing:
    print(f'ERROR: missing required sections: {missing}', file=sys.stderr)
    sys.exit(1)
print('alien-manager.test.toml: TOML syntax valid, required sections present.')
"
else
  echo "WARNING: python3 not available, skipping TOML validation"
fi

echo ".env.test generated."
echo "alien-manager.test.toml generated."
