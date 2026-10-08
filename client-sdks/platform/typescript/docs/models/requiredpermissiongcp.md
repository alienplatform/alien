# RequiredPermissionGcp

## Example Usage

```typescript
import { RequiredPermissionGcp } from "@alienplatform/platform-api/models";

let value: RequiredPermissionGcp = {
  grant: {},
  binding: {},
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `label`                                                                          | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `description`                                                                    | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `grant`                                                                          | [models.RequiredPermissionGcpGrant](../models/requiredpermissiongcpgrant.md)     | :heavy_check_mark:                                                               | N/A                                                                              |
| `binding`                                                                        | [models.RequiredPermissionGcpBinding](../models/requiredpermissiongcpbinding.md) | :heavy_check_mark:                                                               | N/A                                                                              |