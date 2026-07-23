# DeploymentDetailResponsePendingPreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { DeploymentDetailResponsePendingPreparedStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePendingPreparedStack = {
  id: "<id>",
  resources: {
    "key": {
      config: {
        id: "<id>",
        type: "<value>",
      },
      dependencies: [],
      lifecycle: "live",
    },
  },
};
```

## Fields

| Field                                                                                                                                                | Type                                                                                                                                                 | Required                                                                                                                                             | Description                                                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                 | *string*                                                                                                                                             | :heavy_check_mark:                                                                                                                                   | Unique identifier for the stack                                                                                                                      |
| `inputs`                                                                                                                                             | [models.DeploymentDetailResponsePendingPreparedStackInput](../models/deploymentdetailresponsependingpreparedstackinput.md)[]                         | :heavy_minus_sign:                                                                                                                                   | Input definitions required before setup or deployment can proceed.                                                                                   |
| `permissions`                                                                                                                                        | [models.DeploymentDetailResponsePendingPreparedStackPermissions](../models/deploymentdetailresponsependingpreparedstackpermissions.md)               | :heavy_minus_sign:                                                                                                                                   | Combined permissions configuration that contains both profiles and management                                                                        |
| `resources`                                                                                                                                          | Record<string, [models.DeploymentDetailResponsePendingPreparedStackResources](../models/deploymentdetailresponsependingpreparedstackresources.md)>   | :heavy_check_mark:                                                                                                                                   | Map of resource IDs to their configurations and lifecycle settings                                                                                   |
| `supportedPlatforms`                                                                                                                                 | [models.DeploymentDetailResponsePendingPreparedStackSupportedPlatform](../models/deploymentdetailresponsependingpreparedstacksupportedplatform.md)[] | :heavy_minus_sign:                                                                                                                                   | Which platforms this stack supports. When None, all platforms are supported.                                                                         |
