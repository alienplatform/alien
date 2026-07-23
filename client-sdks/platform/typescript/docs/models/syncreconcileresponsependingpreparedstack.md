# SyncReconcileResponsePendingPreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStack = {
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

| Field                                                                                                                                          | Type                                                                                                                                           | Required                                                                                                                                       | Description                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                           | *string*                                                                                                                                       | :heavy_check_mark:                                                                                                                             | Unique identifier for the stack                                                                                                                |
| `inputs`                                                                                                                                       | [models.SyncReconcileResponsePendingPreparedStackInput](../models/syncreconcileresponsependingpreparedstackinput.md)[]                         | :heavy_minus_sign:                                                                                                                             | Input definitions required before setup or deployment can proceed.                                                                             |
| `permissions`                                                                                                                                  | [models.SyncReconcileResponsePendingPreparedStackPermissions](../models/syncreconcileresponsependingpreparedstackpermissions.md)               | :heavy_minus_sign:                                                                                                                             | Combined permissions configuration that contains both profiles and management                                                                  |
| `resources`                                                                                                                                    | Record<string, [models.SyncReconcileResponsePendingPreparedStackResources](../models/syncreconcileresponsependingpreparedstackresources.md)>   | :heavy_check_mark:                                                                                                                             | Map of resource IDs to their configurations and lifecycle settings                                                                             |
| `supportedPlatforms`                                                                                                                           | [models.SyncReconcileResponsePendingPreparedStackSupportedPlatform](../models/syncreconcileresponsependingpreparedstacksupportedplatform.md)[] | :heavy_minus_sign:                                                                                                                             | Which platforms this stack supports. When None, all platforms are supported.                                                                   |
