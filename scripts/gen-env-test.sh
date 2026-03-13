#!/usr/bin/env bash
# Generate .env.test from Terraform outputs.
# Usage: ./scripts/gen-env-test.sh
# Requires: terraform (logged in to TF Cloud), jq
set -euo pipefail

TF=$(cd infra/test && terraform output -json)
jq_val() { echo "$TF" | jq -r ".$1.value"; }

cat > .env.test <<EOF
# AWS - Management
AWS_MANAGEMENT_REGION=$(jq_val management_aws_region)
AWS_MANAGEMENT_ACCESS_KEY_ID=$(jq_val management_aws_access_key_id)
AWS_MANAGEMENT_SECRET_ACCESS_KEY=$(jq_val management_aws_secret_access_key)
AWS_MANAGEMENT_ACCOUNT_ID=$(jq_val management_aws_account_id)

# AWS - Target
AWS_TARGET_REGION=$(jq_val target_aws_region)
AWS_TARGET_ACCESS_KEY_ID=$(jq_val target_aws_access_key_id)
AWS_TARGET_SECRET_ACCESS_KEY=$(jq_val target_aws_secret_access_key)
AWS_TARGET_ACCOUNT_ID=$(jq_val target_aws_account_id)

# AWS test resources
ALIEN_TEST_AWS_S3_BUCKET=$(jq_val aws_s3_bucket)
ALIEN_TEST_AWS_LAMBDA_IMAGE=$(jq_val aws_lambda_image_uri)
ALIEN_TEST_AWS_LAMBDA_EXECUTION_ROLE_ARN=$(jq_val aws_lambda_execution_role_arn)

# GCP - Management
GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY=$(jq_val management_gcp_service_account_key)
GOOGLE_MANAGEMENT_PROJECT_ID=$(jq_val management_gcp_project_id)
GOOGLE_MANAGEMENT_REGION=$(jq_val management_gcp_region)

# GCP - Target
GOOGLE_TARGET_SERVICE_ACCOUNT_KEY=$(jq_val target_gcp_service_account_key)
GOOGLE_TARGET_PROJECT_ID=$(jq_val target_gcp_project_id)
GOOGLE_TARGET_REGION=$(jq_val target_gcp_region)

# GCP test resources
ALIEN_TEST_GCP_GCS_BUCKET=$(jq_val gcp_gcs_bucket)
ALIEN_TEST_GCP_CLOUDRUN_IMAGE=$(jq_val gcp_cloudrun_image_uri)

# Azure - Management
AZURE_MANAGEMENT_SUBSCRIPTION_ID=$(jq_val management_azure_subscription_id)
AZURE_MANAGEMENT_TENANT_ID=$(jq_val management_azure_tenant_id)
AZURE_MANAGEMENT_CLIENT_ID=$(jq_val management_azure_client_id)
AZURE_MANAGEMENT_CLIENT_SECRET=$(jq_val management_azure_client_secret)
AZURE_MANAGEMENT_REGION=$(jq_val management_azure_region)

# Azure - Target
AZURE_TARGET_SUBSCRIPTION_ID=$(jq_val target_azure_subscription_id)
AZURE_TARGET_TENANT_ID=$(jq_val target_azure_tenant_id)
AZURE_TARGET_CLIENT_ID=$(jq_val target_azure_client_id)
AZURE_TARGET_CLIENT_SECRET=$(jq_val target_azure_client_secret)

AZURE_REGION=$(jq_val management_azure_region)

# Azure test resources
ALIEN_TEST_AZURE_RESOURCE_GROUP=$(jq_val azure_resource_group)
ALIEN_TEST_AZURE_STORAGE_ACCOUNT=$(jq_val azure_storage_account)
ALIEN_TEST_AZURE_TEST_BLOB_CONTAINER=$(jq_val azure_blob_container)
ALIEN_TEST_AZURE_CONTAINER_APP_IMAGE=$(jq_val azure_container_app_image_uri)
ALIEN_TEST_AZURE_MANAGED_ENVIRONMENT_NAME=$(jq_val azure_managed_environment)
ALIEN_TEST_AZURE_REGISTRY_NAME=$(jq_val azure_acr_name)

# Telemetry
AXIOM_OTLP_ENDPOINT=$(jq_val axiom_otlp_endpoint)
AXIOM_TOKEN=$(jq_val axiom_token)
AXIOM_DATASET=$(jq_val axiom_dataset)
EOF

echo ".env.test generated."
