# DeploymentStatePendingPreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { DeploymentStatePendingPreparedStack } from "@alienplatform/platform-api/models";

let value: DeploymentStatePendingPreparedStack = {
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
      lifecycle: "live",
    },
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                               | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Unique identifier for the stack                                                                                                    |
| `inputs`                                                                                                                           | [models.DeploymentStatePendingPreparedStackInput](../models/deploymentstatependingpreparedstackinput.md)[]                         | :heavy_minus_sign:                                                                                                                 | Input definitions required before setup or deployment can proceed.                                                                 |
| `permissions`                                                                                                                      | [models.DeploymentStatePendingPreparedStackPermissions](../models/deploymentstatependingpreparedstackpermissions.md)               | :heavy_minus_sign:                                                                                                                 | Combined permissions configuration that contains both profiles and management                                                      |
| `resources`                                                                                                                        | Record<string, [models.DeploymentStatePendingPreparedStackResources](../models/deploymentstatependingpreparedstackresources.md)>   | :heavy_check_mark:                                                                                                                 | Map of resource IDs to their configurations and lifecycle settings                                                                 |
| `supportedPlatforms`                                                                                                               | [models.DeploymentStatePendingPreparedStackSupportedPlatform](../models/deploymentstatependingpreparedstacksupportedplatform.md)[] | :heavy_minus_sign:                                                                                                                 | Which platforms this stack supports. When None, all platforms are supported.                                                       |