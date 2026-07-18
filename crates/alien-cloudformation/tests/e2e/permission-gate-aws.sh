#!/usr/bin/env bash
#
# Permission-gate apply-and-inspect e2e (AWS, real cloud).
#
# Renders the gated CloudFormation template (one Kv store whose kv/data-write
# grant on the service-account role is gated on the boolean `kvEnabled` input),
# deploys it with the input OFF then ON, and asserts the baked IAM role lacks
# then carries the gated `dynamodb:PutItem` action. Tears everything down.
#
# This proves the security end-state on real IAM: `count`/`Fn::Equals` gating a
# grant genuinely leaves it out of the role when the deployer opts out. It is
# not run in CI (real cloud); run it manually with target-account creds:
#
#   ./permission-gate-aws.sh [path-to-env-file]   # default: <repo-root>/.env.test
#
# The env file must export AWS_TARGET_ACCESS_KEY_ID / AWS_TARGET_SECRET_ACCESS_KEY
# / AWS_TARGET_REGION for an account allowed to create CFN stacks, IAM roles, and
# DynamoDB tables.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
ENV_FILE="${1:-$REPO_ROOT/.env.test}"
STACK="alien-permission-gate-e2e"
ROLE="${STACK}-execution-sa"
TEMPLATE="$(mktemp -t gated-cfn-XXXX.yaml)"

# shellcheck disable=SC1090
set -a; source "$ENV_FILE"; set +a
export AWS_ACCESS_KEY_ID="$AWS_TARGET_ACCESS_KEY_ID"
export AWS_SECRET_ACCESS_KEY="$AWS_TARGET_SECRET_ACCESS_KEY"
export AWS_REGION="${AWS_TARGET_REGION:-us-east-1}"
export AWS_DEFAULT_REGION="$AWS_REGION"

cleanup() {
  echo "--- cleanup: deleting stack + retained table ---"
  # The Kv table has DeletionPolicy: Retain and a CloudFormation-generated name
  # (${STACK}-Store-<random>), so resolve its real name while the stack still
  # exists, then delete it explicitly after the stack is gone.
  local table
  table="$(aws cloudformation describe-stack-resources --stack-name "$STACK" \
    --logical-resource-id Store --query 'StackResources[0].PhysicalResourceId' \
    --output text 2>/dev/null || true)"
  aws cloudformation delete-stack --stack-name "$STACK" 2>/dev/null || true
  aws cloudformation wait stack-delete-complete --stack-name "$STACK" 2>/dev/null || true
  if [ -n "$table" ] && [ "$table" != "None" ]; then
    aws dynamodb delete-table --table-name "$table" 2>/dev/null || true
    aws dynamodb wait table-not-exists --table-name "$table" 2>/dev/null || true
  fi
  # Ground-truth: fail loudly if anything with the stack prefix is left behind.
  local leftover
  leftover="$(aws dynamodb list-tables --query "TableNames[?starts_with(@,'$STACK')]" --output text 2>/dev/null || true)"
  [ -z "$leftover" ] || echo "WARN: leftover DynamoDB table(s) after teardown: $leftover"
  rm -f "$TEMPLATE"
}
trap cleanup EXIT

# The gated inline policy carries dynamodb:PutItem, so its presence on the role
# is the observable signal for whether the grant was baked in.
role_has_putitem() {
  local names
  names="$(aws iam list-role-policies --role-name "$ROLE" --query 'PolicyNames' --output text)"
  for name in $names; do
    if aws iam get-role-policy --role-name "$ROLE" --policy-name "$name" \
      --query 'PolicyDocument' --output json | grep -q 'dynamodb:PutItem'; then
      return 0
    fi
  done
  return 1
}

deploy() {
  local mode="$1"
  echo "=== deploy with kvEnabled=$mode ==="
  # Token is the (unused here) install token the setup page would supply; the
  # rest default. Only InputKvEnabled and Token need values.
  aws cloudformation deploy \
    --template-file "$TEMPLATE" \
    --stack-name "$STACK" \
    --parameter-overrides "InputKvEnabled=$mode" "Token=e2e-unused" \
    --capabilities CAPABILITY_NAMED_IAM \
    --no-fail-on-empty-changeset
}

echo "=== render the gated template ==="
(cd "$REPO_ROOT" && cargo run --quiet --example emit_gated_cfn -p alien-cloudformation) > "$TEMPLATE"
echo "rendered $(wc -l < "$TEMPLATE") lines to $TEMPLATE"

# Fresh start.
aws cloudformation delete-stack --stack-name "$STACK" 2>/dev/null || true
aws cloudformation wait stack-delete-complete --stack-name "$STACK" 2>/dev/null || true

deploy false
if role_has_putitem; then
  echo "FAIL: role $ROLE carries dynamodb:PutItem with the gate OFF (fail-open)"
  exit 1
fi
echo "PASS: gate OFF -> role lacks dynamodb:PutItem"

deploy true
if ! role_has_putitem; then
  echo "FAIL: role $ROLE lacks dynamodb:PutItem with the gate ON"
  exit 1
fi
echo "PASS: gate ON -> role carries dynamodb:PutItem"

echo "=== e2e PASSED: the gated grant follows the deployer input on real IAM ==="
