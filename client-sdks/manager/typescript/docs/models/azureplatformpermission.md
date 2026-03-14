# AzurePlatformPermission

Azure-specific platform permission configuration

## Example Usage

```typescript
import { AzurePlatformPermission } from "@alienplatform/manager-api/models";

let value: AzurePlatformPermission = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `binding`                                                                                        | [models.BindingConfigurationAzureBindingSpec](../models/bindingconfigurationazurebindingspec.md) | :heavy_check_mark:                                                                               | Generic binding configuration for permissions                                                    |
| `grant`                                                                                          | [models.PermissionGrant](../models/permissiongrant.md)                                           | :heavy_check_mark:                                                                               | Grant permissions for a specific cloud platform                                                  |