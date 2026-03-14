# ManagementPermissions2

Replace auto-derived permissions entirely

## Example Usage

```typescript
import { ManagementPermissions2 } from "@alienplatform/manager-api/models";

let value: ManagementPermissions2 = {
  override: {
    "key": [],
    "key1": [
      "<value>",
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.PermissionSetReference*[]>                                                                                 | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |