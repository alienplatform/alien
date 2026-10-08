# SyncListResponsePendingPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [],
    },
    "key1": {},
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncListResponsePendingPreparedStackManagementUnion*                                                                       | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncListResponsePendingPreparedStackProfileUnion*[]>>                                        | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
