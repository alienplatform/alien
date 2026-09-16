# PermissionGcp

## Example Usage

```typescript
import { PermissionGcp } from "@alienplatform/platform-api/models";

let value: PermissionGcp = {
  grant: {},
  binding: {},
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `label`                                                          | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `description`                                                    | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `grant`                                                          | [models.PermissionGcpGrant](../models/permissiongcpgrant.md)     | :heavy_check_mark:                                               | N/A                                                              |
| `binding`                                                        | [models.PermissionGcpBinding](../models/permissiongcpbinding.md) | :heavy_check_mark:                                               | N/A                                                              |