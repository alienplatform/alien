# SyncReconcileRequestTargetReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleasePermissions } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleasePermissions = {
  profiles: {
    "key": {
      "key": [],
    },
    "key1": {
      "key": [],
      "key1": [
        "<value>",
      ],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncReconcileRequestTargetReleaseManagementUnion*                                                                          | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncReconcileRequestTargetReleaseProfileUnion*[]>>                                           | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |