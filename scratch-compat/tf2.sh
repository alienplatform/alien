#!/bin/bash
# tf2.sh use <main|pr> | plan | apply | destroy   (compat-proof setup, RG alien1609-vro-compat)
set -uo pipefail
set -a; source ~/creds/env.test >/dev/null 2>&1; set +a
W=/workspace/a1690; D=$W/tfcompat
export ARM_CLIENT_ID=$AZURE_MANAGEMENT_CLIENT_ID ARM_CLIENT_SECRET=$AZURE_MANAGEMENT_CLIENT_SECRET ARM_TENANT_ID=$AZURE_MANAGEMENT_TENANT_ID ARM_SUBSCRIPTION_ID=$AZURE_MANAGEMENT_SUBSCRIPTION_ID
export TF_PLUGIN_CACHE_DIR=$W/tfcache TF_PLUGIN_CACHE_MAY_BREAK_DEPENDENCY_LOCK_FILE=1 TF_IN_AUTOMATION=1
VARS=(-var "name=alien1609-vro" -var "token=unused" -var "resource_prefix=a1609cmp" -var "azure_subscription_id=$ARM_SUBSCRIPTION_ID" -var "azure_resource_group_name=alien1609-vro-compat" -var "azure_location=eastus")
targets() { grep -ho '^resource "azurerm_role_assignment" "[^"]*"' $D/*.tf | awk -F'"' '{print "-target=azurerm_role_assignment."$4}'; }
case $1 in
use) mkdir -p $D $W/tfcache; find $D -maxdepth 1 -name '*.tf' -delete; cp $W/compat-$2/*.tf $D/; cd $D && terraform init -input=false -no-color >/dev/null && echo "module: $2" ;;
plan) cd $D && terraform plan -input=false -no-color "${VARS[@]}" $(targets) -out=plan.bin 2>&1 | grep -E "^  # |Plan:|forces replacement|Error" ;;
apply) cd $D && for i in 1 2 3; do terraform apply -input=false -no-color -auto-approve "${VARS[@]}" $(targets) 2>&1 | grep -E "Apply complete|Error:" | head -5 | tee /tmp/a1690-apply.txt; grep -q "Apply complete" /tmp/a1690-apply.txt && break; sleep 20; done ;;
destroy) cd $D && terraform destroy -input=false -no-color -auto-approve "${VARS[@]}" 2>&1 | grep -E "Destroy complete|Error" | head ;;
out) cd $D && terraform output -json 2>/dev/null | head -c 0; terraform state show -no-color azurerm_user_assigned_identity.execution_sa | grep -E " (id|principal_id|client_id) " ;;
esac
