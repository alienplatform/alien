# SyncReconcileRequestPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackPermissions } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [
        {
          description:
            "yowza runny maestro coil meanwhile superb ah anti zowie round",
          id: "<id>",
          platforms: {},
        },
      ],
      "key1": [],
      "key2": [],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncReconcileRequestPreparedStackManagementUnion*                                                                          | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncReconcileRequestPreparedStackProfileUnion*[]>>                                           | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |