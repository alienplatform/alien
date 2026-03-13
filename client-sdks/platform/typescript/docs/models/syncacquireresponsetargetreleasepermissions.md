# SyncAcquireResponseTargetReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncAcquireResponseTargetReleasePermissions } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTargetReleasePermissions = {
  profiles: {
    "key": {
      "key": [],
      "key1": [],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncAcquireResponseTargetReleaseManagementUnion*                                                                           | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncAcquireResponseTargetReleaseProfileUnion*[]>>                                            | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |