# GcpPlatformPermission

GCP-specific platform permission configuration

## Example Usage

```typescript
import { GcpPlatformPermission } from "@alienplatform/manager-api/models";

let value: GcpPlatformPermission = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `binding`                                                                                    | [models.BindingConfigurationGcpBindingSpec](../models/bindingconfigurationgcpbindingspec.md) | :heavy_check_mark:                                                                           | Generic binding configuration for permissions                                                |
| `grant`                                                                                      | [models.PermissionGrant](../models/permissiongrant.md)                                       | :heavy_check_mark:                                                                           | Grant permissions for a specific cloud platform                                              |