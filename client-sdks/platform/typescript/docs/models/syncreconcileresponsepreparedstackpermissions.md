# SyncReconcileResponsePreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncReconcileResponsePreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [],
      "key1": [
        "<value>",
      ],
      "key2": [
        {
          description:
            "replicate ha quietly furthermore shiny a lest forenenst yippee",
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
| `management`                                                                                                                       | *models.SyncReconcileResponsePreparedStackManagementUnion*                                                                         | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncReconcileResponsePreparedStackProfileUnion*[]>>                                          | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |