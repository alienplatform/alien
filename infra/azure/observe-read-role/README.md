# Azure Observe Read Role

Grants read-only access that Alien uses to discover and observe existing Azure resources.
Apply this at the resource group or subscription scope you want to observe.

## Terraform

```hcl
module "alien_observe_read_role" {
  source = "github.com/aliendotdev/alien//infra/azure/observe-read-role"

  scope        = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/production"
  principal_id = "11111111-1111-1111-1111-111111111111"
}

output "alien_observe_role_assignment_id" {
  value = module.alien_observe_read_role.role_assignment_id
}
```

## Azure CLI

```bash
az role assignment create \
  --assignee-object-id "11111111-1111-1111-1111-111111111111" \
  --assignee-principal-type ServicePrincipal \
  --role Reader \
  --scope "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/production"
```

`Reader` is enough for Resource Graph discovery and Azure Monitor metric reads at the selected scope.
