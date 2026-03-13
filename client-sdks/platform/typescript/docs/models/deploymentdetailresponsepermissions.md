# DeploymentDetailResponsePermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { DeploymentDetailResponsePermissions } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponsePermissions = {
  profiles: {
    "key": {
      "key": [
        "<value>",
      ],
      "key1": [],
      "key2": [],
    },
    "key1": {},
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.DeploymentDetailResponseManagementUnion*                                                                                   | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.DeploymentDetailResponseProfileUnion*[]>>                                                    | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |