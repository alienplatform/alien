#!/usr/bin/env bash
set -uo pipefail

resource_prefix="${ALIEN_E2E_RESOURCE_PREFIX:-}"
if [ -z "$resource_prefix" ]; then
  if [ -z "${ALIEN_E2E_SLOT:-}" ]; then
    echo "ALIEN_E2E_SLOT or ALIEN_E2E_RESOURCE_PREFIX must be set" >&2
    exit 1
  fi
  resource_prefix="e2e-${ALIEN_E2E_SLOT}"
fi

matches_e2e_name() {
  local name="$1"
  case "$name" in
    "${resource_prefix}"-*|alien-e2e-initial-setup-*|alien-e2e-scoped-*) return 0 ;;
    *) return 1 ;;
  esac
}

cleanup_iam_role() {
  local role="$1"

  case "$role" in
    alien-e2e-eks-*)
      echo "Skipping Terraform-managed EKS role: $role"
      return
      ;;
  esac

  if ! matches_e2e_name "$role"; then
    return
  fi

  echo "Detaching policies and deleting IAM role: $role"
  local policies
  policies=$(aws iam list-attached-role-policies \
    --role-name "$role" \
    --query 'AttachedPolicies[].PolicyArn' \
    --output text) || true
  for policy in $policies; do
    aws iam detach-role-policy --role-name "$role" --policy-arn "$policy" || true
  done

  local inline_policies
  inline_policies=$(aws iam list-role-policies \
    --role-name "$role" \
    --query 'PolicyNames[]' \
    --output text) || true
  for policy_name in $inline_policies; do
    aws iam delete-role-policy --role-name "$role" --policy-name "$policy_name" || true
  done

  aws iam delete-role --role-name "$role" || true
}

cleanup_managed_policy() {
  local policy_name="$1"
  local policy_arn="$2"

  if ! matches_e2e_name "$policy_name"; then
    return
  fi

  echo "Detaching entities and deleting IAM policy: $policy_name"
  local roles
  roles=$(aws iam list-entities-for-policy \
    --policy-arn "$policy_arn" \
    --query 'PolicyRoles[].RoleName' \
    --output text) || true
  for role in $roles; do
    aws iam detach-role-policy --role-name "$role" --policy-arn "$policy_arn" || true
  done

  local users
  users=$(aws iam list-entities-for-policy \
    --policy-arn "$policy_arn" \
    --query 'PolicyUsers[].UserName' \
    --output text) || true
  for user in $users; do
    aws iam detach-user-policy --user-name "$user" --policy-arn "$policy_arn" || true
  done

  local groups
  groups=$(aws iam list-entities-for-policy \
    --policy-arn "$policy_arn" \
    --query 'PolicyGroups[].GroupName' \
    --output text) || true
  for group in $groups; do
    aws iam detach-group-policy --group-name "$group" --policy-arn "$policy_arn" || true
  done

  local versions
  versions=$(aws iam list-policy-versions \
    --policy-arn "$policy_arn" \
    --query 'Versions[?IsDefaultVersion==`false`].VersionId' \
    --output text) || true
  for version in $versions; do
    aws iam delete-policy-version --policy-arn "$policy_arn" --version-id "$version" || true
  done

  aws iam delete-policy --policy-arn "$policy_arn" || true
}

wait_for_stack_delete() {
  local stack="$1"
  local attempts=30
  local delay_seconds=30

  for _ in $(seq 1 "$attempts"); do
    local status
    local error_file
    error_file=$(mktemp)
    status=$(aws cloudformation describe-stacks \
      --stack-name "$stack" \
      --query 'Stacks[0].StackStatus' \
      --output text 2>"$error_file") && {
      rm -f "$error_file"
      case "$status" in
        DELETE_COMPLETE) return ;;
        DELETE_FAILED) return ;;
      esac
      sleep "$delay_seconds"
      continue
    }

    if grep -q "does not exist" "$error_file"; then
      rm -f "$error_file"
      return
    fi

    cat "$error_file" >&2 || true
    rm -f "$error_file"
    sleep "$delay_seconds"
  done

  echo "Timed out waiting for stack deletion: $stack" >&2
}

echo "Cleaning AWS E2E resources for resource prefix: $resource_prefix"

echo "Deleting CloudFormation stacks in target account..."
stacks=$(aws cloudformation list-stacks \
  --stack-status-filter CREATE_COMPLETE UPDATE_COMPLETE ROLLBACK_COMPLETE \
                        CREATE_FAILED UPDATE_FAILED DELETE_FAILED DELETE_IN_PROGRESS \
  --query 'StackSummaries[].StackName' --output text) || true
for stack in $stacks; do
  if ! matches_e2e_name "$stack"; then
    continue
  fi

  echo "Deleting stack: $stack"
  aws cloudformation delete-stack --stack-name "$stack" || true
done

echo "Waiting for stack deletions to complete..."
for stack in $stacks; do
  if ! matches_e2e_name "$stack"; then
    continue
  fi

  wait_for_stack_delete "$stack" || true
done

