# SyncAcquireResponseDeploymentCurrentReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentCurrentReleaseStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentCurrentReleaseStack = {
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
      lifecycle: "frozen",
    },
  },
};
```

## Fields

| Field                                                                                                                                              | Type                                                                                                                                               | Required                                                                                                                                           | Description                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                               | *string*                                                                                                                                           | :heavy_check_mark:                                                                                                                                 | Unique identifier for the stack                                                                                                                    |
| `inputs`                                                                                                                                           | [models.SyncAcquireResponseDeploymentCurrentReleaseInput](../models/syncacquireresponsedeploymentcurrentreleaseinput.md)[]                         | :heavy_minus_sign:                                                                                                                                 | Input definitions required before setup or deployment can proceed.                                                                                 |
| `permissions`                                                                                                                                      | [models.SyncAcquireResponseDeploymentCurrentReleasePermissions](../models/syncacquireresponsedeploymentcurrentreleasepermissions.md)               | :heavy_minus_sign:                                                                                                                                 | Combined permissions configuration that contains both profiles and management                                                                      |
| `resources`                                                                                                                                        | Record<string, [models.SyncAcquireResponseDeploymentCurrentReleaseResources](../models/syncacquireresponsedeploymentcurrentreleaseresources.md)>   | :heavy_check_mark:                                                                                                                                 | Map of resource IDs to their configurations and lifecycle settings                                                                                 |
| `supportedPlatforms`                                                                                                                               | [models.SyncAcquireResponseDeploymentCurrentReleaseSupportedPlatform](../models/syncacquireresponsedeploymentcurrentreleasesupportedplatform.md)[] | :heavy_minus_sign:                                                                                                                                 | Which platforms this stack supports. When None, all platforms are supported.                                                                       |