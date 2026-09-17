# PermissionAw

## Example Usage

```typescript
import { PermissionAw } from "@alienplatform/platform-api/models";

let value: PermissionAw = {
  grant: {},
  binding: {},
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `label`                                                        | *string*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `description`                                                  | *string*                                                       | :heavy_minus_sign:                                             | N/A                                                            |
| `effect`                                                       | [models.PermissionEffect](../models/permissioneffect.md)       | :heavy_minus_sign:                                             | N/A                                                            |
| `grant`                                                        | [models.PermissionAwGrant](../models/permissionawgrant.md)     | :heavy_check_mark:                                             | N/A                                                            |
| `binding`                                                      | [models.PermissionAwBinding](../models/permissionawbinding.md) | :heavy_check_mark:                                             | N/A                                                            |