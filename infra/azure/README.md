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
