#!/bin/bash

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_DIR="$SCRIPT_DIR/../openapi"

# Create output directory if it doesn't exist
mkdir -p "$OUTPUT_DIR"

# Create temporary directory for bundling
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

BASE_URL=https://raw.githubusercontent.com/Azure/azure-rest-api-specs/refs/heads/main/specification

npx --yes @redocly/cli bundle -o "$TEMP_DIR" \
    $BASE_URL/app/resource-manager/Microsoft.App/ContainerApps/stable/2025-01-01/ContainerApps.json \
    $BASE_URL/app/resource-manager/Microsoft.App/ContainerApps/stable/2025-01-01/ManagedEnvironments.json \
    $BASE_URL/app/resource-manager/Microsoft.App/ContainerApps/stable/2025-01-01/Jobs.json \
    $BASE_URL/app/resource-manager/Microsoft.App/ContainerApps/stable/2025-01-01/ManagedEnvironmentsDaprComponents.json \
    $BASE_URL/authorization/resource-manager/Microsoft.Authorization/stable/2022-04-01/authorization-RoleDefinitionsCalls.json \
    $BASE_URL/authorization/resource-manager/Microsoft.Authorization/stable/2022-04-01/authorization-RoleAssignmentsCalls.json \
    $BASE_URL/msi/resource-manager/Microsoft.ManagedIdentity/ManagedIdentity/stable/2024-11-30/ManagedIdentity.json \
    $BASE_URL/storage/resource-manager/Microsoft.Storage/stable/2024-01-01/storage.json \
    $BASE_URL/storage/resource-manager/Microsoft.Storage/stable/2024-01-01/blob.json \
    $BASE_URL/storage/resource-manager/Microsoft.Storage/stable/2024-01-01/table.json \
    $BASE_URL/resources/resource-manager/Microsoft.Resources/resources/stable/2025-04-01/resources.json \
    $BASE_URL/containerregistry/resource-manager/Microsoft.ContainerRegistry/Registry/stable/2025-04-01/containerregistry.json \
    $BASE_URL/managedservices/resource-manager/Microsoft.ManagedServices/ManagedServices/stable/2022-10-01/managedservices.json \
    $BASE_URL/keyvault/resource-manager/Microsoft.KeyVault/stable/2024-11-01/keyvault.json \
    $BASE_URL/keyvault/data-plane/Microsoft.KeyVault/stable/7.6/secrets.json \
    $BASE_URL/keyvault/data-plane/Microsoft.KeyVault/stable/7.6/certificates.json \
    $BASE_URL/servicebus/resource-manager/Microsoft.ServiceBus/ServiceBus/stable/2024-01-01/Queue.json \
    $BASE_URL/servicebus/resource-manager/Microsoft.ServiceBus/ServiceBus/stable/2024-01-01/namespace-preview.json \
    $BASE_URL/network/resource-manager/Microsoft.Network/stable/2025-03-01/virtualNetwork.json \
    $BASE_URL/network/resource-manager/Microsoft.Network/stable/2025-03-01/natGateway.json \
    $BASE_URL/network/resource-manager/Microsoft.Network/stable/2025-03-01/publicIpAddress.json \
    $BASE_URL/network/resource-manager/Microsoft.Network/stable/2025-03-01/networkSecurityGroup.json \
    $BASE_URL/network/resource-manager/Microsoft.Network/stable/2025-03-01/loadBalancer.json \
    $BASE_URL/compute/resource-manager/Microsoft.Compute/ComputeRP/stable/2025-04-01/ComputeRP.json \
    $BASE_URL/compute/resource-manager/Microsoft.Compute/DiskRP/stable/2025-01-02/DiskRP.json


# Convert OpenAPI v2 to v3 and apply filters for all JSON files
for file in "$TEMP_DIR"/*.json; do
  if [ -f "$file" ]; then
    filename=$(basename "$file")
    npx --yes swagger2openapi "$file" | jq '.paths = {} | walk(if type == "object" and has("format") and .format == "date-time" then del(.format) else . end)' > "$OUTPUT_DIR/$filename"
  fi
done
