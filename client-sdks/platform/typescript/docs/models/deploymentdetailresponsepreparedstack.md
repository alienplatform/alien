# DeploymentDetailResponsePreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { DeploymentDetailResponsePreparedStack } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePreparedStack = {
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

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                 | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Unique identifier for the stack                                                                                                      |
| `permissions`                                                                                                                        | [models.DeploymentDetailResponsePermissions](../models/deploymentdetailresponsepermissions.md)                                       | :heavy_minus_sign:                                                                                                                   | Combined permissions configuration that contains both profiles and management                                                        |
| `resources`                                                                                                                          | Record<string, [models.DeploymentDetailResponsePreparedStackResources](../models/deploymentdetailresponsepreparedstackresources.md)> | :heavy_check_mark:                                                                                                                   | Map of resource IDs to their configurations and lifecycle settings                                                                   |