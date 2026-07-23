# PersistImportedDeploymentRequestPreparedStackPermissions

Combined permissions configuration that contains both profiles and management

## Example Usage

```typescript
import { PersistImportedDeploymentRequestPreparedStackPermissions } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestPreparedStackPermissions = {
  profiles: {
    "key": {
      "key": [],
      "key1": [],
      "key2": [
        {
          description: "mentor formula hm deliquesce forgery deceivingly nor",
          id: "<id>",
          platforms: {},
        },
      ],
    },
    "key1": {
      "key": [],
      "key1": [
        {
          description: "mentor formula hm deliquesce forgery deceivingly nor",
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
| `management`                                                                                                                       | *models.PersistImportedDeploymentRequestPreparedStackManagementUnion*                                                              | :heavy_minus_sign:                                                                                                                 | Management permissions configuration for stack management access                                                                   |
| `profiles`                                                                                                                         | Record<string, Record<string, *models.PersistImportedDeploymentRequestPreparedStackProfileUnion*[]>>                               | :heavy_check_mark:                                                                                                                 | Permission profiles that define access control for compute services<br/>Key is the profile name, value is the permission configuration |
