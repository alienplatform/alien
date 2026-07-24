# DeploymentPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { DeploymentPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [
        "<value>",
      ],
      "key1": [
        "<value>",
      ],
      "key2": [],
    },
    "key1": {
      "key": [
        {
          description:
            "impartial pigpen whenever whose arid sailor bleak spirited elastic though",
          id: "<id>",
          platforms: {},
        },
      ],
      "key1": [],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.DeploymentPreparedStackManagementUnion*                                                                                    | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.DeploymentPreparedStackProfileUnion*[]>>                                                     | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
