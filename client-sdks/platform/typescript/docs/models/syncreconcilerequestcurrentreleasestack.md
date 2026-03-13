# SyncReconcileRequestCurrentReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseStack } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseStack = {
  id: "<id>",
  resources: {},
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `id`                                                                                                                           | *string*                                                                                                                       | :heavy_check_mark:                                                                                                             | Unique identifier for the stack                                                                                                |
| `permissions`                                                                                                                  | [models.SyncReconcileRequestCurrentReleasePermissions](../models/syncreconcilerequestcurrentreleasepermissions.md)             | :heavy_minus_sign:                                                                                                             | Combined permissions configuration that contains both profiles and management                                                  |
| `resources`                                                                                                                    | Record<string, [models.SyncReconcileRequestCurrentReleaseResources](../models/syncreconcilerequestcurrentreleaseresources.md)> | :heavy_check_mark:                                                                                                             | Map of resource IDs to their configurations and lifecycle settings                                                             |