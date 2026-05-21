# PreparedDeploymentStackStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { PreparedDeploymentStackStack } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackStack = {
  id: "<id>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      dependencies: [],
      lifecycle: "frozen",
    },
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                       | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Unique identifier for the stack                                                                            |
| `permissions`                                                                                              | [models.PreparedDeploymentStackPermissions](../models/prepareddeploymentstackpermissions.md)               | :heavy_minus_sign:                                                                                         | Combined permissions configuration that contains both profiles and management                              |
| `resources`                                                                                                | Record<string, [models.PreparedDeploymentStackResources](../models/prepareddeploymentstackresources.md)>   | :heavy_check_mark:                                                                                         | Map of resource IDs to their configurations and lifecycle settings                                         |
| `supportedPlatforms`                                                                                       | [models.PreparedDeploymentStackSupportedPlatform](../models/prepareddeploymentstacksupportedplatform.md)[] | :heavy_minus_sign:                                                                                         | Which platforms this stack supports. When None, all platforms are supported.                               |