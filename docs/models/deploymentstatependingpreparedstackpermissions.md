# DeploymentStatePendingPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [
        "<value>",
      ],
      "key1": [],
      "key2": [
        "<value>",
      ],
    },
    "key1": {
      "key": [],
    },
    "key2": {
      "key": [
        "<value>",
      ],
      "key1": [
        {
          description: "oof lively save notwithstanding oof drowse",
          id: "<id>",
          platforms: {},
        },
      ],
      "key2": [],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.DeploymentStatePendingPreparedStackManagementUnion*                                                                        | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.DeploymentStatePendingPreparedStackProfileUnion*[]>>                                         | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |