# DeploymentPreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { DeploymentPreparedStack } from "@alienplatform/platform-api/models";

let value: DeploymentPreparedStack = {
  id: "<id>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      dependencies: [
        {
          id: "<id>",
          type: "<value>",
        },
      ],
      lifecycle: "live-on-setup",
    },
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Unique identifier for the stack                                                                          |
| `permissions`                                                                                            | [models.DeploymentPermissions](../models/deploymentpermissions.md)                                       | :heavy_minus_sign:                                                                                       | Combined permissions configuration that contains both profiles and management                            |
| `resources`                                                                                              | Record<string, [models.DeploymentPreparedStackResources](../models/deploymentpreparedstackresources.md)> | :heavy_check_mark:                                                                                       | Map of resource IDs to their configurations and lifecycle settings                                       |