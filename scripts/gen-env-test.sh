#!/usr/bin/env bash
# Generate .env.test from Terraform outputs.
# Usage: ./scripts/gen-env-test.sh
# Requires: terraform (init'd, authenticated to TF Cloud), jq
#
# Environment variables required:
#   AXIOM_TOKEN         - Axiom API token
# Environment variables optional:
#   AXIOM_OTLP_ENDPOINT - (default: https://api.axiom.co/v1/logs)
#   AXIOM_DATASET       - (default: dev)
set -euo pipefail

TF=$(cd infra/standalone && terraform output -json)
jq_val() { echo "$TF" | jq -r ".$1.value"; }

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

aws_s3_bucket=$(jq_val aws_s3_bucket)
aws_lambda_image=$(jq_val aws_lambda_image_uri)
aws_lambda_execution_role_arn=$(jq_val aws_lambda_execution_role_arn)
aws_ecr_push_role_arn=$(jq_val aws_ecr_push_role_arn)
aws_ecr_pull_role_arn=$(jq_val aws_ecr_pull_role_arn)
aws_ecr_repository=$(echo "$aws_lambda_image" | cut -d: -f1)

gcp_management_sa_key=$(jq_val management_gcp_service_account_key)
gcp_management_project_id=$(jq_val management_gcp_project_id)
gcp_management_region=$(jq_val management_gcp_region)

gcp_target_sa_key=$(jq_val target_gcp_service_account_key)
gcp_target_project_id=$(jq_val target_gcp_project_id)
gcp_target_region=$(jq_val target_gcp_region)

gcp_management_identity_email=$(jq_val gcp_management_identity_email)
gcp_management_identity_unique_id=$(jq_val gcp_management_identity_unique_id)

gcp_gcs_bucket=$(jq_val gcp_gcs_bucket)
gcp_cloudrun_image=$(jq_val gcp_cloudrun_image_uri)
gcp_gar_repository=$(echo "$gcp_cloudrun_image" | cut -d: -f1)

azure_management_subscription_id=$(jq_val management_azure_subscription_id)
azure_management_tenant_id=$(jq_val management_azure_tenant_id)
azure_management_client_id=$(jq_val management_azure_client_id)
azure_management_client_secret=$(jq_val management_azure_client_secret)
azure_management_region=$(jq_val management_azure_region)

azure_management_sp_client_id=$(jq_val management_azure_sp_client_id)
azure_management_sp_client_secret=$(jq_val management_azure_sp_client_secret)
azure_management_sp_object_id=$(jq_val management_azure_sp_object_id)

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

# AWS test resources
ALIEN_TEST_AWS_S3_BUCKET='${aws_s3_bucket}'
ALIEN_TEST_AWS_LAMBDA_IMAGE='${aws_lambda_image}'
ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN='${aws_lambda_execution_role_arn}'
ALIEN_TEST_AWS_ECR_PUSH_ROLE_ARN='${aws_ecr_push_role_arn}'
ALIEN_TEST_AWS_ECR_PULL_ROLE_ARN='${aws_ecr_pull_role_arn}'
ALIEN_TEST_AWS_ECR_REPOSITORY='${aws_ecr_repository}'

# GCP - Management
GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY='${gcp_management_sa_key}'
GOOGLE_MANAGEMENT_PROJECT_ID='${gcp_management_project_id}'
GOOGLE_MANAGEMENT_REGION='${gcp_management_region}'

# GCP - Target
GOOGLE_TARGET_SERVICE_ACCOUNT_KEY='${gcp_target_sa_key}'
GOOGLE_TARGET_PROJECT_ID='${gcp_target_project_id}'
GOOGLE_TARGET_REGION='${gcp_target_region}'

# GCP - Management Identity
GOOGLE_MANAGEMENT_IDENTITY_EMAIL='${gcp_management_identity_email}'
GOOGLE_MANAGEMENT_IDENTITY_UNIQUE_ID='${gcp_management_identity_unique_id}'

# GCP test resources
ALIEN_TEST_GCP_GCS_BUCKET='${gcp_gcs_bucket}'
ALIEN_TEST_GCP_CLOUDRUN_IMAGE='${gcp_cloudrun_image}'
ALIEN_TEST_GCP_GAR_REPOSITORY='${gcp_gar_repository}'

# Azure - Management
AZURE_MANAGEMENT_SUBSCRIPTION_ID='${azure_management_subscription_id}'
AZURE_MANAGEMENT_TENANT_ID='${azure_management_tenant_id}'
AZURE_MANAGEMENT_CLIENT_ID='${azure_management_client_id}'
AZURE_MANAGEMENT_CLIENT_SECRET='${azure_management_client_secret}'
AZURE_MANAGEMENT_REGION='${azure_management_region}'

# Azure - Management Service Principal (local dev fallback)
AZURE_MANAGEMENT_SP_CLIENT_ID='${azure_management_sp_client_id}'
AZURE_MANAGEMENT_SP_CLIENT_SECRET='${azure_management_sp_client_secret}'
AZURE_MANAGEMENT_SP_OBJECT_ID='${azure_management_sp_object_id}'

# Azure OIDC (set dynamically in CI, empty for local dev = SP fallback)
AZURE_MANAGEMENT_OIDC_ISSUER=''
AZURE_MANAGEMENT_OIDC_SUBJECT=''

# Azure - Target
AZURE_TARGET_SUBSCRIPTION_ID='${azure_target_subscription_id}'
AZURE_TARGET_TENANT_ID='${azure_target_tenant_id}'
AZURE_TARGET_CLIENT_ID='${azure_target_client_id}'
AZURE_TARGET_CLIENT_SECRET='${azure_target_client_secret}'

AZURE_REGION='${azure_management_region}'

# Azure test resources
ALIEN_TEST_AZURE_RESOURCE_GROUP='${azure_resource_group}'
ALIEN_TEST_AZURE_STORAGE_ACCOUNT='${azure_storage_account}'
ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER='${azure_blob_container}'
ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE='${azure_container_app_image}'
ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME='${azure_managed_environment}'
ALIEN_TEST_AZURE_REGISTRY_NAME='${azure_acr_name}'
ALIEN_TEST_AZURE_ACR_REPOSITORY='${azure_acr_repository}'

# Telemetry
AXIOM_OTLP_ENDPOINT='${AXIOM_OTLP_ENDPOINT:-https://api.axiom.co/v1/logs}'
AXIOM_TOKEN='${AXIOM_TOKEN:?AXIOM_TOKEN environment variable must be set}'
AXIOM_DATASET='${AXIOM_DATASET:-dev}'
EOF

echo ".env.test generated."
