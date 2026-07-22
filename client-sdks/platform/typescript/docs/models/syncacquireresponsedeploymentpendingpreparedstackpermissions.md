# SyncAcquireResponseDeploymentPendingPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPendingPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPendingPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [
        {
          description: "yowza formamide now recent claw",
          id: "<id>",
          platforms: {},
        },
      ],
      "key1": [
        {
          description: "yowza formamide now recent claw",
          id: "<id>",
          platforms: {},
        },
      ],
    },
    "key1": {
      "key": [
        {
          description: "yowza formamide now recent claw",
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
| `management`                                                                                                                       | *models.SyncAcquireResponseDeploymentPendingPreparedStackManagementUnion*                                                          | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncAcquireResponseDeploymentPendingPreparedStackProfileUnion*[]>>                           | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
