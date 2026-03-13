# SyncReconcileResponseTargetReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleasePermissions } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleasePermissions = {
  profiles: {
    "key": {
      "key": [],
      "key1": [
        "<value>",
      ],
      "key2": [
        "<value>",
      ],
    },
    "key1": {
      "key": [
        "<value>",
      ],
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
| `management`                                                                                                                       | *models.SyncReconcileResponseTargetReleaseManagementUnion*                                                                         | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncReconcileResponseTargetReleaseProfileUnion*[]>>                                          | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |