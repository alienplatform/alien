# RequiredPermissionAw

## Example Usage

```typescript
import { RequiredPermissionAw } from "@alienplatform/platform-api/models";

let value: RequiredPermissionAw = {
  grant: {},
  binding: {},
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `label`                                                                        | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `description`                                                                  | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `effect`                                                                       | [models.RequiredPermissionEffect](../models/requiredpermissioneffect.md)       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `grant`                                                                        | [models.RequiredPermissionAwGrant](../models/requiredpermissionawgrant.md)     | :heavy_check_mark:                                                             | N/A                                                                            |
| `binding`                                                                      | [models.RequiredPermissionAwBinding](../models/requiredpermissionawbinding.md) | :heavy_check_mark:                                                             | N/A                                                                            |