# SyncReconcileResponseTargetReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseTargetReleaseStack = {
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

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                           | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | Unique identifier for the stack                                                                                                |
| `permissions`                                                                                                                  | [models.SyncReconcileResponseTargetReleasePermissions](../models/syncreconcileresponsetargetreleasepermissions.md)             | :heavy_minus_sign:                                                                                                             | Combined permissions configuration that contains both profiles and management                                                  |
| `resources`                                                                                                                    | Record<string, [models.SyncReconcileResponseTargetReleaseResources](../models/syncreconcileresponsetargetreleaseresources.md)> | :heavy_check_mark:                                                                                                             | Map of resource IDs to their configurations and lifecycle settings                                                             |