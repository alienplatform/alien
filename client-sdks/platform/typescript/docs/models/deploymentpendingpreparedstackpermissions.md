# DeploymentPendingPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { DeploymentPendingPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: DeploymentPendingPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [],
      "key1": [
        "<value>",
      ],
    },
    "key1": {
      "key": [
        "<value>",
      ],
      "key1": [],
    },
    "key2": {
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
| `management`                                                                                                                       | *models.DeploymentPendingPreparedStackManagementUnion*                                                                             | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.DeploymentPendingPreparedStackProfileUnion*[]>>                                              | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
