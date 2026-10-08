# RequiredPermissionAzure

## Example Usage

```typescript
import { RequiredPermissionAzure } from "@alienplatform/platform-api/models";

let value: RequiredPermissionAzure = {
  grant: {},
  binding: {},
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `label`                                                                              | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `description`                                                                        | *string*                                                                             | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `grant`                                                                              | [models.RequiredPermissionAzureGrant](../models/requiredpermissionazuregrant.md)     | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `binding`                                                                            | [models.RequiredPermissionAzureBinding](../models/requiredpermissionazurebinding.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |