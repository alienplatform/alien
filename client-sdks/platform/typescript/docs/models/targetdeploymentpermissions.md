# TargetDeploymentPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { TargetDeploymentPermissions } from "@alienplatform/platform-api/models";

let value: TargetDeploymentPermissions = {
  profiles: {
    "key": {
      "key": [
        {
          description: "indeed because bleach boo graceful congregate whoever",
          id: "<id>",
          platforms: {},
        },
      ],
      "key1": [],
    },
    "key1": {
      "key": [],
      "key1": [
        "<value>",
      ],
      "key2": [
        {
          description: "indeed because bleach boo graceful congregate whoever",
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
| `management`                                                                                                                       | *models.TargetDeploymentManagementUnion*                                                                                           | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.TargetDeploymentProfileUnion*[]>>                                                            | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |