# SyncReconcileRequestTargetReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseStack } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestTargetReleaseStack = {
  id: "<id>",
  resources: {},
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                         | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Unique identifier for the stack                                                                                              |
| `permissions`                                                                                                                | [models.SyncReconcileRequestTargetReleasePermissions](../models/syncreconcilerequesttargetreleasepermissions.md)             | :heavy_minus_sign:                                                                                                           | Combined permissions configuration that contains both profiles and management                                                |
| `resources`                                                                                                                  | Record<string, [models.SyncReconcileRequestTargetReleaseResources](../models/syncreconcilerequesttargetreleaseresources.md)> | :heavy_check_mark:                                                                                                           | Map of resource IDs to their configurations and lifecycle settings                                                           |