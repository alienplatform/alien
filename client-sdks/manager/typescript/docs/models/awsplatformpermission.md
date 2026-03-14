# AwsPlatformPermission

AWS-specific platform permission configuration

## Example Usage

```typescript
import { AwsPlatformPermission } from "@alienplatform/manager-api/models";

let value: AwsPlatformPermission = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `binding`                                                                                    | [models.BindingConfigurationAwsBindingSpec](../models/bindingconfigurationawsbindingspec.md) | :heavy_check_mark:                                                                           | Generic binding configuration for permissions                                                |
| `grant`                                                                                      | [models.PermissionGrant](../models/permissiongrant.md)                                       | :heavy_check_mark:                                                                           | Grant permissions for a specific cloud platform                                              |