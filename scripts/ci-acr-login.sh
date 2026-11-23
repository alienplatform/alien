#!/usr/bin/env bash
# Logs docker in to an Azure container registry with a federated (OIDC) token, no stored secret.
#
# Usage: ci-acr-login.sh REGISTRY TENANT_ID CLIENT_ID TOKEN_FILE
set -euo pipefail

registry="${1:?registry}"; tenant="${2:?tenant id}"; client="${3:?client id}"; token_file="${4:?token file}"

aad=$(curl -sf "https://login.microsoftonline.com/$tenant/oauth2/v2.0/token" \
  --data-urlencode "client_id=$client" \
  --data-urlencode "scope=https://management.azure.com/.default" \
  --data-urlencode "grant_type=client_credentials" \
  --data-urlencode "client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer" \
  --data-urlencode "client_assertion=$(cat "$token_file")" | jq -er .access_token)
echo "::add-mask::$aad"

refresh=$(curl -sf "https://$registry/oauth2/exchange" \
  --data-urlencode "grant_type=access_token" \
  --data-urlencode "service=$registry" \
  --data-urlencode "tenant=$tenant" \
  --data-urlencode "access_token=$aad" | jq -er .refresh_token)
echo "::add-mask::$refresh"

printf '%s' "$refresh" | docker login "$registry" -u 00000000-0000-0000-0000-000000000000 --password-stdin
