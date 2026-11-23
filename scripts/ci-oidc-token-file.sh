#!/usr/bin/env bash
# Keeps a GitHub Actions OIDC token for a cloud's federated credentials in a file.
#
# Azure clients read the token from AZURE_FEDERATED_TOKEN_FILE; Google external account
# credentials read it from their credential source file. A GitHub OIDC token
# lives only a few minutes, so a background loop replaces the file (atomically) before it ends.
#
# Usage: ci-oidc-token-file.sh FILE [AUDIENCE]   (audience defaults to api://AzureADTokenExchange)
# Needs the job's `id-token: write` permission.
set -euo pipefail

file="${1:?usage: ci-oidc-token-file.sh FILE [AUDIENCE]}"
AUDIENCE="${2:-api://AzureADTokenExchange}"
export ENCODED_AUDIENCE
ENCODED_AUDIENCE=$(jq -rn --arg a "$AUDIENCE" '$a|@uri')

fetch() {
  local token
  token=$(curl -sf -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
    "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=$ENCODED_AUDIENCE" | jq -er .value)
  echo "::add-mask::$token"
  umask 077
  printf '%s' "$token" > "$file.tmp"
  mv "$file.tmp" "$file"
}

fetch
# The request token and URL stay in the environment, not on the command line.
export TOKEN_FILE="$file"
setsid nohup bash -c '
  while sleep 240; do
    token=$(curl -sf -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" \
      "$ACTIONS_ID_TOKEN_REQUEST_URL&audience=$ENCODED_AUDIENCE" | jq -er .value) || continue
    printf "%s" "$token" > "$TOKEN_FILE.tmp" && mv "$TOKEN_FILE.tmp" "$TOKEN_FILE"
  done
' >/dev/null 2>&1 &
