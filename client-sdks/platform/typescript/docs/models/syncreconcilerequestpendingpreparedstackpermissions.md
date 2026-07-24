# SyncReconcileRequestPendingPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncReconcileRequestPendingPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPendingPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [
        {
          description: "unfortunately hmph voluntarily issue pure finding aw",
          id: "<id>",
          platforms: {},
        },
      ],
      "key1": [
        {
          description: "unfortunately hmph voluntarily issue pure finding aw",
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
| `management`                                                                                                                       | *models.SyncReconcileRequestPendingPreparedStackManagementUnion*                                                                   | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncReconcileRequestPendingPreparedStackProfileUnion*[]>>                                    | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
