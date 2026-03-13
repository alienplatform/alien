# Azure Platform

Platform-specific details for working on Azure controllers.

## Cross-Account Access

Azure uses **Azure Lighthouse** for cross-tenant management. A registration definition specifies which permissions the managing tenant gets, and a registration assignment activates it on the customer's subscription.

## Resource Mapping

| Alien Resource | Azure Service |
|---|---|
| Function | Container Apps |
| Container | VMSS (via Horizon) |
| Storage | Blob Storage |
| KV | Table Storage |
| Queue | Storage Queue |
| Vault | Key Vault |
| Build | Container Apps Jobs |
| ServiceAccount | User-assigned Managed Identity |

## Resource Groups

All Azure resources must belong to a resource group. The `AzureResourceGroupMutation` preflight automatically adds a `default-resource-group` resource (frozen) to every Azure stack.

## Container Apps Environment

Azure Functions run as Container Apps, which need a Container Apps Environment. The `AzureContainerAppsEnvironmentMutation` preflight adds this automatically.

## Service Bus

Command request queues use Azure Service Bus. The `AzureServiceBusNamespaceMutation` preflight adds a shared namespace when any function has commands enabled. Queues are named `{function-name}-rq`.

## Networking

- **No default VNet** — unlike AWS and GCP, Azure has no default network. The `use-default` mode still creates a VNet + subnets + NAT Gateway, making it functionally identical to `create` mode on Azure.
- **Create mode**: VNet + public/private subnets + NAT Gateway + Public IP + NSG
- VMSS networking always uses `public_ip_address_configuration: None` — NAT handles all egress

## Build Targets

Default: `linux-x64`

## Permissions

Permissions go into **custom role definitions** with role assignments. Stack-level scope targets the resource group. Resource-level scope targets specific resources. Uses `dataActions` for data-plane operations (e.g. blob read/write) vs `actions` for control-plane operations.

## Quirks

- Storage Accounts are shared — the `AzureStorageAccountMutation` preflight adds a single storage account used by both Storage (Blob) and Queue (Storage Queue) resources.
- Key Vault is an actual standalone resource (unlike AWS/GCP where "vault" is a naming convention over Secrets Manager / Secret Manager).
- KEDA is used for autoscaling Container Apps based on Service Bus queue depth (for commands and queue-triggered functions).
- Managed Identity federation requires explicit federated credential setup for cross-tenant scenarios.
