# SyncReconcileRequestCurrentReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleasePermissions } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleasePermissions = {
  profiles: {
    "key": {},
    "key1": {},
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncReconcileRequestCurrentReleaseManagementUnion*                                                                         | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncReconcileRequestCurrentReleaseProfileUnion*[]>>                                          | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |