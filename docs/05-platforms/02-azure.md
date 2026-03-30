# Azure Platform

Platform-specific details for working on Azure controllers.

## Cross-Tenant Access

Azure uses **UAMI + FIC + custom RBAC** for cross-tenant management:

1. The `AzureRemoteStackManagementController` creates a **User-Assigned Managed Identity (UAMI)** in the customer's resource group.
2. A **Federated Identity Credential (FIC)** is created on the UAMI, trusting the manager's OIDC issuer (EKS, GitHub Actions, or Container Apps).
3. A **custom role definition** is created from the stack's `/provision` permission sets, scoped to the resource group.
4. **Role assignments** bind the custom role to the UAMI principal.

At runtime, the manager exchanges an OIDC token (projected K8s SA token with `audience: api://AzureADTokenExchange`) for an ARM access token via the customer's Azure AD token endpoint. For local development, a multi-tenant Service Principal uses cross-tenant `client_credentials` as a fallback.

This is symmetric with AWS (AssumeRole) and GCP (service account impersonation) — all three platforms use token exchange against the customer's identity provider.

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

Permissions go into **custom role definitions** with role assignments. Two tiers:

1. **RG-scoped (provision)**: The RSM controller creates a custom role from `/provision` permission sets and assigns it at resource group scope. This gives the management UAMI the ability to create/delete resources.
2. **Resource-scoped (non-provision)**: Resource controllers assign management permissions (execute, heartbeat, management) scoped to specific resource instances via `AzurePermissionsHelper::apply_management_permissions`.

Uses `dataActions` for data-plane operations (e.g., blob read/write) vs `actions` for control-plane operations.

## Quirks

- Storage Accounts are shared — the `AzureStorageAccountMutation` preflight adds a single storage account used by both Storage (Blob) and Queue (Storage Queue) resources.
- Key Vault is an actual standalone resource (unlike AWS/GCP where "vault" is a naming convention over Secrets Manager / Secret Manager).
- KEDA is used for autoscaling Container Apps based on Service Bus queue depth (for commands and queue-triggered functions).
