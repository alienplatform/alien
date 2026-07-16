# SyncAcquireResponseDeploymentTargetReleasePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentTargetReleasePermissions } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentTargetReleasePermissions = {
  profiles: {
    "key": {
      "key": [
        "<value>",
      ],
      "key1": [],
    },
    "key1": {
      "key": [],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncAcquireResponseDeploymentTargetReleaseManagementUnion*                                                                 | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncAcquireResponseDeploymentTargetReleaseProfileUnion*[]>>                                  | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |