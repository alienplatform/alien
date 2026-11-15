#!/usr/bin/env bash
# Live proof of Azure `internet` egress, end to end, then teardown with proof of deletion.
#
#   scripts/azure-internet-live.sh run        # apply, prove, tear down
#   scripts/azure-internet-live.sh teardown   # only tear down (after an interrupted run)
#
# What it does: renders the shipped Terraform module (alien-terraform's azure_internet_live_render
# test), applies it, runs the real StackExecutor over the setup (alien-infra's azure_internet_live
# test), which publishes the gateway config and the sandbox binding, then runs the in-VNet driver
# (alien-bindings' azure_sandbox_internet_live test) as a Container Apps job on the setup's
# container-apps subnet. The driver's verdict comes back through a Key Vault secret.
#
# Needs: bash, curl, jq, python3, docker, terraform, cargo, a musl `alien-sandbox-agent` and
# `alien-egress-gateway` (see AGENT_BINARY and GATEWAY_BINARY), and a service principal that can
# create resources and role assignments in the subscription.
#
# Env (nothing is defaulted that names a customer, subscription or secret):
#   AZURE_TARGET_SUBSCRIPTION_ID, AZURE_TARGET_TENANT_ID, AZURE_TARGET_CLIENT_ID, AZURE_TARGET_CLIENT_SECRET
#   AZURE_INTERNET_PREFIX      short lowercase prefix, 3-10 chars (names every resource)
#   AZURE_INTERNET_LOCATION    region, default eastus
#   AZURE_INTERNET_VM_SIZE     gateway VM size, default Standard_D2s_v4 (pick one with quota)
#   AGENT_BINARY               path to a static (musl) alien-sandbox-agent
#   GATEWAY_BINARY             path to a static (musl) alien-egress-gateway built from this branch
#                              (the pinned release image can predate the admin protocol; bump the pin at release)
#   AZURE_INTERNET_KEEP        set to keep the resources after the run (debugging); tear down with the `teardown` command
#   AZURE_INTERNET_WORK        working directory, default ./azure-internet-work (kept for the evidence)
#
# Every resource lives in two resource groups, `<prefix>-rg` (the module) and `<prefix>-acr-rg`
# (the registry), and teardown deletes both and proves it with a 404 and a list.
set -euo pipefail

cmd="${1:-run}"
: "${AZURE_TARGET_SUBSCRIPTION_ID:?}" "${AZURE_TARGET_TENANT_ID:?}" "${AZURE_TARGET_CLIENT_ID:?}" "${AZURE_TARGET_CLIENT_SECRET:?}"
: "${AZURE_INTERNET_PREFIX:?}"
LOCATION="${AZURE_INTERNET_LOCATION:-eastus}"
VM_SIZE="${AZURE_INTERNET_VM_SIZE:-Standard_D2s_v4}"
WORK="${AZURE_INTERNET_WORK:-$PWD/azure-internet-work}"
P="$AZURE_INTERNET_PREFIX"
SUB="$AZURE_TARGET_SUBSCRIPTION_ID"
RG="$P-rg"
ACR_RG="$P-acr-rg"
ACR="${P}acr"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$WORK"

token() { # token <scope>
  curl -fsS --max-time 60 -X POST "https://login.microsoftonline.com/$AZURE_TARGET_TENANT_ID/oauth2/v2.0/token" \
    -d grant_type=client_credentials -d client_id="$AZURE_TARGET_CLIENT_ID" \
    -d client_secret="$AZURE_TARGET_CLIENT_SECRET" -d scope="$1" | jq -r .access_token
}
arm() { # arm <METHOD> <path> [curl args]; prints the body, fails on HTTP errors
  local method=$1 path=$2; shift 2
  curl -sS --max-time 120 -X "$method" -H "Authorization: Bearer $(token https://management.azure.com/.default)" \
    -H 'content-type: application/json' "https://management.azure.com$path" "$@"
}
arm_status() { # arm_status <METHOD> <path>  -> HTTP status only
  curl -sS --max-time 60 -o /dev/null -w '%{http_code}' -X "$1" -H "Authorization: Bearer $(token https://management.azure.com/.default)" \
    "https://management.azure.com$2"
}

teardown() {
  local out="$WORK/cleanup.txt"
  : > "$out"
  for group in "$RG" "$ACR_RG"; do
    echo "$(date -u +%FT%TZ) DELETE resource group $group" | tee -a "$out"
    arm DELETE "/subscriptions/$SUB/resourcegroups/$group?api-version=2021-04-01" >/dev/null || true
  done
  for group in "$RG" "$ACR_RG"; do
    for _ in $(seq 1 480); do
      status=$(arm_status GET "/subscriptions/$SUB/resourcegroups/$group?api-version=2021-04-01")
      [ "$status" = 404 ] && break
      sleep 15
    done
    echo "$(date -u +%FT%TZ) GET resource group $group -> $status" | tee -a "$out"
    [ "$status" = 404 ] || { echo "resource group $group still exists" | tee -a "$out"; exit 1; }
  done
  echo "--- resources left under the prefix (must be empty):" | tee -a "$out"
  left=$(arm GET "/subscriptions/$SUB/resources?api-version=2021-04-01&\$filter=startswith(resourceGroup,'$P-')" | jq -r '.value[]?.id')
  echo "${left:-<none>}" | tee -a "$out"
  [ -z "$left" ] || exit 1
  # Role assignments scoped inside a deleted group go with it; list any left at subscription scope.
  arm GET "/subscriptions/$SUB/providers/Microsoft.Authorization/roleAssignments?api-version=2022-04-01&\$filter=principalId%20eq%20'$(sp_object_id)'" \
    | jq -r '.value[]? | select(.properties.scope | contains("'"$P"'")) | .id' | tee -a "$out"
  echo "cleanup complete" | tee -a "$out"
}

sp_object_id() {
  curl -fsS --max-time 60 -H "Authorization: Bearer $(token https://graph.microsoft.com/.default)" \
    "https://graph.microsoft.com/v1.0/servicePrincipals(appId='$AZURE_TARGET_CLIENT_ID')?\$select=id" | jq -r .id
}

if [ "$cmd" = teardown ]; then teardown; exit 0; fi
: "${AGENT_BINARY:?}" "${GATEWAY_BINARY:?}"
export PATH="$HOME/bin:$PATH"
command -v terraform >/dev/null || { echo "terraform is required"; exit 1; }

# Whatever happens next, tear everything down and prove it.
[ -n "${AZURE_INTERNET_KEEP:-}" ] || trap 'status=$?; trap - EXIT; set +e; teardown; code=$?; [ "$status" -ne 0 ] && exit "$status"; exit "$code"' EXIT

# 1. The registry, in its own group so the module's group can be applied and destroyed freely.
echo "== registry"
arm PUT "/subscriptions/$SUB/resourcegroups/$ACR_RG?api-version=2021-04-01" -d "{\"location\":\"$LOCATION\"}" >/dev/null
arm PUT "/subscriptions/$SUB/resourceGroups/$ACR_RG/providers/Microsoft.ContainerRegistry/registries/$ACR?api-version=2023-07-01" \
  -d "{\"location\":\"$LOCATION\",\"sku\":{\"name\":\"Standard\"},\"properties\":{\"adminUserEnabled\":true,\"anonymousPullEnabled\":true}}" >/dev/null
for _ in $(seq 1 40); do
  state=$(arm GET "/subscriptions/$SUB/resourceGroups/$ACR_RG/providers/Microsoft.ContainerRegistry/registries/$ACR?api-version=2023-07-01" | jq -r .properties.provisioningState)
  [ "$state" = Succeeded ] && break; sleep 5
done
creds=$(arm POST "/subscriptions/$SUB/resourceGroups/$ACR_RG/providers/Microsoft.ContainerRegistry/registries/$ACR/listCredentials?api-version=2023-07-01" -d '')
ACR_USER=$(jq -r .username <<<"$creds"); ACR_PASS=$(jq -r '.passwords[0].value' <<<"$creds")
REGISTRY="$ACR.azurecr.io"
echo "$ACR_PASS" | docker login "$REGISTRY" -u "$ACR_USER" --password-stdin >/dev/null

# 2. Images: the conformance fixture assembled with the agent the way a release does (root
#    ENTRYPOINT), the gateway from this branch, and (later) the driver.
echo "== images"
IMAGE="$REGISTRY/fixture-agent:root"
# A throwaway plain-HTTP registry holds the base and the agent image the assembler reads.
docker rm -f "$P-localreg" >/dev/null 2>&1 || true
docker run -d --name "$P-localreg" -p 5000:5000 registry:2 >/dev/null
docker build -q --build-arg METADATA_BACKEND=azure -f "$REPO/crates/alien-sandbox-conformance/contract/Dockerfile" \
  -t localhost:5000/fixture:base "$REPO/crates/alien-sandbox-conformance" >/dev/null
docker push -q localhost:5000/fixture:base >/dev/null
agentimg="$WORK/agent-image"; rm -rf "$agentimg"; mkdir -p "$agentimg"
cp "$AGENT_BINARY" "$agentimg/alien-sandbox-agent"
printf 'FROM scratch\nCOPY alien-sandbox-agent /usr/local/bin/alien-sandbox-agent\n' > "$agentimg/Dockerfile"
docker build -q -t localhost:5000/agent:dev "$agentimg" >/dev/null
docker push -q localhost:5000/agent:dev >/dev/null
(cd "$REPO" && ACR_USER="$ACR_USER" ACR_PASS="$ACR_PASS" cargo run -q -p alien-build --example azure-sandbox-assemble -- \
  localhost:5000/fixture:base localhost:5000/agent:dev "$IMAGE")
docker rm -f "$P-localreg" >/dev/null
gw="$WORK/gateway-image"; rm -rf "$gw"; mkdir -p "$gw"
cp "$GATEWAY_BINARY" "$gw/alien-egress-gateway"
printf 'FROM scratch\nCOPY alien-egress-gateway /alien-egress-gateway\nENTRYPOINT ["/alien-egress-gateway"]\n' > "$gw/Dockerfile"
docker build -q -t "$REGISTRY/gateway:dev" "$gw" >/dev/null
docker push -q "$REGISTRY/gateway:dev" >/dev/null
GATEWAY_REF="$REGISTRY/gateway@$(docker inspect --format '{{index .RepoDigests 0}}' "$REGISTRY/gateway:dev" | sed 's/.*@//')"

# 3. Render and apply the shipped module.
echo "== render + apply"
rm -rf "$WORK/render"; mkdir -p "$WORK/render"
(cd "$REPO" && AZURE_INTERNET_OUT="$WORK/render" AZURE_INTERNET_IMAGE="$IMAGE" \
  cargo test -p alien-terraform --test generator azure_internet_live_render -- --ignored)
module="$WORK/render/module"
# Proof-only: run the branch-built gateway instead of the pinned release image.
pinned=$(grep -rhoE '[a-z0-9./-]+/alien-egress-gateway@sha256:[0-9a-f]{64}' "$module" | sort -u | head -1 || true)
[ -n "$pinned" ] && grep -rl "$pinned" "$module" | xargs sed -i "s#$pinned#$GATEWAY_REF#g"
cat > "$module/terraform.tfvars" <<EOF
name = "azint"
resource_prefix = "$P"
azure_subscription_id = "$SUB"
azure_resource_group_name = "$RG"
azure_location = "$LOCATION"
default_network_sandbox_gateway_vm_size = "$VM_SIZE"
EOF
export ARM_SUBSCRIPTION_ID="$SUB" ARM_TENANT_ID="$AZURE_TARGET_TENANT_ID" ARM_CLIENT_ID="$AZURE_TARGET_CLIENT_ID" ARM_CLIENT_SECRET="$AZURE_TARGET_CLIENT_SECRET"
# Azure occasionally resets a connection mid-apply; apply is idempotent, so one retry continues it.
(cd "$module" && terraform init -input=false >/dev/null \
  && { terraform apply -input=false -auto-approve -parallelism=3 || terraform apply -input=false -auto-approve -parallelism=3; }) | tee "$WORK/apply.log"

# 4. Facts from the applied module.
tf=$(cd "$module" && terraform show -json)
jq '[.values.root_module.resources[] | select(.address=="terraform_data.deployment_registration")][0].values.input' <<<"$tf" > "$WORK/registration.json"
jq '. + {region: "'"$LOCATION"'"}' "$WORK/registration.json" > "$WORK/registration.tmp" && mv "$WORK/registration.tmp" "$WORK/registration.json"
vault=$(jq -r '[.values.root_module.resources[] | select(.address=="azurerm_key_vault.secrets")][0].values.name' <<<"$tf")
vms=$(jq -r '[.values.root_module.resources[] | select(.type=="azurerm_linux_virtual_machine" and (.address|test("sandbox_gateway")))|.values.id]|sort|join(",")' <<<"$tf")
ips=$(jq -r '[(.resources|if type=="string" then fromjson else . end)[]|select(.id=="default-network")][0].importData.sandboxNetwork.gatewayIps|join(",")' "$WORK/registration.json")
subnet=$(jq -r '[.values.root_module.resources[] | select(.address=="azurerm_subnet.default_network_container_apps")][0].values.id' <<<"$tf")
private_probe=$(jq -r '[.values.root_module.resources[] | select(.address=="azurerm_subnet.default_network_private")][0].values.address_prefixes[0]' <<<"$tf" | sed 's#\.0/.*#.4#')
echo "vault=$vault vms=$vms ips=$ips subnet=$subnet"

# 5. The principal needs the data-plane role on the group and secret rights on the vault.
role_id() { arm GET "/subscriptions/$SUB/providers/Microsoft.Authorization/roleDefinitions?api-version=2022-04-01&\$filter=roleName%20eq%20'${1// /%20}'" | jq -r '.value[0].id'; }
sp=$(sp_object_id)
assign() { # assign <scope> <role name>
  local body
  body=$(arm PUT "$1/providers/Microsoft.Authorization/roleAssignments/$(python3 -c 'import uuid,sys;print(uuid.uuid5(uuid.NAMESPACE_URL,sys.argv[1]))' "$1$2$sp")?api-version=2022-04-01" \
    -d "{\"properties\":{\"roleDefinitionId\":\"$(role_id "$2")\",\"principalId\":\"$sp\",\"principalType\":\"ServicePrincipal\"}}")
  jq -e '.id or (.error.code=="RoleAssignmentExists")' <<<"$body" >/dev/null || { echo "role assignment failed: $body"; exit 1; }
}
assign "/subscriptions/$SUB/resourceGroups/$RG" "Container Apps SandboxGroup Data Owner"
assign "/subscriptions/$SUB/resourceGroups/$RG/providers/Microsoft.KeyVault/vaults/$vault" "Key Vault Secrets Officer"
sleep 90 # role assignments take a while to propagate

# 6. The controllers publish through the real executor.
echo "== publish"
(cd "$REPO" && AZURE_INTERNET_STACK="$WORK/render/stack.json" AZURE_INTERNET_REGISTRATION="$WORK/registration.json" \
  AZURE_INTERNET_BINDING_OUT="$WORK/binding.json" \
  cargo test -p alien-infra --features all-platforms,test-utils --test azure_internet_live -- --ignored --nocapture) | tee "$WORK/publish.log"

# 7. The driver, as a Container Apps job inside the VNet.
echo "== driver"
exe=$(cd "$REPO" && cargo test -p alien-bindings --features azure --test azure_sandbox_internet_live --no-run --message-format=json 2>/dev/null \
  | jq -r 'select(.profile.test==true and .target.name=="azure_sandbox_internet_live") | .executable' | tail -1)
drv="$WORK/driver-image"; rm -rf "$drv"; mkdir -p "$drv"
cp "$exe" "$drv/driver"
printf 'FROM ubuntu:24.04\nRUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*\nCOPY driver /driver\nENTRYPOINT ["/driver","--ignored","--nocapture","--exact","live_internet_proof"]\n' > "$drv/Dockerfile"
docker build -q -t "$REGISTRY/driver:1" "$drv" >/dev/null
docker push -q "$REGISTRY/driver:1" >/dev/null
ENV_ID="/subscriptions/$SUB/resourceGroups/$RG/providers/Microsoft.App/managedEnvironments/$P-aca"
arm PUT "$ENV_ID?api-version=2024-03-01" \
  -d "{\"location\":\"$LOCATION\",\"properties\":{\"vnetConfiguration\":{\"infrastructureSubnetId\":\"$subnet\"},\"workloadProfiles\":[{\"name\":\"Consumption\",\"workloadProfileType\":\"Consumption\"}]}}" >/dev/null
for _ in $(seq 1 80); do
  state=$(arm GET "$ENV_ID?api-version=2024-03-01" | jq -r .properties.provisioningState); [ "$state" = Succeeded ] && break; sleep 15
done
[ "$state" = Succeeded ] || { echo "container apps environment: $state"; exit 1; }
binding=$(jq -c . "$WORK/binding.json")
job=$(jq -n --arg env "$ENV_ID" --arg loc "$LOCATION" --arg reg "$REGISTRY" --arg user "$ACR_USER" --arg pass "$ACR_PASS" \
  --arg image "$REGISTRY/driver:1" --arg sub "$SUB" --arg tenant "$AZURE_TARGET_TENANT_ID" --arg cid "$AZURE_TARGET_CLIENT_ID" \
  --arg csecret "$AZURE_TARGET_CLIENT_SECRET" --arg rg "$RG" --arg vault "$vault" --arg binding "$binding" --arg ips "$ips" \
  --arg vms "$vms" --arg probe "$private_probe" --arg region "$LOCATION" '
  {location:$loc, properties:{environmentId:$env, workloadProfileName:"Consumption",
   configuration:{triggerType:"Manual", replicaTimeout:3000, replicaRetryLimit:0,
     manualTriggerConfig:{parallelism:1, replicaCompletionCount:1},
     secrets:[{name:"client-secret",value:$csecret},{name:"acr-pass",value:$pass}],
     registries:[{server:$reg, username:$user, passwordSecretRef:"acr-pass"}]},
   template:{containers:[{name:"driver", image:$image, resources:{cpu:1, memory:"2Gi"}, env:[
     {name:"AZURE_TARGET_SUBSCRIPTION_ID",value:$sub},{name:"AZURE_TARGET_TENANT_ID",value:$tenant},
     {name:"AZURE_TARGET_CLIENT_ID",value:$cid},{name:"AZURE_TARGET_CLIENT_SECRET",secretRef:"client-secret"},
     {name:"AZURE_RESOURCE_GROUP",value:$rg},{name:"AZURE_SANDBOX_REGION",value:$region},
     {name:"AZURE_INTERNET_VAULT_NAME",value:$vault},{name:"AZURE_INTERNET_BINDING",value:$binding},
     {name:"AZURE_INTERNET_GATEWAY_IPS",value:$ips},{name:"AZURE_INTERNET_GATEWAY_VMS",value:$vms},
     {name:"AZURE_INTERNET_PRIVATE_PROBES",value:$probe},{name:"AZURE_INTERNET_RESULT_SECRET",value:"azure-internet-proof-log"}]}]}}}')
JOB_ID="/subscriptions/$SUB/resourceGroups/$RG/providers/Microsoft.App/jobs/$P-driver"
arm PUT "$JOB_ID?api-version=2024-03-01" -d "$job" | jq -e '.id' >/dev/null
arm POST "$JOB_ID/start?api-version=2024-03-01" -d '' >/dev/null
for _ in $(seq 1 240); do
  sleep 15
  st=$(arm GET "$JOB_ID/executions?api-version=2024-03-01" | jq -r '[.value[]?|.properties|select(.startTime)]|sort_by(.startTime)|last|.status')
  echo "job: $st"
  # Only a terminal status ends the wait; a just-started execution can report Unknown or none.
  case "$st" in Succeeded|Failed|Stopped|Degraded) break ;; esac
done
secret=$(curl -fsS -H "Authorization: Bearer $(token https://vault.azure.net/.default)" \
  "https://$vault.vault.azure.net/secrets/azure-internet-proof-log?api-version=7.4" | jq -r .value)
echo "$secret" | tee "$WORK/proof.log"
verdict=$(head -1 <<<"$secret")
[ "$verdict" = PASS ] || { echo "proof failed"; exit 1; }
echo "proof passed; evidence in $WORK"
