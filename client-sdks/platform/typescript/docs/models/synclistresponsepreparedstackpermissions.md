# SyncListResponsePreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { SyncListResponsePreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.SyncListResponsePreparedStackManagementUnion*                                                                              | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.SyncListResponsePreparedStackProfileUnion*[]>>                                               | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
