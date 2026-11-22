#!/usr/bin/env bash
# Print shell exports of short-lived AWS credentials for the cloud tests, from your SSO sign-ins.
#
#   aws sso login --profile alien-test-mgmt && aws sso login --profile alien-test-target
#   eval "$(./scripts/cloud-test-credentials.sh)"
#
# Exports AWS_MANAGEMENT_{ACCESS_KEY_ID,SECRET_ACCESS_KEY,SESSION_TOKEN} and the AWS_TARGET_*
# equivalents. They expire with the SSO session; run it again after `aws sso login`.
# Profiles can be overridden with AWS_MANAGEMENT_PROFILE and AWS_TARGET_PROFILE.
set -euo pipefail

emit() { # emit <PREFIX> <profile>
  local json
  json=$(aws configure export-credentials --profile "$2" --format process) \
    || { echo "run: aws sso login --profile $2" >&2; exit 1; }
  jq -r --arg p "$1" '
    "export AWS_\($p)_ACCESS_KEY_ID=\(.AccessKeyId | @sh)",
    "export AWS_\($p)_SECRET_ACCESS_KEY=\(.SecretAccessKey | @sh)",
    "export AWS_\($p)_SESSION_TOKEN=\(.SessionToken | @sh)"' <<<"$json"
}

emit MANAGEMENT "${AWS_MANAGEMENT_PROFILE:-alien-test-mgmt}"
emit TARGET "${AWS_TARGET_PROFILE:-alien-test-target}"
