# DeploymentPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { DeploymentPermissions } from "@alienplatform/platform-api/models";

let value: DeploymentPermissions = {
  profiles: {
    "key": {
      "key": [],
    },
    "key1": {},
    "key2": {
      "key": [],
      "key1": [
        {
          description: "aw strong everlasting",
          id: "<id>",
          platforms: {},
        },
      ],
      "key2": [
        {
          description: "aw strong everlasting",
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
| `management`                                                                                                                       | *models.DeploymentManagementUnion*                                                                                                 | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.DeploymentProfileUnion*[]>>                                                                  | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |