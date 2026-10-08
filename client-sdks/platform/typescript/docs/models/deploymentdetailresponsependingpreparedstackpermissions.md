# DeploymentDetailResponsePendingPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStackPermissions = {
  profiles: {
    "key": {},
    "key1": {
      "key": [
        "<value>",
      ],
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `management`                                                                                                                       | *models.DeploymentDetailResponsePendingPreparedStackManagementUnion*                                                               | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.DeploymentDetailResponsePendingPreparedStackProfileUnion*[]>>                                | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
