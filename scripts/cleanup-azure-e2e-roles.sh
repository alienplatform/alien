#!/usr/bin/env bash
# Delete the custom Azure role definitions a cloud E2E run left in the target subscription.
#
# Custom role definitions are directory objects: deleting a run's resource groups leaves the
# roles setup wrote for them, and a role name is unique per directory, so the next run with the
# same resource prefix is refused. This runs only with the E2E subscription credentials, before
# setup and in cleanup, never in a product path.
#
# A role is deleted only when BOTH hold:
#   - its roleName starts with the run resource prefix followed by "-";
#   - every assignable scope is a resource group of this run (the same group names the cleanup
#     job deletes). Any role with another scope is listed as skipped and kept.
# Role assignments of a role are deleted first. Exact ids are logged before anything is deleted.
#
# Usage (az logged in to the target subscription):
#   ALIEN_E2E_SLOT=03 AZURE_TARGET_SUBSCRIPTION_ID=... ./scripts/cleanup-azure-e2e-roles.sh
#   ALIEN_E2E_RESOURCE_PREFIX=e2e-03-azcr ... ./scripts/cleanup-azure-e2e-roles.sh
set -uo pipefail

prefix="${ALIEN_E2E_RESOURCE_PREFIX:-}"
slot="${ALIEN_E2E_SLOT:-}"
if [ -z "$prefix" ]; then
  if [[ ! "$slot" =~ ^[0-9]{2}$ ]]; then
    echo "ALIEN_E2E_SLOT (two digits) or ALIEN_E2E_RESOURCE_PREFIX must be set" >&2
    exit 1
  fi
  prefix="e2e-${slot}"
fi
if [[ ! "$prefix" =~ ^e2e-[0-9]{2}(-[a-z0-9]+)*$ ]]; then
  echo "Refusing resource prefix '$prefix': it must look like e2e-NN[-suffix]" >&2
  exit 1
fi
: "${AZURE_TARGET_SUBSCRIPTION_ID:?AZURE_TARGET_SUBSCRIPTION_ID must be set}"
sub="$AZURE_TARGET_SUBSCRIPTION_ID"
slot_prefix="e2e-$(echo "$prefix" | cut -d- -f2)"

# A resource group of this run: same names the cleanup job deletes (never the shared ones).
is_run_group() {
  local rg="$1"
  case "$rg" in alien-e2e-shared-*) return 1 ;; esac
  [[ "$rg" =~ ^[a-z][0-9a-f]{7}-default-resource-group$ ]] \
    || [[ "$rg" =~ ^alien-e2e-[0-9a-f]{8}$ ]] \
    || [[ "$rg" == "$slot_prefix"-* ]]
}

# Every scope must be /subscriptions/<sub>/resourceGroups/<run group>, and there must be one.
all_scopes_in_run_groups() {
  local scopes="$1" scope rg count=0
  while IFS= read -r scope; do
    [ -n "$scope" ] || continue
    count=$((count + 1))
    if [[ "$scope" =~ ^/subscriptions/${sub}/resourceGroups/([^/]+)$ ]]; then
      rg="${BASH_REMATCH[1]}"
      is_run_group "$rg" || return 1
    else
      return 1
    fi
  done <<< "$scopes"
  [ "$count" -gt 0 ]
}

roles_json="$(az role definition list --subscription "$sub" --custom-role-only true -o json)" || {
  echo "Could not list custom role definitions; nothing deleted" >&2
  exit 1
}

mapfile -t rows < <(jq -r --arg p "${prefix}-" '
  .[] | select(.roleName | startswith($p))
      | [.name, .roleName, (.assignableScopes | join(","))] | @tsv' <<< "$roles_json")

to_delete=()
for row in "${rows[@]}"; do
  IFS=$'\t' read -r role_id role_name scopes_csv <<< "$row"
  scopes="${scopes_csv//,/$'\n'}"
  if all_scopes_in_run_groups "$scopes"; then
    echo "Will delete custom role: id=$role_id name='$role_name' scopes=$scopes_csv"
    to_delete+=("$role_id")
  else
    echo "Skipping custom role (a scope is not a resource group of this run): id=$role_id name='$role_name' scopes=$scopes_csv"
  fi
done

if [ "${#to_delete[@]}" -eq 0 ]; then
  echo "No custom role definitions to delete for prefix ${prefix}-"
  exit 0
fi

status=0
for role_id in "${to_delete[@]}"; do
  mapfile -t assignments < <(az role assignment list --all --subscription "$sub" \
    --role "$role_id" --query "[].id" -o tsv)
  for assignment_id in "${assignments[@]}"; do
    [ -n "$assignment_id" ] || continue
    echo "Deleting role assignment: $assignment_id (role $role_id)"
    az role assignment delete --ids "$assignment_id" || status=1
  done
  echo "Deleting role definition: $role_id"
  az role definition delete --name "$role_id" --scope "/subscriptions/$sub" || status=1
done
exit "$status"
