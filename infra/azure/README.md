# Azure Module

Provisions the Azure resources that alien-manager needs for push-mode deployments. Does **not** deploy compute — run the manager wherever you like and point it at these resources.

## Resources Created

- **Resource Group**: Container for all resources
- **Managed Identity**: IAM identity for the manager (attach to your compute)
- **Storage Account**: Used by the commands store
- **Artifact Registry** (optional): Azure Container Registry (Basic SKU)
- **Commands Store** (optional): Table Storage and Blob Storage
- **Impersonation** (optional): User-assigned managed identity

## Usage

```hcl
module "alien_infra" {
  source = "github.com/aliendotdev/alien//infra/azure"

  name     = "my-project"
  location = "eastus"

  enable_artifact_registry = true
  enable_commands_store    = true
  enable_impersonation     = true

  tags = {
    Environment = "production"
  }
}
```

Use `config_values` to populate your `alien-manager.toml`:

```hcl
output "toml_sections" {
  value = module.alien_infra.config_values
}
```

Attach the managed identity to your compute:

```hcl
output "manager_identity_id" {
  value = module.alien_infra.managed_identity_id
}
```

## Variables

| Name | Description | Type | Default | Required |
|------|-------------|------|---------|----------|
| `name` | Name prefix for all resources | `string` | — | yes |
| `location` | Azure region | `string` | — | yes |
| `resource_group_name` | Resource group name (auto-generated if empty) | `string` | `""` | no |
| `enable_artifact_registry` | Create ACR | `bool` | `true` | no |
| `enable_commands_store` | Create Table + Blob for commands | `bool` | `false` | no |
| `enable_impersonation` | Create managed identity for impersonation | `bool` | `false` | no |
| `tags` | Tags for all resources | `map(string)` | `{}` | no |

## Outputs

| Name | Description |
|------|-------------|
| `config_values` | Structured values for `alien-manager.toml` sections |
| `resource_group_name` | Resource group name |
| `managed_identity_id` | Manager managed identity ID (attach to your compute) |
| `managed_identity_client_id` | Manager managed identity client ID |
| `acr_login_server` | ACR login server (if enabled) |
| `storage_account_name` | Storage account name |

## Requirements

Terraform or OpenTofu 1.7 or later, with the `hashicorp/azurerm` (`>= 3.80, < 5.0`) and `Azure/azapi` (`>= 2.6, < 3.0`) providers.

The module declares no provider blocks, so it uses the `azurerm` and `azapi` configurations of the calling module. Configure `azapi` there with the same credentials as `azurerm`. Both read the `ARM_*` environment variables and an Azure CLI login. If your `azurerm` block sets credentials explicitly, set them on `azapi` too:

```hcl
provider "azapi" {
  subscription_id = var.subscription_id
  tenant_id       = var.tenant_id
}
```

## Upgrading the commands table

The commands table is created through Azure Resource Manager (`azapi_resource`). Earlier versions of this module declared it as `azurerm_storage_table`, whose table access-policy calls Azure Table storage now rejects with `415 Unsupported Media Type`, so those installations can no longer plan. To upgrade one with `enable_commands_store = true`, move the existing table, with its data, to the new address before the next apply (replace `module.alien_infra` with your module address):

```bash
ACCOUNT_ID=$(terraform state show -no-color 'module.alien_infra.azurerm_storage_account.manager' | sed -n 's/^ *id *= *"\(.*\)"$/\1/p' | head -1)
terraform import 'module.alien_infra.azapi_resource.commands[0]' "$ACCOUNT_ID/tableServices/default/tables/commands?api-version=2024-01-01"
terraform state rm 'module.alien_infra.azurerm_storage_table.commands'
terraform apply
```

Import before forgetting, so a failed import (for example, an unconfigured `azapi` provider) leaves the state as it was. `terraform state rm` deletes nothing in Azure. With OpenTofu, run the same commands with `tofu`. If you apply without these steps, the `removed` block still keeps the table: Terraform forgets it and then fails to create the new resource because the table exists, and OpenTofu fails on the 415 before changing anything. If Terraform already forgot the table that way, `terraform state rm` reports no matching objects; continue with the import.