for prefix in "${resource_prefix}-"; do
  echo "Deleting orphaned Lambda functions with $prefix prefix..."
  lambdas=$(aws lambda list-functions \
    --query "Functions[?starts_with(FunctionName, \`${prefix}\`)].FunctionName" \
    --output text) || true
  for fn in $lambdas; do
    echo "Deleting Lambda function: $fn"
    aws lambda delete-function --function-name "$fn" || true
  done

  echo "Deleting orphaned SQS queues with $prefix prefix..."
  queues=$(aws sqs list-queues \
    --queue-name-prefix "$prefix" \
    --query 'QueueUrls[]' \
    --output text 2>/dev/null) || true
  for queue in $queues; do
    [ "$queue" = "None" ] && continue
    echo "Deleting SQS queue: $queue"
    aws sqs delete-queue --queue-url "$queue" || true
  done

  echo "Deleting orphaned DynamoDB tables with $prefix prefix..."
  tables=$(aws dynamodb list-tables \
    --query "TableNames[?starts_with(@, \`${prefix}\`)]" \
    --output text) || true
  for table in $tables; do
    echo "Deleting DynamoDB table: $table"
    aws dynamodb delete-table --table-name "$table" || true
  done

  echo "Deleting orphaned ECR repositories with $prefix prefix..."
  repositories=$(aws ecr describe-repositories \
    --query "repositories[?starts_with(repositoryName, \`${prefix}\`)].repositoryName" \
    --output text) || true
  for repository in $repositories; do
    echo "Deleting ECR repository: $repository"
    aws ecr delete-repository --repository-name "$repository" --force || true
  done

  echo "Deleting orphaned S3 buckets with $prefix prefix..."
  buckets=$(aws s3api list-buckets \
    --query "Buckets[?starts_with(Name, \`${prefix}\`)].Name" \
    --output text) || true
  for bucket in $buckets; do
    echo "Deleting S3 bucket: $bucket"
    aws s3 rm "s3://$bucket" --recursive || true
    aws s3api delete-bucket --bucket "$bucket" || true
  done

  echo "Deleting orphaned Secrets Manager secrets with $prefix prefix..."
  secrets=$(aws secretsmanager list-secrets \
    --include-planned-deletion \
    --query "SecretList[?starts_with(Name, \`${prefix}\`)].Name" \
    --output text) || true
  for secret in $secrets; do
    echo "Deleting secret: $secret"
    aws secretsmanager delete-secret \
      --secret-id "$secret" \
      --force-delete-without-recovery || true
  done
done

for param_prefix in "/${resource_prefix}-"; do
  echo "Deleting orphaned SSM parameters with $param_prefix prefix..."
  params=$(aws ssm describe-parameters \
    --parameter-filters "Key=Name,Option=BeginsWith,Values=${param_prefix}" \
    --query 'Parameters[].Name' \
    --output text) || true
  for param in $params; do
    echo "Deleting SSM parameter: $param"
    aws ssm delete-parameter --name "$param" || true
  done
done

echo "Deleting orphaned IAM roles..."
roles=$(aws iam list-roles --query 'Roles[].RoleName' --output text) || true
for role in $roles; do
  cleanup_iam_role "$role"
done

echo "Deleting orphaned IAM managed policies..."
policies=$(aws iam list-policies \
  --scope Local \
  --query 'Policies[].[PolicyName,Arn]' \
  --output text) || true
while read -r policy_name policy_arn; do
  [ -z "${policy_name:-}" ] && continue
  [ -z "${policy_arn:-}" ] && continue
  cleanup_managed_policy "$policy_name" "$policy_arn"
done <<< "$policies"

if [ "${ALIEN_E2E_CLEAN_EKS_OIDC_PROVIDER:-false}" = "true" ] && [ -n "${ALIEN_TEST_EKS_CLUSTER_NAME:-}" ]; then
  echo "Deleting EKS OIDC provider for cluster: $ALIEN_TEST_EKS_CLUSTER_NAME"
  issuer=$(aws eks describe-cluster \
    --name "$ALIEN_TEST_EKS_CLUSTER_NAME" \
    --query 'cluster.identity.oidc.issuer' \
    --output text 2>/dev/null | sed 's#^https://##') || true

  if [ -n "${issuer:-}" ] && [ "$issuer" != "None" ]; then
    providers=$(aws iam list-open-id-connect-providers \
      --query 'OpenIDConnectProviderList[].Arn' \
      --output text) || true
    for provider_arn in $providers; do
      provider_url=$(aws iam get-open-id-connect-provider \
        --open-id-connect-provider-arn "$provider_arn" \
        --query 'Url' \
        --output text 2>/dev/null) || true
      if [ "$provider_url" = "$issuer" ]; then
        echo "Deleting IAM OIDC provider: $provider_arn"
        aws iam delete-open-id-connect-provider \
          --open-id-connect-provider-arn "$provider_arn" || true
      fi
    done
  fi
elif [ -n "${ALIEN_TEST_EKS_CLUSTER_NAME:-}" ]; then
  echo "Skipping EKS OIDC provider cleanup for cluster: $ALIEN_TEST_EKS_CLUSTER_NAME"
fi
