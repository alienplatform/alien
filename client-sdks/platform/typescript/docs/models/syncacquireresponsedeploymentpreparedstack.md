# SyncAcquireResponseDeploymentPreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPreparedStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPreparedStack = {
  id: "<id>",
  resources: {},
};
```

## Fields

| Field                                                                                                                                            | Type                                                                                                                                             | Required                                                                                                                                         | Description                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                                             | *string*                                                                                                                                         | :heavy_check_mark:                                                                                                                               | Unique identifier for the stack                                                                                                                  |
| `inputs`                                                                                                                                         | [models.SyncAcquireResponseDeploymentPreparedStackInput](../models/syncacquireresponsedeploymentpreparedstackinput.md)[]                         | :heavy_minus_sign:                                                                                                                               | Input definitions required before setup or deployment can proceed.                                                                               |
| `permissions`                                                                                                                                    | [models.SyncAcquireResponseDeploymentPreparedStackPermissions](../models/syncacquireresponsedeploymentpreparedstackpermissions.md)               | :heavy_minus_sign:                                                                                                                               | Combined permissions configuration that contains both profiles and management                                                                    |
| `resources`                                                                                                                                      | Record<string, [models.SyncAcquireResponseDeploymentPreparedStackResources](../models/syncacquireresponsedeploymentpreparedstackresources.md)>   | :heavy_check_mark:                                                                                                                               | Map of resource IDs to their configurations and lifecycle settings                                                                               |
| `supportedPlatforms`                                                                                                                             | [models.SyncAcquireResponseDeploymentPreparedStackSupportedPlatform](../models/syncacquireresponsedeploymentpreparedstacksupportedplatform.md)[] | :heavy_minus_sign:                                                                                                                               | Which platforms this stack supports. When None, all platforms are supported.                                                                     |