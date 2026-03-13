# SyncAcquireResponsePreparedStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePreparedStack = {
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

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                       | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Unique identifier for the stack                                                                                            |
| `permissions`                                                                                                              | [models.SyncAcquireResponsePreparedStackPermissions](../models/syncacquireresponsepreparedstackpermissions.md)             | :heavy_minus_sign:                                                                                                         | Combined permissions configuration that contains both profiles and management                                              |
| `resources`                                                                                                                | Record<string, [models.SyncAcquireResponsePreparedStackResources](../models/syncacquireresponsepreparedstackresources.md)> | :heavy_check_mark:                                                                                                         | Map of resource IDs to their configurations and lifecycle settings                                                         |