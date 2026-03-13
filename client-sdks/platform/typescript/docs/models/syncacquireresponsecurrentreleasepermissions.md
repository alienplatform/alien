# SyncAcquireResponseCurrentReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleasePermissions } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCurrentReleasePermissions = {
  profiles: {
    "key": {},
    "key1": {
      "key": [],
      "key1": [
        {
          description:
            "gee indeed nearly heartbeat rapidly bah ick gosh warmly manage",
          id: "<id>",
          platforms: {},
        },
      ],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncAcquireResponseCurrentReleaseManagementUnion*                                                                          | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncAcquireResponseCurrentReleaseProfileUnion*[]>>                                           | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |