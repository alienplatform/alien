# DeploymentDetailResponsePreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [],
      "key1": [
        "<value>",
      ],
      "key2": [],
    },
    "key1": {
      "key": [
        {
          description: "ignorant pecan indeed orderly",
          id: "<id>",
          platforms: {},
        },
      ],
      "key1": [
        "<value>",
      ],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.DeploymentDetailResponsePreparedStackManagementUnion*                                                                      | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.DeploymentDetailResponsePreparedStackProfileUnion*[]>>                                       | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
