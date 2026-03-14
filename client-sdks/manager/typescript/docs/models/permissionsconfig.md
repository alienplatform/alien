# PermissionsConfig

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { PermissionsConfig } from "@alienplatform/manager-api/models";

let value: PermissionsConfig = {
  profiles: {
    "key": {},
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.ManagementPermissionsUnion*                                                                                                | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.PermissionSetReference*[]>>                                                                  | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |