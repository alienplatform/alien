# PermissionAzure

## Example Usage

```typescript
import { PermissionAzure } from "@alienplatform/platform-api/models";

let value: PermissionAzure = {
  grant: {},
  binding: {},
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `label`                                                              | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `description`                                                        | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `grant`                                                              | [models.PermissionAzureGrant](../models/permissionazuregrant.md)     | :heavy_check_mark:                                                   | N/A                                                                  |
| `binding`                                                            | [models.PermissionAzureBinding](../models/permissionazurebinding.md) | :heavy_check_mark:                                                   | N/A                                                                  |