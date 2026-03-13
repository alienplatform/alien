# ReleaseInfoPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { ReleaseInfoPermissions } from "@aliendotdev/platform-api/models";

let value: ReleaseInfoPermissions = {
  profiles: {
    "key": {
      "key": [
        "<value>",
      ],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.ReleaseInfoManagementUnion*                                                                                                | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.ReleaseInfoProfileUnion*[]>>                                                                 | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |