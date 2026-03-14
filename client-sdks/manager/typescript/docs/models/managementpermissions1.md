# ManagementPermissions1

Add permissions to auto-derived baseline

## Example Usage

```typescript
import { ManagementPermissions1 } from "@alienplatform/manager-api/models";

let value: ManagementPermissions1 = {
  extend: {
    "key": [
      "<value>",
    ],
    "key1": [
      {
        description: "blacken given outlaw",
        id: "<id>",
        platforms: {},
      },
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.PermissionSetReference*[]>                                                                                 | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |