#!/bin/bash
# Probes with a stand-in principal holding the roles the real setup created.
set -uo pipefail
W=/workspace/a1690; cd $W
ADMIN="env AZURE_CONFIG_DIR=$W/az-mgmt az"; MGMT="env AZURE_CONFIG_DIR=$W/az-vro az"
SUB=$($ADMIN account show --query id -o tsv); RG=alien1609-vro-tf; RGID=/subscriptions/$SUB/resourceGroups/$RG
KV=$($ADMIN keyvault list -g $RG --query "[0].name" -o tsv); KVID=$RGID/providers/Microsoft.KeyVault/vaults/$KV
OID=$(cat vro-oid.txt 2>/dev/null || true)
role_id() { $ADMIN role definition list --custom-role-only true --query "[?roleName=='$1'].id | [0]" -o tsv; }
relogin() { rm -rf $W/az-vro; $MGMT login --service-principal -u $(cat vro-app.txt) --password="$(cat vro-secret.txt)" --tenant $($ADMIN account show --query tenantId -o tsv) --allow-no-subscriptions -o none; }
case $1 in
standin)
  APP=$($ADMIN ad app create --display-name alien1609-vro-mgmt --query appId -o tsv); echo $APP > vro-app.txt
  OID=$($ADMIN ad sp create --id $APP --query id -o tsv); echo $OID > vro-oid.txt
  (umask 077; $ADMIN ad app credential reset --id $APP --years 1 --query password -o tsv > vro-secret.txt)
  echo "stand-in ${OID:0:8}" ;;
grant_main) # what the main setup gives the management identity
  $ADMIN role assignment create --assignee-object-id $OID --assignee-principal-type ServicePrincipal --role "$(role_id a1609vro-management-resource-group-role)" --scope $RGID -o none && echo "RG management role"
  $ADMIN role assignment create --assignee-object-id $OID --assignee-principal-type ServicePrincipal --role "$(role_id 'a1609vro-Write Key Vault Secrets [mgmt]')" --scope $KVID -o none && echo "vault write role"
  $ADMIN role assignment create --assignee-object-id $(set -a; source ~/creds/env.test >/dev/null 2>&1; echo $AZURE_MANAGEMENT_PRINCIPAL_ID) --assignee-principal-type ServicePrincipal --role "Key Vault Secrets Officer" --scope $KVID -o none && echo "setup identity may write the test secret"
  for i in $(seq 1 12); do $ADMIN keyvault secret set --vault-name $KV -n api-key --value vault-value -o none 2>/dev/null && { echo "test secret written"; break; }; sleep 10; done ;;
grant_pr) # the extra conditioned assignment the PR setup adds
  COND=$($ADMIN role assignment list --scope $RGID --query "[?condition!=null].condition | [0]" -o tsv | sed "s/{[0-9a-f-]*}))$/{$OID}))/")
  echo "condition: $COND" | cut -c1-220
  $ADMIN role assignment create --assignee-object-id $OID --assignee-principal-type ServicePrincipal --role "$(role_id a1609vro-management-role-assignments-resource-group-role)" --scope $RGID --condition "$COND" --condition-version 2.0 -o none && echo "conditioned role-assignment role" ;;
probe)
  L=$2; sleep ${WAIT:-60}; relogin
  WL=$($ADMIN identity show -g $RG -n alien1609-vro-workload2 --query principalId -o tsv 2>/dev/null || $ADMIN identity create -g $RG -n alien1609-vro-workload2 --query principalId -o tsv)
  try() { if r=$($MGMT role assignment create --assignee-object-id $2 --assignee-principal-type ServicePrincipal --role "Key Vault Secrets User" --scope $KVID --query id -o tsv 2>/tmp/a1707-err); then echo "[$L] $1: ALLOWED"; $ADMIN role assignment delete --ids "$r" -o none; else echo "[$L] $1: DENIED ($(grep -o AuthorizationFailed /tmp/a1707-err | head -1))"; fi; }
  try "assign itself Key Vault Secrets User" $OID
  try "assign the workload identity Key Vault Secrets User" $WL
  r=$($MGMT resource update --ids $KVID --set properties.enableRbacAuthorization=false --query properties.enableRbacAuthorization -o tsv 2>&1)
  if [ "$r" = false ]; then echo "[$L] switch the vault to access policies: ALLOWED"; $ADMIN resource update --ids $KVID --set properties.enableRbacAuthorization=true -o none; else echo "[$L] switch the vault to access policies: DENIED ($(echo "$r" | grep -o 'InsufficientPermissions\|AuthorizationFailed' | head -1))"; fi
  for R in "a1609vro-Write Key Vault Secrets [mgmt]" a1609vro-management-resource-group-role; do
    ID=$(role_id "$R"); URL="https://management.azure.com$RGID/providers/Microsoft.Authorization/roleDefinitions/${ID##*/}?api-version=2022-04-01"
    $ADMIN rest --method GET --url "$URL" > rd.json
    python3 -c "
import json; d=json.load(open('rd.json')); p=d['properties']['permissions'][0]
p['dataActions']=sorted(set((p.get('dataActions') or [])+['Microsoft.KeyVault/vaults/secrets/getSecret/action']))
json.dump({'properties':d['properties']},open('rd.new.json','w'))"
    if $MGMT rest --method PUT --url "$URL" --body @rd.new.json -o none 2>/tmp/a1707-err; then echo "[$L] add getSecret to its role '$R': ALLOWED"; $ADMIN rest --method PUT --url "$URL" --body @rd.json -o none && echo "    (restored)"; else echo "[$L] add getSecret to its role '$R': DENIED ($(grep -o AuthorizationFailed /tmp/a1707-err | head -1))"; fi
  done
  if $MGMT keyvault secret set --vault-name $KV -n api-key --value "$L-write" -o none 2>/dev/null; then echo "[$L] setSecret: ALLOWED"; else echo "[$L] setSecret: DENIED"; fi
  r=$($MGMT keyvault secret show --vault-name $KV -n api-key --query value -o tsv 2>&1); echo "[$L] getSecret: $(echo "$r" | grep -q -- "-write$\|vault-value" && echo "ALLOWED ($r)" || echo "DENIED ($(echo "$r" | grep -o Forbidden | head -1))")" ;;
secret) (umask 077; for i in $(seq 1 12); do $ADMIN ad app credential reset --id $(cat vro-app.txt) --years 1 --query password -o tsv > vro-secret.txt 2>/dev/null && [ -s vro-secret.txt ] && break; sleep 10; done); echo "credential ready: $( [ -s vro-secret.txt ] && echo yes)" ;;
cleanup_standin)
  APPID=$(cat vro-app.txt); OBJ=$($ADMIN ad app show --id $APPID --query id -o tsv 2>/dev/null)
  $ADMIN ad app delete --id $APPID && echo "app deleted"
  for i in $(seq 1 12); do $ADMIN rest --method DELETE --url "https://graph.microsoft.com/v1.0/directory/deletedItems/$OBJ" 2>/dev/null && { echo "app purged"; break; }; sleep 10; done
  rm -rf $W/az-vro vro-secret.txt rd.json rd.new.json ;;
esac
