# PersistImportedDeploymentRequestPendingPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPendingPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPendingPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [],
    },
    "key1": {
      "key": [],
      "key1": [
        "<value>",
      ],
    },
    "key2": {},
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.PersistImportedDeploymentRequestPendingPreparedStackManagementUnion*                                                       | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.PersistImportedDeploymentRequestPendingPreparedStackProfileUnion*[]>>                        | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
