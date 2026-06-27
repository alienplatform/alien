# SyncReconcileResponseCurrentReleaseStack

A bag of resources, unaware of any cloud.

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseStack = {
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

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                               | *string*                                                                                                                           | :heavy_check_mark:                                                                                                                 | Unique identifier for the stack                                                                                                    |
| `inputs`                                                                                                                           | [models.SyncReconcileResponseCurrentReleaseInput](../models/syncreconcileresponsecurrentreleaseinput.md)[]                         | :heavy_minus_sign:                                                                                                                 | Input definitions required before setup or deployment can proceed.                                                                 |
| `permissions`                                                                                                                      | [models.SyncReconcileResponseCurrentReleasePermissions](../models/syncreconcileresponsecurrentreleasepermissions.md)               | :heavy_minus_sign:                                                                                                                 | Combined permissions configuration that contains both profiles and management                                                      |
| `resources`                                                                                                                        | Record<string, [models.SyncReconcileResponseCurrentReleaseResources](../models/syncreconcileresponsecurrentreleaseresources.md)>   | :heavy_check_mark:                                                                                                                 | Map of resource IDs to their configurations and lifecycle settings                                                                 |
| `supportedPlatforms`                                                                                                               | [models.SyncReconcileResponseCurrentReleaseSupportedPlatform](../models/syncreconcileresponsecurrentreleasesupportedplatform.md)[] | :heavy_minus_sign:                                                                                                                 | Which platforms this stack supports. When None, all platforms are supported.                                                       |