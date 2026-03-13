# SyncReconcileResponseCurrentReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleasePermissions } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleasePermissions = {
  profiles: {
    "key": {
      "key": [],
      "key1": [],
      "key2": [],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncReconcileResponseCurrentReleaseManagementUnion*                                                                        | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncReconcileResponseCurrentReleaseProfileUnion*[]>>                                         | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |