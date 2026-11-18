#!/usr/bin/env bash
# Fail when an Azure E2E run left custom role definitions or role assignments with its resource
# prefix in the target subscription. Runs after the test teardown, before the cleanup job, so a
# teardown that leaves roles behind fails the run instead of being tidied up silently.
#
# Matches (read-only, nothing is deleted):
#   - custom role definitions whose roleName starts with "<prefix>-";
#   - role assignments of such a role, and role assignments whose description names the prefix.
#
# Usage (az logged in to the target subscription):
#   ALIEN_E2E_RESOURCE_PREFIX=e2e-03-azcr AZURE_TARGET_SUBSCRIPTION_ID=... ./scripts/assert-azure-e2e-no-leftovers.sh
set -uo pipefail

prefix="${ALIEN_E2E_RESOURCE_PREFIX:-}"
if [[ ! "$prefix" =~ ^e2e-[0-9]{2}(-[a-z0-9]+)*$ ]]; then
  echo "ALIEN_E2E_RESOURCE_PREFIX must look like e2e-NN[-suffix], got '$prefix'" >&2
  exit 1
fi
: "${AZURE_TARGET_SUBSCRIPTION_ID:?AZURE_TARGET_SUBSCRIPTION_ID must be set}"
sub="$AZURE_TARGET_SUBSCRIPTION_ID"

roles_json="$(az role definition list --subscription "$sub" --custom-role-only true -o json)" || {
  echo "Could not list custom role definitions" >&2
  exit 1
}
assignments_json="$(az role assignment list --all --subscription "$sub" -o json)" || {
  echo "Could not list role assignments" >&2
  exit 1
}

roles="$(jq -r --arg p "${prefix}-" '.[] | select(.roleName | startswith($p))
  | "role definition id=\(.name) name=\(.roleName) scopes=\(.assignableScopes | join(","))"' <<< "$roles_json")"
assignments="$(jq -r --arg p "${prefix}-" --arg d "(${prefix}" '.[]
  | select(((.roleDefinitionName // "") | startswith($p)) or ((.description // "") | contains($d)))
  | "role assignment id=\(.id) role=\(.roleDefinitionName // "") scope=\(.scope)"' <<< "$assignments_json")"

if [ -z "$roles" ] && [ -z "$assignments" ]; then
  echo "No custom role definitions or role assignments with prefix ${prefix} remain"
  exit 0
fi
echo "Leftovers with prefix ${prefix} after teardown:" >&2
[ -z "$roles" ] || echo "$roles" >&2
[ -z "$assignments" ] || echo "$assignments" >&2
exit 1
